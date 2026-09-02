use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use agentworth_adapter_sdk::{
    AgentAdapter, DetectionResult, ParseResult, ScanOptions, SessionSource,
};
use agentworth_schema::{
    AgentWorthTrace, EventPayload, FileActionType, ModelSwitch, NormalizedEvent, OutcomeEvidence, OutcomeKind,
    Provenance, ShellCommand, TokenUsage, ToolCall, ToolResult,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use directories::BaseDirs;
use serde_json::Value;
use walkdir::WalkDir;

/// Adapter for discovering and normalizing Nous Hermes agent turn logs and stream sessions.
pub struct HermesAdapter;

impl Default for HermesAdapter {
    fn default() -> Self {
        Self
    }
}

impl HermesAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Candidate directory paths for Hermes on the host machine, used for presence
    /// detection only. Broader than `session_roots()`: bare `~/.hermes` can also hold a
    /// full dev checkout of the Hermes agent itself (`hermes-agent/node_modules/...`),
    /// which is not session data.
    pub fn candidate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".hermes"));
            roots.push(home.join(".hermes").join("sessions"));
            roots.push(home.join(".hermes").join("turns"));
            roots.push(home.join(".config").join("hermes"));
        }
        roots.push(PathBuf::from(".hermes"));
        roots.push(PathBuf::from(".hermes").join("sessions"));
        roots.push(PathBuf::from(".hermes").join("turns"));
        roots
    }

    /// Directories that actually hold Hermes session/turn data, used for the default
    /// (unscoped) `enumerate()` walk.
    pub fn session_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".hermes").join("sessions"));
            roots.push(home.join(".hermes").join("turns"));
        }
        roots
    }
}

/// Skip directories that are never Hermes session data but can appear under `~/.hermes`
/// when it doubles as a dev checkout of the agent itself.
fn should_skip_hermes_dir(entry: &walkdir::DirEntry) -> bool {
    if entry.file_type().is_dir() {
        let name = entry.file_name().to_string_lossy();
        name == ".git"
            || name == "node_modules"
            || name == "target"
            || name == "dist"
            || name == ".venv"
    } else {
        false
    }
}

impl AgentAdapter for HermesAdapter {
    fn name(&self) -> &'static str {
        "hermes"
    }

    fn detect(&self, options: &ScanOptions) -> Result<DetectionResult> {
        let mut discovered = Vec::new();

        for root in self.candidate_roots() {
            if root.exists() {
                discovered.push(root);
            }
        }

        for custom in &options.custom_paths {
            if !custom.exists() {
                continue;
            }
            let s = custom.to_string_lossy().to_lowercase();
            if s.contains(".hermes") || s.contains("hermes") {
                discovered.push(custom.clone());
            } else if custom.is_dir() {
                // custom_paths may point at a generic parent directory rather than
                // the adapter-specific dir itself; look a few levels in before
                // giving up, matching how `enumerate()` already recurses.
                let mut found_nested = false;
                for sub in &[custom.join(".hermes"), custom.join(".config").join("hermes")] {
                    if sub.exists() {
                        discovered.push(sub.clone());
                        found_nested = true;
                    }
                }
                if !found_nested {
                    for entry in WalkDir::new(custom).max_depth(4).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        let ps = path.to_string_lossy().to_lowercase();
                        if ps.contains(".hermes") || ps.contains("hermes") {
                            discovered.push(path.to_path_buf());
                            break;
                        }
                    }
                }
            }
        }

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
                    if is_candidate_hermes_file(custom) {
                        if let Ok(source) = SessionSource::from_path(custom, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if custom.is_dir() {
                    for entry in WalkDir::new(custom)
                        .into_iter()
                        .filter_entry(|e| !should_skip_hermes_dir(e))
                        .filter_map(|e| e.ok())
                    {
                        let path = entry.path();
                        if path.is_file() && is_candidate_hermes_file(path) {
                            if let Ok(source) = SessionSource::from_path(path, self.name()) {
                                sources.push(source);
                            }
                        }
                    }
                }
            }
        } else {
            for root in self.session_roots() {
                if root.is_file() {
                    if is_candidate_hermes_file(&root) {
                        if let Ok(source) = SessionSource::from_path(&root, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if root.is_dir() {
                    for entry in WalkDir::new(&root)
                        .into_iter()
                        .filter_entry(|e| !should_skip_hermes_dir(e))
                        .filter_map(|e| e.ok())
                    {
                        let path = entry.path();
                        if path.is_file() && is_candidate_hermes_file(path) {
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
        let mut last_model: Option<String> = None;

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
                    } else if let Some(turns) = json_val
                        .get("turns")
                        .or_else(|| json_val.get("messages"))
                        .or_else(|| json_val.get("history"))
                        .or_else(|| json_val.get("events"))
                        .or_else(|| json_val.get("conversation"))
                        .and_then(|t| t.as_array())
                    {
                        turns.clone()
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

                        let evts = parse_hermes_record(item, &mut sequence, timestamp, idx + 1, &mut last_model);
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

                let events = parse_hermes_record(&val, &mut sequence, timestamp, line_num, &mut last_model);
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

fn is_candidate_hermes_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    if !path_str.contains("hermes") && !path_str.contains("nous") {
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
    // `~/.hermes/sessions/request_dump_<id>_<ts>.json` is a failed-HTTP-request debug
    // dump (reason: "max_retries_exhausted", full request/response incl. auth headers),
    // written by Hermes' own retry logic -- not a conversation transcript, despite living
    // in the "sessions" directory. Measured: every file in `sessions/` on a real machine
    // matched this prefix and none were real turns.
    if lower.starts_with("request_dump") {
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
    if let Some(millis) = val.get("timestamp").and_then(|v| v.as_i64()) {
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
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let cache_creation_tokens = usage_val
        .get("cache_creation_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    TokenUsage::new(
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
    )
}

fn parse_hermes_record(
    val: &Value,
    seq: &mut u64,
    ts: DateTime<Utc>,
    line_num: usize,
    last_model: &mut Option<String>,
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
        .or_else(|| val.get("tokens"))
        .or_else(|| val.get("token_usage"))
    {
        let usage = extract_token_usage(usage_val);
        if usage.total() > 0 {
            let model = val
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("Hermes-3-Llama-3.1-70B")
                .to_string();

            if last_model.as_deref() != Some(model.as_str()) {
                if let Some(prev) = last_model.take() {
                    *seq += 1;
                    events.push(
                        NormalizedEvent::new(
                            *seq,
                            ts,
                            EventPayload::ModelSwitch(ModelSwitch {
                                from_model: Some(prev),
                                to_model: model.clone(),
                                reason: None,
                            }),
                        )
                        .with_raw_ref(&raw_ref),
                    );
                }
                *last_model = Some(model.clone());
            }

            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::ModelInvocation {
                        model,
                        token_usage: usage,
                        cost_usd: val.get("cost").and_then(|c| c.as_f64()),
                        latency_ms: val
                            .get("latency_ms")
                            .or_else(|| val.get("duration_ms"))
                            .and_then(|d| d.as_u64()),
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    match role {
        "user" | "human" => {
            let content = extract_hermes_content(val);
            *seq += 1;
            events.push(
                NormalizedEvent::new(*seq, ts, EventPayload::UserMessage { content })
                    .with_raw_ref(&raw_ref),
            );
        }

        "assistant" | "model" | "agent" => {
            let mut content = extract_hermes_content(val);
            let mut thinking = val
                .get("thinking")
                .or_else(|| val.get("scratchpad"))
                .and_then(|v| v.as_str())
                .map(String::from);

            // Extract XML style <thinking> or <scratchpad> tags if embedded in content
            if thinking.is_none() {
                if let Some(start) = content.find("<thinking>") {
                    if let Some(end) = content.find("</thinking>") {
                        let th = content[start + 10..end].trim().to_string();
                        thinking = Some(th);
                        content = format!("{}{}", &content[..start], &content[end + 11..])
                            .trim()
                            .to_string();
                    }
                } else if let Some(start) = content.find("<scratchpad>") {
                    if let Some(end) = content.find("</scratchpad>") {
                        let th = content[start + 12..end].trim().to_string();
                        thinking = Some(th);
                        content = format!("{}{}", &content[..start], &content[end + 13..])
                            .trim()
                            .to_string();
                    }
                }
            }

            // Extract function/tool calls
            if let Some(tools) = val
                .get("tool_calls")
                .or_else(|| val.get("function_calls"))
                .and_then(|v| v.as_array())
            {
                for tc in tools {
                    let id = tc.get("id").and_then(|v| v.as_str()).map(String::from);
                    let name = tc
                        .get("name")
                        .or_else(|| tc.get("function").and_then(|f| f.get("name")))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let args = tc
                        .get("arguments")
                        .or_else(|| tc.get("function").and_then(|f| f.get("arguments")))
                        .cloned()
                        .unwrap_or(Value::Null);

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

                    process_specific_hermes_tool_call(&name, &args, seq, ts, &raw_ref, &mut events);
                }
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

        "tool_call" | "tool_use" => {
            let id = val.get("id").and_then(|v| v.as_str()).map(String::from);
            let name = val
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let args = val.get("arguments").cloned().unwrap_or(Value::Null);

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

            process_specific_hermes_tool_call(&name, &args, seq, ts, &raw_ref, &mut events);
        }

        "tool" | "tool_result" => {
            let call_id = val
                .get("tool_call_id")
                .or_else(|| val.get("call_id"))
                .and_then(|v| v.as_str())
                .map(String::from);
            let is_error = val
                .get("is_error")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let output = val
                .get("output")
                .or_else(|| val.get("content"))
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
                if out_str.contains("test result: ok.") || out_str.contains("PASSED") {
                    *seq += 1;
                    events.push(
                        NormalizedEvent::new(
                            *seq,
                            ts,
                            EventPayload::OutcomeEvidence(OutcomeEvidence {
                                kind: OutcomeKind::TestOrBuildPassed,
                                summary: "Test suite executed successfully in Hermes".to_string(),
                                confidence: 0.9,
                            }),
                        )
                        .with_raw_ref(&raw_ref),
                    );
                }
            }
        }

        "error" => {
            let message = val
                .get("message")
                .or_else(|| val.get("error"))
                .and_then(|v| v.as_str())
                .unwrap_or("Hermes execution error")
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
                        kind: role.to_string(),
                        data: val.clone(),
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    events
}

fn process_specific_hermes_tool_call(
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
        || lower.contains("exec")
        || lower.contains("terminal")
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
                        action: FileActionType::Edit,
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

fn extract_hermes_content(val: &Value) -> String {
    if let Some(text) = val.get("content").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    if let Some(text) = val.get("message").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    if let Some(text) = val.get("text").and_then(|v| v.as_str()) {
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
    fn test_detect_and_enumerate_hermes() {
        let temp = tempdir().unwrap();
        let hermes_dir = temp.path().join(".hermes").join("sessions");
        std::fs::create_dir_all(&hermes_dir).unwrap();

        let turn_file = hermes_dir.join("session_001.jsonl");
        let mut f = File::create(&turn_file).unwrap();
        writeln!(f, "{{\"role\":\"user\",\"content\":\"Write function\"}}").unwrap();

        let adapter = HermesAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
            ..Default::default()
        };

        let detection = adapter.detect(&options).unwrap();
        assert!(detection.is_present);

        let enumerated = adapter.enumerate(&options).unwrap();
        assert_eq!(enumerated.len(), 1);
        assert_eq!(enumerated[0].adapter_name, "hermes");
    }

    /// Regression test: `sessions/request_dump_*.json` is a failed-HTTP-request debug
    /// dump, not a chat turn, even though it lives in the "sessions" directory. A
    /// `node_modules` tree under a dev checkout at bare `~/.hermes` must also be skipped.
    #[test]
    fn test_enumerate_hermes_skips_request_dumps_and_node_modules_junk() {
        let temp = tempdir().unwrap();

        let sessions_dir = temp.path().join(".hermes").join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        File::create(sessions_dir.join("request_dump_20260709_140807_c7de2a_20260709_141026_110730.json")).unwrap();
        let mut real = File::create(sessions_dir.join("session_001.jsonl")).unwrap();
        writeln!(real, "{{\"role\":\"user\",\"content\":\"Write function\"}}").unwrap();

        let vendored_dir = temp
            .path()
            .join(".hermes")
            .join("hermes-agent")
            .join("node_modules")
            .join("spdx-license-ids");
        std::fs::create_dir_all(&vendored_dir).unwrap();
        File::create(vendored_dir.join("index.json")).unwrap();

        let adapter = HermesAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
            ..Default::default()
        };

        let enumerated = adapter.enumerate(&options).unwrap();
        assert_eq!(enumerated.len(), 1, "only the real turn file should be enumerated");
        assert!(enumerated[0].path.to_string_lossy().ends_with("session_001.jsonl"));
    }

    #[test]
    fn test_parse_standard_hermes_jsonl_with_thinking_tags() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"role":"user","timestamp":"2026-08-29T10:00:00Z","content":"Implement Hermes agent tools"}
{"role":"assistant","timestamp":"2026-08-29T10:00:03Z","model":"Hermes-3-Llama-3.1-70B","usage":{"input_tokens":400,"output_tokens":120,"cache_read_tokens":50},"content":"<thinking>Analyzing requirements and writing test suite</thinking>I will run the test command.","tool_calls":[{"name":"bash","arguments":{"command":"cargo test"}}]}
{"role":"tool","timestamp":"2026-08-29T10:00:06Z","output":"test result: ok. 10 passed; 0 failed"}
{"role":"assistant","timestamp":"2026-08-29T10:00:08Z","content":"Everything is verified."}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = HermesAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 0);
        let trace = result.trace;
        assert_eq!(trace.adapter, "hermes");
        assert_eq!(
            trace.stats.models_used,
            vec!["Hermes-3-Llama-3.1-70B".to_string()]
        );
        assert_eq!(trace.stats.token_usage.input_tokens, 400);
        assert_eq!(trace.stats.token_usage.output_tokens, 120);
        assert_eq!(trace.stats.token_usage.cache_read_tokens, 50);
        assert_eq!(trace.stats.token_usage.total(), 570);
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.assistant_messages_count, 2);
    }

    #[test]
    fn test_parse_hermes_turns_json() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"{
  "session_id": "hermes-session-007",
  "turns": [
    {"role": "user", "content": "Hello Hermes"},
    {"role": "assistant", "model": "Hermes-3-Llama-3.1-405B", "usage": {"input_tokens": 100, "output_tokens": 30}, "content": "Hello! How can I help you today?"}
  ]
}"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = HermesAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        let trace = result.trace;
        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.assistant_messages_count, 1);
        assert_eq!(trace.stats.token_usage.total(), 130);
    }

    #[test]
    fn test_parse_graceful_on_malformed_lines() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = "{\"role\":\"user\",\"content\":\"hello\"}\n{CORRUPT_HERMES_JSON}\n{\"role\":\"assistant\",\"content\":\"hi\"}\n";
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = HermesAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 1);
        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
    }
}
