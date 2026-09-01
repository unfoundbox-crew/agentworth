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

/// Adapter for discovering and normalizing Goose (Block) agent sessions.
pub struct GooseAdapter;

impl Default for GooseAdapter {
    fn default() -> Self {
        Self
    }
}

impl GooseAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Candidate directory paths for Goose on the host machine.
    pub fn candidate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".config").join("goose"));
            roots.push(home.join(".config").join("goose").join("sessions"));
            roots.push(home.join(".goose"));
            roots.push(home.join(".goose").join("sessions"));
            roots.push(
                home.join(".local")
                    .join("share")
                    .join("goose")
                    .join("sessions"),
            );
        }
        roots.push(PathBuf::from(".config").join("goose"));
        roots.push(PathBuf::from(".config").join("goose").join("sessions"));
        roots.push(PathBuf::from(".goose"));
        roots.push(PathBuf::from(".goose").join("sessions"));
        roots
    }
}

impl AgentAdapter for GooseAdapter {
    fn name(&self) -> &'static str {
        "goose"
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
            if custom.ends_with(".goose")
                || custom.ends_with("goose")
                || custom.to_string_lossy().contains("goose")
            {
                discovered.push(custom.clone());
            } else if custom.is_dir() {
                // A caller can pass a parent directory (a repo root, a tempdir in tests)
                // rather than the goose directory itself -- look one level down before
                // giving up, matching how `enumerate()` already recurses for this adapter.
                for sub in &[custom.join(".goose"), custom.join(".config").join("goose")] {
                    if sub.exists() {
                        discovered.push(sub.clone());
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
                    if is_candidate_goose_file(custom) {
                        if let Ok(source) = SessionSource::from_path(custom, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if custom.is_dir() {
                    for entry in WalkDir::new(custom).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_goose_file(path) {
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
                    if is_candidate_goose_file(&root) {
                        if let Ok(source) = SessionSource::from_path(&root, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if root.is_dir() {
                    for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_goose_file(path) {
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
                    } else if let Some(messages) =
                        json_val.get("messages").and_then(|m| m.as_array())
                    {
                        messages.clone()
                    } else if let Some(turns) = json_val.get("turns").and_then(|t| t.as_array()) {
                        turns.clone()
                    } else if let Some(events) = json_val.get("events").and_then(|e| e.as_array()) {
                        events.clone()
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

                        let evts = parse_goose_record(item, &mut sequence, timestamp, idx + 1);
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

            // Fallback to line-by-line JSONL parsing
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

                let events = parse_goose_record(&val, &mut sequence, timestamp, line_num);
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

fn is_candidate_goose_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    if !path_str.contains("goose") {
        return false;
    }
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if filename.starts_with('.') && !filename.ends_with(".jsonl") && !filename.ends_with(".json") {
        return false;
    }
    let lower = filename.to_lowercase();
    if lower == "config.json"
        || lower == "config.yaml"
        || lower == "settings.json"
        || lower == "credentials.json"
        || lower == "secrets.json"
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
    if let Some(millis) = val.get("timestamp").and_then(|v| v.as_i64()) {
        return DateTime::from_timestamp_millis(millis);
    }
    if let Some(epoch) = val.get("created").and_then(|v| v.as_i64()) {
        if epoch > 1_000_000_000_000 {
            return DateTime::from_timestamp_millis(epoch);
        } else {
            return DateTime::from_timestamp(epoch, 0);
        }
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
        .or_else(|| usage_val.get("cache_read_input_tokens"))
        .or_else(|| usage_val.get("cached_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let cache_creation_tokens = usage_val
        .get("cache_creation_tokens")
        .or_else(|| usage_val.get("cache_creation_input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    TokenUsage::new(
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
    )
}

fn parse_goose_record(
    val: &Value,
    seq: &mut u64,
    ts: DateTime<Utc>,
    line_num: usize,
) -> Vec<NormalizedEvent> {
    let mut events = Vec::new();
    let raw_ref = format!("line:{}", line_num);

    let role = val
        .get("role")
        .or_else(|| val.get("message").and_then(|m| m.get("role")))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let event_type = val.get("type").and_then(|v| v.as_str()).unwrap_or(role);

    // Model invocation & token usage extraction
    if let Some(usage_val) = val
        .get("usage")
        .or_else(|| val.get("tokens"))
        .or_else(|| val.get("token_usage"))
        .or_else(|| val.get("response").and_then(|r| r.get("usage")))
    {
        let usage = extract_token_usage(usage_val);
        if usage.total() > 0 {
            let model = val
                .get("model")
                .or_else(|| val.get("model_name"))
                .or_else(|| val.get("response").and_then(|r| r.get("model")))
                .and_then(|v| v.as_str())
                .unwrap_or("goose-model")
                .to_string();

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

    match event_type {
        "user" | "human" | "user_message" => {
            let content = extract_goose_content(val);
            *seq += 1;
            events.push(
                NormalizedEvent::new(*seq, ts, EventPayload::UserMessage { content })
                    .with_raw_ref(&raw_ref),
            );
        }

        "assistant" | "agent" | "assistant_message" => {
            let thinking = val
                .get("thinking")
                .or_else(|| val.get("reasoning"))
                .and_then(|v| v.as_str())
                .map(String::from);

            let content = extract_goose_content(val);

            // Check for skills / tool calls in assistant message
            if let Some(tools) = val
                .get("tool_calls")
                .or_else(|| val.get("tools"))
                .or_else(|| val.get("skills"))
                .and_then(|v| v.as_array())
            {
                for tc in tools {
                    let id = tc.get("id").and_then(|v| v.as_str()).map(String::from);
                    let raw_name = tc
                        .get("name")
                        .or_else(|| tc.get("tool"))
                        .or_else(|| tc.get("skill"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let args = tc
                        .get("arguments")
                        .or_else(|| tc.get("input"))
                        .or_else(|| tc.get("parameters"))
                        .cloned()
                        .unwrap_or(Value::Null);

                    let name = normalize_mcp_tool_name(&raw_name, &args);

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

                    process_specific_goose_tool_call(&raw_name, &name, &args, seq, ts, &raw_ref, &mut events);
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

        "tool_use" | "tool_call" | "skill_call" => {
            let id = val.get("id").and_then(|v| v.as_str()).map(String::from);
            let raw_name = val
                .get("name")
                .or_else(|| val.get("tool"))
                .or_else(|| val.get("skill"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let args = val
                .get("arguments")
                .or_else(|| val.get("input"))
                .or_else(|| val.get("parameters"))
                .cloned()
                .unwrap_or(Value::Null);
            let name = normalize_mcp_tool_name(&raw_name, &args);

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

            process_specific_goose_tool_call(&raw_name, &name, &args, seq, ts, &raw_ref, &mut events);
        }

        "tool_result" | "tool_output" | "skill_result" | "tool" => {
            let call_id = val
                .get("tool_call_id")
                .or_else(|| val.get("call_id"))
                .or_else(|| val.get("id"))
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

            let tool_name = val
                .get("name")
                .or_else(|| val.get("tool"))
                .and_then(|v| v.as_str())
                .map(|n| normalize_mcp_tool_name(n, &Value::Null));

            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::ToolResult(ToolResult {
                        call_id,
                        name: tool_name,
                        output: output.clone(),
                        is_error,
                    }),
                )
                .with_raw_ref(&raw_ref),
            );

            // Infer outcome evidence
            if let Some(out_str) = output.as_str() {
                if out_str.contains("test result: ok.")
                    || out_str.contains("PASSED")
                    || out_str.contains("100% tests passed")
                {
                    *seq += 1;
                    events.push(
                        NormalizedEvent::new(
                            *seq,
                            ts,
                            EventPayload::OutcomeEvidence(OutcomeEvidence {
                                kind: OutcomeKind::TestOrBuildPassed,
                                summary: "Test suite executed successfully in Goose".to_string(),
                                confidence: 0.9,
                            }),
                        )
                        .with_raw_ref(&raw_ref),
                    );
                } else if out_str.contains("[main ")
                    || out_str.contains("commit ")
                    || out_str.contains("files changed,")
                {
                    *seq += 1;
                    events.push(
                        NormalizedEvent::new(
                            *seq,
                            ts,
                            EventPayload::OutcomeEvidence(OutcomeEvidence {
                                kind: OutcomeKind::CommitObserved,
                                summary: "Git commit observed in Goose tool output".to_string(),
                                confidence: 0.85,
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
                .unwrap_or("Unknown Goose error")
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
                        kind: event_type.to_string(),
                        data: val.clone(),
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    events
}

fn process_specific_goose_tool_call(
    raw_name: &str,
    name: &str,
    args: &Value,
    seq: &mut u64,
    ts: DateTime<Utc>,
    raw_ref: &str,
    events: &mut Vec<NormalizedEvent>,
) {
    let lower_raw = raw_name.to_lowercase();
    let lower_name = name.to_lowercase();
    if lower_raw.contains("shell")
        || lower_raw.contains("bash")
        || lower_raw == "developer__text_editor"
        || lower_raw == "developer__shell"
        || lower_raw == "run_command"
        || lower_raw == "exec"
        || lower_name.ends_with(":bash")
        || lower_name.ends_with(":shell")
        || lower_name.ends_with(":run_command")
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
    }

    if lower_raw.contains("edit")
        || lower_raw.contains("write")
        || lower_raw.contains("patch")
        || lower_raw == "developer__text_editor"
        || lower_name.ends_with(":text_editor")
        || lower_name.ends_with(":edit_file")
        || lower_name.ends_with(":write_file")
    {
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
                            .or_else(|| args.get("patch"))
                            .or_else(|| args.get("new_str"))
                            .or_else(|| args.get("content"))
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

fn extract_goose_content(val: &Value) -> String {
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
    fn test_detect_and_enumerate_goose() {
        let temp = tempdir().unwrap();
        let goose_dir = temp.path().join(".config").join("goose").join("sessions");
        std::fs::create_dir_all(&goose_dir).unwrap();

        let session_file = goose_dir.join("session_001.jsonl");
        let mut f = File::create(&session_file).unwrap();
        writeln!(f, "{{\"role\":\"user\",\"content\":\"Deploy app\"}}").unwrap();

        let adapter = GooseAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
        };

        let detection = adapter.detect(&options).unwrap();
        assert!(detection.is_present);

        let enumerated = adapter.enumerate(&options).unwrap();
        assert_eq!(enumerated.len(), 1);
        assert_eq!(enumerated[0].adapter_name, "goose");
    }

    #[test]
    fn test_parse_standard_goose_jsonl() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"role":"user","timestamp":"2026-08-29T10:00:00Z","content":"Run tests with Goose"}
{"role":"assistant","timestamp":"2026-08-29T10:00:04Z","model":"claude-3-5-sonnet","usage":{"input_tokens":420,"output_tokens":110,"cache_read_input_tokens":80,"cache_creation_input_tokens":20},"thinking":"Calling developer shell skill","skills":[{"name":"developer__shell","arguments":{"command":"cargo test"}}]}
{"type":"tool_result","timestamp":"2026-08-29T10:00:08Z","name":"developer__shell","output":"test result: ok. 15 passed; 0 failed","is_error":false}
{"role":"assistant","timestamp":"2026-08-29T10:00:10Z","content":"Goose verified all 15 tests pass."}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = GooseAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 0);
        let trace = result.trace;
        assert_eq!(trace.adapter, "goose");
        assert_eq!(
            trace.stats.models_used,
            vec!["claude-3-5-sonnet".to_string()]
        );
        assert_eq!(trace.stats.token_usage.input_tokens, 420);
        assert_eq!(trace.stats.token_usage.output_tokens, 110);
        assert_eq!(trace.stats.token_usage.cache_read_tokens, 80);
        assert_eq!(trace.stats.token_usage.cache_creation_tokens, 20);
        assert_eq!(trace.stats.token_usage.total(), 630);
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(trace.stats.tools_used.get("mcp:developer:shell"), Some(&1));
        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.assistant_messages_count, 2);
    }

    #[test]
    fn test_parse_goose_json_session_object() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"{
  "session_id": "goose-sess-99",
  "model": "gpt-4o",
  "messages": [
    {"role": "user", "content": "Update config file"},
    {"role": "assistant", "model": "gpt-4o", "usage": {"prompt_tokens": 150, "completion_tokens": 50}, "tool_calls": [{"name": "developer__text_editor", "arguments": {"path": "src/config.rs", "diff": "+pub const TIMEOUT: u32 = 30;"}}]}
  ]
}"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = GooseAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        let trace = result.trace;
        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(
            trace.stats.tools_used.get("mcp:developer:text_editor"),
            Some(&1)
        );
        assert_eq!(trace.stats.token_usage.input_tokens, 150);
        assert_eq!(trace.stats.token_usage.output_tokens, 50);
    }

    #[test]
    fn test_parse_graceful_on_malformed_lines() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = "{\"role\":\"user\",\"content\":\"hello\"}\n{CORRUPT_GOOSE_JSON}\n{\"role\":\"assistant\",\"content\":\"hi\"}\n";
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = GooseAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 1);
        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
    }

    #[test]
    fn test_parse_goose_mcp_normalization() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"role":"user","timestamp":"2026-08-29T10:00:00Z","content":"Search knowledge catalog"}
{"role":"assistant","timestamp":"2026-08-29T10:00:02Z","tool_calls":[{"name":"mcp__knowledge_catalog__search_entries","arguments":{"query":"sales"}}]}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = GooseAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        let trace = result.trace;
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(
            trace.stats.tools_used.get("mcp:knowledge_catalog:search_entries"),
            Some(&1)
        );
    }
}
