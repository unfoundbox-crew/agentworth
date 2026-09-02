//! Recovery loop detection engine.
//!
//! Detects sequences where an agent encounters a failure (e.g. compilation failure,
//! failed unit test, runtime error, or tool error), applies corrective actions (edits,
//! modified parameters, debugging commands), and subsequently achieves a successful state.

use agentworth_schema::{AgentWorthTrace, EventPayload, NormalizedEvent};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

static RUST_ARROW_REGEX: OnceLock<regex::Regex> = OnceLock::new();
static JEST_FAIL_REGEX: OnceLock<regex::Regex> = OnceLock::new();
static STACK_TRACE_REGEX: OnceLock<regex::Regex> = OnceLock::new();
static COMPILER_LOC_REGEX: OnceLock<regex::Regex> = OnceLock::new();
static FILE_TOKEN_REGEX: OnceLock<regex::Regex> = OnceLock::new();

/// Signal capturing a successful recovery from an earlier failure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecoverySignal {
    /// Sequence number of the failure event.
    pub failure_sequence: u64,
    /// Summary explanation of the failure.
    pub failure_summary: String,
    /// Sequence number of the event marking resolution.
    pub recovery_sequence: u64,
    /// Summary explanation of the recovery.
    pub recovery_summary: String,
    /// Number of steps/events between failure and recovery.
    pub steps_to_recover: usize,
    /// Elapsed seconds between the failure and its resolution.
    pub duration_seconds: Option<f64>,
    /// Number of distinct corrective actions (e.g. file modifications, fixes) performed.
    pub corrective_actions_count: usize,
    /// Correlated file paths identified between failure diagnostics and corrective edits.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub correlated_files: Vec<String>,
}

/// Detector that scans normalized events for failure-and-recovery patterns.
#[derive(Debug, Default, Clone)]
pub struct RecoveryDetector;

impl RecoveryDetector {
    pub fn new() -> Self {
        Self
    }

    /// Detect all failure-recovery loops within a trace.
    pub fn detect_recoveries(&self, trace: &AgentWorthTrace) -> Vec<RecoverySignal> {
        self.detect_recoveries_from_events(&trace.events)
    }

    /// Detect failure-recovery loops from an event slice.
    pub fn detect_recoveries_from_events(&self, events: &[NormalizedEvent]) -> Vec<RecoverySignal> {
        let mut recoveries = Vec::new();
        let mut active_failures: Vec<ActiveFailure> = Vec::new();

        for (idx, event) in events.iter().enumerate() {
            // 1. Check if event is a failure
            if let Some(failure_info) = self.is_failure_event(event) {
                let failure_files = self.extract_files_from_failure_event(event);
                active_failures.push(ActiveFailure {
                    event_index: idx,
                    sequence: event.sequence,
                    timestamp: event.timestamp,
                    summary: failure_info,
                    corrective_actions: 0,
                    referenced_files: failure_files,
                    correlated_files: Vec::new(),
                });
                continue;
            }

            // 2. Check if event is a corrective action
            let action_files = self.extract_action_files(event);
            let is_corrective = self.is_corrective_action(event) || !action_files.is_empty();

            if is_corrective {
                for failure in &mut active_failures {
                    let mut matched_any = false;
                    for af in &action_files {
                        if failure.referenced_files.is_empty()
                            || failure.referenced_files.iter().any(|rf| paths_match(rf, af))
                        {
                            if !failure.correlated_files.contains(af) {
                                failure.correlated_files.push(af.clone());
                            }
                            matched_any = true;
                        }
                    }
                    if matched_any || failure.referenced_files.is_empty() {
                        failure.corrective_actions += 1;
                    }
                }
            }

            // 3. Check if event is a successful recovery for any active failure
            if let Some(recovery_summary) = self.is_success_event(event) {
                if !active_failures.is_empty() {
                    // Resolve all active failures
                    for failure in active_failures.drain(..) {
                        let duration = event
                            .timestamp
                            .signed_duration_since(failure.timestamp)
                            .num_milliseconds() as f64
                            / 1000.0;
                        let steps = idx.saturating_sub(failure.event_index);

                        let rec_summary = if !failure.correlated_files.is_empty() {
                            format!(
                                "{} (correlated fix in: {})",
                                recovery_summary,
                                failure.correlated_files.join(", ")
                            )
                        } else {
                            recovery_summary.clone()
                        };

                        recoveries.push(RecoverySignal {
                            failure_sequence: failure.sequence,
                            failure_summary: failure.summary,
                            recovery_sequence: event.sequence,
                            recovery_summary: rec_summary,
                            steps_to_recover: steps,
                            duration_seconds: Some(duration.max(0.0)),
                            corrective_actions_count: failure.corrective_actions,
                            correlated_files: failure.correlated_files,
                        });
                    }
                }
            }
        }

        recoveries
    }

    fn is_failure_event(&self, event: &NormalizedEvent) -> Option<String> {
        match &event.payload {
            EventPayload::Error { message, .. } => Some(format!("Error encountered: {}", message)),
            EventPayload::ToolResult(res) => {
                if res.is_error {
                    Some(format!(
                        "Tool '{}' failed",
                        res.name.as_deref().unwrap_or("unknown")
                    ))
                } else {
                    let output_str = extract_output_text(&res.output);
                    if has_failure_text(&output_str) {
                        Some(format!(
                            "Tool execution returned failure: {}",
                            truncate_str(&output_str, 80)
                        ))
                    } else {
                        None
                    }
                }
            }
            EventPayload::ShellCommand(cmd) => {
                if let Some(code) = cmd.exit_code {
                    if code != 0 {
                        return Some(format!(
                            "Command '{}' exited with code {}",
                            cmd.command, code
                        ));
                    }
                }
                if let Some(out) = &cmd.output {
                    if has_failure_text(out) {
                        return Some(format!(
                            "Command '{}' failed with output: {}",
                            cmd.command,
                            truncate_str(out, 80)
                        ));
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn is_corrective_action(&self, event: &NormalizedEvent) -> bool {
        match &event.payload {
            EventPayload::FileAction { action, .. } => {
                matches!(
                    action,
                    agentworth_schema::FileActionType::Write
                        | agentworth_schema::FileActionType::Edit
                        | agentworth_schema::FileActionType::Delete
                )
            }
            EventPayload::ToolCall(tool) => {
                let name = tool.name.to_lowercase();
                name.contains("write")
                    || name.contains("edit")
                    || name.contains("replace")
                    || name.contains("patch")
                    || name.contains("fix")
            }
            _ => false,
        }
    }

    fn extract_files_from_failure_event(&self, event: &NormalizedEvent) -> Vec<String> {
        let mut text = String::new();
        match &event.payload {
            EventPayload::Error { message, .. } => {
                text.push_str(message);
            }
            EventPayload::ToolResult(res) => {
                text.push_str(&extract_output_text(&res.output));
            }
            EventPayload::ShellCommand(cmd) => {
                if let Some(out) = &cmd.output {
                    text.push_str(out);
                }
                text.push(' ');
                text.push_str(&cmd.command);
            }
            _ => {}
        }

        self.extract_files_from_text(&text)
    }

    pub fn extract_files_from_text(&self, text: &str) -> Vec<String> {
        let mut results = Vec::new();
        if text.is_empty() {
            return results;
        }

        let rust_arrow = RUST_ARROW_REGEX
            .get_or_init(|| regex::Regex::new(r"-->\s+([^\s:]+)(?::\d+)*").expect("valid regex"));
        let jest_fail = JEST_FAIL_REGEX.get_or_init(|| {
            regex::Regex::new(r"(?:FAIL|PASS)\s+([^\s:]+\.[a-zA-Z0-9]+)").expect("valid regex")
        });
        let stack_trace = STACK_TRACE_REGEX.get_or_init(|| {
            regex::Regex::new(
                r#"(?:File\s+["']|at\s+(?:[^\(]*\()?|in\s+file\s+["']?)([^\s"':\(\)]+\.[a-zA-Z0-9]+)"#,
            )
            .expect("valid regex")
        });
        let compiler_loc = COMPILER_LOC_REGEX.get_or_init(|| {
            regex::Regex::new(
                r#"([a-zA-Z0-9_.\-\\/]+\.(?:rs|ts|tsx|js|jsx|py|go|c|cpp|h|hpp|toml|json|yaml|yml|sql|sh|html|css|vue|svelte|java|rb|php|swift|kt))(?::\d+|\(\d+)"#,
            )
            .expect("valid regex")
        });
        let file_token = FILE_TOKEN_REGEX.get_or_init(|| {
            regex::Regex::new(
                r#"(?:^|[\s"'\(`<])([a-zA-Z0-9_.\-\\/]+\.(?:rs|ts|tsx|js|jsx|py|go|c|cpp|h|hpp|toml|json|yaml|yml|sql|sh|html|css|vue|svelte|java|rb|php|swift|kt))(?:$|[\s"'\)`>:,])"#,
            )
            .expect("valid regex")
        });

        // 1. Rust arrow pointer
        for cap in rust_arrow.captures_iter(text) {
            if let Some(m) = cap.get(1) {
                if let Some(clean) = clean_extracted_path(m.as_str()) {
                    results.push(clean);
                }
            }
        }

        // 2. Jest/Vitest
        for cap in jest_fail.captures_iter(text) {
            if let Some(m) = cap.get(1) {
                if let Some(clean) = clean_extracted_path(m.as_str()) {
                    results.push(clean);
                }
            }
        }

        // 3. Stack trace
        for cap in stack_trace.captures_iter(text) {
            if let Some(m) = cap.get(1) {
                if let Some(clean) = clean_extracted_path(m.as_str()) {
                    results.push(clean);
                }
            }
        }

        // 4. Compiler location (e.g. src/types.ts:14:5)
        for cap in compiler_loc.captures_iter(text) {
            if let Some(m) = cap.get(1) {
                if let Some(clean) = clean_extracted_path(m.as_str()) {
                    results.push(clean);
                }
            }
        }

        // 5. Generic file tokens with extension
        for cap in file_token.captures_iter(text) {
            if let Some(m) = cap.get(1) {
                if let Some(clean) = clean_extracted_path(m.as_str()) {
                    results.push(clean);
                }
            }
        }

        results.sort();
        results.dedup();
        results
    }

    fn extract_action_files(&self, event: &NormalizedEvent) -> Vec<String> {
        let mut files = Vec::new();
        match &event.payload {
            EventPayload::FileAction { path, .. } => {
                if let Some(clean) = clean_extracted_path(path) {
                    files.push(clean);
                }
            }
            EventPayload::ToolCall(tool) => {
                if let Some(obj) = tool.arguments.as_object() {
                    for key in &[
                        "path",
                        "target_file",
                        "TargetFile",
                        "file_path",
                        "FilePath",
                        "file",
                        "filename",
                        "FileName",
                        "AbsolutePath",
                        "path_to_file",
                    ] {
                        if let Some(v) = obj.get(*key).and_then(|val| val.as_str()) {
                            if let Some(clean) = clean_extracted_path(v) {
                                files.push(clean);
                            }
                        }
                    }
                } else if let Some(s) = tool.arguments.as_str() {
                    for f in self.extract_files_from_text(s) {
                        files.push(f);
                    }
                }
            }
            _ => {}
        }
        files.sort();
        files.dedup();
        files
    }

    fn is_success_event(&self, event: &NormalizedEvent) -> Option<String> {
        match &event.payload {
            EventPayload::ShellCommand(cmd) => {
                if cmd.exit_code == Some(0) {
                    if let Some(out) = &cmd.output {
                        if is_successful_test_or_build_output(out) {
                            return Some(format!(
                                "Test/build command '{}' passed successfully",
                                cmd.command
                            ));
                        }
                    }
                    if is_test_command(&cmd.command) || cmd.command.contains("git commit") {
                        return Some(format!(
                            "Command '{}' succeeded with exit code 0",
                            cmd.command
                        ));
                    }
                }
                None
            }
            EventPayload::ToolResult(res) => {
                if !res.is_error {
                    let out = extract_output_text(&res.output);
                    if is_successful_test_or_build_output(&out) {
                        return Some(format!(
                            "Tool '{}' execution succeeded with verified passing output",
                            res.name.as_deref().unwrap_or("unknown")
                        ));
                    }
                }
                None
            }
            EventPayload::OutcomeEvidence(ev) => {
                if matches!(
                    ev.kind,
                    agentworth_schema::OutcomeKind::TestOrBuildPassed
                        | agentworth_schema::OutcomeKind::CommitObserved
                        | agentworth_schema::OutcomeKind::CiOrDeploymentVerified
                ) {
                    Some(format!("Outcome verified: {:?}", ev.kind))
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

struct ActiveFailure {
    event_index: usize,
    sequence: u64,
    timestamp: chrono::DateTime<chrono::Utc>,
    summary: String,
    corrective_actions: usize,
    referenced_files: Vec<String>,
    correlated_files: Vec<String>,
}

fn clean_extracted_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches(|c| {
        c == '\''
            || c == '"'
            || c == '`'
            || c == '('
            || c == ')'
            || c == '<'
            || c == '>'
            || c == ':'
            || c == ','
    });

    if trimmed.is_empty()
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("git://")
    {
        return None;
    }

    let normalized = trimmed.replace('\\', "/");
    let clean = normalized.strip_prefix("./").unwrap_or(&normalized);

    // Verify it contains a plausible file extension
    if let Some(dot_idx) = clean.rfind('.') {
        let ext = &clean[dot_idx + 1..];
        if !ext.is_empty() && ext.len() <= 10 && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Some(clean.to_string());
        }
    }
    None
}

fn normalize_path(path: &str) -> String {
    let p = path.replace('\\', "/");
    let trimmed = p.trim().trim_matches(|c| {
        c == '\''
            || c == '"'
            || c == '`'
            || c == '('
            || c == ')'
            || c == '<'
            || c == '>'
            || c == ':'
            || c == ','
    });
    trimmed.strip_prefix("./").unwrap_or(trimmed).to_string()
}

pub fn paths_match(path_a: &str, path_b: &str) -> bool {
    let a = normalize_path(path_a);
    let b = normalize_path(path_b);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    // Suffix match on path boundary
    if a.ends_with(&b) && (a.len() == b.len() || a.as_bytes()[a.len() - b.len() - 1] == b'/') {
        return true;
    }
    if b.ends_with(&a) && (b.len() == a.len() || b.as_bytes()[b.len() - a.len() - 1] == b'/') {
        return true;
    }
    // Filename match
    let fname_a = a.split('/').next_back().unwrap_or(&a);
    let fname_b = b.split('/').next_back().unwrap_or(&b);
    if fname_a == fname_b {
        return true;
    }
    // Stem match (e.g. hero_receipt.test.tsx vs hero_receipt.tsx)
    let stem_a = file_stem(fname_a);
    let stem_b = file_stem(fname_b);
    if !stem_a.is_empty() && stem_a == stem_b {
        return true;
    }
    false
}

fn file_stem(filename: &str) -> String {
    let mut stem = filename;
    if let Some(idx) = stem.rfind('.') {
        stem = &stem[..idx];
    }
    if let Some(stripped) = stem
        .strip_suffix(".test")
        .or_else(|| stem.strip_suffix(".spec"))
    {
        stem = stripped;
    }
    if let Some(stripped) = stem
        .strip_prefix("test_")
        .or_else(|| stem.strip_prefix("test-"))
    {
        stem = stripped;
    }
    stem.to_string()
}

fn extract_output_text(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(o) => {
            if let Some(s) = o.get("output").and_then(|v| v.as_str()) {
                s.to_string()
            } else if let Some(s) = o.get("stdout").and_then(|v| v.as_str()) {
                s.to_string()
            } else if let Some(s) = o.get("stderr").and_then(|v| v.as_str()) {
                s.to_string()
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

fn has_failure_text(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("test result: failed")
        || lower.contains("compilation error")
        || (lower.contains("failures:") && !lower.contains("failures: 0"))
        || (lower.contains("failed:") && !lower.contains("failed: 0"))
        || lower.contains("error[e")
        || lower.contains("syntaxerror:")
        || lower.contains("panic:")
        || (text.contains("FAIL ") && (text.contains(".test.") || text.contains(".spec.")))
}

fn is_successful_test_or_build_output(text: &str) -> bool {
    let lower = text.to_lowercase();
    if has_failure_text(text) {
        return false;
    }

    text.contains("test result: ok.")
        || text.contains("Doc-tests") && text.contains("ok")
        || text.contains("Test Suites: ") && text.contains("passed")
        || text.contains("Tests:       ") && text.contains("passed")
        || text.contains("PASSED [")
        || lower.contains("all tests passed")
        || (text.contains("PASS ") && (text.contains(".test.") || text.contains(".spec.")))
        || lower.contains("build succeeded")
        || lower.contains("compiled successfully")
}

fn is_test_command(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    lower.contains("cargo test")
        || lower.contains("cargo check")
        || lower.contains("npm test")
        || lower.contains("pnpm test")
        || lower.contains("yarn test")
        || lower.contains("pytest")
        || lower.contains("go test")
        || lower.contains("vitest")
        || lower.contains("jest")
}

fn truncate_str(s: &str, max_len: usize) -> String {
    let clean = s.lines().next().unwrap_or("").trim();
    if clean.len() > max_len {
        // `clean.len()` is bytes, but `max_len` is meant as a byte budget for a slice
        // boundary that has to land on a char -- real tool output routinely carries
        // multi-byte UTF-8 (box-drawing glyphs from a nested `agentworth` run, CJK text,
        // etc.), and a plain `&clean[..max_len]` panics ("not a char boundary") the moment
        // that boundary falls inside one. Walk char boundaries and stop at the last one at
        // or before `max_len` instead.
        let cut = clean
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= max_len)
            .last()
            .unwrap_or(0);
        format!("{}...", &clean[..cut])
    } else {
        clean.to_string()
    }
}

#[cfg(test)]
mod truncate_str_tests {
    use super::truncate_str;

    #[test]
    fn cuts_on_a_char_boundary_not_a_byte_offset() {
        // Each "─" is 3 bytes (U+2500); byte offset 80 lands inside one, which is exactly
        // what panicked before this fix.
        let s = "a".repeat(78) + "───────────";
        let out = truncate_str(&s, 80);
        assert!(out.ends_with("..."));
    }

    #[test]
    fn leaves_short_strings_untouched() {
        assert_eq!(truncate_str("short", 80), "short");
    }
}

