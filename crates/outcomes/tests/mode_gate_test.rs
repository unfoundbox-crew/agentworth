//! mode_gate TDD gate (fleet-spec §15, decisions-v1 rows #10/M).
//!
//! Watched-fail first: this file references `agentworth_outcomes::mode_gate`,
//! which does not exist yet, so the suite FAILS until the lane lands.
//! Each test names the failure it watches.

use agentworth_outcomes::mode_gate::{
    evaluate_mode_gate, extract_commits, extract_test_passes, find_bypass, Mode,
};
use agentworth_schema::{
    AgentWorthTrace, EventPayload, NormalizedEvent, OutcomeEvidence, OutcomeKind, Provenance,
};
use chrono::{Duration, Utc};

fn trace_with(events: Vec<NormalizedEvent>) -> AgentWorthTrace {
    let prov = Provenance::new(
        "/tmp/mode_gate.jsonl",
        "claude_code",
        100,
        1000,
        "fp_mode_gate",
    );
    let mut t = AgentWorthTrace::new("sess_mode_gate", "claude_code", prov, Utc::now());
    t.events = events;
    t
}

fn msg(seq: u64, content: &str) -> NormalizedEvent {
    NormalizedEvent::new(
        seq,
        Utc::now() + Duration::seconds(seq as i64),
        EventPayload::AssistantMessage {
            content: content.to_string(),
            thinking: None,
        },
    )
}

fn pass_evidence() -> OutcomeEvidence {
    OutcomeEvidence {
        test_provenance: None,
        kind: OutcomeKind::TestOrBuildPassed,
        summary: "cargo test exited 0".to_string(),
        confidence: 0.85,
    }
}

fn commit_evidence() -> OutcomeEvidence {
    OutcomeEvidence {
        test_provenance: None,
        kind: OutcomeKind::CommitObserved,
        summary: "git commit observed".to_string(),
        confidence: 0.90,
    }
}

/// §15 mode_gate.test.build_zero_tests_red: build handoff, zero tests -> RED.
#[test]
fn build_handoff_with_zero_tests_is_red() {
    let trace = trace_with(vec![msg(1, "did some work")]);
    assert!(
        !trace.events.is_empty(),
        "extractor rule: test fixture must be non-empty"
    );
    let outcomes: Vec<OutcomeEvidence> = vec![];
    assert!(
        extract_test_passes(&outcomes).is_empty(),
        "watched failure: zero test passes extracted"
    );
    let res = evaluate_mode_gate(Mode::Build, &trace.events, &outcomes);
    assert!(!res.pass, "build + zero tests must be RED");
    assert_eq!(res.reason, "build_zero_tests");
    assert_eq!(res.bypass_count, 0);
}

/// Receipted bypass turns the same build handoff GREEN and is counted.
#[test]
fn build_zero_tests_with_receipted_bypass_is_green_and_counted() {
    let trace = trace_with(vec![
        msg(1, "did some work"),
        msg(2, "bypass: docs-only change, no code to test"),
    ]);
    assert!(
        !trace.events.is_empty(),
        "extractor rule: test fixture must be non-empty"
    );
    let bypass = find_bypass(&trace.events)
        .expect("watched failure: receipted bypass:<reason> must be found");
    assert!(!bypass.reason.is_empty(), "bypass reason must be non-empty");
    let outcomes: Vec<OutcomeEvidence> = vec![];
    let res = evaluate_mode_gate(Mode::Build, &trace.events, &outcomes);
    assert!(
        res.pass,
        "build + zero tests + receipted bypass must be GREEN"
    );
    assert_eq!(res.bypass_count, 1);
}

/// §15 mode_gate.test.explore_zero_commits_green: explore, zero commits -> GREEN.
#[test]
fn explore_handoff_with_zero_commits_is_green() {
    let trace = trace_with(vec![msg(1, "looked around, changed nothing")]);
    assert!(
        !trace.events.is_empty(),
        "extractor rule: test fixture must be non-empty"
    );
    let outcomes: Vec<OutcomeEvidence> = vec![];
    assert!(
        extract_commits(&outcomes).is_empty(),
        "watched failure: zero commits extracted"
    );
    let res = evaluate_mode_gate(Mode::Explore, &trace.events, &outcomes);
    assert!(res.pass, "explore + zero commits must be GREEN");
}

/// Extractor rule: empty trace fails closed, never a false GREEN.
#[test]
fn empty_trace_fails_closed_not_green() {
    let trace = trace_with(vec![]);
    assert!(
        trace.events.is_empty(),
        "this fixture is the empty-trace case"
    );
    let outcomes: Vec<OutcomeEvidence> = vec![];
    let res = evaluate_mode_gate(Mode::Build, &trace.events, &outcomes);
    assert!(!res.pass, "empty trace must not pass any gate");
    assert_eq!(res.reason, "empty_trace_no_evidence");
}

/// Sanity: build with a real test pass is GREEN without any bypass.
#[test]
fn build_with_test_pass_is_green_without_bypass() {
    let trace = trace_with(vec![msg(1, "built it, tests green")]);
    assert!(
        !trace.events.is_empty(),
        "extractor rule: test fixture must be non-empty"
    );
    let outcomes = vec![pass_evidence()];
    assert_eq!(
        extract_test_passes(&outcomes).len(),
        1,
        "extractor must see the pass"
    );
    let res = evaluate_mode_gate(Mode::Build, &trace.events, &outcomes);
    assert!(res.pass, "build + tests must be GREEN");
    assert_eq!(res.bypass_count, 0);
}

/// Sanity: commit extractor sees commits (guards the explore-side wiring).
#[test]
fn commit_extractor_sees_commit_evidence() {
    let outcomes = vec![commit_evidence()];
    assert!(
        !outcomes.is_empty(),
        "extractor rule: fixture must be non-empty"
    );
    assert_eq!(extract_commits(&outcomes).len(), 1);
}
