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
use crate::normalize_mcp_tool_name;

/// Adapter for discovering and normalizing DeepSeek (DeepSeek-Coder, R1, V3) agent traces.
pub struct DeepSeekAdapter;

impl Default for DeepSeekAdapter {
    fn default() -> Self {
        Self
    }
}

impl DeepSeekAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Candidate directory paths for DeepSeek on the host machine.
    pub fn candidate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".deepseek"));
            roots.push(home.join(".deepseek").join("sessions"));
            roots.push(home.join(".deepseek").join("logs"));
            roots.push(home.join(".deepseek-coder"));
            roots.push(home.join(".deepseek-coder").join("sessions"));
            roots.push(home.join(".deepseek-coder").join("logs"));
            roots.push(home.join(".config").join("deepseek"));
            roots.push(home.join(".config").join("deepseek-coder"));
        }
        roots.push(PathBuf::from(".deepseek"));
        roots.push(PathBuf::from(".deepseek").join("sessions"));
        roots.push(PathBuf::from(".deepseek-coder"));
        roots.push(PathBuf::from(".deepseek-coder").join("sessions"));
        roots
    }
}

impl AgentAdapter for DeepSeekAdapter {
    fn name(&self) -> &'static str {
        "deepseek"
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
                if s.contains(".deepseek")
                    || s.contains("deepseek")
                    || s.contains("deepseek-coder")
                    || s.contains("deepseek_r1")
                {
                    discovered.push(custom.clone());
                } else if custom.is_dir() {
                    for sub in &[
                        custom.join(".deepseek"),
                        custom.join(".deepseek-coder"),
                        custom.join(".config").join("deepseek"),
                    ] {
                        if sub.exists() {
                            discovered.push(sub.clone());
                        }
                    }
                    if discovered.is_empty() {
                        for entry in WalkDir::new(custom).max_depth(4).into_iter().filter_map(|e| e.ok()) {
                            let path = entry.path();
                            let s = path.to_string_lossy().to_lowercase();
                            if s.contains(".deepseek") || s.contains("deepseek") {
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
                    if is_candidate_deepseek_file(custom) {
                        if let Ok(source) = SessionSource::from_path(custom, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if custom.is_dir() {
                    for entry in WalkDir::new(custom).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_deepseek_file(path) {
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
                    if is_candidate_deepseek_file(&root) {
                        if let Ok(source) = SessionSource::from_path(&root, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if root.is_dir() {
                    for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_deepseek_file(path) {
                            if let Ok(source) = SessionSource::from_path(path, self.name()) {
                                sources.push(source);
                            }
                        }
                    }
                }
            }
        }

        // Deduplicate sources by path
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
                        .or_else(|| json_val.get("steps"))
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

                        let evts = parse_deepseek_record(item, &mut sequence, timestamp, idx + 1, &mut last_model);
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

                let events = parse_deepseek_record(&val, &mut sequence, timestamp, line_num, &mut last_model);
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

fn is_candidate_deepseek_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    if !path_str.contains("deepseek") {
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
        .get("prompt_cache_hit_tokens")
        .or_else(|| {
            usage_val
                .get("prompt_tokens_details")
                .and_then(|d| d.get("cached_tokens"))
        })
        .or_else(|| usage_val.get("cached_tokens"))
        .or_else(|| usage_val.get("cache_read_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let cache_creation_tokens = usage_val
        .get("prompt_cache_miss_tokens")
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

fn parse_deepseek_record(
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
                .unwrap_or("deepseek-coder-v2")
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
                        effort: None,
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    match role {
        "user" | "human" | "user_message" => {
            let content = extract_deepseek_content(val);
            *seq += 1;
            events.push(
                NormalizedEvent::new(*seq, ts, EventPayload::UserMessage { content })
                    .with_raw_ref(&raw_ref),
            );
        }

        "assistant" | "model" | "deepseek" | "assistant_message" => {
            // Support R1 reasoning streams: reasoning_content, thought, thoughts, thinking
            let thinking = val
                .get("reasoning_content")
                .or_else(|| val.get("thought"))
                .or_else(|| val.get("thoughts"))
                .or_else(|| val.get("reasoning"))
                .or_else(|| val.get("thinking"))
                .and_then(|v| v.as_str())
                .map(String::from);

            let content = extract_deepseek_content(val);

            // Extract tool_calls / function_calls
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

                    process_specific_deepseek_tool_call(
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

            process_specific_deepseek_tool_call(&raw_name, &name, &args, seq, ts, &raw_ref, &mut events);
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

            // Infer outcome evidence from tool output
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
                                summary: "Test suite executed successfully in DeepSeek".to_string(),
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
                                summary: "Git commit observed in DeepSeek tool output".to_string(),
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
                .unwrap_or("DeepSeek execution error")
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

fn process_specific_deepseek_tool_call(
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
        || lower_raw.contains("shell")
        || lower_raw.contains("exec")
        || lower_raw.contains("run_command")
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
    } else if lower_raw.contains("edit")
        || lower_raw.contains("write")
        || lower_raw.contains("modify")
        || lower_raw.contains("str_replace")
        || lower_raw.contains("patch")
        || lower_name.ends_with(":edit")
        || lower_name.ends_with(":write")
        || lower_name.ends_with(":write_file")
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
                            .or_else(|| args.get("replacement"))
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
        || lower_raw.contains("cat")
        || lower_name.ends_with(":read_file")
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
    } else if lower_raw.contains("delete") || lower_raw.contains("remove") {
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
                        action: FileActionType::Delete,
                        diff: None,
                        lines_changed: None,
                    },
                )
                .with_raw_ref(raw_ref),
            );
        }
    }
}

fn extract_deepseek_content(val: &Value) -> String {
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
    fn test_detect_and_enumerate_deepseek() {
        let temp = tempdir().unwrap();
        let ds_dir = temp.path().join(".deepseek-coder").join("sessions");
        std::fs::create_dir_all(&ds_dir).unwrap();

        let log_file = ds_dir.join("ds_sess_01.jsonl");
        let mut f = File::create(&log_file).unwrap();
        writeln!(f, "{{\"role\":\"user\",\"content\":\"Refactor algorithms\"}}").unwrap();

        let adapter = DeepSeekAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
            ..Default::default()
        };

        let detection = adapter.detect(&options).unwrap();
        assert!(detection.is_present);

        let enumerated = adapter.enumerate(&options).unwrap();
        assert_eq!(enumerated.len(), 1);
        assert_eq!(enumerated[0].adapter_name, "deepseek");
    }

    #[test]
    fn test_parse_deepseek_r1_reasoning_and_tokens() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"role":"user","timestamp":"2026-08-30T09:00:00Z","content":"Optimize quicksort in Rust"}
{"role":"assistant","timestamp":"2026-08-30T09:00:05Z","model":"deepseek-r1","usage":{"prompt_tokens":450,"completion_tokens":220,"prompt_cache_hit_tokens":120,"prompt_cache_miss_tokens":330,"completion_tokens_details":{"reasoning_tokens":180}},"reasoning_content":"Let's analyze dual-pivot quicksort vs standard Hoare partition...","content":"Here is the optimized partition implementation:","tool_calls":[{"id":"call_bash_1","function":{"name":"bash","arguments":"{\"command\":\"cargo test --bin quicksort\"}"}}]}
{"role":"tool","timestamp":"2026-08-30T09:00:08Z","tool_call_id":"call_bash_1","output":"test result: ok. 8 passed; 0 failed"}
{"role":"assistant","timestamp":"2026-08-30T09:00:10Z","content":"All tests passed with zero allocations."}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = DeepSeekAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 0);
        let trace = result.trace;
        assert_eq!(trace.adapter, "deepseek");
        assert_eq!(trace.stats.models_used, vec!["deepseek-r1".to_string()]);
        assert_eq!(trace.stats.token_usage.input_tokens, 450);
        assert_eq!(trace.stats.token_usage.output_tokens, 220);
        assert_eq!(trace.stats.token_usage.cache_read_tokens, 120);
        assert_eq!(trace.stats.token_usage.cache_creation_tokens, 330);
        assert_eq!(trace.stats.token_usage.total(), 1120);
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.assistant_messages_count, 2);

        // Verify reasoning_content preserved
        let ast_msg = trace
            .events
            .iter()
            .find(|e| matches!(&e.payload, EventPayload::AssistantMessage { thinking: Some(_), .. }))
            .expect("should find assistant message with thinking");
        if let EventPayload::AssistantMessage { thinking, .. } = &ast_msg.payload {
            assert!(thinking.as_ref().unwrap().contains("dual-pivot quicksort"));
        }
    }

    #[test]
    fn test_parse_deepseek_file_actions() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"role":"user","content":"Edit file and check"}
{"role":"assistant","tool_calls":[{"function":{"name":"edit_file","arguments":{"path":"src/sort.rs","content":"fn sort() {}"}}}]}
{"role":"assistant","tool_calls":[{"function":{"name":"read_file","arguments":{"path":"src/sort.rs"}}}]}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = DeepSeekAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        let trace = result.trace;
        let file_actions: Vec<_> = trace
            .events
            .iter()
            .filter_map(|e| match &e.payload {
                EventPayload::FileAction { path, action, .. } => Some((path.clone(), *action)),
                _ => None,
            })
            .collect();

        assert_eq!(file_actions.len(), 2);
        assert_eq!(file_actions[0], ("src/sort.rs".to_string(), FileActionType::Edit));
        assert_eq!(file_actions[1], ("src/sort.rs".to_string(), FileActionType::Read));
    }

    #[test]
    fn test_parse_deepseek_corrupt_lines_graceful() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = "{\"role\":\"user\",\"content\":\"Hi\"}\n{CORRUPT_JSON_LINE}\n{\"role\":\"assistant\",\"content\":\"Hello\"}\n";
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = DeepSeekAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 1);
        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
    }
}
