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
use crate::exit_status::backfill_shell_exit_codes;

/// Adapter for discovering and normalizing Cursor (Composer / Chat) agent sessions.
pub struct CursorAdapter;

impl Default for CursorAdapter {
    fn default() -> Self {
        Self
    }
}

impl CursorAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Candidate directory paths for Cursor on the host machine, used for presence
    /// detection only. Broader than `session_roots()`: bare `~/.cursor` also holds the
    /// editor's `extensions/` bundles (each with its own vendored `node_modules`), and
    /// bare `Library/Application Support/Cursor` also holds `User/History` -- VS Code's
    /// local-file-history snapshots, unrelated to any AI chat.
    pub fn candidate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".cursor"));
            roots.push(home.join(".cursor").join("sessions"));
            roots.push(home.join(".cursor").join("composer"));
            roots.push(
                home.join("Library")
                    .join("Application Support")
                    .join("Cursor"),
            );
            roots.push(
                home.join("Library")
                    .join("Application Support")
                    .join("Cursor")
                    .join("User")
                    .join("workspaceStorage"),
            );
            roots.push(
                home.join("Library")
                    .join("Application Support")
                    .join("Cursor")
                    .join("User")
                    .join("globalStorage"),
            );
            roots.push(home.join(".config").join("Cursor"));
            roots.push(
                home.join(".config")
                    .join("Cursor")
                    .join("User")
                    .join("workspaceStorage"),
            );
        }
        roots.push(PathBuf::from(".cursor"));
        roots.push(PathBuf::from(".cursor").join("sessions"));
        roots.push(PathBuf::from(".cursor").join("composer"));
        roots
    }

    /// Directories that actually hold Cursor chat/composer data, used for the default
    /// (unscoped) `enumerate()` walk.
    pub fn session_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".cursor").join("sessions"));
            roots.push(home.join(".cursor").join("composer"));
            roots.push(home.join(".cursor").join("chats"));
            roots.push(
                home.join("Library")
                    .join("Application Support")
                    .join("Cursor")
                    .join("User")
                    .join("workspaceStorage"),
            );
            roots.push(
                home.join("Library")
                    .join("Application Support")
                    .join("Cursor")
                    .join("User")
                    .join("globalStorage"),
            );
            roots.push(
                home.join(".config")
                    .join("Cursor")
                    .join("User")
                    .join("workspaceStorage"),
            );
        }
        roots.push(PathBuf::from(".cursor").join("sessions"));
        roots.push(PathBuf::from(".cursor").join("composer"));
        roots.push(PathBuf::from(".cursor").join("chats"));
        roots
    }
}

/// Skip directories that are never Cursor chat data: `extensions/` bundles ship their
/// own `node_modules`, and `User/History` is VS Code's local-file-history feature, not
/// AI chat data, despite sharing a parent with `workspaceStorage`.
fn should_skip_cursor_dir(entry: &walkdir::DirEntry) -> bool {
    if entry.file_type().is_dir() {
        let name = entry.file_name().to_string_lossy();
        name == ".git"
            || name == "node_modules"
            || name == "target"
            || name == "dist"
            || name == ".venv"
            || name == "extensions"
            || name == "History"
            || name == "logs"
            || name == "Cache"
            || name == "CachedData"
            || name == "GPUCache"
            || name == "Backups"
    } else {
        false
    }
}

impl AgentAdapter for CursorAdapter {
    fn name(&self) -> &'static str {
        "cursor"
    }

    fn capabilities(&self) -> agentworth_adapter_sdk::AdapterCapabilities {
        agentworth_adapter_sdk::AdapterCapabilities {
            prompts: true,
            tokens: false,
            tools: false,
            shell: false,
            diffs: true,
            thinking: false,
            outcomes: false,
        }
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
            if s.contains(".cursor") || s.contains("cursor") {
                discovered.push(custom.clone());
            } else if custom.is_dir() {
                // custom_paths may point at a generic parent directory rather than
                // the adapter-specific dir itself; look a few levels in before
                // giving up, matching how `enumerate()` already recurses.
                let mut found_nested = false;
                if custom.join(".cursor").exists() {
                    discovered.push(custom.join(".cursor"));
                    found_nested = true;
                }
                if !found_nested {
                    for entry in WalkDir::new(custom).max_depth(4).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        let ps = path.to_string_lossy().to_lowercase();
                        if ps.contains(".cursor") || ps.contains("cursor") {
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
                    if is_candidate_cursor_file(custom) {
                        if let Ok(source) = SessionSource::from_path(custom, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if custom.is_dir() {
                    for entry in WalkDir::new(custom)
                        .into_iter()
                        .filter_entry(|e| !should_skip_cursor_dir(e))
                        .filter_map(|e| e.ok())
                    {
                        let path = entry.path();
                        if path.is_file() && is_candidate_cursor_file(path) {
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
                    if is_candidate_cursor_file(&root) {
                        if let Ok(source) = SessionSource::from_path(&root, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if root.is_dir() {
                    for entry in WalkDir::new(&root)
                        .into_iter()
                        .filter_entry(|e| !should_skip_cursor_dir(e))
                        .filter_map(|e| e.ok())
                    {
                        let path = entry.path();
                        if path.is_file() && is_candidate_cursor_file(path) {
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
                    } else if let Some(bubbles) = json_val.get("bubbles").and_then(|b| b.as_array())
                    {
                        bubbles.clone()
                    } else if let Some(messages) =
                        json_val.get("messages").and_then(|m| m.as_array())
                    {
                        messages.clone()
                    } else if let Some(turns) =
                        json_val.get("conversation").and_then(|c| c.as_array())
                    {
                        turns.clone()
                    } else if let Some(turns) = json_val.get("turns").and_then(|t| t.as_array()) {
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

                        let evts = parse_cursor_record(item, &mut sequence, timestamp, idx + 1, &mut last_model);
                        trace.events.extend(evts);
                    }

                    if let Some(earliest) = earliest_ts {
                        trace.started_at = earliest;
                    }
                    if let Some(latest) = latest_ts {
                        trace.ended_at = Some(latest);
                    }

                    backfill_shell_exit_codes(&mut trace.events);
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

                let events = parse_cursor_record(&val, &mut sequence, timestamp, line_num, &mut last_model);
                trace.events.extend(events);
            }
        }

        if let Some(earliest) = earliest_ts {
            trace.started_at = earliest;
        }
        if let Some(latest) = latest_ts {
            trace.ended_at = Some(latest);
        }

        backfill_shell_exit_codes(&mut trace.events);
        trace.recalculate_stats();

        Ok(ParseResult {
            trace,
            malformed_lines,
            warnings,
        })
    }
}

fn is_candidate_cursor_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    if !path_str.contains("cursor") && !path_str.contains("composer") {
        return false;
    }
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if filename.starts_with('.') && !filename.ends_with(".jsonl") && !filename.ends_with(".json") {
        return false;
    }
    let lower = filename.to_lowercase();
    if lower == "config.json"
        || lower == "settings.json"
        || lower == "keybindings.json"
        || lower == "extensions.json"
        || lower == "argv.json"
        || lower == "credentials.json"
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
    if let Some(ts_str) = val
        .get("createdAt")
        .or_else(|| val.get("created_at"))
        .and_then(|v| v.as_str())
    {
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    if let Some(millis) = val
        .get("timestamp")
        .or_else(|| val.get("createdAt"))
        .and_then(|v| v.as_i64())
    {
        if millis > 1_000_000_000_000 {
            return DateTime::from_timestamp_millis(millis);
        } else {
            return DateTime::from_timestamp(millis, 0);
        }
    }
    None
}

fn extract_token_usage(usage_val: &Value) -> TokenUsage {
    let input_tokens = usage_val
        .get("promptTokens")
        .or_else(|| usage_val.get("prompt_tokens"))
        .or_else(|| usage_val.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let output_tokens = usage_val
        .get("completionTokens")
        .or_else(|| usage_val.get("completion_tokens"))
        .or_else(|| usage_val.get("output_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let cache_read_tokens = usage_val
        .get("cachedTokens")
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

fn parse_cursor_record(
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
        .or_else(|| val.get("bubbleType"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Model invocation / Token extraction
    if let Some(usage_val) = val
        .get("tokens")
        .or_else(|| val.get("usage"))
        .or_else(|| val.get("tokenUsage"))
    {
        let usage = extract_token_usage(usage_val);
        if usage.total() > 0 {
            let model = val
                .get("model")
                .or_else(|| val.get("modelType"))
                .or_else(|| val.get("modelName"))
                .and_then(|v| v.as_str())
                .unwrap_or("cursor-composer-model")
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
                            .get("durationMs")
                            .or_else(|| val.get("latency_ms"))
                            .and_then(|d| d.as_u64()),
                        effort: None,
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    match role {
        "user" | "human" | "prompt" => {
            let content = extract_cursor_content(val);
            *seq += 1;
            events.push(
                NormalizedEvent::new(*seq, ts, EventPayload::UserMessage { content })
                    .with_raw_ref(&raw_ref),
            );
        }

        "ai" | "assistant" | "composer" | "bot" => {
            let thinking = val
                .get("thinking")
                .or_else(|| val.get("reasoning"))
                .and_then(|v| v.as_str())
                .map(String::from);

            let content = extract_cursor_content(val);

            // Extract tool diffs / code edits in Composer
            if let Some(diffs) = val
                .get("diffs")
                .or_else(|| val.get("codeBlocks"))
                .or_else(|| val.get("fileModifications"))
                .and_then(|v| v.as_array())
            {
                for d in diffs {
                    let path = d
                        .get("file")
                        .or_else(|| d.get("filePath"))
                        .or_else(|| d.get("path"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    if !path.is_empty() {
                        let diff_text = d
                            .get("diff")
                            .or_else(|| d.get("content"))
                            .or_else(|| d.get("newContent"))
                            .and_then(|v| v.as_str())
                            .map(String::from);

                        *seq += 1;
                        events.push(
                            NormalizedEvent::new(
                                *seq,
                                ts,
                                EventPayload::FileAction {
                                    path,
                                    action: FileActionType::Edit,
                                    diff: diff_text,
                                    lines_changed: d.get("linesChanged").and_then(|v| v.as_u64()),
                                },
                            )
                            .with_raw_ref(&raw_ref),
                        );
                    }
                }
            }

            // Check tool calls
            if let Some(tools) = val
                .get("toolCalls")
                .or_else(|| val.get("tool_calls"))
                .and_then(|v| v.as_array())
            {
                for tc in tools {
                    let id = tc.get("id").and_then(|v| v.as_str()).map(String::from);
                    let name = tc
                        .get("name")
                        .or_else(|| tc.get("tool"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let args = tc
                        .get("arguments")
                        .or_else(|| tc.get("params"))
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

                    process_specific_cursor_tool_call(&name, &args, seq, ts, &raw_ref, &mut events);
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

        "tool_call" | "tool" => {
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

            process_specific_cursor_tool_call(&name, &args, seq, ts, &raw_ref, &mut events);
        }

        "tool_result" => {
            let call_id = val
                .get("call_id")
                .or_else(|| val.get("tool_call_id"))
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
                                summary: "Tests passed in Cursor terminal output".to_string(),
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
                .unwrap_or("Cursor error")
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

fn process_specific_cursor_tool_call(
    name: &str,
    args: &Value,
    seq: &mut u64,
    ts: DateTime<Utc>,
    raw_ref: &str,
    events: &mut Vec<NormalizedEvent>,
) {
    let lower = name.to_lowercase();
    if lower.contains("terminal")
        || lower.contains("command")
        || lower.contains("bash")
        || lower.contains("shell")
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
}

fn extract_cursor_content(val: &Value) -> String {
    if let Some(text) = val.get("text").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    if let Some(text) = val.get("content").and_then(|v| v.as_str()) {
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
    fn test_detect_and_enumerate_cursor() {
        let temp = tempdir().unwrap();
        let cursor_dir = temp.path().join(".cursor").join("composer");
        std::fs::create_dir_all(&cursor_dir).unwrap();

        let composer_file = cursor_dir.join("composer_001.jsonl");
        let mut f = File::create(&composer_file).unwrap();
        writeln!(f, "{{\"type\":\"user\",\"text\":\"Fix bug in component\"}}").unwrap();

        let adapter = CursorAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
            ..Default::default()
        };

        let detection = adapter.detect(&options).unwrap();
        assert!(detection.is_present);

        let enumerated = adapter.enumerate(&options).unwrap();
        assert_eq!(enumerated.len(), 1);
        assert_eq!(enumerated[0].adapter_name, "cursor");
    }

    /// Regression test: `~/.cursor/extensions/<ext>/node_modules/...` and
    /// `Library/Application Support/Cursor/User/History/...` (VS Code local file history)
    /// both matched the old "path contains cursor" + json/jsonl filter. Both must now be
    /// skipped even when nested under a scanned root.
    #[test]
    fn test_enumerate_cursor_skips_extensions_and_history_junk() {
        let temp = tempdir().unwrap();

        let ext_dir = temp
            .path()
            .join(".cursor")
            .join("extensions")
            .join("some.ext-1.0.0")
            .join("node_modules")
            .join("dep");
        std::fs::create_dir_all(&ext_dir).unwrap();
        File::create(ext_dir.join("package.json")).unwrap();

        let history_dir = temp
            .path()
            .join("Cursor")
            .join("User")
            .join("History")
            .join("-1f86dea4");
        std::fs::create_dir_all(&history_dir).unwrap();
        File::create(history_dir.join("vPbv.json")).unwrap();

        let composer_dir = temp.path().join(".cursor").join("composer");
        std::fs::create_dir_all(&composer_dir).unwrap();
        let mut real = File::create(composer_dir.join("composer_001.jsonl")).unwrap();
        writeln!(real, "{{\"type\":\"user\",\"text\":\"Fix bug in component\"}}").unwrap();

        let adapter = CursorAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
            ..Default::default()
        };

        let enumerated = adapter.enumerate(&options).unwrap();
        assert_eq!(enumerated.len(), 1, "only the real composer session should be enumerated");
        assert!(enumerated[0].path.to_string_lossy().ends_with("composer_001.jsonl"));
    }

    #[test]
    fn test_parse_standard_cursor_composer_jsonl() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"bubbleType":"user","createdAt":"2026-08-29T10:00:00Z","text":"Create a auth middleware in Rust"}
{"bubbleType":"ai","createdAt":"2026-08-29T10:00:04Z","modelType":"claude-3.5-sonnet","tokens":{"promptTokens":500,"completionTokens":120,"cachedTokens":100},"text":"Here is the middleware implementation","diffs":[{"filePath":"src/auth.rs","diff":"+pub fn auth_middleware() {}","linesChanged":1}]}
{"type":"tool_call","timestamp":"2026-08-29T10:00:06Z","name":"run_terminal_command","arguments":{"command":"cargo test"}}
{"type":"tool_result","timestamp":"2026-08-29T10:00:08Z","output":"test result: ok. 4 passed; 0 failed"}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = CursorAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 0);
        let trace = result.trace;
        assert_eq!(trace.adapter, "cursor");
        assert_eq!(
            trace.stats.models_used,
            vec!["claude-3.5-sonnet".to_string()]
        );
        assert_eq!(trace.stats.token_usage.input_tokens, 500);
        assert_eq!(trace.stats.token_usage.output_tokens, 120);
        assert_eq!(trace.stats.token_usage.cache_read_tokens, 100);
        assert_eq!(trace.stats.token_usage.total(), 720);
        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.assistant_messages_count, 1);
    }

    #[test]
    fn test_parse_cursor_workspace_storage_json() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"{
  "composerData": {
    "conversationId": "comp-777"
  },
  "bubbles": [
    {"type": "user", "text": "Refactor styling"},
    {"type": "ai", "model": "cursor-fast", "tokens": {"promptTokens": 100, "completionTokens": 30}, "text": "Updated styles", "codeBlocks": [{"file": "src/App.css", "newContent": ".main { margin: 0; }"}]}
  ]
}"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = CursorAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        let trace = result.trace;
        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.assistant_messages_count, 1);
        assert_eq!(trace.stats.token_usage.input_tokens, 100);
        assert_eq!(trace.stats.token_usage.output_tokens, 30);
    }

    #[test]
    fn test_parse_graceful_on_malformed_lines() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = "{\"type\":\"user\",\"text\":\"hello\"}\n{CORRUPT_CURSOR_JSON}\n{\"type\":\"ai\",\"text\":\"hi\"}\n";
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = CursorAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 1);
        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
    }
}
