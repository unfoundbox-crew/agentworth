//! Recovery loop detection engine.
//!
//! Detects sequences where an agent encounters a failure (e.g. compilation failure,
//! failed unit test, runtime error, or tool error), applies corrective actions (edits,
//! modified parameters, debugging commands), and subsequently achieves a successful state.

use agentworth_schema::{AgentWorthTrace, EventPayload, NormalizedEvent};
use serde::{Deserialize, Serialize};

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
                // Record new active failure
                active_failures.push(ActiveFailure {
                    event_index: idx,
                    sequence: event.sequence,
                    timestamp: event.timestamp,
                    summary: failure_info,
                    corrective_actions: 0,
                });
                continue;
            }

            // 2. Check if event is a corrective action
            let is_corrective = self.is_corrective_action(event);
            if is_corrective {
                for failure in &mut active_failures {
                    failure.corrective_actions += 1;
                }
            }

            // 3. Check if event is a successful recovery for any active failure
            if let Some(recovery_summary) = self.is_success_event(event) {
                if !active_failures.is_empty() {
                    // Resolve the most recent active failure (or all matching)
                    for failure in active_failures.drain(..) {
                        let duration = event
                            .timestamp
                            .signed_duration_since(failure.timestamp)
                            .num_milliseconds() as f64
                            / 1000.0;
                        let steps = idx.saturating_sub(failure.event_index);

                        recoveries.push(RecoverySignal {
                            failure_sequence: failure.sequence,
                            failure_summary: failure.summary,
                            recovery_sequence: event.sequence,
                            recovery_summary: recovery_summary.clone(),
                            steps_to_recover: steps,
                            duration_seconds: Some(duration.max(0.0)),
                            corrective_actions_count: failure.corrective_actions,
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
        format!("{}...", &clean[..max_len])
    } else {
        clean.to_string()
    }
}
