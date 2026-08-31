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

/// Adapter for discovering and normalizing Moonshot Kimi / Kimi-Code wire sessions and trajectories.
pub struct KimiAdapter;

impl Default for KimiAdapter {
    fn default() -> Self {
        Self
    }
}

impl KimiAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Candidate directory paths for Kimi on the host machine.
    pub fn candidate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".kimi-code"));
            roots.push(home.join(".kimi-code").join("sessions"));
            roots.push(home.join(".kimi"));
            roots.push(home.join(".kimi").join("sessions"));
            roots.push(home.join(".kimi").join("wire"));
            roots.push(home.join(".config").join("kimi"));
            roots.push(home.join(".config").join("kimi-code"));
        }
        roots.push(PathBuf::from(".kimi-code"));
        roots.push(PathBuf::from(".kimi-code").join("sessions"));
        roots.push(PathBuf::from(".kimi"));
        roots.push(PathBuf::from(".kimi").join("sessions"));
        roots.push(PathBuf::from(".kimi").join("wire"));
        roots
    }
}

impl AgentAdapter for KimiAdapter {
    fn name(&self) -> &'static str {
        "kimi"
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
                if s.contains(".kimi")
                    || s.contains("kimi")
                    || s.contains("kimi-code")
                    || s.contains("moonshot")
                    || s.ends_with("wire.jsonl")
                {
                    discovered.push(custom.clone());
                } else if custom.is_dir() {
                    for sub in &[
                        custom.join(".kimi"),
                        custom.join(".kimi-code"),
                        custom.join(".config").join("kimi"),
                    ] {
                        if sub.exists() {
                            discovered.push(sub.clone());
                        }
                    }
                    if discovered.is_empty() {
                        for entry in WalkDir::new(custom).max_depth(4).into_iter().filter_map(|e| e.ok()) {
                            let path = entry.path();
                            let s = path.to_string_lossy().to_lowercase();
                            if s.contains(".kimi") || s.contains("kimi") || s.contains("moonshot") {
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
                    if is_candidate_kimi_file(custom) {
                        if let Ok(source) = SessionSource::from_path(custom, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if custom.is_dir() {
                    for entry in WalkDir::new(custom).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_kimi_file(path) {
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
                    if is_candidate_kimi_file(&root) {
                        if let Ok(source) = SessionSource::from_path(&root, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if root.is_dir() {
                    for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_kimi_file(path) {
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
                    } else if let Some(turns) = json_val
                        .get("turns")
                        .or_else(|| json_val.get("messages"))
                        .or_else(|| json_val.get("events"))
                        .or_else(|| json_val.get("wire_events"))
                        .or_else(|| json_val.get("steps"))
                        .or_else(|| json_val.get("history"))
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

                        let evts = parse_kimi_record(item, &mut sequence, timestamp, idx + 1);
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

                let events = parse_kimi_record(&val, &mut sequence, timestamp, line_num);
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

fn is_candidate_kimi_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    if !path_str.contains("kimi") && !path_str.contains("moonshot") && !path_str.ends_with("wire.jsonl") {
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
        .get("cached_tokens")
        .or_else(|| usage_val.get("prompt_cache_hit_tokens"))
        .or_else(|| {
            usage_val
                .get("prompt_tokens_details")
                .and_then(|d| d.get("cached_tokens"))
        })
        .or_else(|| usage_val.get("cache_read_tokens"))
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

fn parse_kimi_record(
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
        .or_else(|| val.get("event"))
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
                .unwrap_or("kimi-k1.5")
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

    match role {
        "user" | "human" | "user_message" => {
            let content = extract_kimi_content(val);
            *seq += 1;
            events.push(
                NormalizedEvent::new(*seq, ts, EventPayload::UserMessage { content })
                    .with_raw_ref(&raw_ref),
            );
        }

        "assistant" | "model" | "kimi" | "assistant_message" => {
            let thinking = val
                .get("reasoning_content")
                .or_else(|| val.get("thought"))
                .or_else(|| val.get("thinking"))
                .or_else(|| val.get("thoughts"))
                .and_then(|v| v.as_str())
                .map(String::from);

            let content = extract_kimi_content(val);

            // Tool calls / subagent delegations in assistant turn
            if let Some(tools) = val
                .get("tool_calls")
                .or_else(|| val.get("function_calls"))
                .or_else(|| val.get("tools"))
                .and_then(|v| v.as_array())
            {
                for tc in tools {
                    let id = tc.get("id").and_then(|v| v.as_str()).map(String::from);
                    let fn_val = tc.get("function").unwrap_or(tc);
                    let raw_name = fn_val
                        .get("name")
                        .or_else(|| fn_val.get("tool"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();

                    let args: Value = match fn_val.get("arguments").or_else(|| fn_val.get("input")) {
                        Some(Value::String(s)) => {
                            serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.clone()))
                        }
                        Some(v) => v.clone(),
                        None => Value::Null,
                    };

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

                    process_specific_kimi_tool_call(
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

        // Subagent delegations / wire delegations
        "subagent_delegation" | "subagent_call" | "agent_call" | "delegate" | "subagent" => {
            let agent_role = val
                .get("agent_role")
                .or_else(|| val.get("target_agent"))
                .or_else(|| val.get("subagent"))
                .or_else(|| val.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("subagent")
                .to_string();

            let prompt = val
                .get("prompt")
                .or_else(|| val.get("task"))
                .or_else(|| val.get("instruction"))
                .or_else(|| val.get("arguments"))
                .cloned()
                .unwrap_or_else(|| Value::String(String::new()));

            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::ToolCall(ToolCall {
                        id: val.get("id").and_then(|v| v.as_str()).map(String::from),
                        name: format!("subagent_delegate:{}", agent_role),
                        arguments: serde_json::json!({
                            "agent_role": agent_role,
                            "prompt": prompt,
                        }),
                    }),
                )
                .with_raw_ref(&raw_ref),
            );

            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::Custom {
                        kind: "subagent_delegation".to_string(),
                        data: val.clone(),
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }

        "tool_call" | "function_call" | "tool_use" => {
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

            process_specific_kimi_tool_call(&raw_name, &name, &args, seq, ts, &raw_ref, &mut events);
        }

        "tool" | "tool_result" | "function" | "tool_output" => {
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
                                summary: "Test suite executed successfully in Kimi".to_string(),
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
                                summary: "Git commit observed in Kimi tool output".to_string(),
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
                .unwrap_or("Kimi execution error")
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

fn process_specific_kimi_tool_call(
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

    if lower_raw.contains("bash")
        || lower_raw.contains("terminal")
        || lower_raw.contains("shell")
        || lower_raw.contains("exec")
        || lower_raw.contains("run_command")
        || lower_name.ends_with(":bash")
        || lower_name.ends_with(":terminal")
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
    } else if lower_raw.contains("edit")
        || lower_raw.contains("write")
        || lower_raw.contains("create_file")
        || lower_raw.contains("modify")
        || lower_raw.contains("patch")
        || lower_name.ends_with(":edit")
        || lower_name.ends_with(":write")
    {
        let path = args
            .get("path")
            .or_else(|| args.get("file_path"))
            .or_else(|| args.get("target_file"))
            .or_else(|| args.get("file"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if !path.is_empty() {
            let action = if lower_raw.contains("write") || lower_raw.contains("create") {
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
    } else if lower_raw.contains("read")
        || lower_raw.contains("view")
        || lower_name.ends_with(":read")
        || lower_name.ends_with(":view")
    {
        let path = args
            .get("path")
            .or_else(|| args.get("file_path"))
            .or_else(|| args.get("target_file"))
            .or_else(|| args.get("file"))
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
                        action: FileActionType::Read,
                        diff: None,
                        lines_changed: None,
                    },
                )
                .with_raw_ref(raw_ref),
            );
        }
    } else if lower_raw.contains("delegate")
        || lower_raw.contains("subagent")
        || lower_raw.contains("spawn_agent")
    {
        *seq += 1;
        events.push(
            NormalizedEvent::new(
                *seq,
                ts,
                EventPayload::Custom {
                    kind: "subagent_delegation".to_string(),
                    data: args.clone(),
                },
            )
            .with_raw_ref(raw_ref),
        );
    }
}

fn extract_kimi_content(val: &Value) -> String {
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
    fn test_detect_and_enumerate_kimi() {
        let temp = tempdir().unwrap();
        let kimi_dir = temp.path().join(".kimi").join("sessions");
        std::fs::create_dir_all(&kimi_dir).unwrap();

        let log_file = kimi_dir.join("wire.jsonl");
        let mut f = File::create(&log_file).unwrap();
        writeln!(f, "{{\"role\":\"user\",\"content\":\"Launch Kimi coder\"}}").unwrap();

        let adapter = KimiAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
        };

        let detection = adapter.detect(&options).unwrap();
        assert!(detection.is_present);

        let enumerated = adapter.enumerate(&options).unwrap();
        assert_eq!(enumerated.len(), 1);
        assert_eq!(enumerated[0].adapter_name, "kimi");
    }

    #[test]
    fn test_parse_kimi_wire_session_with_subagents() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"role":"user","timestamp":"2026-08-30T14:00:00Z","content":"Build an automated parser with subagents"}
{"role":"assistant","timestamp":"2026-08-30T14:00:02Z","model":"kimi-k1.5","usage":{"prompt_tokens":520,"completion_tokens":180,"cached_tokens":200},"reasoning_content":"Delegating schema analysis to subagent...","content":"Delegating schema validation to subagent."}
{"role":"subagent_delegation","timestamp":"2026-08-30T14:00:03Z","agent_role":"schema_specialist","task":"Validate JSON schemas against draft-07"}
{"role":"tool_call","timestamp":"2026-08-30T14:00:05Z","id":"tc_term_1","name":"terminal","arguments":{"command":"cargo test --test parser"}}
{"role":"tool_result","timestamp":"2026-08-30T14:00:08Z","tool_call_id":"tc_term_1","output":"test result: ok. 12 passed; 0 failed"}
{"role":"assistant","timestamp":"2026-08-30T14:00:10Z","content":"Subagent completed schema validation and test suite passed."}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = KimiAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 0);
        let trace = result.trace;
        assert_eq!(trace.adapter, "kimi");
        assert_eq!(trace.stats.models_used, vec!["kimi-k1.5".to_string()]);
        assert_eq!(trace.stats.token_usage.input_tokens, 520);
        assert_eq!(trace.stats.token_usage.output_tokens, 180);
        assert_eq!(trace.stats.token_usage.cache_read_tokens, 200);
        assert_eq!(trace.stats.token_usage.total(), 900);
        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.assistant_messages_count, 2);

        // Verify subagent delegation detected
        let subagent_evt = trace
            .events
            .iter()
            .find(|e| matches!(&e.payload, EventPayload::Custom { kind, .. } if kind == "subagent_delegation"));
        assert!(subagent_evt.is_some());
    }

    #[test]
    fn test_parse_kimi_corrupt_lines_graceful() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = "{\"role\":\"user\",\"content\":\"Hi Kimi\"}\n{INVALID_WIRE_JSON}\n{\"role\":\"assistant\",\"content\":\"Hello\"}\n";
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = KimiAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 1);
        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
    }
}
