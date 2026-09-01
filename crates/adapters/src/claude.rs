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

/// Adapter for discovering and normalizing Claude Code sessions.
pub struct ClaudeCodeAdapter;

impl Default for ClaudeCodeAdapter {
    fn default() -> Self {
        Self
    }
}

impl ClaudeCodeAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Candidate directory paths for Claude Code on the host machine.
    pub fn candidate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".claude").join("projects"));
            roots.push(home.join(".claude").join("sessions"));
            roots.push(home.join(".config").join("claude"));
            roots.push(home.join(".claude"));
        }
        roots
    }
}

impl AgentAdapter for ClaudeCodeAdapter {
    fn name(&self) -> &'static str {
        "claude_code"
    }

    fn capabilities(&self) -> agentworth_adapter_sdk::AdapterCapabilities {
        agentworth_adapter_sdk::AdapterCapabilities {
            prompts: true,
            tokens: true,
            tools: true,
            shell: true,
            diffs: true,
            thinking: true,
            outcomes: true,
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
            if custom.ends_with(".claude") || s.contains("claude") {
                discovered.push(custom.clone());
            } else if custom.is_dir() {
                // custom_paths may point at a generic parent directory rather than
                // the adapter-specific dir itself; look a few levels in before
                // giving up, matching how `enumerate()` already recurses.
                let mut found_nested = false;
                for sub in &[custom.join(".claude"), custom.join(".config").join("claude")] {
                    if sub.exists() {
                        discovered.push(sub.clone());
                        found_nested = true;
                    }
                }
                if !found_nested {
                    for entry in WalkDir::new(custom).max_depth(4).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        let ps = path.to_string_lossy().to_lowercase();
                        if ps.contains("claude") {
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

        let should_skip = |entry: &walkdir::DirEntry| -> bool {
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
        };

        let roots_to_scan = if !options.custom_paths.is_empty() {
            options.custom_paths.clone()
        } else {
            let mut roots = Vec::new();
            for r in self.candidate_roots() {
                if r.exists() && !roots.iter().any(|existing: &PathBuf| r.starts_with(existing)) {
                    roots.push(r);
                }
            }
            roots
        };

        for root in roots_to_scan {
            if root.is_file() {
                if is_candidate_claude_file(&root) {
                    if let Ok(source) = SessionSource::from_path(&root, self.name()) {
                        sources.push(source);
                    }
                }
            } else if root.is_dir() {
                for entry in WalkDir::new(&root)
                    .into_iter()
                    .filter_entry(|e| !should_skip(e))
                    .filter_map(|e| e.ok())
                {
                    let path = entry.path();
                    if path.is_file() && is_candidate_claude_file(path) {
                        if let Ok(source) = SessionSource::from_path(path, self.name()) {
                            sources.push(source);
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
                    } else if let Some(turns) =
                        json_val.get("conversation").and_then(|c| c.as_array())
                    {
                        turns.clone()
                    } else if let Some(turns) = json_val.get("turns").and_then(|t| t.as_array()) {
                        turns.clone()
                    } else if let Some(events) = json_val.get("events").and_then(|e| e.as_array()) {
                        events.clone()
                    } else if let Some(history) = json_val.get("history").and_then(|h| h.as_array()) {
                        history.clone()
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

                        let evts = parse_claude_record(item, &mut sequence, timestamp, idx + 1);
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

                let events = parse_claude_record(&val, &mut sequence, timestamp, line_num);
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

fn is_candidate_claude_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    if path_str.contains("codex")
        || path_str.contains("gemini")
        || path_str.contains("antigravity")
        || path_str.contains("opencode")
        || path_str.contains("goose")
        || path_str.contains("cursor")
        || path_str.contains("composer")
        || path_str.contains("herdr")
        || path_str.contains("hermes")
        || path_str.contains("openclaw")
        || path_str.contains("grok")
        || path_str.contains("xai")
        || path_str.contains("/.pi/")
        || path_str.contains("/pi/")
        || path_str.contains(".pi")
        || path_str.contains("deepseek")
        || path_str.contains("kimi")
        || path_str.contains("minimax")
        || path_str.contains("qwen")
        || path_str.contains("zhipu")
        || path_str.contains("codegeex")
        || path_str.contains("manus")
        || path_str.contains("aider")
        || path_str.contains("cline")
        || path_str.contains("windsurf")
    {
        return false;
    }
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if filename.starts_with('.') && !filename.ends_with(".jsonl") && !filename.ends_with(".json") {
        return false;
    }
    let lower = filename.to_lowercase();
    if lower == "config.json" || lower == "settings.json" || lower == "credentials.json" {
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
        .get("cache_read_input_tokens")
        .or_else(|| usage_val.get("cache_read_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let cache_creation_tokens = usage_val
        .get("cache_creation_input_tokens")
        .or_else(|| usage_val.get("cache_creation_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    TokenUsage::new(
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
    )
}

fn parse_claude_record(
    val: &Value,
    seq: &mut u64,
    ts: DateTime<Utc>,
    line_num: usize,
) -> Vec<NormalizedEvent> {
    let mut events = Vec::new();
    let raw_ref = format!("line:{}", line_num);

    let event_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");

    // Model invocation / Token usage check
    if let Some(usage_val) = val
        .get("usage")
        .or_else(|| val.get("message").and_then(|m| m.get("usage")))
    {
        let usage = extract_token_usage(usage_val);
        if usage.total() > 0 {
            let model = val
                .get("model")
                .or_else(|| val.get("message").and_then(|m| m.get("model")))
                .and_then(|v| v.as_str())
                .unwrap_or("claude-unknown")
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
                        latency_ms: val.get("duration_ms").and_then(|d| d.as_u64()),
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    match event_type {
        "user" | "human" => {
            let content = if let Some(text) = val.get("content").and_then(|v| v.as_str()) {
                text.to_string()
            } else if let Some(msg) = val
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
            {
                msg.to_string()
            } else if let Some(arr) = val.get("content").and_then(|v| v.as_array()) {
                let mut text_parts = Vec::new();
                for block in arr {
                    let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if block_type == "tool_result" {
                        let call_id = block
                            .get("tool_use_id")
                            .or_else(|| block.get("call_id"))
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        let is_error = block
                            .get("is_error")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let output = block
                            .get("content")
                            .or_else(|| block.get("output"))
                            .cloned()
                            .unwrap_or(Value::Null);

                        *seq += 1;
                        events.push(
                            NormalizedEvent::new(
                                *seq,
                                ts,
                                EventPayload::ToolResult(ToolResult {
                                    call_id,
                                    name: block
                                        .get("name")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                    output: output.clone(),
                                    is_error,
                                }),
                            )
                            .with_raw_ref(&raw_ref),
                        );

                        if let Some(out_str) = output.as_str() {
                            if out_str.contains("test result: ok.")
                                || out_str.contains("PASSED")
                                || out_str.contains("100% tests passed")
                                || out_str.contains("passed in ")
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
                                            summary: "Git commit observed in tool output"
                                                .to_string(),
                                            confidence: 0.85,
                                        }),
                                    )
                                    .with_raw_ref(&raw_ref),
                                );
                            }
                        }
                    } else if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                        text_parts.push(t.to_string());
                    }
                }
                text_parts.join("\n")
            } else {
                val.to_string()
            };

            if !content.is_empty() {
                *seq += 1;
                events.push(
                    NormalizedEvent::new(*seq, ts, EventPayload::UserMessage { content })
                        .with_raw_ref(&raw_ref),
                );
            }
        }

        "assistant" => {
            let mut thinking = None;
            let mut text_parts = Vec::new();

            if let Some(content_val) = val
                .get("content")
                .or_else(|| val.get("message").and_then(|m| m.get("content")))
            {
                if let Some(text) = content_val.as_str() {
                    text_parts.push(text.to_string());
                } else if let Some(arr) = content_val.as_array() {
                    for block in arr {
                        let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        match block_type {
                            "text" => {
                                if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                                    text_parts.push(t.to_string());
                                }
                            }
                            "thinking" => {
                                if let Some(th) = block.get("thinking").and_then(|v| v.as_str()) {
                                    thinking = Some(th.to_string());
                                }
                            }
                            "tool_use" => {
                                let id = block.get("id").and_then(|v| v.as_str()).map(String::from);
                                let raw_name = block
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();
                                let input = block.get("input").cloned().unwrap_or(Value::Null);
                                let name = normalize_mcp_tool_name(&raw_name, &input);

                                *seq += 1;
                                events.push(
                                    NormalizedEvent::new(
                                        *seq,
                                        ts,
                                        EventPayload::ToolCall(ToolCall {
                                            id: id.clone(),
                                            name: name.clone(),
                                            arguments: input.clone(),
                                        }),
                                    )
                                    .with_raw_ref(&raw_ref),
                                );

                                // Check specific tool types: Bash, FileEdit, etc.
                                if raw_name == "Bash" || raw_name == "bash" || name.ends_with(":bash") || name.ends_with(":shell") {
                                    if let Some(cmd) = input.get("command").and_then(|v| v.as_str())
                                    {
                                        *seq += 1;
                                        events.push(
                                            NormalizedEvent::new(
                                                *seq,
                                                ts,
                                                EventPayload::ShellCommand(ShellCommand {
                                                    command: cmd.to_string(),
                                                    cwd: input
                                                        .get("cwd")
                                                        .and_then(|v| v.as_str())
                                                        .map(String::from),
                                                    exit_code: None,
                                                    output: None,
                                                }),
                                            )
                                            .with_raw_ref(&raw_ref),
                                        );
                                    }
                                } else if raw_name == "FileEdit"
                                    || raw_name == "Edit"
                                    || raw_name == "MultiEdit"
                                    || raw_name == "Write"
                                    || raw_name == "NotebookEdit"
                                    || raw_name == "create_file"
                                    || name.ends_with(":edit_file")
                                    || name.ends_with(":write_file")
                                    || name.ends_with(":text_editor")
                                {
                                    // NotebookEdit addresses the target file via `notebook_path`,
                                    // not `file_path`; every other Claude Code file-editing tool
                                    // uses `file_path` (MultiEdit predates its Oct-2025 removal
                                    // but historical session logs still carry it).
                                    let path = input
                                        .get("file_path")
                                        .or_else(|| input.get("path"))
                                        .or_else(|| input.get("target_file"))
                                        .or_else(|| input.get("notebook_path"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();

                                    if !path.is_empty() {
                                        let action = if raw_name == "Write"
                                            || raw_name == "create_file"
                                            || name.ends_with(":write_file")
                                        {
                                            FileActionType::Write
                                        } else {
                                            FileActionType::Edit
                                        };

                                        *seq += 1;
                                        events.push(
                                            NormalizedEvent::new(
                                                *seq,
                                                ts,
                                                EventPayload::FileAction {
                                                    path,
                                                    action,
                                                    diff: input
                                                        .get("diff")
                                                        .and_then(|v| v.as_str())
                                                        .map(String::from),
                                                    lines_changed: None,
                                                },
                                            )
                                            .with_raw_ref(&raw_ref),
                                        );
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
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

        "tool_use" => {
            let id = val.get("id").and_then(|v| v.as_str()).map(String::from);
            let raw_name = val
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let input = val
                .get("input")
                .or_else(|| val.get("arguments"))
                .cloned()
                .unwrap_or(Value::Null);
            let name = normalize_mcp_tool_name(&raw_name, &input);

            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::ToolCall(ToolCall {
                        id,
                        name,
                        arguments: input,
                    }),
                )
                .with_raw_ref(&raw_ref),
            );
        }

        "tool_result" => {
            let call_id = val
                .get("tool_use_id")
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

            // Infer outcome evidence from tool execution
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
            // Fallback for unclassified custom records
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_standard_claude_jsonl() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"Fix the bug in src/main.rs"}
{"type":"assistant","timestamp":"2026-08-29T10:00:05Z","model":"claude-3-5-sonnet-20241022","usage":{"input_tokens":500,"output_tokens":120,"cache_read_input_tokens":200,"cache_creation_input_tokens":50},"content":[{"type":"text","text":"I will check the file."},{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"cargo test"}}]}
{"type":"tool_result","timestamp":"2026-08-29T10:00:07Z","tool_use_id":"toolu_1","content":"test result: ok. 4 passed; 0 failed","is_error":false}
{"type":"assistant","timestamp":"2026-08-29T10:00:10Z","content":[{"type":"text","text":"All tests pass now!"}]}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = ClaudeCodeAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 0);
        let trace = result.trace;
        assert_eq!(trace.adapter, "claude_code");
        assert_eq!(
            trace.stats.models_used,
            vec!["claude-3-5-sonnet-20241022".to_string()]
        );
        assert_eq!(trace.stats.token_usage.input_tokens, 500);
        assert_eq!(trace.stats.token_usage.output_tokens, 120);
        assert_eq!(trace.stats.token_usage.cache_read_tokens, 200);
        assert_eq!(trace.stats.token_usage.cache_creation_tokens, 50);
        assert_eq!(trace.stats.token_usage.total(), 870);
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(trace.stats.tools_used.get("Bash"), Some(&1));
        assert!(trace.stats.duration_seconds.unwrap() >= 10.0);
    }

    #[test]
    fn test_parse_multi_model_session_tracks_per_model_usage() {
        let mut temp = NamedTempFile::new().unwrap();
        // A session where the primary model delegates to a subagent running a
        // different model, then hands back — the shape that made session-level
        // token totals unusable for a per-model cost/usage breakdown.
        let sample = r#"
{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"Delegate the search to a subagent"}
{"type":"assistant","timestamp":"2026-08-29T10:00:05Z","model":"claude-opus-5","usage":{"input_tokens":500,"output_tokens":120,"cache_read_input_tokens":200,"cache_creation_input_tokens":50},"content":[{"type":"text","text":"Delegating."}]}
{"type":"assistant","timestamp":"2026-08-29T10:00:10Z","model":"claude-fable-5","usage":{"input_tokens":300,"output_tokens":80,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"Subagent result."}]}
{"type":"assistant","timestamp":"2026-08-29T10:00:15Z","model":"claude-opus-5","usage":{"input_tokens":100,"output_tokens":40,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"Done."}]}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = ClaudeCodeAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");
        let trace = result.trace;

        assert_eq!(
            trace.stats.models_used,
            vec!["claude-opus-5".to_string(), "claude-fable-5".to_string()]
        );

        // Opus ran twice (500+100 in, 120+40 out, plus its cache activity); Fable ran once.
        let opus_usage = trace
            .stats
            .per_model_token_usage
            .get("claude-opus-5")
            .expect("opus usage present");
        assert_eq!(opus_usage.input_tokens, 600);
        assert_eq!(opus_usage.output_tokens, 160);
        assert_eq!(opus_usage.cache_read_tokens, 200);
        assert_eq!(opus_usage.cache_creation_tokens, 50);

        let fable_usage = trace
            .stats
            .per_model_token_usage
            .get("claude-fable-5")
            .expect("fable usage present");
        assert_eq!(fable_usage.input_tokens, 300);
        assert_eq!(fable_usage.output_tokens, 80);
        assert_eq!(fable_usage.cache_read_tokens, 0);
        assert_eq!(fable_usage.cache_creation_tokens, 0);

        // The pre-existing flat aggregate is untouched (backward compat).
        assert_eq!(trace.stats.token_usage.input_tokens, 900);
        assert_eq!(trace.stats.token_usage.output_tokens, 240);
        assert_eq!(trace.stats.token_usage.cache_read_tokens, 200);
        assert_eq!(trace.stats.token_usage.cache_creation_tokens, 50);
        assert_eq!(
            opus_usage.total() + fable_usage.total(),
            trace.stats.token_usage.total()
        );
    }

    #[test]
    fn test_parse_graceful_on_malformed_lines() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = "{\"type\":\"user\",\"content\":\"hello\"}\n{CORRUPT_JSON}\n{\"type\":\"assistant\",\"content\":\"hi\"}\n";
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = ClaudeCodeAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 1);
        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
    }

    #[test]
    fn test_parse_claude_json_array_and_object() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"[
            {"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"Fix the bug"},
            {"type":"assistant","timestamp":"2026-08-29T10:00:05Z","content":[{"type":"text","text":"Fixed."}]}
        ]"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = ClaudeCodeAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
    }

    #[test]
    fn test_parse_claude_mcp_tool_calls() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"Query postgres and edit"}
{"type":"assistant","timestamp":"2026-08-29T10:00:02Z","content":[{"type":"tool_use","id":"t1","name":"mcp__postgres__query","input":{"query":"SELECT 1"}},{"type":"tool_use","id":"t2","name":"developer__text_editor","input":{"command":"view","path":"/tmp/test.rs"}}]}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = ClaudeCodeAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        let trace = result.trace;
        assert_eq!(trace.stats.tool_calls_count, 2);
        assert_eq!(trace.stats.tools_used.get("mcp:postgres:query"), Some(&1));
        assert_eq!(
            trace.stats.tools_used.get("mcp:developer:text_editor"),
            Some(&1)
        );
    }

    #[test]
    fn test_parse_claude_file_action_tool_variants() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"type":"assistant","timestamp":"2026-08-29T10:00:01Z","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"src/session.rs","old_string":"a","new_string":"b"}}]}
{"type":"assistant","timestamp":"2026-08-29T10:00:02Z","content":[{"type":"tool_use","id":"t2","name":"Write","input":{"file_path":"src/new_file.rs","content":"fn main() {}"}}]}
{"type":"assistant","timestamp":"2026-08-29T10:00:03Z","content":[{"type":"tool_use","id":"t3","name":"MultiEdit","input":{"file_path":"src/session.rs","edits":[{"old_string":"c","new_string":"d"}]}}]}
{"type":"assistant","timestamp":"2026-08-29T10:00:04Z","content":[{"type":"tool_use","id":"t4","name":"NotebookEdit","input":{"notebook_path":"analysis.ipynb","cell_id":"abc123","new_source":"print(1)"}}]}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = ClaudeCodeAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        let file_actions: Vec<_> = result
            .trace
            .events
            .iter()
            .filter_map(|e| match &e.payload {
                EventPayload::FileAction { path, action, .. } => Some((path.clone(), *action)),
                _ => None,
            })
            .collect();

        assert_eq!(
            file_actions,
            vec![
                ("src/session.rs".to_string(), FileActionType::Edit),
                ("src/new_file.rs".to_string(), FileActionType::Write),
                ("src/session.rs".to_string(), FileActionType::Edit),
                ("analysis.ipynb".to_string(), FileActionType::Edit),
            ]
        );
    }
}
