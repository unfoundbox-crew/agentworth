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
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::normalize_mcp_tool_name;

/// Adapter for discovering and normalizing OpenCode agent session histories.
pub struct OpenCodeAdapter;

impl Default for OpenCodeAdapter {
    fn default() -> Self {
        Self
    }
}

impl OpenCodeAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Candidate directory paths for OpenCode on the host machine.
    pub fn candidate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".local").join("share").join("opencode").join("opencode.db"));
            roots.push(home.join(".local").join("share").join("opencode"));
            roots.push(home.join(".opencode").join("sessions"));
            roots.push(home.join(".opencode"));
            roots.push(home.join(".config").join("opencode"));
        }
        roots.push(PathBuf::from(".opencode"));
        roots
    }
}

impl AgentAdapter for OpenCodeAdapter {
    fn name(&self) -> &'static str {
        "opencode"
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
                && (custom.ends_with(".opencode") || custom.to_string_lossy().contains("opencode"))
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
        let mut db_paths = Vec::new();
        let mut dir_roots = Vec::new();

        let roots_to_scan = if !options.custom_paths.is_empty() {
            options.custom_paths.clone()
        } else {
            self.candidate_roots()
        };

        for root in roots_to_scan {
            if root.is_file() {
                if root.file_name().and_then(|n| n.to_str()) == Some("opencode.db") {
                    db_paths.push(root);
                } else if is_candidate_opencode_file(&root) {
                    if let Ok(source) = SessionSource::from_path(&root, self.name()) {
                        sources.push(source);
                    }
                }
            } else if root.is_dir() {
                let db_file = root.join("opencode.db");
                if db_file.exists() {
                    db_paths.push(db_file);
                } else {
                    dir_roots.push(root);
                }
            }
        }

        db_paths.sort();
        db_paths.dedup();

        for db_path in db_paths {
            if let Ok(conn) = Connection::open_with_flags(
                &db_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
            ) {
                if let Ok(mut stmt) =
                    conn.prepare("SELECT id, time_created, time_updated FROM session")
                {
                    let session_rows = stmt.query_map([], |row| {
                        let id: String = row.get(0)?;
                        let time_created: i64 = row.get(1).unwrap_or(0);
                        let time_updated: i64 = row.get(2).unwrap_or(time_created);
                        Ok((id, time_created, time_updated))
                    });

                    if let Ok(rows) = session_rows {
                        for row in rows.flatten() {
                            let (session_id, _created, updated) = row;
                            let virtual_path =
                                PathBuf::from(format!("{}#{}", db_path.display(), session_id));
                            let mtime_secs = if updated > 1_000_000_000_000 {
                                updated / 1000
                            } else {
                                updated
                            };
                            let mut hasher = Sha256::new();
                            hasher.update(session_id.as_bytes());
                            hasher.update(updated.to_string().as_bytes());
                            let fingerprint = hex::encode(hasher.finalize());

                            sources.push(SessionSource {
                                path: virtual_path,
                                adapter_name: self.name().to_string(),
                                file_size_bytes: 4096,
                                mtime_epoch_secs: mtime_secs,
                                fingerprint,
                            });
                        }
                    }
                }
            }
        }

        for root in dir_roots {
            for entry in WalkDir::new(&root)
                .into_iter()
                .filter_entry(|e| {
                    let name = e.file_name().to_string_lossy();
                    name != "node_modules" && name != ".git" && name != "target" && name != "dist"
                })
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if path.is_file() && is_candidate_opencode_file(path) {
                    if let Ok(source) = SessionSource::from_path(path, self.name()) {
                        sources.push(source);
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
        let path_str = source.path.to_string_lossy();
        if path_str.contains(".db#") {
            let mut parts = path_str.splitn(2, '#');
            if let (Some(db_str), Some(session_id)) = (parts.next(), parts.next()) {
                return parse_opencode_sqlite_session(
                    Path::new(db_str),
                    session_id,
                    source,
                    self.name(),
                );
            }
        }

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
                    } else if let Some(history) = json_val.get("history").and_then(|h| h.as_array()) {
                        history.clone()
                    } else if let Some(conversation) =
                        json_val.get("conversation").and_then(|c| c.as_array())
                    {
                        conversation.clone()
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

                        let evts = parse_opencode_record(item, &mut sequence, timestamp, idx + 1);
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

                let events = parse_opencode_record(&val, &mut sequence, timestamp, line_num);
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

fn parse_opencode_sqlite_session(
    db_path: &Path,
    session_id: &str,
    source: &SessionSource,
    adapter_name: &'static str,
) -> Result<ParseResult> {
    let provenance = Provenance::new(
        source.path.to_string_lossy().to_string(),
        adapter_name,
        source.file_size_bytes,
        source.mtime_epoch_secs,
        &source.fingerprint,
    );

    let mut trace = AgentWorthTrace::new(session_id, adapter_name, provenance, Utc::now());
    let mut malformed_lines = 0;
    let mut warnings = Vec::new();
    let mut sequence = 0u64;

    let mut earliest_ts: Option<DateTime<Utc>> = None;
    let mut latest_ts: Option<DateTime<Utc>> = None;

    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?;

    // 1. Fetch messages for this session
    let mut msg_stmt = conn.prepare(
        "SELECT id, time_created, data FROM message WHERE session_id = ? ORDER BY time_created ASC",
    )?;

    let msg_rows = msg_stmt.query_map([session_id], |row| {
        let msg_id: String = row.get(0)?;
        let time_created: i64 = row.get(1).unwrap_or(0);
        let data: String = row.get(2)?;
        Ok((msg_id, time_created, data))
    })?;

    let messages: Vec<(String, i64, String)> = msg_rows.filter_map(|r| r.ok()).collect();

    for (msg_id, time_created, data_str) in messages {
        let msg_val: Value = match serde_json::from_str(&data_str) {
            Ok(v) => v,
            Err(e) => {
                malformed_lines += 1;
                warnings.push(format!("Failed parsing message data: {}", e));
                continue;
            }
        };

        let ts_millis = if time_created > 1_000_000_000_000 {
            time_created
        } else {
            time_created * 1000
        };
        let timestamp = DateTime::from_timestamp_millis(ts_millis).unwrap_or_else(Utc::now);

        if earliest_ts.is_none_or(|ts| timestamp < ts) {
            earliest_ts = Some(timestamp);
        }
        if latest_ts.is_none_or(|ts| timestamp > ts) {
            latest_ts = Some(timestamp);
        }

        let role = msg_val.get("role").and_then(|r| r.as_str()).unwrap_or("unknown");
        let model_id = msg_val
            .get("modelID")
            .or_else(|| msg_val.get("model").and_then(|m| m.get("modelID")))
            .and_then(|m| m.as_str());

        // Extract tokens
        if let Some(tokens_val) = msg_val.get("tokens") {
            let input = tokens_val.get("input").and_then(|v| v.as_u64()).unwrap_or(0);
            let output = tokens_val.get("output").and_then(|v| v.as_u64()).unwrap_or(0);
            let cache_read = tokens_val
                .get("cache")
                .and_then(|c| c.get("read"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cache_write = tokens_val
                .get("cache")
                .and_then(|c| c.get("write"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            let usage = TokenUsage {
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: cache_read,
                cache_creation_tokens: cache_write,
            };

            if let Some(model) = model_id {
                sequence += 1;
                trace.events.push(NormalizedEvent::new(
                    sequence,
                    timestamp,
                    EventPayload::ModelInvocation {
                        model: model.to_string(),
                        token_usage: usage,
                        cost_usd: msg_val.get("cost").and_then(|c| c.as_f64()),
                        latency_ms: None,
                    },
                ));
            }
        }

        // Summary diffs
        if let Some(diffs) = msg_val
            .get("summary")
            .and_then(|s| s.get("diffs"))
            .and_then(|d| d.as_array())
        {
            for diff in diffs {
                let file = diff.get("file").and_then(|f| f.as_str()).unwrap_or("");
                let patch = diff.get("patch").and_then(|p| p.as_str()).map(String::from);
                let adds = diff.get("additions").and_then(|a| a.as_u64()).unwrap_or(0);
                let dels = diff.get("deletions").and_then(|d| d.as_u64()).unwrap_or(0);

                if !file.is_empty() {
                    sequence += 1;
                    trace.events.push(NormalizedEvent::new(
                        sequence,
                        timestamp,
                        EventPayload::FileAction {
                            action: FileActionType::Edit,
                            path: file.to_string(),
                            diff: patch,
                            lines_changed: Some(adds + dels),
                        },
                    ));
                }
            }
        }

        // 2. Fetch parts for this message
        let mut part_stmt =
            conn.prepare("SELECT id, data FROM part WHERE message_id = ? ORDER BY id ASC")?;
        let part_rows = part_stmt.query_map([&msg_id], |row| {
            let _part_id: String = row.get(0)?;
            let part_data: String = row.get(1)?;
            Ok(part_data)
        })?;

        for part_str in part_rows.filter_map(|r| r.ok()) {
            let part_val: Value = match serde_json::from_str(&part_str) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let part_type = part_val
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("");

            match part_type {
                "text" => {
                    let text = part_val.get("text").and_then(|t| t.as_str()).unwrap_or("");
                    if !text.is_empty() {
                        sequence += 1;
                        if role == "user" {
                            trace.events.push(NormalizedEvent::new(
                                sequence,
                                timestamp,
                                EventPayload::UserMessage {
                                    content: text.to_string(),
                                },
                            ));
                        } else {
                            trace.events.push(NormalizedEvent::new(
                                sequence,
                                timestamp,
                                EventPayload::AssistantMessage {
                                    content: text.to_string(),
                                    thinking: None,
                                },
                            ));
                        }
                    }
                }
                "thought" | "reasoning" => {
                    let thought = part_val
                        .get("thought")
                        .or_else(|| part_val.get("text"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("");
                    if !thought.is_empty() {
                        sequence += 1;
                        trace.events.push(NormalizedEvent::new(
                            sequence,
                            timestamp,
                            EventPayload::AssistantMessage {
                                content: String::new(),
                                thinking: Some(thought.to_string()),
                            },
                        ));
                    }
                }
                "tool" => {
                    let raw_tool = part_val.get("tool").and_then(|t| t.as_str()).unwrap_or("");
                    let call_id = part_val
                        .get("callID")
                        .and_then(|c| c.as_str())
                        .unwrap_or(&msg_id)
                        .to_string();

                    let state = part_val.get("state");
                    let input = state
                        .and_then(|s| s.get("input"))
                        .cloned()
                        .unwrap_or(Value::Null);
                    let output = state
                        .and_then(|s| s.get("output"))
                        .and_then(|o| o.as_str())
                        .unwrap_or("");
                    let status = state
                        .and_then(|s| s.get("status"))
                        .and_then(|s| s.as_str())
                        .unwrap_or("completed");
                    let is_error = status == "error" || status == "failed";

                    let normalized_name = normalize_mcp_tool_name(raw_tool, &input);

                    sequence += 1;
                    trace.events.push(NormalizedEvent::new(
                        sequence,
                        timestamp,
                        EventPayload::ToolCall(ToolCall {
                            id: Some(call_id.clone()),
                            name: normalized_name.clone(),
                            arguments: input.clone(),
                        }),
                    ));

                    if raw_tool == "bash" || raw_tool == "exec" || raw_tool == "shell" {
                        let cmd = input
                            .get("command")
                            .or_else(|| input.get("cmd"))
                            .and_then(|c| c.as_str())
                            .unwrap_or("");
                        if !cmd.is_empty() {
                            sequence += 1;
                            trace.events.push(NormalizedEvent::new(
                                sequence,
                                timestamp,
                                EventPayload::ShellCommand(ShellCommand {
                                    command: cmd.to_string(),
                                    cwd: None,
                                    exit_code: if is_error { Some(1) } else { Some(0) },
                                    output: Some(output.to_string()),
                                }),
                            ));
                        }
                    } else if raw_tool == "edit" || raw_tool == "write" || raw_tool == "patch" {
                        let path = input
                            .get("path")
                            .or_else(|| input.get("file"))
                            .and_then(|p| p.as_str())
                            .unwrap_or("");
                        if !path.is_empty() {
                            sequence += 1;
                            trace.events.push(NormalizedEvent::new(
                                sequence,
                                timestamp,
                                EventPayload::FileAction {
                                    action: if raw_tool == "write" {
                                        FileActionType::Write
                                    } else {
                                        FileActionType::Edit
                                    },
                                    path: path.to_string(),
                                    diff: None,
                                    lines_changed: None,
                                },
                            ));
                        }
                    }

                    sequence += 1;
                    trace.events.push(NormalizedEvent::new(
                        sequence,
                        timestamp,
                        EventPayload::ToolResult(ToolResult {
                            call_id: Some(call_id),
                            name: Some(normalized_name),
                            output: Value::String(output.to_string()),
                            is_error,
                        }),
                    ));
                }
                _ => {}
            }
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

fn is_candidate_opencode_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    if !path_str.contains("opencode") {
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
        || lower == "package-lock.json"
        || lower == "tsconfig.json"
        || lower == "manifest.json"
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

fn parse_opencode_record(
    val: &Value,
    seq: &mut u64,
    ts: DateTime<Utc>,
    line_num: usize,
) -> Vec<NormalizedEvent> {
    let mut events = Vec::new();
    let raw_ref = format!("line:{}", line_num);

    let event_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let role = val.get("role").and_then(|v| v.as_str()).unwrap_or("");

    // Model invocation / Token usage extraction
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
                .unwrap_or("opencode-model")
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
                        latency_ms: val.get("latency_ms").and_then(|d| d.as_u64()),
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    match event_type {
        "user" | "user_message" => {
            let content = extract_opencode_content(val);
            *seq += 1;
            events.push(
                NormalizedEvent::new(*seq, ts, EventPayload::UserMessage { content })
                    .with_raw_ref(&raw_ref),
            );
        }

        "assistant" | "assistant_message" => {
            let thinking = val
                .get("thinking")
                .or_else(|| val.get("reasoning"))
                .and_then(|v| v.as_str())
                .map(String::from);

            let content = extract_opencode_content(val);

            // Extract tool calls if present in assistant message
            if let Some(tcs) = val
                .get("tool_calls")
                .or_else(|| val.get("tools"))
                .and_then(|v| v.as_array())
            {
                for tc in tcs {
                    let id = tc.get("id").and_then(|v| v.as_str()).map(String::from);
                    let raw_name = tc
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let args = tc
                        .get("arguments")
                        .or_else(|| tc.get("input"))
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

                    process_specific_opencode_tool_call(
                        &raw_name,
                        &name,
                        &args,
                        seq,
                        ts,
                        &raw_ref,
                        &mut events,
                    );
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
            let raw_name = val
                .get("name")
                .or_else(|| val.get("tool"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let args = val
                .get("arguments")
                .or_else(|| val.get("input"))
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

            process_specific_opencode_tool_call(&raw_name, &name, &args, seq, ts, &raw_ref, &mut events);
        }

        "tool_result" | "tool_output" => {
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

            let tool_name = val
                .get("name")
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
                                summary: "Test suite executed successfully".to_string(),
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
                                summary: "Git commit observed in tool output".to_string(),
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

        _ if role == "user" => {
            let content = extract_opencode_content(val);
            *seq += 1;
            events.push(
                NormalizedEvent::new(*seq, ts, EventPayload::UserMessage { content })
                    .with_raw_ref(&raw_ref),
            );
        }

        _ if role == "assistant" => {
            let content = extract_opencode_content(val);
            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::AssistantMessage {
                        content,
                        thinking: None,
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

fn process_specific_opencode_tool_call(
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
    if lower_raw == "bash"
        || lower_raw == "shell"
        || lower_raw == "exec"
        || lower_raw == "run_command"
        || lower_raw == "terminal"
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
    } else if lower_raw == "edit_file"
        || lower_raw == "write_file"
        || lower_raw == "edit"
        || lower_raw == "patch"
        || lower_raw == "create_file"
        || lower_name.ends_with(":edit_file")
        || lower_name.ends_with(":write_file")
        || lower_name.ends_with(":text_editor")
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

fn extract_opencode_content(val: &Value) -> String {
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
    fn test_parse_standard_opencode_jsonl() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"type":"user_message","timestamp":"2026-08-29T10:00:00Z","content":"Optimize database queries"}
{"type":"assistant_message","timestamp":"2026-08-29T10:00:04Z","model":"deepseek-coder-v2","usage":{"input_tokens":350,"output_tokens":110,"cache_read_tokens":50},"thinking":"Checking queries...","content":"I will inspect the query logs."}
{"type":"tool_call","timestamp":"2026-08-29T10:00:06Z","id":"tc_1","name":"bash","arguments":{"command":"cargo test"}}
{"type":"tool_result","timestamp":"2026-08-29T10:00:08Z","tool_call_id":"tc_1","output":"test result: ok. 5 passed; 0 failed","is_error":false}
{"type":"assistant_message","timestamp":"2026-08-29T10:00:10Z","content":"Queries are optimized and verified."}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = OpenCodeAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 0);
        let trace = result.trace;
        assert_eq!(trace.adapter, "opencode");
        assert_eq!(
            trace.stats.models_used,
            vec!["deepseek-coder-v2".to_string()]
        );
        assert_eq!(trace.stats.token_usage.input_tokens, 350);
        assert_eq!(trace.stats.token_usage.output_tokens, 110);
        assert_eq!(trace.stats.token_usage.cache_read_tokens, 50);
        assert_eq!(trace.stats.token_usage.total(), 510);
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(trace.stats.tools_used.get("bash"), Some(&1));
        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.assistant_messages_count, 2);
        assert!(trace.stats.duration_seconds.unwrap() >= 10.0);
    }

    #[test]
    fn test_parse_graceful_on_empty_and_corrupt_lines() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = "{\"type\":\"user_message\",\"content\":\"start\"}\n\n{CORRUPT_JSON_DATA}\n{\"type\":\"assistant_message\",\"content\":\"finish\"}\n";
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = OpenCodeAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 1);
        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
    }

    #[test]
    fn test_detect_and_enumerate_opencode() {
        let temp = tempdir().unwrap();
        let opencode_dir = temp.path().join(".opencode").join("sessions");
        std::fs::create_dir_all(&opencode_dir).unwrap();

        let session_file = opencode_dir.join("session_001.jsonl");
        let mut f = File::create(&session_file).unwrap();
        writeln!(f, "{{\"type\":\"user_message\",\"content\":\"test\"}}").unwrap();

        let adapter = OpenCodeAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
        };

        let detection = adapter.detect(&options).unwrap();
        assert!(detection.is_present);

        let enumerated = adapter.enumerate(&options).unwrap();
        assert_eq!(enumerated.len(), 1);
        assert_eq!(enumerated[0].adapter_name, "opencode");
    }

    #[test]
    fn test_parse_opencode_json_array() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"[
            {"type":"user_message","timestamp":"2026-08-29T10:00:00Z","content":"Start task"},
            {"type":"assistant_message","timestamp":"2026-08-29T10:00:02Z","content":"Working on it"}
        ]"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = OpenCodeAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
    }
}
