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

/// Adapter for discovering and normalizing Cline & Roo-Code VSCode agent task logs.
pub struct ClineAdapter;

impl Default for ClineAdapter {
    fn default() -> Self {
        Self
    }
}

impl ClineAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Candidate directory paths for Cline & Roo-Code tasks on the host machine.
    pub fn candidate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();

            // macOS VSCode & VSCodium paths
            roots.push(
                home.join("Library")
                    .join("Application Support")
                    .join("Code")
                    .join("User")
                    .join("globalStorage")
                    .join("saoudrizwan.claude-dev")
                    .join("tasks"),
            );
            roots.push(
                home.join("Library")
                    .join("Application Support")
                    .join("Code")
                    .join("User")
                    .join("globalStorage")
                    .join("rooveterinaryinc.roo-cline")
                    .join("tasks"),
            );
            roots.push(
                home.join("Library")
                    .join("Application Support")
                    .join("Code")
                    .join("User")
                    .join("globalStorage")
                    .join("cline.cline")
                    .join("tasks"),
            );
            roots.push(
                home.join("Library")
                    .join("Application Support")
                    .join("VSCodium")
                    .join("User")
                    .join("globalStorage")
                    .join("saoudrizwan.claude-dev")
                    .join("tasks"),
            );
            roots.push(
                home.join("Library")
                    .join("Application Support")
                    .join("VSCodium")
                    .join("User")
                    .join("globalStorage")
                    .join("rooveterinaryinc.roo-cline")
                    .join("tasks"),
            );

            // Linux / standard config paths
            roots.push(
                home.join(".config")
                    .join("Code")
                    .join("User")
                    .join("globalStorage")
                    .join("saoudrizwan.claude-dev")
                    .join("tasks"),
            );
            roots.push(
                home.join(".config")
                    .join("Code")
                    .join("User")
                    .join("globalStorage")
                    .join("rooveterinaryinc.roo-cline")
                    .join("tasks"),
            );
            roots.push(
                home.join(".config")
                    .join("Code - OSS")
                    .join("User")
                    .join("globalStorage")
                    .join("saoudrizwan.claude-dev")
                    .join("tasks"),
            );
            roots.push(
                home.join(".config")
                    .join("Code - OSS")
                    .join("User")
                    .join("globalStorage")
                    .join("rooveterinaryinc.roo-cline")
                    .join("tasks"),
            );
            roots.push(
                home.join(".vscode")
                    .join("globalStorage")
                    .join("saoudrizwan.claude-dev")
                    .join("tasks"),
            );
            roots.push(
                home.join(".vscode")
                    .join("globalStorage")
                    .join("rooveterinaryinc.roo-cline")
                    .join("tasks"),
            );

            // Generic / user home dirs
            roots.push(home.join(".cline").join("tasks"));
            roots.push(home.join(".roo-cline").join("tasks"));
        }

        // Relative paths
        roots.push(PathBuf::from(".cline"));
        roots.push(PathBuf::from(".cline").join("tasks"));
        roots.push(PathBuf::from(".roo-cline"));
        roots.push(PathBuf::from(".roo-cline").join("tasks"));
        roots
    }
}

impl AgentAdapter for ClineAdapter {
    fn name(&self) -> &'static str {
        "cline"
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
                if s.contains("claude-dev")
                    || s.contains("roo-cline")
                    || s.contains("cline")
                    || s.contains("saoudrizwan")
                    || custom.is_file()
                {
                    discovered.push(custom.clone());
                } else if custom.is_dir() {
                    for entry in WalkDir::new(custom).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_cline_file(path) {
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
                    if is_candidate_cline_file(custom) || custom.exists() {
                        if let Ok(source) = SessionSource::from_path(custom, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if custom.is_dir() {
                    for entry in WalkDir::new(custom).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_cline_file(path) {
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
                    if is_candidate_cline_file(&root) {
                        if let Ok(source) = SessionSource::from_path(&root, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if root.is_dir() {
                    for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_cline_file(path) {
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

            // 1. Array or JSON Object fallback (e.g. ui_messages.json or api_conversation_history.json)
            if trimmed.starts_with('[')
                || (trimmed.starts_with('{') && !trimmed.contains('\n'))
                || (trimmed.starts_with('{') && serde_json::from_str::<Value>(trimmed).is_ok())
            {
                if let Ok(json_val) = serde_json::from_str::<Value>(trimmed) {
                    let items = if let Some(arr) = json_val.as_array() {
                        arr.clone()
                    } else if let Some(messages) = json_val
                        .get("messages")
                        .or_else(|| json_val.get("ui_messages"))
                        .or_else(|| json_val.get("history"))
                        .or_else(|| json_val.get("tasks"))
                        .or_else(|| json_val.get("turns"))
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

                        let evts = parse_cline_record(item, &mut sequence, timestamp, idx + 1, &mut last_model);
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

                let events = parse_cline_record(&val, &mut sequence, timestamp, line_num, &mut last_model);
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

fn is_candidate_cline_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    if !path_str.contains("claude-dev")
        && !path_str.contains("roo-cline")
        && !path_str.contains("cline")
        && !path_str.contains("saoudrizwan")
        && !path_str.contains("rooveterinaryinc")
    {
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

    if lower == "ui_messages.json"
        || lower == "api_conversation_history.json"
        || lower == "task.json"
        || lower == "state.json"
        || lower.ends_with(".json")
        || lower.ends_with(".jsonl")
    {
        return true;
    }

    path.extension().is_some_and(|ext| ext == "json" || ext == "jsonl")
}

fn derive_session_id(path: &Path) -> String {
    // If the file is inside a task folder like tasks/<taskId>/ui_messages.json, use taskId
    if let Some(parent) = path.parent() {
        if let Some(folder_name) = parent.file_name().and_then(|f| f.to_str()) {
            if folder_name != "tasks" && !folder_name.is_empty() {
                return folder_name.to_string();
            }
        }
    }

    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

fn parse_timestamp(val: &Value) -> Option<DateTime<Utc>> {
    if let Some(ts_num) = val.get("ts").and_then(|v| v.as_i64()) {
        if ts_num > 1_000_000_000_000 {
            return DateTime::from_timestamp_millis(ts_num);
        } else {
            return DateTime::from_timestamp(ts_num, 0);
        }
    }
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
    if let Some(ts_num) = val.get("timestamp").and_then(|v| v.as_i64()) {
        if ts_num > 1_000_000_000_000 {
            return DateTime::from_timestamp_millis(ts_num);
        } else {
            return DateTime::from_timestamp(ts_num, 0);
        }
    }
    None
}

/// Helper to parse XML-style tool tags often used in Cline/Roo-Code prompts/completions.
fn extract_xml_tag_content(text: &str, tag: &str) -> Option<String> {
    let open_tag = format!("<{}>", tag);
    let close_tag = format!("</{}>", tag);
    if let Some(start_pos) = text.find(&open_tag) {
        let content_start = start_pos + open_tag.len();
        if let Some(end_pos) = text[content_start..].find(&close_tag) {
            return Some(text[content_start..content_start + end_pos].trim().to_string());
        }
    }
    None
}

/// Parses a Cline or Roo-Code message object (from ui_messages.json or api_conversation_history.json).
fn parse_cline_record(
    val: &Value,
    sequence: &mut u64,
    timestamp: DateTime<Utc>,
    line_num: usize,
    last_model: &mut Option<String>,
) -> Vec<NormalizedEvent> {
    let mut events = Vec::new();
    let raw_ref = format!("line:{}", line_num);

    let _msg_type = val.get("type").and_then(|t| t.as_str()).unwrap_or("");
    let say = val.get("say").and_then(|s| s.as_str()).unwrap_or("");
    let ask = val.get("ask").and_then(|a| a.as_str()).unwrap_or("");
    let role = val.get("role").and_then(|r| r.as_str()).unwrap_or("");

    let text_val = val.get("text").or_else(|| val.get("content"));
    let text_str = text_val
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();

    // 1. User Message Cases
    if say == "task"
        || say == "user_feedback"
        || say == "user_feedback_response"
        || ask == "followup"
        || role == "user"
        || role == "human"
    {
        let content = if !text_str.is_empty() {
            text_str.clone()
        } else if let Some(arr) = text_val.and_then(|v| v.as_array()) {
            arr.iter()
                .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        };

        if !content.is_empty() {
            *sequence += 1;
            events.push(
                NormalizedEvent::new(
                    *sequence,
                    timestamp,
                    EventPayload::UserMessage { content },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    // 2. Assistant Message Cases
    if (say == "text" || say == "reasoning" || role == "assistant") && say != "task" && say != "user_feedback" {
        let mut thinking: Option<String> = None;
        let mut content = text_str.clone();

        if let Some(t) = extract_xml_tag_content(&content, "thinking") {
            thinking = Some(t);
        }

        if let Some(arr) = text_val.and_then(|v| v.as_array()) {
            let mut parts = Vec::new();
            for block in arr {
                let b_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if b_type == "text" {
                    if let Some(txt) = block.get("text").and_then(|t| t.as_str()) {
                        parts.push(txt.to_string());
                    }
                } else if b_type == "thinking" {
                    if let Some(th) = block.get("thinking").and_then(|t| t.as_str()) {
                        thinking = Some(th.to_string());
                    }
                }
            }
            if !parts.is_empty() {
                content = parts.join("\n");
            }
        }

        if !content.is_empty() || thinking.is_some() {
            *sequence += 1;
            events.push(
                NormalizedEvent::new(
                    *sequence,
                    timestamp,
                    EventPayload::AssistantMessage { content, thinking },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    // 3. Tool Calls & Actions
    if say == "tool" || ask == "tool" || say == "use_mcp_tool" {
        // Try parsing `text` as JSON
        if let Ok(tool_json) = serde_json::from_str::<Value>(&text_str) {
            let tool_name = tool_json
                .get("tool")
                .or_else(|| tool_json.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("unknown_tool");

            let norm_name = normalize_mcp_tool_name(tool_name, &tool_json);

            // Check specific operations
            if tool_name == "readFile" || tool_name == "read_file" {
                let path = tool_json
                    .get("path")
                    .or_else(|| tool_json.get("filePath"))
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
            } else if tool_name == "writeToFile" || tool_name == "write_to_file" || tool_name == "newFile" {
                let path = tool_json
                    .get("path")
                    .or_else(|| tool_json.get("filePath"))
                    .and_then(|p| p.as_str())
                    .unwrap_or("");

                *sequence += 1;
                events.push(
                    NormalizedEvent::new(
                        *sequence,
                        timestamp,
                        EventPayload::FileAction {
                            path: path.to_string(),
                            action: FileActionType::Write,
                            diff: None,
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
                        summary: format!("Wrote to {}", path),
                        confidence: 0.85,
                    }),
                ));
            } else if tool_name == "replaceInFile" || tool_name == "apply_diff" || tool_name == "editFile" {
                let path = tool_json
                    .get("path")
                    .or_else(|| tool_json.get("filePath"))
                    .and_then(|p| p.as_str())
                    .unwrap_or("");

                let diff = tool_json.get("diff").and_then(|d| d.as_str()).map(|s| s.to_string());

                *sequence += 1;
                events.push(
                    NormalizedEvent::new(
                        *sequence,
                        timestamp,
                        EventPayload::FileAction {
                            path: path.to_string(),
                            action: FileActionType::Edit,
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
                        summary: format!("Edited {}", path),
                        confidence: 0.85,
                    }),
                ));
            } else if tool_name == "executeCommand" || tool_name == "execute_command" {
                let cmd = tool_json
                    .get("command")
                    .and_then(|c| c.as_str())
                    .unwrap_or("");

                *sequence += 1;
                events.push(
                    NormalizedEvent::new(
                        *sequence,
                        timestamp,
                        EventPayload::ShellCommand(ShellCommand {
                            command: cmd.to_string(),
                            cwd: None,
                            exit_code: None,
                            output: None,
                        }),
                    )
                    .with_raw_ref(&raw_ref),
                );
            }

            *sequence += 1;
            events.push(
                NormalizedEvent::new(
                    *sequence,
                    timestamp,
                    EventPayload::ToolCall(ToolCall {
                        id: val.get("tool_call_id").and_then(|i| i.as_str()).map(|s| s.to_string()),
                        name: norm_name,
                        arguments: tool_json,
                    }),
                )
                .with_raw_ref(&raw_ref),
            );
        } else {
            // Check XML tool tags
            if let Some(cmd) = extract_xml_tag_content(&text_str, "execute_command")
                .or_else(|| extract_xml_tag_content(&text_str, "command"))
            {
                *sequence += 1;
                events.push(
                    NormalizedEvent::new(
                        *sequence,
                        timestamp,
                        EventPayload::ShellCommand(ShellCommand {
                            command: cmd,
                            cwd: None,
                            exit_code: None,
                            output: None,
                        }),
                    )
                    .with_raw_ref(&raw_ref),
                );
            } else if let Some(path) = extract_xml_tag_content(&text_str, "read_file") {
                *sequence += 1;
                events.push(
                    NormalizedEvent::new(
                        *sequence,
                        timestamp,
                        EventPayload::FileAction {
                            path,
                            action: FileActionType::Read,
                            diff: None,
                            lines_changed: None,
                        },
                    )
                    .with_raw_ref(&raw_ref),
                );
            } else if let Some(path) = extract_xml_tag_content(&text_str, "write_to_file") {
                *sequence += 1;
                events.push(
                    NormalizedEvent::new(
                        *sequence,
                        timestamp,
                        EventPayload::FileAction {
                            path: path.clone(),
                            action: FileActionType::Write,
                            diff: None,
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
                        summary: format!("Wrote to {}", path),
                        confidence: 0.85,
                    }),
                ));
            } else {
                *sequence += 1;
                events.push(
                    NormalizedEvent::new(
                        *sequence,
                        timestamp,
                        EventPayload::ToolCall(ToolCall {
                            id: None,
                            name: normalize_mcp_tool_name(say, val),
                            arguments: serde_json::json!({ "text": text_str }),
                        }),
                    )
                    .with_raw_ref(&raw_ref),
                );
            }
        }
    }

    // 4. Anthropic content blocks with `tool_use` / `tool_result`
    if let Some(blocks) = text_val.and_then(|v| v.as_array()) {
        for block in blocks {
            let b_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if b_type == "tool_use" {
                let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                let input = block.get("input").cloned().unwrap_or(Value::Null);
                let norm_name = normalize_mcp_tool_name(name, &input);

                *sequence += 1;
                events.push(
                    NormalizedEvent::new(
                        *sequence,
                        timestamp,
                        EventPayload::ToolCall(ToolCall {
                            id: block.get("id").and_then(|i| i.as_str()).map(|s| s.to_string()),
                            name: norm_name,
                            arguments: input,
                        }),
                    )
                    .with_raw_ref(&raw_ref),
                );
            } else if b_type == "tool_result" {
                let call_id = block.get("tool_use_id").and_then(|i| i.as_str()).map(|s| s.to_string());
                let output = block.get("content").cloned().unwrap_or(Value::Null);
                let is_error = block.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false);

                *sequence += 1;
                events.push(
                    NormalizedEvent::new(
                        *sequence,
                        timestamp,
                        EventPayload::ToolResult(ToolResult {
                            call_id,
                            name: None,
                            output,
                            is_error,
                        }),
                    )
                    .with_raw_ref(&raw_ref),
                );
            }
        }
    }

    // 5. Command and Output Execution
    if say == "command" || ask == "command" {
        *sequence += 1;
        events.push(
            NormalizedEvent::new(
                *sequence,
                timestamp,
                EventPayload::ShellCommand(ShellCommand {
                    command: text_str.clone(),
                    cwd: None,
                    exit_code: None,
                    output: None,
                }),
            )
            .with_raw_ref(&raw_ref),
        );
    } else if say == "command_output" {
        let is_ok = !text_str.to_lowercase().contains("error") && !text_str.to_lowercase().contains("failed");

        *sequence += 1;
        events.push(
            NormalizedEvent::new(
                *sequence,
                timestamp,
                EventPayload::ToolResult(ToolResult {
                    call_id: None,
                    name: Some("command".to_string()),
                    output: Value::String(text_str.clone()),
                    is_error: !is_ok,
                }),
            )
            .with_raw_ref(&raw_ref),
        );

        if text_str.contains("test result: ok") || text_str.contains("PASSED") || text_str.contains("passed") {
            *sequence += 1;
            events.push(NormalizedEvent::new(
                *sequence,
                timestamp,
                EventPayload::OutcomeEvidence(OutcomeEvidence {
                    kind: OutcomeKind::TestOrBuildPassed,
                    summary: "Build / test command passed".to_string(),
                    confidence: 0.9,
                }),
            ));
        }
    }

    // 6. Completion & Outcomes
    if say == "completion_result" || ask == "completion_result" {
        *sequence += 1;
        events.push(
            NormalizedEvent::new(
                *sequence,
                timestamp,
                EventPayload::OutcomeEvidence(OutcomeEvidence {
                    kind: OutcomeKind::DoneClaimed,
                    summary: if text_str.is_empty() { "Task completed".to_string() } else { text_str.clone() },
                    confidence: 0.8,
                }),
            )
            .with_raw_ref(&raw_ref),
        );
    }

    // 7. Error events
    if say == "error" {
        *sequence += 1;
        events.push(
            NormalizedEvent::new(
                *sequence,
                timestamp,
                EventPayload::Error {
                    message: text_str.clone(),
                    is_recovered: false,
                },
            )
            .with_raw_ref(&raw_ref),
        );
    }

    // 8. Token Accounting & Model Invocations
    let mut tokens_in = val.get("tokensIn").and_then(|t| t.as_u64()).unwrap_or(0);
    let mut tokens_out = val.get("tokensOut").and_then(|t| t.as_u64()).unwrap_or(0);
    let mut cache_reads = val.get("cacheReads").and_then(|t| t.as_u64()).unwrap_or(0);
    let mut cache_writes = val.get("cacheWrites").and_then(|t| t.as_u64()).unwrap_or(0);
    let cost_usd = val.get("totalCost").or_else(|| val.get("cost")).and_then(|c| c.as_f64());

    if let Some(usage) = val.get("usage") {
        if let Some(inp) = usage.get("input_tokens").and_then(|t| t.as_u64()) {
            tokens_in = inp;
        }
        if let Some(out) = usage.get("output_tokens").and_then(|t| t.as_u64()) {
            tokens_out = out;
        }
        if let Some(cr) = usage.get("cache_read_input_tokens").and_then(|t| t.as_u64()) {
            cache_reads = cr;
        }
        if let Some(cw) = usage.get("cache_creation_input_tokens").and_then(|t| t.as_u64()) {
            cache_writes = cw;
        }
    }

    let model = val
        .get("model")
        .or_else(|| val.get("apiProvider"))
        .and_then(|m| m.as_str())
        .unwrap_or("cline");

    if tokens_in > 0 || tokens_out > 0 || cache_reads > 0 || cache_writes > 0 || cost_usd.is_some() {
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
                    token_usage: TokenUsage::new(tokens_in, tokens_out, cache_reads, cache_writes),
                    cost_usd,
                    latency_ms: None,
                },
            )
            .with_raw_ref(&raw_ref),
        );
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_detect_and_enumerate_cline() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(
            temp_file,
            "{}",
            r#"[
  {
    "ts": 1716000000000,
    "type": "say",
    "say": "task",
    "text": "Fix user auth bug",
    "tokensIn": 1500,
    "tokensOut": 250,
    "totalCost": 0.015
  }
]"#
        )
        .unwrap();

        let adapter = ClineAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp_file.path().to_path_buf()],
            force: true,
            ..Default::default()
        };

        let detection = adapter.detect(&options).unwrap();
        assert!(detection.is_present);
        assert_eq!(detection.adapter_name, "cline");

        let sources = adapter.enumerate(&options).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].adapter_name, "cline");
    }

    #[test]
    fn test_parse_cline_ui_messages_json() {
        let content = r#"[
  {
    "ts": 1716000000000,
    "type": "say",
    "say": "task",
    "text": "Please refactor the billing service"
  },
  {
    "ts": 1716000005000,
    "type": "say",
    "say": "text",
    "text": "I will examine billing.ts first.<thinking>Need to inspect payment webhook</thinking>",
    "tokensIn": 2400,
    "tokensOut": 320,
    "cacheReads": 100,
    "cacheWrites": 50,
    "totalCost": 0.02
  },
  {
    "ts": 1716000010000,
    "type": "ask",
    "ask": "tool",
    "say": "tool",
    "text": "{\"tool\":\"writeToFile\",\"path\":\"src/billing.ts\",\"content\":\"export const charge = () => {};\"}"
  },
  {
    "ts": 1716000015000,
    "type": "say",
    "say": "command",
    "text": "npm test"
  },
  {
    "ts": 1716000020000,
    "type": "say",
    "say": "command_output",
    "text": "test result: ok. 12 passed"
  },
  {
    "ts": 1716000025000,
    "type": "say",
    "say": "completion_result",
    "text": "Successfully refactored billing service."
  }
]"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "{}", content).unwrap();

        let adapter = ClineAdapter::new();
        let source = SessionSource::from_path(temp_file.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).unwrap();

        assert_eq!(result.malformed_lines, 0);
        assert_eq!(result.trace.adapter, "cline");
        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
        assert_eq!(result.trace.stats.token_usage.input_tokens, 2400);
        assert_eq!(result.trace.stats.token_usage.output_tokens, 320);
        assert_eq!(result.trace.stats.token_usage.cache_read_tokens, 100);
        assert_eq!(result.trace.stats.token_usage.cache_creation_tokens, 50);

        // Verify artifact changed and test passed evidence
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
    fn test_parse_cline_mcp_tool_calls() {
        let content = r#"[
  {
    "ts": 1716000000000,
    "type": "say",
    "say": "tool",
    "text": "{\"tool\":\"use_mcp_tool\",\"server_name\":\"postgres\",\"tool_name\":\"execute_sql\",\"arguments\":{\"query\":\"SELECT 1;\"}}"
  }
]"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "{}", content).unwrap();

        let adapter = ClineAdapter::new();
        let source = SessionSource::from_path(temp_file.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).unwrap();

        assert_eq!(result.malformed_lines, 0);
        let has_mcp = result.trace.events.iter().any(|e| {
            if let EventPayload::ToolCall(tc) = &e.payload {
                tc.name == "mcp:postgres:execute_sql"
            } else {
                false
            }
        });
        assert!(has_mcp);
    }

    #[test]
    fn test_parse_graceful_on_malformed_lines() {
        let content = r#"{"ts": 1716000000000, "type": "say", "say": "task", "text": "Valid task"}
{INVALID_JSON_CORRUPT}
{"ts": 1716000010000, "type": "say", "say": "completion_result", "text": "Completed"}
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "{}", content).unwrap();

        let adapter = ClineAdapter::new();
        let source = SessionSource::from_path(temp_file.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).unwrap();

        assert!(result.malformed_lines >= 1);
        assert_eq!(result.trace.stats.user_messages_count, 1);
    }

    #[test]
    fn test_parse_cline_detects_model_switch_mid_task() {
        // Cline lets a user swap the underlying model mid-task (e.g. dropping from a
        // frontier model to a cheaper one to finish mechanical work). Each `say`
        // record with usage carries its own `model`/`apiProvider`, which is exactly
        // the per-event identity `ModelSwitch` detection needs.
        let content = r#"[
  {
    "ts": 1716000000000,
    "type": "say",
    "say": "api_req_started",
    "model": "claude-opus-5",
    "tokensIn": 1500,
    "tokensOut": 250,
    "totalCost": 0.045
  },
  {
    "ts": 1716000010000,
    "type": "say",
    "say": "api_req_started",
    "model": "claude-haiku-5",
    "tokensIn": 400,
    "tokensOut": 90,
    "totalCost": 0.002
  }
]"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "{}", content).unwrap();

        let adapter = ClineAdapter::new();
        let source = SessionSource::from_path(temp_file.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).unwrap();

        assert_eq!(result.malformed_lines, 0);
        let trace = result.trace;

        assert_eq!(
            trace.stats.models_used,
            vec!["claude-opus-5".to_string(), "claude-haiku-5".to_string()]
        );

        let switches: Vec<_> = trace
            .events
            .iter()
            .filter_map(|e| match &e.payload {
                EventPayload::ModelSwitch(ms) => Some((ms.from_model.clone(), ms.to_model.clone())),
                _ => None,
            })
            .collect();

        assert_eq!(
            switches,
            vec![(
                Some("claude-opus-5".to_string()),
                "claude-haiku-5".to_string()
            )]
        );

        // No switch is recorded for the very first model used in the session.
        let first_invocation_idx = trace
            .events
            .iter()
            .position(|e| matches!(&e.payload, EventPayload::ModelInvocation { .. }))
            .expect("expected at least one ModelInvocation");
        assert!(matches!(
            &trace.events[first_invocation_idx].payload,
            EventPayload::ModelInvocation { model, .. } if model == "claude-opus-5"
        ));
        assert!(first_invocation_idx == 0 || !matches!(
            &trace.events[first_invocation_idx - 1].payload,
            EventPayload::ModelSwitch(_)
        ));
    }
}
