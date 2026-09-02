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

/// The crash, reproduced as a test: `crates/outcomes/src/recovery.rs:571` used to byte-slice
/// failure output at a fixed offset (`&s[..80]`) to build the failure summary. On Saurabh's
/// machine, a transcript containing Hebrew ('ך', 2 bytes) hit that offset mid-character and
/// panicked mid-scan. This fixture packs every text field the detector touches (the shell
/// command's failure output, the tool result's failure output, and the corrective/success
/// output) with Hebrew, Arabic, CJK, emoji, and a combining mark, at lengths chosen so a
/// byte-index cut at 79, 80, 81, 120, or 200 would have landed inside a multi-byte character
/// under the old code. `detect_recoveries` must return a clean recovery signal instead of
/// panicking.
#[test]
fn test_recovery_detection_survives_multibyte_failure_text() {
    let mut trace = create_trace("sess_multibyte_recovery");
    let start = trace.started_at;

    // 7 ASCII bytes ("panic: ") + Hebrew ('ך', 2 bytes/char) puts byte offset 80 one byte
    // into the 37th Hebrew character -- exactly the old panic's shape ("bytes 79..81").
    let hebrew_panic = format!("panic: {}", "ך".repeat(150)); // 7 + 300 = 307 bytes

    // Arabic (2 bytes/char) failure text for the ToolResult path, long enough to straddle
    // 120 and 200 as well as 79-81.
    let arabic_failure = format!(
        "error[E0384]: {}",
        "أصلح الاختبار الفاشل رجاء وأعد المحاولة ".repeat(6)
    );

    // CJK (3 bytes/char) plus an emoji (4 bytes) and a combining mark, for the corrective
    // and success text so those code paths see multi-byte content too.
    let cjk_note = format!("正在修复失败的测试🎉e\u{0301}f\u{0301} {}", "测试".repeat(40));

    trace.events.push(NormalizedEvent::new(
        1,
        start + Duration::seconds(1),
        EventPayload::ShellCommand(ShellCommand {
            command: "npm run build".to_string(),
            cwd: None,
            exit_code: Some(0),
            output: Some(hebrew_panic.clone()),
        }),
    ));

    trace.events.push(NormalizedEvent::new(
        2,
        start + Duration::seconds(2),
        EventPayload::ToolResult(ToolResult {
            call_id: Some("t_arabic".to_string()),
            name: Some("Bash".to_string()),
            output: serde_json::json!(arabic_failure),
            is_error: false,
        }),
    ));

    trace.events.push(NormalizedEvent::new(
        3,
        start + Duration::seconds(3),
        EventPayload::FileAction {
            path: "src/calc.ts".to_string(),
            action: FileActionType::Write,
            diff: Some(cjk_note.clone()),
            lines_changed: Some(1),
        },
    ));

    trace.events.push(NormalizedEvent::new(
        4,
        start + Duration::seconds(4),
        EventPayload::ShellCommand(ShellCommand {
            command: "npm test".to_string(),
            cwd: None,
            exit_code: Some(0),
            output: Some(format!("test result: ok. {}", cjk_note)),
        }),
    ));

    let recovery_detector = RecoveryDetector::new();
    let recoveries = recovery_detector.detect_recoveries(&trace);

    // Both multibyte failures (the ShellCommand and the ToolResult) resolve against the one
    // success event that follows them -- the detector must not panic and must still find them.
    assert_eq!(recoveries.len(), 2);
    for r in &recoveries {
        assert!(
            r.failure_summary.chars().all(|c| c != '\u{FFFD}'),
            "failure summary must be valid truncated UTF-8, not lossy-replaced: {}",
            r.failure_summary
        );
    }
}
