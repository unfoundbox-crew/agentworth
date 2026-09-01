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

/// Adapter for discovering and normalizing Herdr multi-agent coordination traces.
pub struct HerdrAdapter;

impl Default for HerdrAdapter {
    fn default() -> Self {
        Self
    }
}

impl HerdrAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Candidate directory paths for Herdr on the host machine.
    pub fn candidate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".config").join("herdr"));
            roots.push(home.join(".config").join("herdr").join("sessions"));
            roots.push(home.join(".herdr"));
            roots.push(home.join(".herdr").join("sessions"));
        }
        roots.push(PathBuf::from(".config").join("herdr"));
        roots.push(PathBuf::from(".herdr"));
        roots.push(PathBuf::from(".herdr").join("sessions"));
        roots
    }
}

impl AgentAdapter for HerdrAdapter {
    fn name(&self) -> &'static str {
        "herdr"
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
            if s.contains(".herdr") || s.contains("herdr") {
                discovered.push(custom.clone());
            } else if custom.is_dir() {
                // A caller can pass a parent directory (a repo root, a tempdir in tests)
                // rather than the herdr directory itself -- look one level down before
                // giving up, matching how `enumerate()` already recurses for this adapter.
                for sub in &[custom.join(".herdr"), custom.join(".config").join("herdr")] {
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
                    if is_candidate_herdr_file(custom) {
                        if let Ok(source) = SessionSource::from_path(custom, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if custom.is_dir() {
                    for entry in WalkDir::new(custom).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_herdr_file(path) {
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
                    if is_candidate_herdr_file(&root) {
                        if let Ok(source) = SessionSource::from_path(&root, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if root.is_dir() {
                    for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_herdr_file(path) {
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
                    } else if let Some(traces) = json_val
                        .get("coordination_traces")
                        .or_else(|| json_val.get("traces"))
                        .and_then(|t| t.as_array())
                    {
                        traces.clone()
                    } else if let Some(events) = json_val
                        .get("events")
                        .or_else(|| json_val.get("messages"))
                        .and_then(|e| e.as_array())
                    {
                        events.clone()
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

                        let evts = parse_herdr_record(item, &mut sequence, timestamp, idx + 1);
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

                let events = parse_herdr_record(&val, &mut sequence, timestamp, line_num);
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

fn is_candidate_herdr_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    if !path_str.contains("herdr") {
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

fn parse_herdr_record(
    val: &Value,
    seq: &mut u64,
    ts: DateTime<Utc>,
    line_num: usize,
) -> Vec<NormalizedEvent> {
    let mut events = Vec::new();
    let raw_ref = format!("line:{}", line_num);

    let role = val
        .get("role")
        .or_else(|| val.get("agent_role"))
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
                .or_else(|| val.get("agent_model"))
                .and_then(|v| v.as_str())
                .unwrap_or("herdr-agent-model")
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

    // Coordination / Agent delegation handling (capturing multi-agent DAG hierarchies)
    if let Some(delegation_payload) = extract_delegation_payload(val) {
        *seq += 1;
        events.push(
            NormalizedEvent::new(
                *seq,
                ts,
                EventPayload::Custom {
                    kind: "coordination_delegation".to_string(),
                    data: delegation_payload,
                },
            )
            .with_raw_ref(&raw_ref),
        );
    }

    match role {
        "user" | "human" | "supervisor" => {
            let content = extract_herdr_content(val);
            *seq += 1;
            events.push(
                NormalizedEvent::new(*seq, ts, EventPayload::UserMessage { content })
                    .with_raw_ref(&raw_ref),
            );
        }

        "assistant" | "worker" | "agent" | "subagent" => {
            let thinking = val
                .get("thinking")
                .or_else(|| val.get("reasoning"))
                .or_else(|| val.get("plan"))
                .and_then(|v| v.as_str())
                .map(String::from);

            let content = extract_herdr_content(val);

            // Extract tool calls
            if let Some(tools) = val
                .get("tool_calls")
                .or_else(|| val.get("actions"))
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
                        .or_else(|| tc.get("parameters"))
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

                    process_specific_herdr_tool_call(&name, &args, seq, ts, &raw_ref, &mut events);
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

        "tool_call" | "action" => {
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

            process_specific_herdr_tool_call(&name, &args, seq, ts, &raw_ref, &mut events);
        }

        "tool_result" | "action_result" => {
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
                                summary: "Test suite executed successfully in Herdr agent"
                                    .to_string(),
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
                .unwrap_or("Herdr coordination error")
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

fn extract_delegation_payload(val: &Value) -> Option<Value> {
    let delegation_obj = val
        .get("delegation")
        .or_else(|| val.get("handoff"))
        .or_else(|| val.get("subagent_delegation"));

    let parent_id = val
        .get("parent_agent_id")
        .or_else(|| val.get("parent_id"))
        .or_else(|| val.get("parent"))
        .or_else(|| val.get("caller_id"))
        .or_else(|| val.get("caller"))
        .or_else(|| val.get("supervisor_id"))
        .or_else(|| val.get("from_agent"))
        .or_else(|| val.get("from"))
        .or_else(|| {
            delegation_obj.and_then(|d| {
                d.get("parent_agent_id")
                    .or_else(|| d.get("parent_id"))
                    .or_else(|| d.get("parent"))
                    .or_else(|| d.get("caller_id"))
                    .or_else(|| d.get("caller"))
                    .or_else(|| d.get("from"))
            })
        })
        .and_then(|v| v.as_str());

    let child_id = val
        .get("child_agent_id")
        .or_else(|| val.get("subagent_id"))
        .or_else(|| val.get("child_id"))
        .or_else(|| val.get("subagent"))
        .or_else(|| val.get("worker_id"))
        .or_else(|| val.get("to_agent"))
        .or_else(|| val.get("to"))
        .or_else(|| {
            delegation_obj.and_then(|d| {
                d.get("child_agent_id")
                    .or_else(|| d.get("subagent_id"))
                    .or_else(|| d.get("child_id"))
                    .or_else(|| d.get("subagent"))
                    .or_else(|| d.get("to"))
            })
        })
        .and_then(|v| v.as_str());

    let delegation_id = val
        .get("delegation_id")
        .or_else(|| val.get("task_id"))
        .or_else(|| val.get("handoff_id"))
        .or_else(|| {
            delegation_obj.and_then(|d| {
                d.get("delegation_id")
                    .or_else(|| d.get("task_id"))
                    .or_else(|| d.get("handoff_id"))
                    .or_else(|| d.get("id"))
            })
        })
        .and_then(|v| v.as_str());

    let task = val
        .get("task")
        .or_else(|| {
            delegation_obj.and_then(|d| {
                d.get("task")
                    .or_else(|| d.get("prompt"))
                    .or_else(|| d.get("instructions"))
            })
        })
        .and_then(|v| v.as_str());

    let depth = val
        .get("depth")
        .or_else(|| val.get("dag_level"))
        .or_else(|| delegation_obj.and_then(|d| d.get("depth").or_else(|| d.get("level"))))
        .and_then(|v| v.as_u64());

    if parent_id.is_some()
        || child_id.is_some()
        || delegation_id.is_some()
        || delegation_obj.is_some()
    {
        let mut map = serde_json::Map::new();
        if let Some(p) = parent_id {
            map.insert("parent_agent_id".to_string(), Value::String(p.to_string()));
        }
        if let Some(c) = child_id {
            map.insert("child_agent_id".to_string(), Value::String(c.to_string()));
        }
        if let Some(d) = delegation_id {
            map.insert("delegation_id".to_string(), Value::String(d.to_string()));
        }
        if let Some(t) = task {
            map.insert("task".to_string(), Value::String(t.to_string()));
        }
        if let Some(dp) = depth {
            map.insert("depth".to_string(), Value::Number(dp.into()));
        }
        if let Some(d_val) = delegation_obj {
            map.insert("details".to_string(), d_val.clone());
        }

        Some(Value::Object(map))
    } else {
        None
    }
}

fn process_specific_herdr_tool_call(
    name: &str,
    args: &Value,
    seq: &mut u64,
    ts: DateTime<Utc>,
    raw_ref: &str,
    events: &mut Vec<NormalizedEvent>,
) {
    let lower = name.to_lowercase();
    if lower.contains("subagent")
        || lower.contains("delegate")
        || lower.contains("spawn")
        || lower.contains("worker")
    {
        let parent = args
            .get("parent_agent_id")
            .or_else(|| args.get("parent_id"))
            .or_else(|| args.get("caller_id"))
            .and_then(|v| v.as_str());
        let child = args
            .get("child_agent_id")
            .or_else(|| args.get("subagent_id"))
            .or_else(|| args.get("child_id"))
            .or_else(|| args.get("subagent"))
            .or_else(|| args.get("role"))
            .or_else(|| args.get("TypeName"))
            .and_then(|v| v.as_str());
        let delegation_id = args
            .get("delegation_id")
            .or_else(|| args.get("task_id"))
            .or_else(|| args.get("id"))
            .and_then(|v| v.as_str());
        let task = args
            .get("task")
            .or_else(|| args.get("prompt"))
            .or_else(|| args.get("Prompt"))
            .and_then(|v| v.as_str());

        let mut d_map = serde_json::Map::new();
        if let Some(p) = parent {
            d_map.insert("parent_agent_id".to_string(), Value::String(p.to_string()));
        }
        if let Some(c) = child {
            d_map.insert("child_agent_id".to_string(), Value::String(c.to_string()));
        }
        if let Some(d) = delegation_id {
            d_map.insert("delegation_id".to_string(), Value::String(d.to_string()));
        }
        if let Some(t) = task {
            d_map.insert("task".to_string(), Value::String(t.to_string()));
        }
        d_map.insert("tool".to_string(), Value::String(name.to_string()));
        d_map.insert("arguments".to_string(), args.clone());

        *seq += 1;
        events.push(
            NormalizedEvent::new(
                *seq,
                ts,
                EventPayload::Custom {
                    kind: "coordination_delegation".to_string(),
                    data: Value::Object(d_map),
                },
            )
            .with_raw_ref(raw_ref),
        );
    }

    if lower.contains("shell")
        || lower.contains("bash")
        || lower.contains("exec")
        || lower.contains("cmd")
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
    } else if lower.contains("file")
        || lower.contains("edit")
        || lower.contains("write")
        || lower.contains("patch")
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

fn extract_herdr_content(val: &Value) -> String {
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
    fn test_detect_and_enumerate_herdr() {
        let temp = tempdir().unwrap();
        let herdr_dir = temp.path().join(".config").join("herdr").join("sessions");
        std::fs::create_dir_all(&herdr_dir).unwrap();

        let session_file = herdr_dir.join("session_001.jsonl");
        let mut f = File::create(&session_file).unwrap();
        writeln!(f, "{{\"role\":\"user\",\"content\":\"Coordinate agents\"}}").unwrap();

        let adapter = HerdrAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
        };

        let detection = adapter.detect(&options).unwrap();
        assert!(detection.is_present);

        let enumerated = adapter.enumerate(&options).unwrap();
        assert_eq!(enumerated.len(), 1);
        assert_eq!(enumerated[0].adapter_name, "herdr");
    }

    #[test]
    fn test_parse_standard_herdr_jsonl() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"role":"supervisor","timestamp":"2026-08-29T10:00:00Z","content":"Orchestrate swarm tasks"}
{"role":"worker","timestamp":"2026-08-29T10:00:03Z","model":"herdr-worker-v1","usage":{"input_tokens":300,"output_tokens":100},"delegation":{"subagent":"code_writer","task":"implement module"},"tool_calls":[{"name":"bash","arguments":{"command":"cargo test"}}]}
{"type":"tool_result","timestamp":"2026-08-29T10:00:06Z","output":"test result: ok. 12 passed; 0 failed"}
{"role":"worker","timestamp":"2026-08-29T10:00:08Z","content":"All coordination tasks completed."}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = HerdrAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 0);
        let trace = result.trace;
        assert_eq!(trace.adapter, "herdr");
        assert_eq!(trace.stats.models_used, vec!["herdr-worker-v1".to_string()]);
        assert_eq!(trace.stats.token_usage.input_tokens, 300);
        assert_eq!(trace.stats.token_usage.output_tokens, 100);
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.assistant_messages_count, 1);
    }

    #[test]
    fn test_parse_herdr_session_json() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"{
  "session_id": "herdr-sess-101",
  "coordination_traces": [
    {"role": "user", "content": "Start cluster build"},
    {"role": "agent", "model": "herdr-swarm", "usage": {"input_tokens": 150, "output_tokens": 60}, "content": "Cluster nodes initialized"}
  ]
}"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = HerdrAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        let trace = result.trace;
        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.assistant_messages_count, 1);
        assert_eq!(trace.stats.token_usage.total(), 210);
    }

    #[test]
    fn test_parse_graceful_on_malformed_lines() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = "{\"role\":\"user\",\"content\":\"hello\"}\n{CORRUPT_HERDR_JSON}\n{\"role\":\"agent\",\"content\":\"hi\"}\n";
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = HerdrAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 1);
        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
    }

    #[test]
    fn test_parse_herdr_multi_agent_dag_delegation() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"role":"supervisor","parent_agent_id":"root_coordinator","child_agent_id":"worker_1","delegation_id":"del_100","depth":1,"task":"Spawn indexing workers"}
{"role":"worker","parent_agent_id":"worker_1","child_agent_id":"leaf_worker_a","delegation_id":"del_101","depth":2,"task":"Index shard A","tool_calls":[{"name":"invoke_subagent","arguments":{"parent_id":"worker_1","subagent_id":"leaf_worker_a","task":"Process shard"}}]}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = HerdrAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        let trace = result.trace;
        let delegations: Vec<_> = trace
            .events
            .iter()
            .filter(|e| match &e.payload {
                EventPayload::Custom { kind, .. } => kind == "coordination_delegation",
                _ => false,
            })
            .collect();

        // 2 record delegations + 1 tool call delegation = 3 delegation events
        assert_eq!(delegations.len(), 3);

        if let EventPayload::Custom { data, .. } = &delegations[0].payload {
            assert_eq!(data.get("parent_agent_id").unwrap(), "root_coordinator");
            assert_eq!(data.get("child_agent_id").unwrap(), "worker_1");
            assert_eq!(data.get("delegation_id").unwrap(), "del_100");
            assert_eq!(data.get("depth").unwrap(), 1);
        } else {
            panic!("Expected custom event payload");
        }
    }
}
