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

use crate::normalize_mcp_tool_name;

/// Adapter for discovering and normalizing Windsurf / Codeium Cascade agent sessions.
pub struct WindsurfAdapter;

impl Default for WindsurfAdapter {
    fn default() -> Self {
        Self
    }
}

impl WindsurfAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Candidate directory paths for Windsurf & Cascade on the host machine.
    pub fn candidate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();

            // Codeium & Windsurf home config / storage
            roots.push(home.join(".codeium").join("windsurf"));
            roots.push(home.join(".windsurf"));
            roots.push(home.join(".codeium").join("cascade"));
            roots.push(home.join(".codeium").join("windsurf").join("cascade"));
            roots.push(home.join(".codeium").join("windsurf").join("memories"));
            roots.push(home.join(".codeium").join("windsurf").join("trajectories"));

            // macOS Application Support paths
            roots.push(
                home.join("Library")
                    .join("Application Support")
                    .join("Windsurf"),
            );
            roots.push(
                home.join("Library")
                    .join("Application Support")
                    .join("Windsurf")
                    .join("User")
                    .join("workspaceStorage"),
            );
            roots.push(
                home.join("Library")
                    .join("Application Support")
                    .join("Windsurf")
                    .join("User")
                    .join("globalStorage"),
            );
            roots.push(
                home.join("Library")
                    .join("Application Support")
                    .join("Codeium"),
            );

            // Linux / standard config paths
            roots.push(home.join(".config").join("Windsurf"));
            roots.push(
                home.join(".config")
                    .join("Windsurf")
                    .join("User")
                    .join("workspaceStorage"),
            );
            roots.push(home.join(".config").join("Codeium"));
        }

        // Relative paths
        roots.push(PathBuf::from(".windsurf"));
        roots.push(PathBuf::from(".codeium"));
        roots.push(PathBuf::from(".codeium").join("cascade"));
        roots
    }
}

impl AgentAdapter for WindsurfAdapter {
    fn name(&self) -> &'static str {
        "windsurf"
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
                if s.contains("windsurf") || s.contains("codeium") || s.contains("cascade") || custom.is_file() {
                    discovered.push(custom.clone());
                } else if custom.is_dir() {
                    for entry in WalkDir::new(custom).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_windsurf_file(path) {
                            discovered.push(path.to_path_buf());
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
                    if is_candidate_windsurf_file(custom) || custom.exists() {
                        if let Ok(source) = SessionSource::from_path(custom, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if custom.is_dir() {
                    for entry in WalkDir::new(custom).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_windsurf_file(path) {
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
                    if is_candidate_windsurf_file(&root) {
                        if let Ok(source) = SessionSource::from_path(&root, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if root.is_dir() {
                    for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_windsurf_file(path) {
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

            // 1. Array or JSON Object fallback (e.g. cascade_history.json, session.json)
            if trimmed.starts_with('[')
                || (trimmed.starts_with('{') && !trimmed.contains('\n'))
                || (trimmed.starts_with('{') && serde_json::from_str::<Value>(trimmed).is_ok())
            {
                if let Ok(json_val) = serde_json::from_str::<Value>(trimmed) {
                    let items = if let Some(arr) = json_val.as_array() {
                        arr.clone()
                    } else if let Some(steps) = json_val
                        .get("steps")
                        .or_else(|| json_val.get("trajectories"))
                        .or_else(|| json_val.get("messages"))
                        .or_else(|| json_val.get("turns"))
                        .or_else(|| json_val.get("history"))
                        .or_else(|| json_val.get("conversation"))
                        .or_else(|| json_val.get("events"))
                        .and_then(|s| s.as_array())
                    {
                        steps.clone()
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

                        let evts = parse_windsurf_record(item, &mut sequence, timestamp, idx + 1, &mut last_model);
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

            // 2. Line-by-line JSONL streaming
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

                let events = parse_windsurf_record(&val, &mut sequence, timestamp, line_num, &mut last_model);
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

fn is_candidate_windsurf_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    if !path_str.contains("windsurf") && !path_str.contains("codeium") && !path_str.contains("cascade") {
        return false;
    }
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let lower = filename.to_lowercase();

    if lower == "settings.json"
        || lower == "extensions.json"
        || lower == "keybindings.json"
        || lower == "globalstate.json"
        || lower == "state.vscdb"
    {
        return false;
    }

    if lower == "cascade_history.json"
        || lower == "session.json"
        || lower == "trajectories.jsonl"
        || lower == "chat_history.json"
        || lower == "state.json"
        || lower.ends_with(".json")
        || lower.ends_with(".jsonl")
        || lower.ends_with(".log")
    {
        return true;
    }

    path.extension()
        .is_some_and(|ext| ext == "json" || ext == "jsonl" || ext == "log")
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
    if let Some(ts_str) = val
        .get("created_at")
        .or_else(|| val.get("time"))
        .and_then(|v| v.as_str())
    {
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    if let Some(ts_num) = val.get("ts").or_else(|| val.get("timestamp")).and_then(|v| v.as_i64()) {
        if ts_num > 1_000_000_000_000 {
            return DateTime::from_timestamp_millis(ts_num);
        } else {
            return DateTime::from_timestamp(ts_num, 0);
        }
    }
    None
}

/// Parses a Windsurf / Cascade execution turn, step, or message record.
fn parse_windsurf_record(
    val: &Value,
    sequence: &mut u64,
    timestamp: DateTime<Utc>,
    line_num: usize,
    last_model: &mut Option<String>,
) -> Vec<NormalizedEvent> {
    let mut events = Vec::new();
    let raw_ref = format!("line:{}", line_num);

    let role = val
        .get("role")
        .or_else(|| val.get("type"))
        .or_else(|| val.get("sender"))
        .and_then(|r| r.as_str())
        .unwrap_or("");

    let content_text = if let Some(content) = val.get("content").and_then(|c| c.as_str()) {
        content.to_string()
    } else if let Some(msg) = val.get("message").and_then(|m| m.as_str()) {
        msg.to_string()
    } else if let Some(text) = val.get("text").and_then(|t| t.as_str()) {
        text.to_string()
    } else if let Some(prompt) = val.get("prompt").and_then(|p| p.as_str()) {
        prompt.to_string()
    } else {
        String::new()
    };

    // 1. User Turn
    if role == "user" || role == "human" {
        if !content_text.is_empty() {
            *sequence += 1;
            events.push(
                NormalizedEvent::new(
                    *sequence,
                    timestamp,
                    EventPayload::UserMessage {
                        content: content_text.clone(),
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    } else if role == "assistant" || role == "model" || role == "cascade" || role == "agent" {
        let thinking = val
            .get("thinking")
            .or_else(|| val.get("reasoning"))
            .or_else(|| val.get("thought"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string());

        if !content_text.is_empty() || thinking.is_some() {
            *sequence += 1;
            events.push(
                NormalizedEvent::new(
                    *sequence,
                    timestamp,
                    EventPayload::AssistantMessage {
                        content: content_text.clone(),
                        thinking,
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    // 2. Cascade Tool Calls / Actions
    if let Some(tool_calls) = val
        .get("tool_calls")
        .or_else(|| val.get("actions"))
        .or_else(|| val.get("steps"))
        .and_then(|t| t.as_array())
    {
        for tc in tool_calls {
            let name = tc
                .get("name")
                .or_else(|| tc.get("tool"))
                .or_else(|| tc.get("action"))
                .and_then(|n| n.as_str())
                .unwrap_or("unknown_tool");

            let args = tc
                .get("arguments")
                .or_else(|| tc.get("args"))
                .or_else(|| tc.get("input"))
                .cloned()
                .unwrap_or(Value::Null);

            let norm_name = normalize_mcp_tool_name(name, &args);

            // Handle specific tool action types
            if name == "run_command" || name == "execute_command" || name == "terminal_command" || name == "run_in_terminal" {
                let cmd = args
                    .get("command")
                    .or_else(|| args.get("cmd"))
                    .and_then(|c| c.as_str())
                    .unwrap_or("");

                *sequence += 1;
                events.push(
                    NormalizedEvent::new(
                        *sequence,
                        timestamp,
                        EventPayload::ShellCommand(ShellCommand {
                            command: cmd.to_string(),
                            cwd: args.get("cwd").and_then(|c| c.as_str()).map(|s| s.to_string()),
                            exit_code: None,
                            output: None,
                        }),
                    )
                    .with_raw_ref(&raw_ref),
                );
            } else if name == "view_file" || name == "read_file" || name == "open_file" {
                let path = args
                    .get("path")
                    .or_else(|| args.get("file_path"))
                    .or_else(|| args.get("filepath"))
                    .and_then(|p| p.as_str())
                    .unwrap_or("");

                *sequence += 1;
                events.push(
                    NormalizedEvent::new(
                        *sequence,
                        timestamp,
                        EventPayload::FileAction {
                            path: path.to_string(),
                            action: FileActionType::Read,
                            diff: None,
                            lines_changed: None,
                        },
                    )
                    .with_raw_ref(&raw_ref),
                );
            } else if name == "edit_file" || name == "write_file" || name == "modify_file" || name == "patch_file" {
                let path = args
                    .get("path")
                    .or_else(|| args.get("file_path"))
                    .or_else(|| args.get("filepath"))
                    .and_then(|p| p.as_str())
                    .unwrap_or("");

                let diff = args.get("diff").and_then(|d| d.as_str()).map(|s| s.to_string());
                let action = if name == "write_file" { FileActionType::Write } else { FileActionType::Edit };

                *sequence += 1;
                events.push(
                    NormalizedEvent::new(
                        *sequence,
                        timestamp,
                        EventPayload::FileAction {
                            path: path.to_string(),
                            action,
                            diff,
                            lines_changed: None,
                        },
                    )
                    .with_raw_ref(&raw_ref),
                );

                *sequence += 1;
                events.push(NormalizedEvent::new(
                    *sequence,
                    timestamp,
                    EventPayload::OutcomeEvidence(OutcomeEvidence {
                        kind: OutcomeKind::ArtifactChanged,
                        summary: format!("Modified file {}", path),
                        confidence: 0.85,
                    }),
                ));
            }

            *sequence += 1;
            events.push(
                NormalizedEvent::new(
                    *sequence,
                    timestamp,
                    EventPayload::ToolCall(ToolCall {
                        id: tc.get("id").and_then(|i| i.as_str()).map(|s| s.to_string()),
                        name: norm_name,
                        arguments: args,
                    }),
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    // Direct single tool call object
    if let Some(tool_name) = val.get("tool").or_else(|| val.get("tool_name")).and_then(|t| t.as_str()) {
        let args = val.get("arguments").or_else(|| val.get("args")).cloned().unwrap_or(Value::Null);
        let norm_name = normalize_mcp_tool_name(tool_name, &args);

        *sequence += 1;
        events.push(
            NormalizedEvent::new(
                *sequence,
                timestamp,
                EventPayload::ToolCall(ToolCall {
                    id: val.get("id").and_then(|i| i.as_str()).map(|s| s.to_string()),
                    name: norm_name,
                    arguments: args,
                }),
            )
            .with_raw_ref(&raw_ref),
        );
    }

    // 3. Tool Results & Outputs
    if let Some(tool_results) = val.get("tool_results").or_else(|| val.get("results")).and_then(|r| r.as_array()) {
        for res in tool_results {
            let output = res.get("output").or_else(|| res.get("content")).cloned().unwrap_or(Value::Null);
            let is_error = res.get("is_error").or_else(|| res.get("error")).and_then(|e| e.as_bool()).unwrap_or(false);

            *sequence += 1;
            events.push(
                NormalizedEvent::new(
                    *sequence,
                    timestamp,
                    EventPayload::ToolResult(ToolResult {
                        call_id: res.get("call_id").or_else(|| res.get("id")).and_then(|i| i.as_str()).map(|s| s.to_string()),
                        name: res.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()),
                        output,
                        is_error,
                    }),
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    // Direct shell command property
    if let Some(cmd) = val.get("command").and_then(|c| c.as_str()) {
        let exit_code = val.get("exit_code").and_then(|e| e.as_i64()).map(|ec| ec as i32);
        let output = val.get("output").or_else(|| val.get("stdout")).and_then(|o| o.as_str()).map(|s| s.to_string());

        *sequence += 1;
        events.push(
            NormalizedEvent::new(
                *sequence,
                timestamp,
                EventPayload::ShellCommand(ShellCommand {
                    command: cmd.to_string(),
                    cwd: val.get("cwd").and_then(|c| c.as_str()).map(|s| s.to_string()),
                    exit_code,
                    output: output.clone(),
                }),
            )
            .with_raw_ref(&raw_ref),
        );

        if exit_code == Some(0) {
            if let Some(ref out) = output {
                if out.contains("test result: ok") || out.contains("PASSED") || out.contains("passed") {
                    *sequence += 1;
                    events.push(NormalizedEvent::new(
                        *sequence,
                        timestamp,
                        EventPayload::OutcomeEvidence(OutcomeEvidence {
                            kind: OutcomeKind::TestOrBuildPassed,
                            summary: format!("Test command passed: {}", cmd),
                            confidence: 0.9,
                        }),
                    ));
                }
            }
        }
    }

    // 4. Token Accounting & Model Invocations
    let model = val
        .get("model")
        .or_else(|| val.get("model_name"))
        .and_then(|m| m.as_str())
        .unwrap_or("windsurf-cascade");

    let usage = val.get("usage").or_else(|| val.get("tokens")).or_else(|| val.get("token_usage"));
    if let Some(u) = usage {
        let input_tokens = u
            .get("prompt_tokens")
            .or_else(|| u.get("input_tokens"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0);
        let output_tokens = u
            .get("completion_tokens")
            .or_else(|| u.get("output_tokens"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0);
        let cache_read = u
            .get("cache_read_tokens")
            .or_else(|| u.get("cache_read_input_tokens"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0);
        let cache_creation = u
            .get("cache_creation_tokens")
            .or_else(|| u.get("cache_creation_input_tokens"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0);
        let cost_usd = val.get("cost_usd").or_else(|| val.get("cost")).and_then(|c| c.as_f64());

        if input_tokens > 0 || output_tokens > 0 || cache_read > 0 || cache_creation > 0 || cost_usd.is_some() {
            if last_model.as_deref() != Some(model) {
                if let Some(prev) = last_model.take() {
                    *sequence += 1;
                    events.push(
                        NormalizedEvent::new(
                            *sequence,
                            timestamp,
                            EventPayload::ModelSwitch(ModelSwitch {
                                from_model: Some(prev),
                                to_model: model.to_string(),
                                reason: None,
                            }),
                        )
                        .with_raw_ref(&raw_ref),
                    );
                }
                *last_model = Some(model.to_string());
            }

            *sequence += 1;
            events.push(
                NormalizedEvent::new(
                    *sequence,
                    timestamp,
                    EventPayload::ModelInvocation {
                        model: model.to_string(),
                        token_usage: TokenUsage::new(
                            input_tokens,
                            output_tokens,
                            cache_read,
                            cache_creation,
                        ),
                        cost_usd,
                        latency_ms: None,
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    // 5. Completion or Outcome status
    if let Some(status) = val.get("status").and_then(|s| s.as_str()) {
        if status == "completed" || status == "success" || status == "done" {
            *sequence += 1;
            events.push(
                NormalizedEvent::new(
                    *sequence,
                    timestamp,
                    EventPayload::OutcomeEvidence(OutcomeEvidence {
                        kind: OutcomeKind::DoneClaimed,
                        summary: "Cascade task completed".to_string(),
                        confidence: 0.8,
                    }),
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_detect_and_enumerate_windsurf() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let content =
            r#"{"role":"user","content":"Build an API server in Rust","timestamp":"2024-05-18T10:00:00Z"}"#;
        writeln!(temp_file, "{}", content).unwrap();

        let adapter = WindsurfAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp_file.path().to_path_buf()],
            force: true,
        };

        let detection = adapter.detect(&options).unwrap();
        assert!(detection.is_present);
        assert_eq!(detection.adapter_name, "windsurf");

        let sources = adapter.enumerate(&options).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].adapter_name, "windsurf");
    }

    #[test]
    fn test_parse_windsurf_cascade_trajectory_jsonl() {
        let content = r#"{"role":"user","content":"Fix the database connection timeout","timestamp":"2024-05-18T10:00:00Z"}
{"role":"assistant","content":"I will inspect db.rs and increase the timeout setting.","thinking":"Reviewing pool configuration","model":"claude-3-5-sonnet","usage":{"prompt_tokens":1800,"completion_tokens":220},"timestamp":"2024-05-18T10:00:05Z"}
{"tool_calls":[{"name":"edit_file","arguments":{"path":"src/db.rs","diff":"--- a/src/db.rs\n+++ b/src/db.rs\n@@ -1 +1,2 @@\n-timeout: 5\n+timeout: 30\n"}}],"timestamp":"2024-05-18T10:00:10Z"}
{"command":"cargo test","exit_code":0,"stdout":"test result: ok. 8 passed; 0 failed","timestamp":"2024-05-18T10:00:15Z"}
{"status":"completed","timestamp":"2024-05-18T10:00:20Z"}
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "{}", content).unwrap();

        let adapter = WindsurfAdapter::new();
        let source = SessionSource::from_path(temp_file.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).unwrap();

        assert_eq!(result.malformed_lines, 0);
        assert_eq!(result.trace.adapter, "windsurf");
        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
        assert_eq!(result.trace.stats.token_usage.input_tokens, 1800);
        assert_eq!(result.trace.stats.token_usage.output_tokens, 220);

        // Check outcomes
        let has_artifact = result.trace.events.iter().any(|e| {
            matches!(
                &e.payload,
                EventPayload::OutcomeEvidence(OutcomeEvidence {
                    kind: OutcomeKind::ArtifactChanged,
                    ..
                })
            )
        });
        assert!(has_artifact);

        let has_test_pass = result.trace.events.iter().any(|e| {
            matches!(
                &e.payload,
                EventPayload::OutcomeEvidence(OutcomeEvidence {
                    kind: OutcomeKind::TestOrBuildPassed,
                    ..
                })
            )
        });
        assert!(has_test_pass);

        let has_done = result.trace.events.iter().any(|e| {
            matches!(
                &e.payload,
                EventPayload::OutcomeEvidence(OutcomeEvidence {
                    kind: OutcomeKind::DoneClaimed,
                    ..
                })
            )
        });
        assert!(has_done);
    }

    #[test]
    fn test_parse_windsurf_mcp_tool_call() {
        let content = r#"{
  "steps": [
    {
      "role": "assistant",
      "tool_calls": [
        {
          "name": "call_mcp_tool",
          "arguments": {
            "server_name": "filesystem",
            "tool_name": "list_directory"
          }
        }
      ]
    }
  ]
}"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "{}", content).unwrap();

        let adapter = WindsurfAdapter::new();
        let source = SessionSource::from_path(temp_file.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).unwrap();

        assert_eq!(result.malformed_lines, 0);
        let has_mcp = result.trace.events.iter().any(|e| {
            if let EventPayload::ToolCall(tc) = &e.payload {
                tc.name == "mcp:filesystem:list_directory"
            } else {
                false
            }
        });
        assert!(has_mcp);
    }

    #[test]
    fn test_parse_graceful_on_malformed_lines() {
        let content = r#"{"role":"user","content":"Start task","timestamp":"2024-05-18T10:00:00Z"}
{MALFORMED_JSON_LINE}
{"status":"completed","timestamp":"2024-05-18T10:00:30Z"}
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "{}", content).unwrap();

        let adapter = WindsurfAdapter::new();
        let source = SessionSource::from_path(temp_file.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).unwrap();

        assert!(result.malformed_lines >= 1);
        assert_eq!(result.trace.stats.user_messages_count, 1);
    }
}
