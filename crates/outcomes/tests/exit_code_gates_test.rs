//! Rung 3 means "the tests passed", not "a test command was typed". Rung 5 means the deploy
//! or the CI run really exited 0, not that `gh pr create` appeared in a command string.
//!
//! Without a real exit code the classifier used to hand out `TestOrBuildPassed` at 0.70 (and
//! `CiOrDeploymentVerified` at 0.90) from the command string alone, so `cargo test` that died
//! on a compile error scored the same as `cargo test` that went green. These lock the line in
//! place for both rungs.

use agentworth_outcomes::OutcomeDetector;
use agentworth_schema::{
    AgentWorthTrace, EventPayload, NormalizedEvent, OutcomeKind, Provenance, ShellCommand, ToolCall,
};
use chrono::{Duration, Utc};

fn trace_with(events: Vec<EventPayload>) -> AgentWorthTrace {
    let prov = Provenance::new("/tmp/exit-codes.jsonl", "claude_code", 1024, 1000, "fp-exit");
    let mut trace = AgentWorthTrace::new("sess_exit_codes", "claude_code", prov, Utc::now());
    let start = trace.started_at;
    for (i, payload) in events.into_iter().enumerate() {
        trace.events.push(NormalizedEvent::new(
            i as u64 + 1,
            start + Duration::seconds(i as i64 + 1),
            payload,
        ));
    }
    trace
}

fn ci_command(exit_code: Option<i32>) -> EventPayload {
    EventPayload::ShellCommand(ShellCommand {
        command: "gh pr create --title 'feat: x' --body 'y'".to_string(),
        cwd: None,
        exit_code,
        output: None,
    })
}

#[test]
fn ci_command_with_exit_zero_earns_rung_five() {
    let trace = trace_with(vec![ci_command(Some(0))]);
    let outcomes = OutcomeDetector::new().detect_outcomes(&trace);

    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, OutcomeKind::CiOrDeploymentVerified);
    assert!(outcomes[0].confidence >= 0.95);
}

/// The gap this closes: `verify.rs` only ever demoted a bare `ToolCall`, so a `ShellCommand`
/// carrying a CI command string with no captured exit code sailed through at 0.90 -- the top
/// rung, on nothing but the text of the command.
#[test]
fn ci_command_without_an_exit_code_does_not_earn_rung_five() {
    let trace = trace_with(vec![ci_command(None)]);
    let outcomes = OutcomeDetector::new().detect_outcomes(&trace);

    assert_eq!(outcomes.len(), 1);
    assert_ne!(
        outcomes[0].kind,
        OutcomeKind::CiOrDeploymentVerified,
        "a deploy command with no captured result is not a verified deployment"
    );
    assert!(
        outcomes[0].confidence <= 0.5,
        "an unknown result cannot be reported at high confidence, got {}",
        outcomes[0].confidence
    );
    assert!(
        outcomes[0].summary.contains("result unknown"),
        "the summary should say what is actually known: {}",
        outcomes[0].summary
    );
}

#[test]
fn ci_command_with_a_failing_exit_code_earns_nothing() {
    let trace = trace_with(vec![ci_command(Some(1))]);
    let outcomes = OutcomeDetector::new().detect_outcomes(&trace);
    assert!(
        outcomes.is_empty(),
        "a failed deploy is not evidence of anything positive, got {:?}",
        outcomes
    );
}

#[test]
fn a_requested_ci_tool_call_alone_never_reaches_rung_five() {
    let trace = trace_with(vec![EventPayload::ToolCall(ToolCall {
        id: Some("toolu_ci".to_string()),
        name: "Bash".to_string(),
        arguments: serde_json::json!({"command": "gh pr create --title 'feat: x' --body 'y'"}),
    })]);
    let outcomes = OutcomeDetector::new().detect_outcomes(&trace);

    assert!(
        outcomes
            .iter()
            .all(|o| o.kind != OutcomeKind::CiOrDeploymentVerified),
        "a tool call is a request, not a result: {:?}",
        outcomes
    );
}

fn test_command(exit_code: Option<i32>) -> EventPayload {
    EventPayload::ShellCommand(ShellCommand {
        command: "cargo test --workspace".to_string(),
        cwd: None,
        exit_code,
        output: None,
    })
}

#[test]
fn test_command_with_exit_zero_earns_rung_three() {
    let trace = trace_with(vec![test_command(Some(0))]);
    let outcomes = OutcomeDetector::new().detect_outcomes(&trace);

    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].kind, OutcomeKind::TestOrBuildPassed);
    assert!(outcomes[0].confidence >= 0.85);
}

#[test]
fn test_command_without_an_exit_code_does_not_earn_rung_three() {
    let trace = trace_with(vec![test_command(None)]);
    let outcomes = OutcomeDetector::new().detect_outcomes(&trace);

    assert_eq!(outcomes.len(), 1);
    assert_ne!(
        outcomes[0].kind,
        OutcomeKind::TestOrBuildPassed,
        "a test command with no captured result is not a passing test run"
    );
    assert!(
        outcomes[0].confidence <= 0.5,
        "an unknown result cannot be reported at high confidence, got {}",
        outcomes[0].confidence
    );
    assert!(
        outcomes[0].summary.contains("result unknown"),
        "the summary should say what is actually known: {}",
        outcomes[0].summary
    );
}

#[test]
fn test_command_with_a_failing_exit_code_earns_nothing() {
    let trace = trace_with(vec![test_command(Some(101))]);
    let outcomes = OutcomeDetector::new().detect_outcomes(&trace);
    assert!(
        outcomes.is_empty(),
        "a failed run is not evidence of anything positive, got {:?}",
        outcomes
    );
}

#[test]
fn build_command_follows_the_same_rule_as_a_test_command() {
    let no_code = trace_with(vec![EventPayload::ShellCommand(ShellCommand {
        command: "npm run build".to_string(),
        cwd: None,
        exit_code: None,
        output: None,
    })]);
    let outcomes = OutcomeDetector::new().detect_outcomes(&no_code);
    assert_eq!(outcomes.len(), 1);
    assert_ne!(outcomes[0].kind, OutcomeKind::TestOrBuildPassed);

    let passed = trace_with(vec![EventPayload::ShellCommand(ShellCommand {
        command: "npm run build".to_string(),
        cwd: None,
        exit_code: Some(0),
        output: None,
    })]);
    let outcomes = OutcomeDetector::new().detect_outcomes(&passed);
    assert_eq!(outcomes[0].kind, OutcomeKind::TestOrBuildPassed);
}

/// The classifier and the independent-verification pass have to mean the same thing by rung 3.
/// `verify.rs` will only confirm a bare tool call once some real `ShellCommand` in the same
/// trace exited 0; the classifier now refuses to hand out the rung without one either.
#[test]
fn a_requested_tool_call_alone_never_reaches_rung_three() {
    let trace = trace_with(vec![EventPayload::ToolCall(ToolCall {
        id: Some("toolu_1".to_string()),
        name: "Bash".to_string(),
        arguments: serde_json::json!({"command": "cargo test --workspace"}),
    })]);
    let outcomes = OutcomeDetector::new().detect_outcomes(&trace);

    assert!(
        outcomes
            .iter()
            .all(|o| o.kind != OutcomeKind::TestOrBuildPassed),
        "a tool call is a request, not a result: {:?}",
        outcomes
    );
}

/// ...and the same trace with the real command beside it does reach it, off the command that
/// actually exited 0 rather than off the request.
#[test]
fn the_real_command_beside_the_request_still_earns_rung_three() {
    let trace = trace_with(vec![
        EventPayload::ToolCall(ToolCall {
            id: Some("toolu_1".to_string()),
            name: "Bash".to_string(),
            arguments: serde_json::json!({"command": "cargo test --workspace"}),
        }),
        test_command(Some(0)),
    ]);
    let outcomes = OutcomeDetector::new().detect_outcomes(&trace);

    assert!(
        outcomes
            .iter()
            .any(|o| o.kind == OutcomeKind::TestOrBuildPassed),
        "got {:?}",
        outcomes
    );
}
