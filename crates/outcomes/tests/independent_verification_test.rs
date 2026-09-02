//! End-to-end proof that independent verification changes real classification through the
//! exact same public API every real caller already uses (`OutcomeDetector::detect_outcomes`,
//! consumed by `crates/core`'s `Scanner::run_scan` and `crates/scoring`'s `TraceScorer::score`).
//! No call site anywhere else in the workspace needs to change for these fixes to take effect.

use agentworth_outcomes::OutcomeDetector;
use agentworth_schema::{
    AgentWorthTrace, EventPayload, NormalizedEvent, OutcomeKind, Provenance, ShellCommand,
    ToolCall,
};
use chrono::{Duration, Utc};

fn create_trace(session_id: &str) -> AgentWorthTrace {
    let prov = Provenance::new("/tmp/verify_test.jsonl", "claude_code", 10, 0, "fp");
    AgentWorthTrace::new(session_id, "claude_code", prov, Utc::now())
}

#[test]
fn unconfirmed_ci_tool_call_is_downgraded_and_loses_the_ladder() {
    let mut trace = create_trace("sess_fake_pr");
    let start = trace.started_at;

    // The agent only ever *asks* to open a PR (a ToolCall carries arguments, not a result).
    // It never observes whether that command actually ran.
    trace.events.push(NormalizedEvent::new(
        1,
        start + Duration::seconds(1),
        EventPayload::ToolCall(ToolCall {
            id: Some("call_1".to_string()),
            name: "Bash".to_string(),
            arguments: serde_json::json!({"command": "gh pr create --title 'feat: x' --body 'y'"}),
        }),
    ));

    let detector = OutcomeDetector::new();
    let outcomes = detector.detect_outcomes(&trace);

    assert_eq!(outcomes.len(), 1);
    assert_eq!(
        outcomes[0].kind,
        OutcomeKind::DoneClaimed,
        "an unconfirmed tool-call intent must not be trusted as CiOrDeploymentVerified"
    );
    assert!(outcomes[0].confidence <= 0.20);

    let strongest = detector.strongest_outcome(&outcomes).unwrap();
    assert_eq!(strongest.kind, OutcomeKind::DoneClaimed);
}

#[test]
fn ci_tool_call_confirmed_by_a_real_result_keeps_its_rank() {
    let mut trace = create_trace("sess_real_pr");
    let start = trace.started_at;

    trace.events.push(NormalizedEvent::new(
        1,
        start + Duration::seconds(1),
        EventPayload::ToolCall(ToolCall {
            id: Some("call_1".to_string()),
            name: "Bash".to_string(),
            arguments: serde_json::json!({"command": "gh pr create --title 'feat: x' --body 'y'"}),
        }),
    ));
    // ... followed by the real, observed execution with a real recorded exit code.
    trace.events.push(NormalizedEvent::new(
        2,
        start + Duration::seconds(2),
        EventPayload::ShellCommand(ShellCommand {
            command: "gh pr create --title 'feat: x' --body 'y'".to_string(),
            cwd: None,
            exit_code: Some(0),
            output: Some(
                "https://github.com/org/repo/pull/1\nPull request successfully created"
                    .to_string(),
            ),
        }),
    ));

    let detector = OutcomeDetector::new();
    let outcomes = detector.detect_outcomes(&trace);

    let ci_outcomes: Vec<_> = outcomes
        .iter()
        .filter(|o| o.kind == OutcomeKind::CiOrDeploymentVerified)
        .collect();
    assert!(
        !ci_outcomes.is_empty(),
        "a really-confirmed CI claim must keep its rank"
    );
    // The rung is earned by the command that really exited 0, not by the request beside it:
    // the classifier no longer hands rung 5 to a command string with no captured exit code.
    assert!(
        ci_outcomes.iter().any(|o| o.summary.contains("exited 0")),
        "the surviving CI evidence must be the one with a real exit code: {:?}",
        ci_outcomes
    );

    let strongest = detector.strongest_outcome(&outcomes).unwrap();
    assert_eq!(strongest.kind, OutcomeKind::CiOrDeploymentVerified);
}

#[test]
fn verification_notes_report_the_downgrade_with_a_reason() {
    let mut trace = create_trace("sess_notes");
    let start = trace.started_at;
    trace.events.push(NormalizedEvent::new(
        1,
        start + Duration::seconds(1),
        EventPayload::ToolCall(ToolCall {
            id: Some("call_1".to_string()),
            name: "Bash".to_string(),
            arguments: serde_json::json!({"command": "git commit -m 'wip'"}),
        }),
    ));

    let detector = OutcomeDetector::new();
    let (outcomes, notes) = detector.detect_outcomes_with_verification(&trace);

    assert_eq!(outcomes[0].kind, OutcomeKind::DoneClaimed);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].original_kind, OutcomeKind::CommitObserved);
    assert_eq!(notes[0].final_kind, OutcomeKind::DoneClaimed);
    assert!(notes[0].reason.contains("only ever requested"));
}

#[test]
fn done_claimed_prose_with_no_backing_execution_is_visibly_flagged() {
    let mut trace = create_trace("sess_bare_claim");
    let start = trace.started_at;
    trace.events.push(NormalizedEvent::new(
        1,
        start + Duration::seconds(1),
        EventPayload::AssistantMessage {
            content: "I have completed all the requested tasks!".to_string(),
            thinking: None,
        },
    ));

    let detector = OutcomeDetector::new();
    let outcomes = detector.detect_outcomes(&trace);

    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, OutcomeKind::DoneClaimed);
    assert!(outcomes[0].confidence <= 0.10);
    assert!(outcomes[0].summary.contains("unconfirmed"));
}

#[test]
fn real_test_run_and_real_commit_still_classify_at_full_strength() {
    // Regression guard: a trace built entirely from real, structured, successful events
    // (no bare tool-call intents, no unbacked prose) must classify exactly as it did before
    // verification existed — nothing here should be downgraded.
    let mut trace = create_trace("sess_all_real");
    let start = trace.started_at;

    trace.events.push(NormalizedEvent::new(
        1,
        start + Duration::seconds(1),
        EventPayload::ShellCommand(ShellCommand {
            command: "cargo test --all".to_string(),
            cwd: Some("/repo".to_string()),
            exit_code: Some(0),
            output: Some("test result: ok. 5 passed; 0 failed".to_string()),
        }),
    ));
    trace.events.push(NormalizedEvent::new(
        2,
        start + Duration::seconds(2),
        EventPayload::ShellCommand(ShellCommand {
            command: "git commit -m 'feat: implement feature'".to_string(),
            cwd: Some("/repo".to_string()),
            exit_code: Some(0),
            output: Some("[main 9f3e1a2] feat: implement feature\n 1 file changed".to_string()),
        }),
    ));

    let detector = OutcomeDetector::new();
    let outcomes = detector.detect_outcomes(&trace);

    assert_eq!(outcomes.len(), 2);
    assert_eq!(outcomes[0].kind, OutcomeKind::TestOrBuildPassed);
    assert_eq!(outcomes[1].kind, OutcomeKind::CommitObserved);
    // "/repo" does not exist on disk, so repo-root resolution must cleanly fail into
    // Unverifiable rather than misfiring as Contradicted against a nonexistent path.
    assert!(outcomes[1].confidence >= 0.90);
}
