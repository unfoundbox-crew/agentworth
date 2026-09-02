use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use agentworth_adapter_sdk::{
    AgentAdapter, DetectionResult, ParseResult, ScanOptions, SessionSource,
};
use agentworth_schema::{
    AgentWorthTrace, EventPayload, FileActionType, ModelSwitch, NormalizedEvent, OutcomeEvidence, OutcomeKind,
    Provenance, ShellCommand, TokenUsage, ToolCall,
};
use anyhow::Result;
use chrono::{DateTime, NaiveDateTime, Utc};
use directories::BaseDirs;
use serde_json::Value;
use walkdir::WalkDir;

use crate::normalize_mcp_tool_name;

/// Adapter for discovering and normalizing Aider pair programming histories (.aider.chat.history.md and JSON/JSONL logs).
pub struct AiderAdapter;

impl Default for AiderAdapter {
    fn default() -> Self {
        Self
    }
}

impl AiderAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Candidate directory paths and files for Aider on the host machine.
    pub fn candidate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".aider"));
            roots.push(home.join(".aider.chat.history.md"));
            roots.push(home.join(".aider").join("sessions"));
            roots.push(home.join(".aider").join("chats"));
            roots.push(home.join(".config").join("aider"));
        }
        roots.push(PathBuf::from(".aider.chat.history.md"));
        roots.push(PathBuf::from(".aider"));
        roots.push(PathBuf::from(".aider").join("sessions"));
        roots.push(PathBuf::from(".aider").join("chats"));
        roots.push(PathBuf::from(".aider.chat.history.json"));
        roots.push(PathBuf::from(".aider.chat.history.jsonl"));
        roots
    }
}

impl AgentAdapter for AiderAdapter {
    fn name(&self) -> &'static str {
        "aider"
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
                if s.contains(".aider") || s.contains("aider") || custom.is_file() {
                    discovered.push(custom.clone());
                } else if custom.is_dir() {
                    for entry in WalkDir::new(custom).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_aider_file(path) {
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
                    if is_candidate_aider_file(custom) || custom.exists() {
                        if let Ok(source) = SessionSource::from_path(custom, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if custom.is_dir() {
                    for entry in WalkDir::new(custom).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_aider_file(path) {
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
                    if is_candidate_aider_file(&root) {
                        if let Ok(source) = SessionSource::from_path(&root, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if root.is_dir() {
                    for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_aider_file(path) {
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

            // 1. Try structured JSON / JSONL parsing
            if trimmed.starts_with('[')
                || (trimmed.starts_with('{') && !trimmed.contains('\n'))
                || (trimmed.starts_with('{') && serde_json::from_str::<Value>(trimmed).is_ok())
            {
                if let Ok(json_val) = serde_json::from_str::<Value>(trimmed) {
                    let items = if let Some(arr) = json_val.as_array() {
                        arr.clone()
                    } else if let Some(messages) = json_val
                        .get("messages")
                        .or_else(|| json_val.get("history"))
                        .or_else(|| json_val.get("turns"))
                        .or_else(|| json_val.get("events"))
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

                        let evts = parse_aider_json_record(item, &mut sequence, timestamp, idx + 1, &mut last_model);
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

            // If it's JSONL lines (starts with '{')
            if trimmed.lines().next().map(|l| l.trim().starts_with('{')).unwrap_or(false) {
                let mut is_pure_jsonl = true;
                let mut jsonl_events = Vec::new();

                for (line_idx, line_str) in content_str.lines().enumerate() {
                    let line_num = line_idx + 1;
                    let trimmed_line = line_str.trim();
                    if trimmed_line.is_empty() {
                        continue;
                    }

                    match serde_json::from_str::<Value>(trimmed_line) {
                        Ok(val) => {
                            let timestamp = parse_timestamp(&val).unwrap_or_else(Utc::now);
                            if earliest_ts.is_none_or(|ts| timestamp < ts) {
                                earliest_ts = Some(timestamp);
                            }
                            if latest_ts.is_none_or(|ts| timestamp > ts) {
                                latest_ts = Some(timestamp);
                            }

                            let events = parse_aider_json_record(&val, &mut sequence, timestamp, line_num, &mut last_model);
                            jsonl_events.extend(events);
                        }
                        Err(e) => {
                            if line_idx == 0 {
                                is_pure_jsonl = false;
                                break;
                            } else {
                                malformed_lines += 1;
                                warnings.push(format!("JSON syntax error on line {}: {}", line_num, e));
                            }
                        }
                    }
                }

                if is_pure_jsonl && !jsonl_events.is_empty() {
                    trace.events.extend(jsonl_events);
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

            // 2. Markdown chat history parsing (.aider.chat.history.md)
            let md_events = parse_aider_markdown(
                &content_str,
                &mut sequence,
                &mut earliest_ts,
                &mut latest_ts,
                &mut malformed_lines,
                &mut warnings,
            );
            trace.events.extend(md_events);
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

fn is_candidate_aider_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    if !path_str.contains("aider") {
        return false;
    }
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let lower = filename.to_lowercase();

    if lower == "settings.json"
        || lower == "config.yml"
        || lower == "config.yaml"
        || lower == ".aider.conf.yml"
        || lower.ends_with(".tags.cache.v3")
    {
        return false;
    }

    if lower == ".aider.chat.history.md"
        || lower.ends_with(".aider.chat.history.md")
        || lower == ".aider.input.history"
        || lower.ends_with(".md")
        || lower.ends_with(".json")
        || lower.ends_with(".jsonl")
        || lower.ends_with(".log")
    {
        return true;
    }

    path.extension().is_some_and(|ext| {
        ext == "md" || ext == "json" || ext == "jsonl" || ext == "log"
    })
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
    if let Some(ts_num) = val.get("timestamp").and_then(|v| v.as_i64()) {
        if ts_num > 1_000_000_000_000 {
            return DateTime::from_timestamp_millis(ts_num);
        } else {
            return DateTime::from_timestamp(ts_num, 0);
        }
    }
    None
}

/// Parses human-formatted token counts e.g. "2.3k", "12,345", "1.5M", "412".
fn parse_human_tokens(s: &str) -> u64 {
    let clean = s.trim().replace(',', "").to_lowercase();
    if let Some(num_str) = clean.strip_suffix('k') {
        if let Ok(val) = num_str.parse::<f64>() {
            return (val * 1000.0).round() as u64;
        }
    } else if let Some(num_str) = clean.strip_suffix('m') {
        if let Ok(val) = num_str.parse::<f64>() {
            return (val * 1_000_000.0).round() as u64;
        }
    } else if let Ok(val) = clean.parse::<u64>() {
        return val;
    }
    0
}

/// Parses cost string e.g. "$0.03" or "0.035".
fn parse_cost_usd(s: &str) -> Option<f64> {
    let clean = s.trim().trim_start_matches('$').replace(',', "");
    clean.parse::<f64>().ok()
}

/// Parses a date string like "2024-05-18 14:20:00" or "2024-05-18 14:20:00.123456" into UTC DateTime.
fn parse_aider_start_time(s: &str) -> Option<DateTime<Utc>> {
    let trimmed = s.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S") {
        return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S%.f") {
        return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
    }
    None
}

/// Parses Aider markdown chat history format.
fn parse_aider_markdown(
    content: &str,
    sequence: &mut u64,
    earliest_ts: &mut Option<DateTime<Utc>>,
    latest_ts: &mut Option<DateTime<Utc>>,
    _malformed_lines: &mut usize,
    _warnings: &mut Vec<String>,
) -> Vec<NormalizedEvent> {
    let mut events = Vec::new();
    let mut active_model = "aider".to_string();
    let mut last_invoked_model: Option<String> = None;
    let mut current_ts = Utc::now();

    let mut current_user_buf: Vec<String> = Vec::new();
    let mut current_assistant_buf: Vec<String> = Vec::new();
    let mut in_diff_block = false;
    let mut current_diff_buf: Vec<String> = Vec::new();
    let mut in_bash_block = false;
    let mut current_bash_buf: Vec<String> = Vec::new();

    let flush_user = |buf: &mut Vec<String>, seq: &mut u64, ts: DateTime<Utc>, evs: &mut Vec<NormalizedEvent>| {
        if !buf.is_empty() {
            let content = buf.join("\n").trim().to_string();
            if !content.is_empty() {
                *seq += 1;
                evs.push(NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::UserMessage { content },
                ));
            }
            buf.clear();
        }
    };

    let flush_assistant = |buf: &mut Vec<String>, seq: &mut u64, ts: DateTime<Utc>, evs: &mut Vec<NormalizedEvent>| {
        if !buf.is_empty() {
            let content = buf.join("\n").trim().to_string();
            if !content.is_empty() {
                *seq += 1;
                evs.push(NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::AssistantMessage {
                        content,
                        thinking: None,
                    },
                ));
            }
            buf.clear();
        }
    };

    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        let raw_line_num = line_idx + 1;

        // Check for session start line: # aider chat started at 2024-05-18 14:20:00
        if trimmed.starts_with("# aider chat started at ") {
            flush_user(&mut current_user_buf, sequence, current_ts, &mut events);
            flush_assistant(&mut current_assistant_buf, sequence, current_ts, &mut events);

            let time_part = trimmed.trim_start_matches("# aider chat started at ").trim();
            if let Some(ts) = parse_aider_start_time(time_part) {
                current_ts = ts;
                if earliest_ts.is_none_or(|t| current_ts < t) {
                    *earliest_ts = Some(current_ts);
                }
                if latest_ts.is_none_or(|t| current_ts > t) {
                    *latest_ts = Some(current_ts);
                }
            }
            continue;
        }

        // Model line: Model: claude-3-5-sonnet-20241022 with diff edit format
        if trimmed.starts_with("Model: ") || trimmed.starts_with("Main model: ") {
            let rem = trimmed.trim_start_matches("Model: ").trim_start_matches("Main model: ").trim();
            let model_name = rem.split_whitespace().next().unwrap_or(rem);
            active_model = model_name.to_string();
            continue;
        }

        // Ignore metadata / preamble lines
        if trimmed.starts_with("Git repo:")
            || trimmed.starts_with("Repo-map:")
            || trimmed.starts_with("Weak model:")
            || trimmed.starts_with("Edit format:")
            || trimmed.starts_with("Aider v")
            || trimmed.starts_with("────────────────")
            || trimmed == "---"
        {
            continue;
        }

        // Code fence start/end
        if trimmed.starts_with("```") {
            if in_diff_block {
                in_diff_block = false;
                let diff_content = current_diff_buf.join("\n");
                current_diff_buf.clear();

                // Extract file path from diff header
                let mut target_path = "edited_file".to_string();
                for diff_line in diff_content.lines() {
                    if diff_line.starts_with("+++ b/") {
                        target_path = diff_line.trim_start_matches("+++ b/").trim().to_string();
                        break;
                    } else if diff_line.starts_with("+++ ") {
                        target_path = diff_line.trim_start_matches("+++ ").trim().to_string();
                        break;
                    } else if diff_line.starts_with("--- a/") {
                        target_path = diff_line.trim_start_matches("--- a/").trim().to_string();
                    }
                }

                *sequence += 1;
                events.push(
                    NormalizedEvent::new(
                        *sequence,
                        current_ts,
                        EventPayload::FileAction {
                            path: target_path.clone(),
                            action: FileActionType::Edit,
                            diff: Some(diff_content),
                            lines_changed: None,
                        },
                    )
                    .with_raw_ref(format!("line:{}", raw_line_num)),
                );

                *sequence += 1;
                events.push(NormalizedEvent::new(
                    *sequence,
                    current_ts,
                    EventPayload::OutcomeEvidence(OutcomeEvidence {
                        kind: OutcomeKind::ArtifactChanged,
                        summary: format!("Applied diff edit to {}", target_path),
                        confidence: 0.85,
                    }),
                ));
                continue;
            } else if in_bash_block {
                in_bash_block = false;
                let bash_content = current_bash_buf.join("\n");
                current_bash_buf.clear();

                let mut cmd_line = String::new();
                let mut out_lines = Vec::new();
                for l in bash_content.lines() {
                    if cmd_line.is_empty() && (l.starts_with('$') || l.starts_with('>') || !l.trim().is_empty()) {
                        cmd_line = l.trim_start_matches('$').trim_start_matches('>').trim().to_string();
                    } else {
                        out_lines.push(l);
                    }
                }

                if !cmd_line.is_empty() {
                    let output_str = if out_lines.is_empty() {
                        None
                    } else {
                        Some(out_lines.join("\n"))
                    };

                    *sequence += 1;
                    events.push(
                        NormalizedEvent::new(
                            *sequence,
                            current_ts,
                            EventPayload::ShellCommand(ShellCommand {
                                command: cmd_line.clone(),
                                cwd: None,
                                exit_code: Some(0),
                                output: output_str.clone(),
                            }),
                        )
                        .with_raw_ref(format!("line:{}", raw_line_num)),
                    );

                    if let Some(ref out) = output_str {
                        if out.contains("test result: ok") || out.contains("PASSED") || out.contains("passed") {
                            *sequence += 1;
                            events.push(NormalizedEvent::new(
                                *sequence,
                                current_ts,
                                EventPayload::OutcomeEvidence(OutcomeEvidence {
                                    kind: OutcomeKind::TestOrBuildPassed,
                                    summary: format!("Command passed: {}", cmd_line),
                                    confidence: 0.9,
                                }),
                            ));
                        }
                    }
                }
                continue;
            } else {
                if trimmed.starts_with("```diff") {
                    flush_user(&mut current_user_buf, sequence, current_ts, &mut events);
                    flush_assistant(&mut current_assistant_buf, sequence, current_ts, &mut events);
                    in_diff_block = true;
                    continue;
                } else if trimmed.starts_with("```bash") || trimmed.starts_with("```sh") {
                    flush_user(&mut current_user_buf, sequence, current_ts, &mut events);
                    flush_assistant(&mut current_assistant_buf, sequence, current_ts, &mut events);
                    in_bash_block = true;
                    continue;
                }
            }
        }

        if in_diff_block {
            current_diff_buf.push(line.to_string());
            continue;
        }

        if in_bash_block {
            current_bash_buf.push(line.to_string());
            continue;
        }

        // Tokens & Cost line: Tokens: 2.3k sent, 412 received. Cost: $0.03 message, $0.12 session.
        if trimmed.starts_with("Tokens:") {
            flush_user(&mut current_user_buf, sequence, current_ts, &mut events);
            flush_assistant(&mut current_assistant_buf, sequence, current_ts, &mut events);

            let mut input_tokens = 0u64;
            let mut output_tokens = 0u64;
            let mut message_cost: Option<f64> = None;

            // Split into parts
            // "Tokens: 2.3k sent, 412 received. Cost: $0.03 message, $0.12 session."
            let line_parts: Vec<&str> = trimmed.split("Cost:").collect();
            let token_part = line_parts[0].trim_start_matches("Tokens:").trim();

            for piece in token_part.split(',') {
                let p = piece.trim();
                if p.contains("sent") {
                    let num_str = p.split_whitespace().next().unwrap_or("0");
                    input_tokens = parse_human_tokens(num_str);
                } else if p.contains("received") {
                    let num_str = p.split_whitespace().next().unwrap_or("0");
                    output_tokens = parse_human_tokens(num_str);
                }
            }

            if line_parts.len() > 1 {
                let cost_part = line_parts[1].trim();
                for piece in cost_part.split(',') {
                    let p = piece.trim();
                    if p.contains("message") {
                        let cost_str = p.split_whitespace().next().unwrap_or("");
                        message_cost = parse_cost_usd(cost_str);
                    }
                }
            }

            if last_invoked_model.as_deref() != Some(active_model.as_str()) {
                if let Some(prev) = last_invoked_model.take() {
                    *sequence += 1;
                    events.push(
                        NormalizedEvent::new(
                            *sequence,
                            current_ts,
                            EventPayload::ModelSwitch(ModelSwitch {
                                from_model: Some(prev),
                                to_model: active_model.clone(),
                                reason: None,
                            }),
                        )
                        .with_raw_ref(format!("line:{}", raw_line_num)),
                    );
                }
                last_invoked_model = Some(active_model.clone());
            }

            *sequence += 1;
            events.push(
                NormalizedEvent::new(
                    *sequence,
                    current_ts,
                    EventPayload::ModelInvocation {
                        model: active_model.clone(),
                        token_usage: TokenUsage::new(input_tokens, output_tokens, 0, 0),
                        cost_usd: message_cost,
                        latency_ms: None,
                    },
                )
                .with_raw_ref(format!("line:{}", raw_line_num)),
            );
            continue;
        }

        // Git commit observed: Commit abc1234 feat: implement feature or Commit 1234567: ...
        if trimmed.starts_with("Commit ") || trimmed.starts_with("commit ") {
            let commit_line = trimmed.trim_start_matches("Commit ").trim_start_matches("commit ").trim();
            if !commit_line.is_empty() {
                flush_user(&mut current_user_buf, sequence, current_ts, &mut events);
                flush_assistant(&mut current_assistant_buf, sequence, current_ts, &mut events);

                *sequence += 1;
                events.push(
                    NormalizedEvent::new(
                        *sequence,
                        current_ts,
                        EventPayload::OutcomeEvidence(OutcomeEvidence {
                            kind: OutcomeKind::CommitObserved,
                            summary: format!("Git commit {}", commit_line),
                            confidence: 0.95,
                        }),
                    )
                    .with_raw_ref(format!("line:{}", raw_line_num)),
                );
                continue;
            }
        }

        // Applied edit to <file>
        if trimmed.starts_with("Applied edit to ") {
            let target_path = trimmed.trim_start_matches("Applied edit to ").trim();
            if !target_path.is_empty() {
                flush_user(&mut current_user_buf, sequence, current_ts, &mut events);
                flush_assistant(&mut current_assistant_buf, sequence, current_ts, &mut events);

                *sequence += 1;
                events.push(
                    NormalizedEvent::new(
                        *sequence,
                        current_ts,
                        EventPayload::FileAction {
                            path: target_path.to_string(),
                            action: FileActionType::Edit,
                            diff: None,
                            lines_changed: None,
                        },
                    )
                    .with_raw_ref(format!("line:{}", raw_line_num)),
                );

                *sequence += 1;
                events.push(NormalizedEvent::new(
                    *sequence,
                    current_ts,
                    EventPayload::OutcomeEvidence(OutcomeEvidence {
                        kind: OutcomeKind::ArtifactChanged,
                        summary: format!("Applied edit to {}", target_path),
                        confidence: 0.85,
                    }),
                ));
                continue;
            }
        }

        // User message turn: #### <msg> or > <prompt> or User: <msg>
        if trimmed.starts_with("#### ") {
            flush_user(&mut current_user_buf, sequence, current_ts, &mut events);
            flush_assistant(&mut current_assistant_buf, sequence, current_ts, &mut events);

            let msg = trimmed.trim_start_matches("#### ").trim();
            if !msg.is_empty() {
                current_user_buf.push(msg.to_string());
            }
            continue;
        } else if trimmed.starts_with("> ") && !trimmed.starts_with("> /") {
            // User prompt line in markdown quote
            flush_assistant(&mut current_assistant_buf, sequence, current_ts, &mut events);
            let msg = trimmed.trim_start_matches("> ").trim();
            if !msg.is_empty() {
                current_user_buf.push(msg.to_string());
            }
            continue;
        } else if trimmed.starts_with("> /run ") || trimmed.starts_with("> /test ") {
            // Aider command invocation
            flush_user(&mut current_user_buf, sequence, current_ts, &mut events);
            flush_assistant(&mut current_assistant_buf, sequence, current_ts, &mut events);

            let cmd = trimmed.trim_start_matches("> ").trim();
            *sequence += 1;
            events.push(
                NormalizedEvent::new(
                    *sequence,
                    current_ts,
                    EventPayload::ShellCommand(ShellCommand {
                        command: cmd.to_string(),
                        cwd: None,
                        exit_code: Some(0),
                        output: None,
                    }),
                )
                .with_raw_ref(format!("line:{}", raw_line_num)),
            );
            continue;
        }

        // Otherwise, assistant response text accumulation
        if !current_user_buf.is_empty() {
            flush_user(&mut current_user_buf, sequence, current_ts, &mut events);
        }
        if !trimmed.is_empty() {
            current_assistant_buf.push(line.to_string());
        }
    }

    flush_user(&mut current_user_buf, sequence, current_ts, &mut events);
    flush_assistant(&mut current_assistant_buf, sequence, current_ts, &mut events);

    events
}

/// Parses structured JSON or JSONL records from Aider session logs.
fn parse_aider_json_record(
    val: &Value,
    sequence: &mut u64,
    timestamp: DateTime<Utc>,
    line_num: usize,
    last_model: &mut Option<String>,
) -> Vec<NormalizedEvent> {
    let mut events = Vec::new();

    let raw_ref = format!("line:{}", line_num);

    // Extract role
    let role = val
        .get("role")
        .or_else(|| val.get("type"))
        .and_then(|r| r.as_str())
        .unwrap_or("");

    let content_text = if let Some(content) = val.get("content").and_then(|c| c.as_str()) {
        content.to_string()
    } else if let Some(text) = val.get("text").and_then(|t| t.as_str()) {
        text.to_string()
    } else if let Some(msg) = val.get("message").and_then(|m| m.as_str()) {
        msg.to_string()
    } else {
        String::new()
    };

    // User message
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
    } else if (role == "assistant" || role == "agent" || role == "model") && !content_text.is_empty() {
        *sequence += 1;
        events.push(
            NormalizedEvent::new(
                *sequence,
                timestamp,
                EventPayload::AssistantMessage {
                    content: content_text.clone(),
                    thinking: None,
                },
            )
            .with_raw_ref(&raw_ref),
        );
    }

    // Diffs / File actions
    if let Some(diff) = val.get("diff").and_then(|d| d.as_str()) {
        let path = val
            .get("path")
            .or_else(|| val.get("file"))
            .and_then(|f| f.as_str())
            .unwrap_or("edited_file");

        *sequence += 1;
        events.push(
            NormalizedEvent::new(
                *sequence,
                timestamp,
                EventPayload::FileAction {
                    path: path.to_string(),
                    action: FileActionType::Edit,
                    diff: Some(diff.to_string()),
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
                summary: format!("Modified {}", path),
                confidence: 0.85,
            }),
        ));
    }

    // Git commit
    if let Some(commit) = val.get("commit").and_then(|c| c.as_str()) {
        *sequence += 1;
        events.push(
            NormalizedEvent::new(
                *sequence,
                timestamp,
                EventPayload::OutcomeEvidence(OutcomeEvidence {
                    kind: OutcomeKind::CommitObserved,
                    summary: format!("Commit {}", commit),
                    confidence: 0.95,
                }),
            )
            .with_raw_ref(&raw_ref),
        );
    }

    // Shell command
    if let Some(cmd) = val.get("command").and_then(|c| c.as_str()) {
        let exit_code = val.get("exit_code").and_then(|ec| ec.as_i64()).map(|e| e as i32);
        let output = val.get("output").and_then(|o| o.as_str()).map(|s| s.to_string());

        *sequence += 1;
        events.push(
            NormalizedEvent::new(
                *sequence,
                timestamp,
                EventPayload::ShellCommand(ShellCommand {
                    command: cmd.to_string(),
                    cwd: None,
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
                            summary: format!("Passed command: {}", cmd),
                            confidence: 0.9,
                        }),
                    ));
                }
            }
        }
    }

    // Tool calls / MCP tools
    if let Some(tools) = val.get("tool_calls").and_then(|t| t.as_array()) {
        for t in tools {
            let name = t.get("name").and_then(|n| n.as_str()).unwrap_or("unknown_tool");
            let args = t.get("arguments").cloned().unwrap_or(Value::Null);
            let normalized_name = normalize_mcp_tool_name(name, &args);

            *sequence += 1;
            events.push(
                NormalizedEvent::new(
                    *sequence,
                    timestamp,
                    EventPayload::ToolCall(ToolCall {
                        id: t.get("id").and_then(|i| i.as_str()).map(|s| s.to_string()),
                        name: normalized_name,
                        arguments: args,
                    }),
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    // Token accounting & Model invocation
    let model = val
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("aider");

    let usage = val.get("usage").or_else(|| val.get("token_usage"));
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

        if input_tokens > 0 || output_tokens > 0 || cache_read > 0 || cache_creation > 0 {
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

    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_detect_and_enumerate_aider() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(
            temp_file,
            "# aider chat started at 2024-05-18 14:20:00\nModel: claude-3-5-sonnet-20241022 with diff edit format\n#### Add health check\nTokens: 1.2k sent, 300 received. Cost: $0.02 message, $0.05 session."
        )
        .unwrap();

        let adapter = AiderAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp_file.path().to_path_buf()],
            force: true,
            ..Default::default()
        };

        let detection = adapter.detect(&options).unwrap();
        assert!(detection.is_present);
        assert_eq!(detection.adapter_name, "aider");

        let sources = adapter.enumerate(&options).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].adapter_name, "aider");
    }

    #[test]
    fn test_parse_aider_markdown_chat_history() {
        let md_content = r#"# aider chat started at 2024-05-18 14:20:00

Model: claude-3-5-sonnet-20241022 with diff edit format
Git repo: /Users/dev/myrepo
Repo-map: using 1024 tokens

#### Implement the user login handler

I will write the user login endpoint in `src/auth.rs`.

```diff
--- a/src/auth.rs
+++ b/src/auth.rs
@@ -1,5 +1,12 @@
+pub fn login(user: &str) -> bool {
+    !user.is_empty()
+}
```

Applied edit to src/auth.rs
Commit 7f8a9b0 feat: implement login endpoint

```bash
$ cargo test
test result: ok. 4 passed; 0 failed; 0 ignored
```

Tokens: 2.5k sent, 450 received. Cost: $0.03 message, $0.12 session.
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "{}", md_content).unwrap();

        let adapter = AiderAdapter::new();
        let source = SessionSource::from_path(temp_file.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).unwrap();

        assert_eq!(result.malformed_lines, 0);
        assert_eq!(result.trace.adapter, "aider");
        assert!(result.trace.stats.total_events > 0);
        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
        assert_eq!(result.trace.stats.token_usage.input_tokens, 2500);
        assert_eq!(result.trace.stats.token_usage.output_tokens, 450);

        // Verify outcomes
        let has_commit_outcome = result.trace.events.iter().any(|e| {
            matches!(
                &e.payload,
                EventPayload::OutcomeEvidence(OutcomeEvidence {
                    kind: OutcomeKind::CommitObserved,
                    ..
                })
            )
        });
        assert!(has_commit_outcome);

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

        let has_file_edit = result.trace.events.iter().any(|e| {
            matches!(
                &e.payload,
                EventPayload::FileAction {
                    path,
                    action: FileActionType::Edit,
                    ..
                } if path == "src/auth.rs"
            )
        });
        assert!(has_file_edit);
    }

    #[test]
    fn test_parse_aider_jsonl_and_json() {
        let jsonl_content = r#"{"role":"user","content":"Refactor database pool","timestamp":"2024-05-18T14:20:00Z"}
{"role":"assistant","content":"I am updating db.rs with r2d2 connection pooling.","timestamp":"2024-05-18T14:20:05Z"}
{"path":"src/db.rs","diff":"--- a/src/db.rs\n+++ b/src/db.rs\n@@ -1 +1,5 @@\n+use r2d2::Pool;\n","commit":"a1b2c3d refactor db pool","model":"gpt-4o","usage":{"prompt_tokens":1200,"completion_tokens":350},"cost_usd":0.02,"timestamp":"2024-05-18T14:20:10Z"}
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "{}", jsonl_content).unwrap();

        let adapter = AiderAdapter::new();
        let source = SessionSource::from_path(temp_file.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).unwrap();

        assert_eq!(result.malformed_lines, 0);
        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
        assert_eq!(result.trace.stats.token_usage.input_tokens, 1200);
        assert_eq!(result.trace.stats.token_usage.output_tokens, 350);
    }

    #[test]
    fn test_parse_graceful_on_malformed_lines() {
        let content = r#"{"role":"user","content":"Initial task","timestamp":"2024-05-18T14:00:00Z"}
{CORRUPT_JSON_LINE_WITHOUT_QUOTES}
{"role":"assistant","content":"Recovered and completed.","timestamp":"2024-05-18T14:01:00Z"}
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "{}", content).unwrap();

        let adapter = AiderAdapter::new();
        let source = SessionSource::from_path(temp_file.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).unwrap();

        assert!(result.malformed_lines >= 1);
        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
    }

    #[test]
    fn test_parse_aider_markdown_detects_model_switch_mid_task() {
        // Aider's markdown chat history announces the active model with its own
        // "Model: " line whenever `/model` is used mid-chat (e.g. dropping from a
        // frontier model to a cheaper one partway through a task). That line updates
        // `active_model`; the very next "Tokens:" line is the first real invocation
        // under the new model and is where the switch must be detected.
        let md_content = r#"# aider chat started at 2024-05-18 14:20:00

Model: claude-3-5-sonnet-20241022 with diff edit format
Git repo: /Users/dev/myrepo

#### Implement the user login handler

I will write the login endpoint.

Tokens: 2.0k sent, 400 received. Cost: $0.03 message, $0.03 session.

#### Now just fix the remaining lint warnings

Model: gpt-4o-mini with whole edit format

Switching to a cheaper model for the mechanical cleanup.

Tokens: 800 sent, 150 received. Cost: $0.01 message, $0.04 session.
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "{}", md_content).unwrap();

        let adapter = AiderAdapter::new();
        let source = SessionSource::from_path(temp_file.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).unwrap();

        assert_eq!(result.malformed_lines, 0);
        let trace = result.trace;

        assert_eq!(
            trace.stats.models_used,
            vec![
                "claude-3-5-sonnet-20241022".to_string(),
                "gpt-4o-mini".to_string()
            ]
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
                Some("claude-3-5-sonnet-20241022".to_string()),
                "gpt-4o-mini".to_string()
            )]
        );

        // Two distinct models were actually invoked (via their own Tokens: lines),
        // each attributed correctly — the switch doesn't merge or drop usage.
        assert_eq!(trace.stats.per_model_token_usage.len(), 2);
        assert_eq!(
            trace
                .stats
                .per_model_token_usage
                .get("claude-3-5-sonnet-20241022")
                .unwrap()
                .input_tokens,
            2000
        );
        assert_eq!(
            trace
                .stats
                .per_model_token_usage
                .get("gpt-4o-mini")
                .unwrap()
                .input_tokens,
            800
        );
    }
}
