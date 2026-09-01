use std::fs::File;
use std::io::{BufRead, BufReader};
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

/// Adapter for discovering and normalizing Google Gemini / Antigravity agent sessions.
pub struct GeminiAdapter;

impl Default for GeminiAdapter {
    fn default() -> Self {
        Self
    }
}

impl GeminiAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Candidate directory paths for Google Antigravity (agy / Antigravity IDE / Gemini) on the host machine.
    pub fn candidate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".gemini").join("antigravity-cli").join("brain"));
            roots.push(home.join(".gemini").join("antigravity-ide").join("brain"));
            roots.push(home.join(".gemini").join("history"));
            roots.push(home.join(".gemini").join("sessions"));
            roots.push(home.join(".antigravity").join("sessions"));
            roots.push(home.join(".config").join("antigravity"));
            roots.push(home.join(".config").join("gemini"));
            roots.push(home.join(".gemini"));
        }
        roots.push(PathBuf::from(".gemini"));
        roots.push(PathBuf::from(".antigravity"));
        roots
    }
}

/// Determines whether a transcript path belongs to Google Antigravity or Gemini CLI.
pub fn detect_product_identity(path: &Path) -> &'static str {
    let p = path.to_string_lossy().to_lowercase();
    if p.contains("antigravity") || p.contains("/brain/") || p.contains(".antigravity") {
        "antigravity"
    } else {
        "gemini"
    }
}

impl AgentAdapter for GeminiAdapter {
    fn name(&self) -> &'static str {
        "gemini"
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
            if custom.ends_with(".gemini") || s.contains("gemini") || s.contains("antigravity") {
                discovered.push(custom.clone());
            } else if custom.is_dir() {
                // custom_paths may point at a generic parent directory rather than
                // the adapter-specific dir itself; look a few levels in before
                // giving up, matching how `enumerate()` already recurses.
                let mut found_nested = false;
                for sub in &[
                    custom.join(".gemini"),
                    custom.join(".antigravity"),
                    custom.join(".config").join("gemini"),
                    custom.join(".config").join("antigravity"),
                ] {
                    if sub.exists() {
                        discovered.push(sub.clone());
                        found_nested = true;
                    }
                }
                if !found_nested {
                    for entry in WalkDir::new(custom).max_depth(4).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        let ps = path.to_string_lossy().to_lowercase();
                        if ps.contains("gemini") || ps.contains("antigravity") {
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
                    || name == "tasks"
                    || name == "scratch"
                    || name == "steps"
                    || name == "plugins"
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
                if is_candidate_gemini_file(&root) {
                    let adapter_name = detect_product_identity(&root);
                    if let Ok(source) = SessionSource::from_path(&root, adapter_name) {
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
                    if path.is_file() && is_candidate_gemini_file(path) {
                        let adapter_name = detect_product_identity(path);
                        if let Ok(source) = SessionSource::from_path(path, adapter_name) {
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
        let file = File::open(&source.path)?;
        let reader = BufReader::new(file);

        let adapter_identity = detect_product_identity(&source.path);
        let session_id = derive_session_id(&source.path);
        let provenance = Provenance::new(
            source.path.to_string_lossy().to_string(),
            adapter_identity,
            source.file_size_bytes,
            source.mtime_epoch_secs,
            &source.fingerprint,
        );

        let mut trace = AgentWorthTrace::new(&session_id, adapter_identity, provenance, Utc::now());
        let mut malformed_lines = 0;
        let mut warnings = Vec::new();
        let mut sequence = 0u64;
        let mut last_model: Option<String> = None;

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

            let events = parse_gemini_record(&val, &mut sequence, timestamp, line_num, &mut last_model);
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

fn is_candidate_gemini_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    if !path_str.contains("gemini") && !path_str.contains("antigravity") {
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
        || lower == "oauth.json"
        || lower == "oauth_creds.json"
        || lower == "trustedfolders.json"
        || lower == "projects.json"
        || lower == "google_accounts.json"
        || lower == "state.json"
        || lower == "package.json"
        || lower == "package-lock.json"
        || lower == "tsconfig.json"
        || lower == "hooks.json"
        || lower == "mcp_config.json"
        || lower == "import_manifest.json"
        || lower == "manifest.json"
    {
        return false;
    }
    path.extension()
        .is_some_and(|ext| ext == "jsonl" || ext == "json")
}

fn derive_session_id(path: &Path) -> String {
    // If inside brain/<conversation-id>/..., try to extract conversation-id
    let components: Vec<_> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();

    for (i, part) in components.iter().enumerate() {
        if (part == "brain" || part == "sessions" || part == "history") && i + 1 < components.len()
        {
            let next_comp = &components[i + 1];
            if !next_comp.is_empty() && !next_comp.starts_with('.') {
                return next_comp.clone();
            }
        }
    }

    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

fn parse_timestamp(val: &Value) -> Option<DateTime<Utc>> {
    if let Some(ts_str) = val.get("created_at").and_then(|v| v.as_str()) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    if let Some(ts_str) = val.get("timestamp").and_then(|v| v.as_str()) {
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
        .get("promptTokenCount")
        .or_else(|| usage_val.get("input_tokens"))
        .or_else(|| usage_val.get("prompt_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let output_tokens = usage_val
        .get("candidatesTokenCount")
        .or_else(|| usage_val.get("output_tokens"))
        .or_else(|| usage_val.get("completion_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let cache_read_tokens = usage_val
        .get("cachedContentTokenCount")
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

fn parse_gemini_record(
    val: &Value,
    seq: &mut u64,
    ts: DateTime<Utc>,
    line_num: usize,
    last_model: &mut Option<String>,
) -> Vec<NormalizedEvent> {
    let mut events = Vec::new();
    let raw_ref = format!("line:{}", line_num);

    // Model invocation / Token usage extraction
    if let Some(usage_val) = val
        .get("usageMetadata")
        .or_else(|| val.get("usage"))
        .or_else(|| val.get("tokens"))
    {
        let usage = extract_token_usage(usage_val);
        if usage.total() > 0 {
            let model = val
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("gemini-2.5-pro")
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
                        latency_ms: val.get("latency_ms").and_then(|d| d.as_u64()),
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    // Direct error check
    let is_step_error = val
        .get("status")
        .and_then(|s| s.as_str())
        .map(|s| s == "ERROR")
        .unwrap_or(false);

    if is_step_error {
        let msg = val
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("Step failed with ERROR");
        *seq += 1;
        events.push(
            NormalizedEvent::new(
                *seq,
                ts,
                EventPayload::Error {
                    message: msg.to_string(),
                    is_recovered: false,
                },
            )
            .with_raw_ref(&raw_ref),
        );
    }

    let raw_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let raw_role = val.get("role").and_then(|v| v.as_str()).unwrap_or("");
    let effective_type = if !raw_type.is_empty() {
        raw_type
    } else {
        raw_role
    };

    match effective_type {
        "USER_INPUT" | "USER_EXPLICIT" | "user" | "human" => {
            let content = extract_gemini_content(val);
            *seq += 1;
            events.push(
                NormalizedEvent::new(*seq, ts, EventPayload::UserMessage { content })
                    .with_raw_ref(&raw_ref),
            );
        }

        "PLANNER_RESPONSE" | "MODEL" | "assistant" | "model" => {
            let thinking = val
                .get("thinking")
                .and_then(|v| v.as_str())
                .map(String::from);

            let content = extract_gemini_content(val);

            // Extract tool calls / function calls
            if let Some(tool_calls_arr) = val
                .get("tool_calls")
                .or_else(|| val.get("function_calls"))
                .and_then(|v| v.as_array())
            {
                for tc in tool_calls_arr {
                    let id = tc.get("id").and_then(|v| v.as_str()).map(String::from);
                    let raw_name = tc
                        .get("name")
                        .or_else(|| tc.get("functionName"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();

                    let args = tc
                        .get("arguments")
                        .or_else(|| tc.get("args"))
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

                    process_specific_gemini_tool_call(&raw_name, &name, &args, seq, ts, &raw_ref, &mut events);
                }
            }

            // Also check parts with functionCall
            if let Some(parts_arr) = val.get("parts").and_then(|v| v.as_array()) {
                for part in parts_arr {
                    if let Some(fc) = part.get("functionCall") {
                        let raw_name = fc
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let args = fc.get("args").cloned().unwrap_or(Value::Null);
                        let name = normalize_mcp_tool_name(&raw_name, &args);

                        *seq += 1;
                        events.push(
                            NormalizedEvent::new(
                                *seq,
                                ts,
                                EventPayload::ToolCall(ToolCall {
                                    id: None,
                                    name: name.clone(),
                                    arguments: args.clone(),
                                }),
                            )
                            .with_raw_ref(&raw_ref),
                        );

                        process_specific_gemini_tool_call(
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

        "TOOL_OUTPUT" | "tool_result" | "function_response" | "tool" | "function" => {
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
                .or_else(|| val.get("response"))
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
                                summary: "Git commit observed in tool result".to_string(),
                                confidence: 0.85,
                            }),
                        )
                        .with_raw_ref(&raw_ref),
                    );
                }
            }
        }

        _ => {
            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::Custom {
                        kind: effective_type.to_string(),
                        data: val.clone(),
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    events
}

fn process_specific_gemini_tool_call(
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
    if lower_raw == "run_command"
        || lower_raw == "bash"
        || lower_raw == "exec"
        || lower_raw == "shell"
        || lower_name.ends_with(":run_command")
        || lower_name.ends_with(":bash")
        || lower_name.ends_with(":shell")
        || lower_name.ends_with(":exec")
    {
        if let Some(cmd) = args
            .get("CommandLine")
            .or_else(|| args.get("command"))
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
                        cwd: args
                            .get("Cwd")
                            .or_else(|| args.get("cwd"))
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        exit_code: None,
                        output: None,
                    }),
                )
                .with_raw_ref(raw_ref),
            );
        }
    } else if lower_raw == "replace_file_content"
        || lower_raw == "write_to_file"
        || lower_raw == "edit"
        || lower_raw == "edit_file"
        || lower_name.ends_with(":replace_file_content")
        || lower_name.ends_with(":write_to_file")
        || lower_name.ends_with(":edit_file")
        || lower_name.ends_with(":edit")
    {
        let path = args
            .get("TargetFile")
            .or_else(|| args.get("target_file"))
            .or_else(|| args.get("path"))
            .or_else(|| args.get("file_path"))
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
                            .get("ReplacementContent")
                            .or_else(|| args.get("diff"))
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

fn extract_gemini_content(val: &Value) -> String {
    if let Some(text) = val.get("content").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    if let Some(parts) = val.get("parts").and_then(|v| v.as_array()) {
        let mut texts = Vec::new();
        for p in parts {
            if let Some(t) = p.get("text").and_then(|v| v.as_str()) {
                texts.push(t);
            }
        }
        if !texts.is_empty() {
            return texts.join("\n");
        }
    }
    if let Some(c) = val.get("content") {
        if !c.is_null() {
            return c.to_string();
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
    fn test_parse_standard_gemini_jsonl() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"type":"USER_INPUT","created_at":"2026-08-29T10:00:00Z","content":"Implement a new feature"}
{"type":"PLANNER_RESPONSE","created_at":"2026-08-29T10:00:05Z","model":"gemini-2.5-pro","thinking":"I will run cargo check first.","usageMetadata":{"promptTokenCount":600,"candidatesTokenCount":180,"cachedContentTokenCount":150},"tool_calls":[{"id":"call_1","name":"run_command","arguments":{"CommandLine":"cargo test","Cwd":"/workspace"}}],"content":"Running test suite..."}
{"type":"TOOL_OUTPUT","created_at":"2026-08-29T10:00:08Z","call_id":"call_1","output":"test result: ok. 8 passed; 0 failed","is_error":false}
{"type":"PLANNER_RESPONSE","created_at":"2026-08-29T10:00:10Z","content":"Feature implemented and all tests pass."}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = GeminiAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 0);
        let trace = result.trace;
        // A bare tempfile path carries no antigravity signature (no `/brain/`, no
        // `.antigravity`), so detect_product_identity() correctly resolves this to the
        // "gemini" default rather than the old hardcoded "antigravity" name.
        assert_eq!(trace.adapter, "gemini");
        assert_eq!(trace.stats.models_used, vec!["gemini-2.5-pro".to_string()]);
        assert_eq!(trace.stats.token_usage.input_tokens, 600);
        assert_eq!(trace.stats.token_usage.output_tokens, 180);
        assert_eq!(trace.stats.token_usage.cache_read_tokens, 150);
        assert_eq!(trace.stats.token_usage.total(), 930);
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(trace.stats.tools_used.get("run_command"), Some(&1));
        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.assistant_messages_count, 2);
    }

    #[test]
    fn test_parse_gemini_chat_parts_format() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"role":"user","timestamp":"2026-08-29T11:00:00Z","parts":[{"text":"Please edit file"}]}
{"role":"model","timestamp":"2026-08-29T11:00:02Z","model":"gemini-3.7-sonnet","tokens":{"prompt_tokens":300,"completion_tokens":90},"parts":[{"functionCall":{"name":"replace_file_content","args":{"TargetFile":"/src/main.rs","ReplacementContent":"fn main() {}"}}},{"text":"File replaced."}]}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = GeminiAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        let trace = result.trace;
        assert_eq!(
            trace.stats.models_used,
            vec!["gemini-3.7-sonnet".to_string()]
        );
        assert_eq!(trace.stats.token_usage.input_tokens, 300);
        assert_eq!(trace.stats.token_usage.output_tokens, 90);
        assert_eq!(trace.stats.tool_calls_count, 1);
    }

    #[test]
    fn test_parse_graceful_on_empty_and_corrupt_lines() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = "{\"type\":\"USER_INPUT\",\"content\":\"hello\"}\n\n{INVALID_JSON}\n{\"type\":\"PLANNER_RESPONSE\",\"content\":\"world\"}\n";
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = GeminiAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 1);
        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
    }

    #[test]
    fn test_detect_and_enumerate_gemini_and_antigravity() {
        let temp = tempdir().unwrap();

        // 1. Gemini CLI history
        let gemini_dir = temp.path().join(".gemini").join("history");
        std::fs::create_dir_all(&gemini_dir).unwrap();
        let gemini_file = gemini_dir.join("history_001.jsonl");
        let mut f1 = File::create(&gemini_file).unwrap();
        writeln!(f1, "{{\"type\":\"USER_INPUT\",\"content\":\"gemini prompt\"}}").unwrap();

        // 2. Antigravity CLI brain trajectory
        let agy_dir = temp.path().join(".gemini").join("antigravity-cli").join("brain").join("sess-1").join(".system_generated").join("logs");
        std::fs::create_dir_all(&agy_dir).unwrap();
        let agy_file = agy_dir.join("transcript.jsonl");
        let mut f2 = File::create(&agy_file).unwrap();
        writeln!(f2, "{{\"type\":\"USER_INPUT\",\"content\":\"agy prompt\"}}").unwrap();

        let adapter = GeminiAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
        };

        let detection = adapter.detect(&options).unwrap();
        assert!(detection.is_present);

        let enumerated = adapter.enumerate(&options).unwrap();
        assert_eq!(enumerated.len(), 2);

        let gemini_src = enumerated.iter().find(|s| s.path == gemini_file).unwrap();
        assert_eq!(gemini_src.adapter_name, "gemini");

        let agy_src = enumerated.iter().find(|s| s.path == agy_file).unwrap();
        assert_eq!(agy_src.adapter_name, "antigravity");

        // Parsing verifies trace.adapter is correctly assigned
        let gemini_parsed = adapter.parse(gemini_src).unwrap();
        assert_eq!(gemini_parsed.trace.adapter, "gemini");

        let agy_parsed = adapter.parse(agy_src).unwrap();
        assert_eq!(agy_parsed.trace.adapter, "antigravity");
    }

    #[test]
    fn test_parse_gemini_mcp_tool_call() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"type":"USER_INPUT","created_at":"2026-08-29T10:00:00Z","content":"Browse webpage"}
{"type":"PLANNER_RESPONSE","created_at":"2026-08-29T10:00:02Z","model":"gemini-2.5-pro","tool_calls":[{"id":"call_mcp_1","name":"call_mcp_tool","arguments":{"ServerName":"chrome-devtools","ToolName":"navigate_page","Arguments":{"Url":"https://example.com"}}}]}
{"type":"TOOL_OUTPUT","created_at":"2026-08-29T10:00:04Z","call_id":"call_mcp_1","name":"call_mcp_tool","output":"Navigated successfully"}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = GeminiAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        let trace = result.trace;
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(
            trace.stats.tools_used.get("mcp:chrome-devtools:navigate_page"),
            Some(&1)
        );
    }
}

