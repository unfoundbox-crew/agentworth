use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use agentworth_adapter_sdk::{
    AgentAdapter, DetectionResult, ParseResult, ScanOptions, SessionSource,
};
use agentworth_schema::{
    AgentWorthTrace, EventPayload, FileActionType, NormalizedEvent, OutcomeEvidence, OutcomeKind,
    Provenance, ShellCommand, TokenUsage, ToolCall, ToolResult,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use directories::BaseDirs;
use serde_json::Value;
use walkdir::WalkDir;

use crate::normalize_mcp_tool_name;

/// Adapter for discovering and normalizing Alibaba Cloud Qwen-Agent and Qwen 2.5 Coder trajectories.
pub struct QwenAdapter;

impl Default for QwenAdapter {
    fn default() -> Self {
        Self
    }
}

impl QwenAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Candidate directory paths for Qwen histories on the host machine.
    pub fn candidate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".qwen"));
            roots.push(home.join(".qwen").join("sessions"));
            roots.push(home.join(".qwen").join("logs"));
            roots.push(home.join(".qwen-agent"));
            roots.push(home.join(".qwen-agent").join("sessions"));
            roots.push(home.join(".qwen-agent").join("logs"));
            roots.push(home.join(".config").join("qwen"));
            roots.push(home.join(".config").join("qwen-agent"));
        }
        roots.push(PathBuf::from(".qwen"));
        roots.push(PathBuf::from(".qwen").join("sessions"));
        roots.push(PathBuf::from(".qwen-agent"));
        roots.push(PathBuf::from(".qwen-agent").join("sessions"));
        roots.push(PathBuf::from(".config").join("qwen"));
        roots
    }
}

impl AgentAdapter for QwenAdapter {
    fn name(&self) -> &'static str {
        "qwen"
    }

    fn detect(&self, options: &ScanOptions) -> Result<DetectionResult> {
        let mut discovered = Vec::new();

        for root in self.candidate_roots() {
            if root.exists() {
                discovered.push(root);
            }
        }

        for custom in &options.custom_paths {
            if custom.exists() {
                let s = custom.to_string_lossy().to_lowercase();
                if s.contains(".qwen")
                    || s.contains("qwen")
                    || s.contains("qwen-agent")
                    || s.contains("qwen_agent")
                {
                    discovered.push(custom.clone());
                } else if custom.is_dir() {
                    for sub in &[
                        custom.join(".qwen"),
                        custom.join(".qwen-agent"),
                        custom.join(".config").join("qwen"),
                    ] {
                        if sub.exists() {
                            discovered.push(sub.clone());
                        }
                    }
                    if discovered.is_empty() {
                        for entry in WalkDir::new(custom).max_depth(3).into_iter().filter_map(|e| e.ok()) {
                            let path = entry.path();
                            let s = path.to_string_lossy().to_lowercase();
                            if s.contains(".qwen") || s.contains("qwen") {
                                discovered.push(path.to_path_buf());
                                break;
                            }
                        }
                    }
                }
            }
        }

        discovered.sort();
        discovered.dedup();

        let is_present = !discovered.is_empty();
        let confidence = if is_present { 0.95 } else { 0.0 };

        Ok(DetectionResult {
            adapter_name: self.name(),
            is_present,
            discovered_roots: discovered,
            confidence,
        })
    }

    fn enumerate(&self, options: &ScanOptions) -> Result<Vec<SessionSource>> {
        let mut sources = Vec::new();

        if !options.custom_paths.is_empty() {
            for custom in &options.custom_paths {
                if custom.is_file() {
                    if is_candidate_qwen_file(custom) {
                        if let Ok(source) = SessionSource::from_path(custom, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if custom.is_dir() {
                    for entry in WalkDir::new(custom).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_qwen_file(path) {
                            if let Ok(source) = SessionSource::from_path(path, self.name()) {
                                sources.push(source);
                            }
                        }
                    }
                }
            }
        } else {
            for root in self.candidate_roots() {
                if root.is_file() {
                    if is_candidate_qwen_file(&root) {
                        if let Ok(source) = SessionSource::from_path(&root, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if root.is_dir() {
                    for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_qwen_file(path) {
                            if let Ok(source) = SessionSource::from_path(path, self.name()) {
                                sources.push(source);
                            }
                        }
                    }
                }
            }
        }

        sources.sort_by(|a, b| a.path.cmp(&b.path));
        sources.dedup_by(|a, b| a.path == b.path);

        Ok(sources)
    }

    fn parse(&self, source: &SessionSource) -> Result<ParseResult> {
        let session_id = derive_session_id(&source.path);
        let provenance = Provenance::new(
            source.path.to_string_lossy().to_string(),
            self.name(),
            source.file_size_bytes,
            source.mtime_epoch_secs,
            &source.fingerprint,
        );

        let mut trace = AgentWorthTrace::new(&session_id, self.name(), provenance, Utc::now());
        let mut malformed_lines = 0;
        let mut warnings = Vec::new();
        let mut sequence = 0u64;

        let mut earliest_ts: Option<DateTime<Utc>> = None;
        let mut latest_ts: Option<DateTime<Utc>> = None;

        let file = File::open(&source.path)?;
        let mut reader = BufReader::new(file);

        let has_content = reader.get_ref().metadata()?.len() > 0;

        if has_content {
            let mut content_str = String::new();
            reader.read_to_string(&mut content_str)?;

            let trimmed = content_str.trim();
            if trimmed.starts_with('[')
                || (trimmed.starts_with('{') && !trimmed.contains('\n'))
                || (trimmed.starts_with('{') && serde_json::from_str::<Value>(trimmed).is_ok())
            {
                if let Ok(json_val) = serde_json::from_str::<Value>(trimmed) {
                    let items = if let Some(arr) = json_val.as_array() {
                        arr.clone()
                    } else if let Some(messages) = json_val
                        .get("messages")
                        .or_else(|| json_val.get("turns"))
                        .or_else(|| json_val.get("steps"))
                        .or_else(|| json_val.get("history"))
                        .or_else(|| json_val.get("trajectory"))
                        .or_else(|| json_val.get("chat_history"))
                        .or_else(|| json_val.get("events"))
                        .and_then(|m| m.as_array())
                    {
                        messages.clone()
                    } else {
                        vec![json_val]
                    };

                    for (idx, item) in items.iter().enumerate() {
                        let timestamp = parse_timestamp(item).unwrap_or_else(Utc::now);
                        if earliest_ts.is_none_or(|ts| timestamp < ts) {
                            earliest_ts = Some(timestamp);
                        }
                        if latest_ts.is_none_or(|ts| timestamp > ts) {
                            latest_ts = Some(timestamp);
                        }

                        let evts = parse_qwen_record(item, &mut sequence, timestamp, idx + 1);
                        trace.events.extend(evts);
                    }

                    if let Some(earliest) = earliest_ts {
                        trace.started_at = earliest;
                    }
                    if let Some(latest) = latest_ts {
                        trace.ended_at = Some(latest);
                    }

                    trace.recalculate_stats();

                    return Ok(ParseResult {
                        trace,
                        malformed_lines,
                        warnings,
                    });
                }
            }

            for (line_idx, line_str) in content_str.lines().enumerate() {
                let line_num = line_idx + 1;
                let trimmed_line = line_str.trim();
                if trimmed_line.is_empty() {
                    continue;
                }

                let val: Value = match serde_json::from_str(trimmed_line) {
                    Ok(v) => v,
                    Err(e) => {
                        malformed_lines += 1;
                        warnings.push(format!("JSON syntax error on line {}: {}", line_num, e));
                        continue;
                    }
                };

                let timestamp = parse_timestamp(&val).unwrap_or_else(Utc::now);
                if earliest_ts.is_none_or(|ts| timestamp < ts) {
                    earliest_ts = Some(timestamp);
                }
                if latest_ts.is_none_or(|ts| timestamp > ts) {
                    latest_ts = Some(timestamp);
                }

                let events = parse_qwen_record(&val, &mut sequence, timestamp, line_num);
                trace.events.extend(events);
            }
        }

        if let Some(earliest) = earliest_ts {
            trace.started_at = earliest;
        }
        if let Some(latest) = latest_ts {
            trace.ended_at = Some(latest);
        }

        trace.recalculate_stats();

        Ok(ParseResult {
            trace,
            malformed_lines,
            warnings,
        })
    }
}

fn is_candidate_qwen_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    if !path_str.contains("qwen") {
        return false;
    }
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if filename.starts_with('.') && !filename.ends_with(".jsonl") && !filename.ends_with(".json") {
        return false;
    }
    let lower = filename.to_lowercase();
    if lower == "config.json"
        || lower == "settings.json"
        || lower == "credentials.json"
        || lower == "auth.json"
        || lower == "package.json"
    {
        return false;
    }
    path.extension()
        .is_some_and(|ext| ext == "jsonl" || ext == "json")
}

fn derive_session_id(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

fn parse_timestamp(val: &Value) -> Option<DateTime<Utc>> {
    if let Some(ts_str) = val.get("timestamp").and_then(|v| v.as_str()) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    if let Some(ts_str) = val.get("created_at").and_then(|v| v.as_str()) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    if let Some(ts_str) = val.get("time").and_then(|v| v.as_str()) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    if let Some(millis) = val.get("timestamp").and_then(|v| v.as_i64()) {
        return DateTime::from_timestamp_millis(millis);
    }
    if let Some(secs) = val.get("timestamp").and_then(|v| v.as_f64()) {
        let millis = (secs * 1000.0) as i64;
        return DateTime::from_timestamp_millis(millis);
    }
    None
}

fn extract_token_usage(usage_val: &Value) -> TokenUsage {
    let input_tokens = usage_val
        .get("input_tokens")
        .or_else(|| usage_val.get("prompt_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let output_tokens = usage_val
        .get("output_tokens")
        .or_else(|| usage_val.get("completion_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let cache_read_tokens = usage_val
        .get("cache_read_tokens")
        .or_else(|| usage_val.get("cached_tokens"))
        .or_else(|| usage_val.get("prompt_cache_hit_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let cache_creation_tokens = usage_val
        .get("cache_creation_tokens")
        .or_else(|| usage_val.get("prompt_cache_miss_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    TokenUsage::new(
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
    )
}

fn parse_qwen_record(
    val: &Value,
    seq: &mut u64,
    ts: DateTime<Utc>,
    line_num: usize,
) -> Vec<NormalizedEvent> {
    let mut events = Vec::new();
    let raw_ref = format!("line:{}", line_num);

    let role = val
        .get("role")
        .or_else(|| val.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Model invocation / Token extraction
    if let Some(usage_val) = val
        .get("usage")
        .or_else(|| val.get("token_usage"))
        .or_else(|| val.get("tokens"))
    {
        let usage = extract_token_usage(usage_val);
        if usage.total() > 0 {
            let model = val
                .get("model")
                .or_else(|| val.get("model_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("Qwen2.5-Coder-32B-Instruct")
                .to_string();

            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::ModelInvocation {
                        model,
                        token_usage: usage,
                        cost_usd: val.get("cost").or_else(|| val.get("cost_usd")).and_then(|c| c.as_f64()),
                        latency_ms: val
                            .get("latency_ms")
                            .or_else(|| val.get("duration_ms"))
                            .or_else(|| val.get("latency"))
                            .and_then(|d| d.as_u64()),
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    match role {
        "user" | "human" => {
            let content = extract_qwen_content(val);
            *seq += 1;
            events.push(
                NormalizedEvent::new(*seq, ts, EventPayload::UserMessage { content })
                    .with_raw_ref(&raw_ref),
            );
        }

        "assistant" | "model" | "bot" | "agent" => {
            let mut content = extract_qwen_content(val);
            let mut thinking = val
                .get("thinking")
                .or_else(|| val.get("reasoning_content"))
                .or_else(|| val.get("thought"))
                .and_then(|v| v.as_str())
                .map(String::from);

            // Extract XML style <thought>...</thought> or <thinking>...</thinking> tags if embedded in content
            if thinking.is_none() {
                if let Some(start) = content.find("<thought>") {
                    if let Some(end) = content.find("</thought>") {
                        let th = content[start + 9..end].trim().to_string();
                        thinking = Some(th);
                        content = format!("{}{}", &content[..start], &content[end + 10..])
                            .trim()
                            .to_string();
                    }
                } else if let Some(start) = content.find("<thinking>") {
                    if let Some(end) = content.find("</thinking>") {
                        let th = content[start + 10..end].trim().to_string();
                        thinking = Some(th);
                        content = format!("{}{}", &content[..start], &content[end + 11..])
                            .trim()
                            .to_string();
                    }
                }
            }

            // Extract function/tool calls (Qwen-Agent supports function_call, tool_calls, code_interpreter)
            if let Some(tools) = val
                .get("tool_calls")
                .or_else(|| val.get("function_calls"))
                .or_else(|| val.get("tools"))
                .and_then(|v| v.as_array())
            {
                for tc in tools {
                    let id = tc.get("id").and_then(|v| v.as_str()).map(String::from);
                    let raw_name = tc
                        .get("name")
                        .or_else(|| tc.get("function").and_then(|f| f.get("name")))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let args = parse_or_extract_json(
                        tc.get("arguments")
                            .or_else(|| tc.get("function").and_then(|f| f.get("arguments")))
                            .or_else(|| tc.get("parameters")),
                    );
                    let name = normalize_mcp_tool_name(raw_name, &args);

                    *seq += 1;
                    events.push(
                        NormalizedEvent::new(
                            *seq,
                            ts,
                            EventPayload::ToolCall(ToolCall {
                                id: id.clone(),
                                name: name.clone(),
                                arguments: args.clone(),
                            }),
                        )
                        .with_raw_ref(&raw_ref),
                    );

                    process_specific_qwen_tool_call(&name, &args, seq, ts, &raw_ref, &mut events);
                }
            } else if let Some(fc) = val.get("function_call") {
                let id = fc.get("id").and_then(|v| v.as_str()).map(String::from);
                let raw_name = fc.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                let args = parse_or_extract_json(fc.get("arguments").or_else(|| fc.get("parameters")));
                let name = normalize_mcp_tool_name(raw_name, &args);

                *seq += 1;
                events.push(
                    NormalizedEvent::new(
                        *seq,
                        ts,
                        EventPayload::ToolCall(ToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: args.clone(),
                        }),
                    )
                    .with_raw_ref(&raw_ref),
                );

                process_specific_qwen_tool_call(&name, &args, seq, ts, &raw_ref, &mut events);
            }

            if !content.is_empty() || thinking.is_some() {
                *seq += 1;
                events.push(
                    NormalizedEvent::new(
                        *seq,
                        ts,
                        EventPayload::AssistantMessage { content, thinking },
                    )
                    .with_raw_ref(&raw_ref),
                );
            }
        }

        "tool" | "tool_result" | "function" => {
            let call_id = val
                .get("tool_call_id")
                .or_else(|| val.get("call_id"))
                .and_then(|v| v.as_str())
                .map(String::from);
            let is_error = val
                .get("is_error")
                .or_else(|| val.get("error"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let output = val
                .get("output")
                .or_else(|| val.get("content"))
                .or_else(|| val.get("result"))
                .cloned()
                .unwrap_or(Value::Null);

            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::ToolResult(ToolResult {
                        call_id,
                        name: val.get("name").and_then(|v| v.as_str()).map(String::from),
                        output: output.clone(),
                        is_error,
                    }),
                )
                .with_raw_ref(&raw_ref),
            );

            if let Some(out_str) = output.as_str() {
                if out_str.contains("test result: ok.")
                    || out_str.contains("PASSED")
                    || out_str.contains("tests passed")
                {
                    *seq += 1;
                    events.push(
                        NormalizedEvent::new(
                            *seq,
                            ts,
                            EventPayload::OutcomeEvidence(OutcomeEvidence {
                                kind: OutcomeKind::TestOrBuildPassed,
                                summary: "Test suite executed successfully in Qwen session".to_string(),
                                confidence: 0.9,
                            }),
                        )
                        .with_raw_ref(&raw_ref),
                    );
                }
            }
        }

        "tool_call" | "tool_use" => {
            let id = val.get("id").and_then(|v| v.as_str()).map(String::from);
            let raw_name = val
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let args = parse_or_extract_json(val.get("arguments").or_else(|| val.get("parameters")));
            let name = normalize_mcp_tool_name(raw_name, &args);

            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::ToolCall(ToolCall {
                        id,
                        name: name.clone(),
                        arguments: args.clone(),
                    }),
                )
                .with_raw_ref(&raw_ref),
            );

            process_specific_qwen_tool_call(&name, &args, seq, ts, &raw_ref, &mut events);
        }

        "error" => {
            let message = val
                .get("message")
                .or_else(|| val.get("error"))
                .and_then(|v| v.as_str())
                .unwrap_or("Qwen execution error")
                .to_string();
            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::Error {
                        message,
                        is_recovered: false,
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }

        _ => {
            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::Custom {
                        kind: if role.is_empty() { "unknown".to_string() } else { role.to_string() },
                        data: val.clone(),
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    events
}

fn parse_or_extract_json(val: Option<&Value>) -> Value {
    match val {
        Some(Value::String(s)) => serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.clone())),
        Some(v) => v.clone(),
        None => Value::Null,
    }
}

fn process_specific_qwen_tool_call(
    name: &str,
    args: &Value,
    seq: &mut u64,
    ts: DateTime<Utc>,
    raw_ref: &str,
    events: &mut Vec<NormalizedEvent>,
) {
    let lower = name.to_lowercase();
    if lower.contains("bash")
        || lower.contains("shell")
        || lower.contains("exec_bash")
        || lower.contains("run_command")
        || lower.contains("cmd")
    {
        if let Some(cmd) = args
            .get("command")
            .or_else(|| args.get("cmd"))
            .and_then(|v| v.as_str())
        {
            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::ShellCommand(ShellCommand {
                        command: cmd.to_string(),
                        cwd: args.get("cwd").and_then(|v| v.as_str()).map(String::from),
                        exit_code: None,
                        output: None,
                    }),
                )
                .with_raw_ref(raw_ref),
            );
        }
    } else if lower.contains("code_interpreter") || lower.contains("python") {
        if let Some(code) = args
            .get("code")
            .or_else(|| args.get("command"))
            .and_then(|v| v.as_str())
        {
            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::ShellCommand(ShellCommand {
                        command: format!("python -c {:?}", code),
                        cwd: args.get("cwd").and_then(|v| v.as_str()).map(String::from),
                        exit_code: None,
                        output: None,
                    }),
                )
                .with_raw_ref(raw_ref),
            );
        }
    } else if lower.contains("file") || lower.contains("edit") || lower.contains("write") {
        let path = args
            .get("path")
            .or_else(|| args.get("file_path"))
            .or_else(|| args.get("target_file"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if !path.is_empty() {
            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::FileAction {
                        path,
                        action: if lower.contains("read") {
                            FileActionType::Read
                        } else if lower.contains("write") {
                            FileActionType::Write
                        } else {
                            FileActionType::Edit
                        },
                        diff: args
                            .get("diff")
                            .or_else(|| args.get("content"))
                            .or_else(|| args.get("patch"))
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        lines_changed: None,
                    },
                )
                .with_raw_ref(raw_ref),
            );
        }
    }
}

fn extract_qwen_content(val: &Value) -> String {
    if let Some(text) = val.get("content").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    if let Some(text) = val.get("text").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    if let Some(text) = val.get("message").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    if let Some(arr) = val.get("content").and_then(|v| v.as_array()) {
        let mut texts = Vec::new();
        for item in arr {
            if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                texts.push(t);
            } else if let Some(s) = item.as_str() {
                texts.push(s);
            }
        }
        if !texts.is_empty() {
            return texts.join("\n");
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::{tempdir, NamedTempFile};

    #[test]
    fn test_detect_and_enumerate_qwen() {
        let temp = tempdir().unwrap();
        let qwen_dir = temp.path().join(".qwen").join("sessions");
        std::fs::create_dir_all(&qwen_dir).unwrap();

        let session_file = qwen_dir.join("qwen_coder_001.jsonl");
        let mut f = File::create(&session_file).unwrap();
        writeln!(f, "{{\"role\":\"user\",\"content\":\"Write quicksort in Rust\"}}").unwrap();

        let adapter = QwenAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
        };

        let detection = adapter.detect(&options).unwrap();
        assert!(detection.is_present);
        assert_eq!(detection.adapter_name, "qwen");

        let enumerated = adapter.enumerate(&options).unwrap();
        assert_eq!(enumerated.len(), 1);
        assert_eq!(enumerated[0].adapter_name, "qwen");
    }

    #[test]
    fn test_parse_standard_qwen_jsonl_with_tool_calls_and_tokens() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"role":"user","timestamp":"2026-08-30T12:00:00Z","content":"Run unit tests for math library"}
{"role":"assistant","timestamp":"2026-08-30T12:00:02Z","model":"Qwen2.5-Coder-32B-Instruct","usage":{"prompt_tokens":350,"completion_tokens":85,"cached_tokens":120},"content":"<thought>I should run cargo test to verify.</thought>Executing test suite now.","tool_calls":[{"name":"bash","arguments":{"command":"cargo test --lib"}}]}
{"role":"tool","timestamp":"2026-08-30T12:00:05Z","output":"test result: ok. 15 passed; 0 failed"}
{"role":"assistant","timestamp":"2026-08-30T12:00:07Z","content":"All 15 math tests passed successfully."}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = QwenAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 0);
        let trace = result.trace;
        assert_eq!(trace.adapter, "qwen");
        assert_eq!(
            trace.stats.models_used,
            vec!["Qwen2.5-Coder-32B-Instruct".to_string()]
        );
        assert_eq!(trace.stats.token_usage.input_tokens, 350);
        assert_eq!(trace.stats.token_usage.output_tokens, 85);
        assert_eq!(trace.stats.token_usage.cache_read_tokens, 120);
        assert_eq!(trace.stats.token_usage.total(), 555);
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.assistant_messages_count, 2);
    }

    #[test]
    fn test_parse_qwen_agent_json_object() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"{
  "session_id": "qwen-session-42",
  "messages": [
    {"role": "user", "content": "Calculate fibonacci 10 in Python"},
    {"role": "assistant", "model": "Qwen2.5-Coder-7B", "usage": {"prompt_tokens": 150, "completion_tokens": 40}, "content": "Running code interpreter", "function_call": {"name": "code_interpreter", "arguments": "{\"code\": \"def fib(n): return n if n <= 1 else fib(n-1)+fib(n-2); print(fib(10))\"}"}}
  ]
}"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = QwenAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        let trace = result.trace;
        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.assistant_messages_count, 1);
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(trace.stats.token_usage.total(), 190);
    }

    #[test]
    fn test_parse_graceful_on_malformed_lines() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = "{\"role\":\"user\",\"content\":\"hello\"}\n{CORRUPT_QWEN_LOG}\n{\"role\":\"assistant\",\"content\":\"hi\"}\n";
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = QwenAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 1);
        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
    }
}
