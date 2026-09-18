//! Test-provenance on pass/fail records (decisions-v1 row R + fleet spec §5/§6).
//!
//! Every pass/fail cites WHO wrote the test (human / agent session id) and WHEN,
//! and the promotion path distinguishes agent-chosen tests from held-out tests:
//! promotion on agent-chosen tests only FAILS (the `held_out` gate).

use agentworth_outcomes::{promotion_eligible, OutcomeDetector};
use agentworth_schema::{
    AgentWorthTrace, EventPayload, OutcomeEvidence, OutcomeKind, Provenance, ShellCommand,
    TestOrigin, TestProvenance,
};
use chrono::{Duration, Utc};

fn agent_chosen_pass(session: &str) -> OutcomeEvidence {
    OutcomeEvidence {
        kind: OutcomeKind::TestOrBuildPassed,
        summary: "pytest passed".to_string(),
        confidence: 0.9,
        test_provenance: Some(TestProvenance {
            author: format!("agent:{session}"),
            authored_at: Some(Utc::now()),
            origin: TestOrigin::AgentChosen,
        }),
    }
}

fn held_out_pass() -> OutcomeEvidence {
    OutcomeEvidence {
        kind: OutcomeKind::TestOrBuildPassed,
        summary: "held-out suite passed".to_string(),
        confidence: 0.95,
        test_provenance: Some(TestProvenance {
            author: "human:saurabh".to_string(),
            authored_at: Some(Utc::now()),
            origin: TestOrigin::HeldOut,
        }),
    }
}

#[test]
fn test_pass_cites_who_and_when() {
    let ev = held_out_pass();
    let prov = ev.test_provenance.as_ref().expect("pass must cite test provenance");
    assert_eq!(prov.author, "human:saurabh");
    assert!(prov.authored_at.is_some(), "pass must cite when the test was written");

    // Provenance survives a serde round-trip (index stores evidence as JSON).
    let json = serde_json::to_string(&ev).expect("evidence serializes");
    let back: OutcomeEvidence = serde_json::from_str(&json).expect("evidence deserializes");
    assert_eq!(back.test_provenance, ev.test_provenance);
}

#[test]
fn old_exports_without_provenance_still_deserialize() {
    // Migration gate (decisions-v1 row R): adding the field must not invalidate
    // history. Old exports carry no `test_provenance` and must read back as
    // `None` — which fails the promotion gate closed.
    let old: OutcomeEvidence = serde_json::from_str(
        r#"{"kind":"test_or_build_passed","summary":"pytest passed","confidence":0.9}"#,
    )
    .expect("old export deserializes");
    assert_eq!(old.test_provenance, None);
    assert!(!promotion_eligible(&[old]));
}

#[test]
fn promotion_on_agent_chosen_tests_only_fails() {
    let evidence = vec![agent_chosen_pass("sess-123")];
    assert!(
        !promotion_eligible(&evidence),
        "held_out gate: agent-chosen tests alone must not promote"
    );
}

#[test]
fn promotion_with_held_out_pass_succeeds() {
    let evidence = vec![agent_chosen_pass("sess-123"), held_out_pass()];
    assert!(
        promotion_eligible(&evidence),
        "a held-out pass alongside agent-chosen tests promotes"
    );
}

#[test]
fn detector_pass_without_provenance_is_not_promotable() {
    // The detector cannot know who wrote the test it observed passing, so its
    // evidence must fail closed: present, but not promotable until provenance
    // is attached.
    let prov = Provenance::new("/tmp/test.jsonl", "claude_code", 1024, 1000, "fp123");
    let mut trace = AgentWorthTrace::new("sess-det", "claude_code", prov, Utc::now());
    trace.events.push(agentworth_schema::NormalizedEvent::new(
        1,
        trace.started_at + Duration::seconds(1),
        EventPayload::ShellCommand(ShellCommand {
            command: "pytest tests/".to_string(),
            cwd: Some("/app".to_string()),
            exit_code: Some(0),
            output: Some("2 passed in 0.12s".to_string()),
        }),
    ));
    let outcomes = OutcomeDetector::new().detect_outcomes(&trace);
    assert!(
        outcomes.iter().any(|o| o.kind == OutcomeKind::TestOrBuildPassed),
        "detector still reports the pass"
    );
    assert!(
        !promotion_eligible(&outcomes),
        "unattributed detector pass must not promote"
    );
}
