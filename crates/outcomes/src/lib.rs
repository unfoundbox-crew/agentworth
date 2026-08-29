//! Outcome and recovery extraction engine for AgentWorth.
//!
//! Provides deterministic extraction of outcome signals (from self-claimed 'done'
//! up to externally verified CI and deployment) and recovery signals (when agents
//! fix failing tests, compiler errors, and broken states).

mod outcome;
mod recovery;

pub use outcome::{outcome_rank, OutcomeDetector};
pub use recovery::{RecoveryDetector, RecoverySignal};

/// Evaluates a trace and extracts all inferred outcome evidence.
pub fn evaluate_trace_outcomes(
    trace: &agentworth_schema::AgentWorthTrace,
) -> Vec<agentworth_schema::OutcomeEvidence> {
    OutcomeDetector::new().detect_outcomes(trace)
}

/// Returns the highest confidence outcome in the trace, if any.
pub fn highest_outcome(
    outcomes: &[agentworth_schema::OutcomeEvidence],
) -> Option<&agentworth_schema::OutcomeEvidence> {
    OutcomeDetector::new().strongest_outcome(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{
        AgentWorthTrace, EventPayload, FileActionType, NormalizedEvent, OutcomeEvidence,
        OutcomeKind, Provenance, ShellCommand, ToolCall, ToolResult,
    };
    use chrono::{Duration, Utc};

    fn make_test_trace() -> AgentWorthTrace {
        let start = Utc::now();
        let prov = Provenance::new("/tmp/test.jsonl", "claude_code", 100, 1000, "fp123");
        AgentWorthTrace::new("sess_test_1", "claude_code", prov, start)
    }

    #[test]
    fn test_outcome_hierarchy_detection() {
        let mut trace = make_test_trace();
        let start = trace.started_at;

        // 1. Done claimed
        trace.events.push(NormalizedEvent::new(
            1,
            start + Duration::seconds(1),
            EventPayload::AssistantMessage {
                content: "I have completed all the requested tasks!".to_string(),
                thinking: None,
            },
        ));

        // 2. File action (ArtifactChanged)
        trace.events.push(NormalizedEvent::new(
            2,
            start + Duration::seconds(2),
            EventPayload::FileAction {
                path: "src/main.rs".to_string(),
                action: FileActionType::Write,
                diff: Some("+ fn main() {}".to_string()),
                lines_changed: Some(1),
            },
        ));

        // 3. Test passed
        trace.events.push(NormalizedEvent::new(
            3,
            start + Duration::seconds(3),
            EventPayload::ShellCommand(ShellCommand {
                command: "cargo test --all".to_string(),
                cwd: Some("/repo".to_string()),
                exit_code: Some(0),
                output: Some("running 5 tests\ntest result: ok. 5 passed; 0 failed".to_string()),
            }),
        ));

        // 4. Git commit
        trace.events.push(NormalizedEvent::new(
            4,
            start + Duration::seconds(4),
            EventPayload::ShellCommand(ShellCommand {
                command: "git commit -m 'feat: implement feature'".to_string(),
                cwd: Some("/repo".to_string()),
                exit_code: Some(0),
                output: Some("[main 9f3e1a2] feat: implement feature\n 1 file changed".to_string()),
            }),
        ));

        // 5. CI / PR
        trace.events.push(NormalizedEvent::new(
            5,
            start + Duration::seconds(5),
            EventPayload::ShellCommand(ShellCommand {
                command: "gh pr create --title 'feat: feature' --body 'ready'".to_string(),
                cwd: Some("/repo".to_string()),
                exit_code: Some(0),
                output: Some(
                    "https://github.com/org/repo/pull/42\nPull request successfully created"
                        .to_string(),
                ),
            }),
        ));

        let detector = OutcomeDetector::new();
        let outcomes = detector.detect_outcomes(&trace);

        assert_eq!(outcomes.len(), 5);
        assert_eq!(outcomes[0].kind, OutcomeKind::DoneClaimed);
        assert_eq!(outcomes[1].kind, OutcomeKind::ArtifactChanged);
        assert_eq!(outcomes[2].kind, OutcomeKind::TestOrBuildPassed);
        assert_eq!(outcomes[3].kind, OutcomeKind::CommitObserved);
        assert_eq!(outcomes[4].kind, OutcomeKind::CiOrDeploymentVerified);

        let strongest = detector
            .strongest_outcome(&outcomes)
            .expect("strongest outcome");
        assert_eq!(strongest.kind, OutcomeKind::CiOrDeploymentVerified);
        assert!(strongest.confidence >= 0.95);

        // Also test convenience functions
        let eval_outcomes = evaluate_trace_outcomes(&trace);
        assert_eq!(eval_outcomes.len(), 5);
        let highest = highest_outcome(&eval_outcomes).unwrap();
        assert_eq!(highest.kind, OutcomeKind::CiOrDeploymentVerified);
    }

    #[test]
    fn test_failed_test_does_not_count_as_passed() {
        let mut trace = make_test_trace();
        let start = trace.started_at;

        trace.events.push(NormalizedEvent::new(
            1,
            start + Duration::seconds(1),
            EventPayload::ShellCommand(ShellCommand {
                command: "cargo test".to_string(),
                cwd: None,
                exit_code: Some(101),
                output: Some(
                    "failures:\n    tests::test_foo\ntest result: FAILED. 0 passed; 1 failed"
                        .to_string(),
                ),
            }),
        ));

        let detector = OutcomeDetector::new();
        let outcomes = detector.detect_outcomes(&trace);
        assert!(
            outcomes.is_empty(),
            "Failed tests should not be recognized as TestOrBuildPassed"
        );
    }

    #[test]
    fn test_tool_calls_and_results_outcome_detection() {
        let mut trace = make_test_trace();
        let start = trace.started_at;

        // Tool edit
        trace.events.push(NormalizedEvent::new(
            1,
            start + Duration::seconds(1),
            EventPayload::ToolCall(ToolCall {
                id: Some("call_1".to_string()),
                name: "replace_file_content".to_string(),
                arguments: serde_json::json!({"path": "src/lib.rs"}),
            }),
        ));

        // Tool result with test pass
        trace.events.push(NormalizedEvent::new(
            2,
            start + Duration::seconds(2),
            EventPayload::ToolResult(ToolResult {
                call_id: Some("call_2".to_string()),
                name: Some("Bash".to_string()),
                output: serde_json::json!({"stdout": "PASS src/index.test.ts\nTests: 4 passed, 4 total"}),
                is_error: false,
            }),
        ));

        let detector = OutcomeDetector::new();
        let outcomes = detector.detect_outcomes(&trace);

        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].kind, OutcomeKind::ArtifactChanged);
        assert_eq!(outcomes[1].kind, OutcomeKind::TestOrBuildPassed);
    }

    #[test]
    fn test_recovery_loop_detection() {
        let mut trace = make_test_trace();
        let start = trace.started_at;

        // Step 1: Failed test
        trace.events.push(NormalizedEvent::new(
            1,
            start + Duration::seconds(1),
            EventPayload::ShellCommand(ShellCommand {
                command: "cargo test".to_string(),
                cwd: None,
                exit_code: Some(101),
                output: Some(
                    "error[E0425]: cannot find value `x` in this scope\ntest result: FAILED"
                        .to_string(),
                ),
            }),
        ));

        // Step 2: Agent edits file (corrective action)
        trace.events.push(NormalizedEvent::new(
            2,
            start + Duration::seconds(3),
            EventPayload::FileAction {
                path: "src/lib.rs".to_string(),
                action: FileActionType::Edit,
                diff: Some("+ let x = 42;".to_string()),
                lines_changed: Some(1),
            },
        ));

        // Step 3: Agent runs tests again -> PASS
        trace.events.push(NormalizedEvent::new(
            3,
            start + Duration::seconds(6),
            EventPayload::ShellCommand(ShellCommand {
                command: "cargo test".to_string(),
                cwd: None,
                exit_code: Some(0),
                output: Some("test result: ok. 1 passed; 0 failed".to_string()),
            }),
        ));

        let detector = RecoveryDetector::new();
        let recoveries = detector.detect_recoveries(&trace);

        assert_eq!(recoveries.len(), 1);
        let rec = &recoveries[0];
        assert_eq!(rec.failure_sequence, 1);
        assert_eq!(rec.recovery_sequence, 3);
        assert_eq!(rec.steps_to_recover, 2);
        assert_eq!(rec.corrective_actions_count, 1);
        assert!(rec.duration_seconds.unwrap() >= 5.0);
    }

    #[test]
    fn test_tool_error_recovery_loop() {
        let mut trace = make_test_trace();
        let start = trace.started_at;

        // Step 1: Error event
        trace.events.push(NormalizedEvent::new(
            10,
            start + Duration::seconds(1),
            EventPayload::Error {
                message: "Syntax error on line 45".to_string(),
                is_recovered: false,
            },
        ));

        // Step 2: Tool call to edit
        trace.events.push(NormalizedEvent::new(
            11,
            start + Duration::seconds(2),
            EventPayload::ToolCall(ToolCall {
                id: Some("edit_1".to_string()),
                name: "write_to_file".to_string(),
                arguments: serde_json::json!({}),
            }),
        ));

        // Step 3: Outcome evidence test passed
        trace.events.push(NormalizedEvent::new(
            12,
            start + Duration::seconds(4),
            EventPayload::OutcomeEvidence(OutcomeEvidence {
                kind: OutcomeKind::TestOrBuildPassed,
                summary: "All unit tests pass".to_string(),
                confidence: 0.85,
            }),
        ));

        let detector = RecoveryDetector::new();
        let recoveries = detector.detect_recoveries(&trace);

        assert_eq!(recoveries.len(), 1);
        assert_eq!(recoveries[0].failure_sequence, 10);
        assert_eq!(recoveries[0].recovery_sequence, 12);
        assert_eq!(recoveries[0].corrective_actions_count, 1);
    }

    #[test]
    fn test_contextual_file_path_correlation_recovery() {
        let mut trace = make_test_trace();
        let start = trace.started_at;

        // Step 1: Rust compiler failure pointing directly at src/recovery.rs
        trace.events.push(NormalizedEvent::new(
            1,
            start + Duration::seconds(1),
            EventPayload::ShellCommand(ShellCommand {
                command: "cargo check".to_string(),
                cwd: None,
                exit_code: Some(101),
                output: Some(
                    "error[E0425]: cannot find value `foo` in this scope\n  --> crates/outcomes/src/recovery.rs:52:13\n   |\n52 |     let x = foo;\n   |             ^^^ not found in this scope".to_string(),
                ),
            }),
        ));

        // Step 2: Unrelated edit to another file (should not correlate with the specific failure file)
        trace.events.push(NormalizedEvent::new(
            2,
            start + Duration::seconds(2),
            EventPayload::FileAction {
                path: "README.md".to_string(),
                action: FileActionType::Edit,
                diff: Some("+ docs".to_string()),
                lines_changed: Some(1),
            },
        ));

        // Step 3: Exact corrective edit modifying crates/outcomes/src/recovery.rs
        trace.events.push(NormalizedEvent::new(
            3,
            start + Duration::seconds(3),
            EventPayload::ToolCall(ToolCall {
                id: Some("call_edit".to_string()),
                name: "replace_file_content".to_string(),
                arguments: serde_json::json!({
                    "TargetFile": "/Users/saurabh/code/unfoundbox/agentworth/crates/outcomes/src/recovery.rs",
                    "ReplacementContent": "let x = 42;"
                }),
            }),
        ));

        // Step 4: Cargo check passes
        trace.events.push(NormalizedEvent::new(
            4,
            start + Duration::seconds(5),
            EventPayload::ShellCommand(ShellCommand {
                command: "cargo check".to_string(),
                cwd: None,
                exit_code: Some(0),
                output: Some("Finished `dev` profile in 0.42s".to_string()),
            }),
        ));

        let detector = RecoveryDetector::new();
        let recoveries = detector.detect_recoveries(&trace);

        assert_eq!(recoveries.len(), 1);
        let rec = &recoveries[0];
        assert_eq!(rec.failure_sequence, 1);
        assert_eq!(rec.recovery_sequence, 4);
        assert!(!rec.correlated_files.is_empty());
        assert!(rec.correlated_files.iter().any(|f| f.contains("recovery.rs")));
        assert!(rec.recovery_summary.contains("recovery.rs"));
    }
}

