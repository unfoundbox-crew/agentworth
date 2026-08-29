use std::fs::File;
use std::io::{BufRead, BufReader};
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

/// Adapter for discovering and normalizing OpenAI / Codex agent sessions.
pub struct CodexAdapter;

impl Default for CodexAdapter {
    fn default() -> Self {
        Self
    }
}

impl CodexAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Candidate directory paths for Codex on the host machine.
    pub fn candidate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".codex"));
            roots.push(home.join(".codex").join("sessions"));
            roots.push(home.join(".config").join("codex"));
        }
        roots.push(PathBuf::from(".codex"));
        roots
    }
}

impl AgentAdapter for CodexAdapter {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn detect(&self, options: &ScanOptions) -> Result<DetectionResult> {
        let mut discovered = Vec::new();

        for root in self.candidate_roots() {
            if root.exists() {
                discovered.push(root);
            }
        }

        for custom in &options.custom_paths {
            if custom.exists()
                && (custom.ends_with(".codex") || custom.to_string_lossy().contains("codex"))
            {
                discovered.push(custom.clone());
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
                    if is_candidate_codex_file(custom) {
                        if let Ok(source) = SessionSource::from_path(custom, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if custom.is_dir() {
                    for entry in WalkDir::new(custom).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_codex_file(path) {
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
                    if is_candidate_codex_file(&root) {
                        if let Ok(source) = SessionSource::from_path(&root, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if root.is_dir() {
                    for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_codex_file(path) {
                            if let Ok(source) = SessionSource::from_path(path, self.name()) {
                                sources.push(source);
                            }
                        }
                    }
                }
            }
        }

        // Deduplicate sources by canonical path
        sources.sort_by(|a, b| a.path.cmp(&b.path));
        sources.dedup_by(|a, b| a.path == b.path);

        Ok(sources)
    }

    fn parse(&self, source: &SessionSource) -> Result<ParseResult> {
        let file = File::open(&source.path)?;
        let reader = BufReader::new(file);

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

        for (line_idx, line_res) in reader.lines().enumerate() {
            let line_num = line_idx + 1;
            let line_str = match line_res {
                Ok(l) => l,
                Err(e) => {
                    malformed_lines += 1;
                    warnings.push(format!("I/O read error on line {}: {}", line_num, e));
                    continue;
                }
            };

            let trimmed = line_str.trim();
            if trimmed.is_empty() {
                continue;
            }

            let val: Value = match serde_json::from_str(trimmed) {
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

            let events = parse_codex_record(&val, &mut sequence, timestamp, line_num);
            trace.events.extend(events);
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

fn is_candidate_codex_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    if path_str.contains("/.claude/")
        || path_str.contains("/.gemini/")
        || path_str.contains("/.opencode/")
        || path_str.contains("claude_session")
        || path_str.contains("gemini_session")
        || path_str.contains("opencode_session")
    {
        return false;
    }
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if filename.starts_with('.') && !filename.ends_with(".jsonl") && !filename.ends_with(".json") {
        return false;
    }
    if filename == "config.json"
        || filename == "settings.json"
        || filename == "credentials.json"
        || filename == "auth.json"
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
    if let Some(epoch) = val.get("created").and_then(|v| v.as_i64()) {
        if epoch > 1_000_000_000_000 {
            return DateTime::from_timestamp_millis(epoch);
        } else {
            return DateTime::from_timestamp(epoch, 0);
        }
    }
    if let Some(millis) = val.get("timestamp").and_then(|v| v.as_i64()) {
        return DateTime::from_timestamp_millis(millis);
    }
    None
}

fn extract_token_usage(usage_val: &Value) -> TokenUsage {
    let input_tokens = usage_val
        .get("prompt_tokens")
        .or_else(|| usage_val.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let output_tokens = usage_val
        .get("completion_tokens")
        .or_else(|| usage_val.get("output_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let cache_read_tokens = usage_val
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .or_else(|| usage_val.get("cached_tokens"))
        .or_else(|| usage_val.get("cache_read_tokens"))
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

fn parse_codex_record(
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

    // Model invocation / Token usage extraction
    if let Some(usage_val) = val
        .get("usage")
        .or_else(|| val.get("response").and_then(|r| r.get("usage")))
    {
        let usage = extract_token_usage(usage_val);
        if usage.total() > 0 {
            let model = val
                .get("model")
                .or_else(|| val.get("response").and_then(|r| r.get("model")))
                .and_then(|v| v.as_str())
                .unwrap_or("gpt-4o")
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
                            .get("duration_ms")
                            .or_else(|| val.get("latency_ms"))
                            .and_then(|d| d.as_u64()),
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    match event_type {
        "user" | "user_message" => {
            let content = if let Some(text) = val.get("content").and_then(|v| v.as_str()) {
                text.to_string()
            } else if let Some(msg) = val
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
            {
                msg.to_string()
            } else if let Some(arr) = val.get("content").and_then(|v| v.as_array()) {
                extract_text_from_array(arr)
            } else {
                val.to_string()
            };

            *seq += 1;
            events.push(
                NormalizedEvent::new(*seq, ts, EventPayload::UserMessage { content })
                    .with_raw_ref(&raw_ref),
            );
        }

        "assistant" | "assistant_message" => {
            let thinking = val
                .get("thinking")
                .or_else(|| val.get("reasoning_content"))
                .or_else(|| {
                    val.get("message")
                        .and_then(|m| m.get("reasoning_content").or_else(|| m.get("thinking")))
                })
                .and_then(|v| v.as_str())
                .map(String::from);

            let mut text_parts = Vec::new();
            if let Some(content_val) = val
                .get("content")
                .or_else(|| val.get("message").and_then(|m| m.get("content")))
            {
                if let Some(text) = content_val.as_str() {
                    text_parts.push(text.to_string());
                } else if let Some(arr) = content_val.as_array() {
                    for item in arr {
                        if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                            text_parts.push(text.to_string());
                        }
                    }
                }
            }

            // Extract OpenAI-style tool_calls array if present
            let tool_calls_opt = val
                .get("tool_calls")
                .or_else(|| val.get("message").and_then(|m| m.get("tool_calls")))
                .and_then(|v| v.as_array());

            if let Some(tcs) = tool_calls_opt {
                for tc in tcs {
                    let id = tc.get("id").and_then(|v| v.as_str()).map(String::from);
                    let fn_val = tc.get("function").unwrap_or(tc);
                    let name = fn_val
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();

                    let args: Value = match fn_val.get("arguments") {
                        Some(Value::String(s)) => {
                            serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.clone()))
                        }
                        Some(v) => v.clone(),
                        None => Value::Null,
                    };

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

                    process_specific_codex_tool_call(&name, &args, seq, ts, &raw_ref, &mut events);
                }
            }

            if !text_parts.is_empty() || thinking.is_some() {
                *seq += 1;
                events.push(
                    NormalizedEvent::new(
                        *seq,
                        ts,
                        EventPayload::AssistantMessage {
                            content: text_parts.join("\n"),
                            thinking,
                        },
                    )
                    .with_raw_ref(&raw_ref),
                );
            }
        }

        "tool" | "tool_result" | "function_response" => {
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
                .get("content")
                .or_else(|| val.get("output"))
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
                                summary: "Test suite passed in tool result".to_string(),
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
                                summary: "Git commit observed in tool result".to_string(),
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
                .get("error")
                .or_else(|| val.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error")
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

fn process_specific_codex_tool_call(
    name: &str,
    args: &Value,
    seq: &mut u64,
    ts: DateTime<Utc>,
    raw_ref: &str,
    events: &mut Vec<NormalizedEvent>,
) {
    let lower_name = name.to_lowercase();
    if lower_name == "exec"
        || lower_name == "bash"
        || lower_name == "shell"
        || lower_name == "run_command"
        || lower_name == "cmd"
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
    } else if lower_name == "edit"
        || lower_name == "write_file"
        || lower_name == "patch"
        || lower_name == "create_file"
        || lower_name == "fileedit"
        || lower_name == "apply_diff"
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

fn extract_text_from_array(arr: &[Value]) -> String {
    let mut texts = Vec::new();
    for item in arr {
        if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
            texts.push(text);
        } else if let Some(s) = item.as_str() {
            texts.push(s);
        }
    }
    texts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::{tempdir, NamedTempFile};

    #[test]
    fn test_parse_standard_codex_jsonl() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"role":"user","timestamp":"2026-08-29T10:00:00Z","content":"Refactor the parser"}
{"role":"assistant","timestamp":"2026-08-29T10:00:05Z","model":"gpt-4o","usage":{"prompt_tokens":400,"completion_tokens":150,"prompt_tokens_details":{"cached_tokens":100}},"tool_calls":[{"id":"call_abc","type":"function","function":{"name":"exec","arguments":"{\"command\":\"pytest tests/\"}"}}]}
{"role":"tool","timestamp":"2026-08-29T10:00:08Z","tool_call_id":"call_abc","content":"test result: ok. 12 PASSED"}
{"role":"assistant","timestamp":"2026-08-29T10:00:12Z","content":"Refactoring completed and tests pass."}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = CodexAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 0);
        let trace = result.trace;
        assert_eq!(trace.adapter, "codex");
        assert_eq!(trace.stats.models_used, vec!["gpt-4o".to_string()]);
        assert_eq!(trace.stats.token_usage.input_tokens, 400);
        assert_eq!(trace.stats.token_usage.output_tokens, 150);
        assert_eq!(trace.stats.token_usage.cache_read_tokens, 100);
        assert_eq!(trace.stats.token_usage.total(), 650);
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(trace.stats.tools_used.get("exec"), Some(&1));
        assert!(trace.stats.duration_seconds.unwrap() >= 12.0);
    }

    #[test]
    fn test_parse_codex_file_actions_and_reasoning() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"role":"user","created":1756461600,"content":"Add new helper"}
{"role":"assistant","created":1756461610,"model":"o1-preview","reasoning_content":"Thinking about helper logic...","tool_calls":[{"id":"call_edit","function":{"name":"edit","arguments":{"path":"src/helper.rs","diff":"+fn help() {}"}}}]}
{"role":"tool","created":1756461615,"tool_call_id":"call_edit","content":"[main abc1234] 1 files changed, 5 insertions(+)"}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = CodexAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        let trace = result.trace;
        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.assistant_messages_count, 1);
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(trace.stats.tools_used.get("edit"), Some(&1));
    }

    #[test]
    fn test_parse_graceful_on_empty_and_corrupt_lines() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = "{\"role\":\"user\",\"content\":\"hi\"}\n\n{CORRUPT_JSON}\n{\"role\":\"assistant\",\"content\":\"hello\"}\n";
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = CodexAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 1);
        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
    }

    #[test]
    fn test_detect_and_enumerate_codex() {
        let temp = tempdir().unwrap();
        let codex_dir = temp.path().join(".codex").join("sessions");
        std::fs::create_dir_all(&codex_dir).unwrap();

        let session_file = codex_dir.join("session_001.jsonl");
        let mut f = File::create(&session_file).unwrap();
        writeln!(f, "{{\"role\":\"user\",\"content\":\"test\"}}").unwrap();

        let adapter = CodexAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
        };

        let detection = adapter.detect(&options).unwrap();
        assert!(detection.is_present);

        let enumerated = adapter.enumerate(&options).unwrap();
        assert_eq!(enumerated.len(), 1);
        assert_eq!(enumerated[0].adapter_name, "codex");
    }
}
