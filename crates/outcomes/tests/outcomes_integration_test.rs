use agentworth_outcomes::{outcome_rank, OutcomeDetector, RecoveryDetector};
use agentworth_schema::{
    AgentWorthTrace, EventPayload, FileActionType, NormalizedEvent, OutcomeKind, Provenance,
    ShellCommand, ToolCall, ToolResult,
};
use chrono::{Duration, Utc};

fn create_trace(session_id: &str) -> AgentWorthTrace {
    let prov = Provenance::new("/tmp/test.jsonl", "claude_code", 1024, 1000, "fp123");
    AgentWorthTrace::new(session_id, "claude_code", prov, Utc::now())
}

#[test]
fn test_pytest_and_jest_output_detection() {
    let mut trace = create_trace("sess_tests");
    let start = trace.started_at;

    // Pytest passing
    trace.events.push(NormalizedEvent::new(
        1,
        start + Duration::seconds(1),
        EventPayload::ShellCommand(ShellCommand {
            command: "pytest tests/".to_string(),
            cwd: Some("/app".to_string()),
            exit_code: Some(0),
            output: Some("tests/test_api.py::test_login PASSED [ 50%]\ntests/test_api.py::test_logout PASSED [100%]\n2 passed in 0.12s".to_string()),
        }),
    ));

    // Jest passing in ToolResult
    trace.events.push(NormalizedEvent::new(
        2,
        start + Duration::seconds(2),
        EventPayload::ToolResult(ToolResult {
            call_id: Some("call_jest".to_string()),
            name: Some("Bash".to_string()),
            output: serde_json::json!({"stdout": "PASS  src/App.test.tsx\nTest Suites: 1 passed, 1 total\nTests: 2 passed, 2 total"}),
            is_error: false,
        }),
    ));

    let detector = OutcomeDetector::new();
    let outcomes = detector.detect_outcomes(&trace);

    assert_eq!(outcomes.len(), 2);
    assert_eq!(outcomes[0].kind, OutcomeKind::TestOrBuildPassed);
    assert_eq!(outcomes[1].kind, OutcomeKind::TestOrBuildPassed);
}

#[test]
fn test_complex_multi_loop_recovery() {
    let mut trace = create_trace("sess_recovery_multi");
    let start = trace.started_at;

    // Loop 1: TypeScript compilation failure -> Fix tsconfig -> Success
    trace.events.push(NormalizedEvent::new(
        1,
        start + Duration::seconds(1),
        EventPayload::ShellCommand(ShellCommand {
            command: "npm run build".to_string(),
            cwd: None,
            exit_code: Some(2),
            output: Some("error TS2304: Cannot find name 'Config'.".to_string()),
        }),
    ));

    trace.events.push(NormalizedEvent::new(
        2,
        start + Duration::seconds(2),
        EventPayload::FileAction {
            path: "src/types.ts".to_string(),
            action: FileActionType::Write,
            diff: Some("export type Config = {};".to_string()),
            lines_changed: Some(1),
        },
    ));

    trace.events.push(NormalizedEvent::new(
        3,
        start + Duration::seconds(4),
        EventPayload::ShellCommand(ShellCommand {
            command: "npm run build".to_string(),
            cwd: None,
            exit_code: Some(0),
            output: Some("Build succeeded. Compiled successfully.".to_string()),
        }),
    ));

    // Loop 2: Unit test failure -> Fix function -> Success
    trace.events.push(NormalizedEvent::new(
        4,
        start + Duration::seconds(5),
        EventPayload::ShellCommand(ShellCommand {
            command: "npm test".to_string(),
            cwd: None,
            exit_code: Some(1),
            output: Some("FAIL src/calc.test.ts\nExpected 4 got 5".to_string()),
        }),
    ));

    trace.events.push(NormalizedEvent::new(
        5,
        start + Duration::seconds(7),
        EventPayload::ToolCall(ToolCall {
            id: Some("edit_calc".to_string()),
            name: "replace_file_content".to_string(),
            arguments: serde_json::json!({"path": "src/calc.ts"}),
        }),
    ));

    trace.events.push(NormalizedEvent::new(
        6,
        start + Duration::seconds(9),
        EventPayload::ShellCommand(ShellCommand {
            command: "npm test".to_string(),
            cwd: None,
            exit_code: Some(0),
            output: Some("PASS src/calc.test.ts\nTests: 3 passed, 3 total".to_string()),
        }),
    ));

    let recovery_detector = RecoveryDetector::new();
    let recoveries = recovery_detector.detect_recoveries(&trace);

    assert_eq!(recoveries.len(), 2);
    assert_eq!(recoveries[0].failure_sequence, 1);
    assert_eq!(recoveries[0].recovery_sequence, 3);
    assert!(recoveries[0].correlated_files.iter().any(|f| f.contains("types.ts")));

    assert_eq!(recoveries[1].failure_sequence, 4);
    assert_eq!(recoveries[1].recovery_sequence, 6);
    assert!(recoveries[1].correlated_files.iter().any(|f| f.contains("calc.ts")));
}

#[test]
fn test_outcome_ranking_comparisons() {
    assert!(outcome_rank(OutcomeKind::DoneClaimed) < outcome_rank(OutcomeKind::ArtifactChanged));
    assert!(
        outcome_rank(OutcomeKind::ArtifactChanged) < outcome_rank(OutcomeKind::TestOrBuildPassed)
    );
    assert!(
        outcome_rank(OutcomeKind::TestOrBuildPassed) < outcome_rank(OutcomeKind::CommitObserved)
    );
    assert!(
        outcome_rank(OutcomeKind::CommitObserved)
            < outcome_rank(OutcomeKind::CiOrDeploymentVerified)
    );
}
