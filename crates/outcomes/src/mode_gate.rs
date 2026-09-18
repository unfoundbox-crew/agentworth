//! `mode_gate` (fleet-spec §15, decisions-v1 rows #10/M).
//!
//! Deterministic, no model. The gate reads the *observed* mode, never the
//! declared hint:
//!
//! - `build` handoff with zero test passes -> RED, unless a receipted
//!   `bypass: <reason>` is present (then GREEN with the bypass counted).
//! - `explore` handoff with zero commits -> GREEN.
//! - Fail-closed: an empty event slice carries no evidence, so it is RED,
//!   never a false GREEN (the extractor rule: an extractor that can return
//!   empty is only ever read after asserting its input is non-empty).

use agentworth_schema::{EventPayload, NormalizedEvent, OutcomeEvidence, OutcomeKind};

/// Lane mode the gate evaluates. Two modes only (§5): everything else
/// (`demo`, `spike`, `review`, brainstorm) is a field, not a mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Build,
    Explore,
}

/// Deterministic observation (§6): what the evidence says, never a model call.
/// `Build` iff rung >= 3 evidence (test pass, commit, or CI/deploy) exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeObserved {
    Build,
    Explore,
    Unknown,
}

/// A receipted bypass: the `bypass: <reason>` line plus where it was said.
/// No receipt (no event sequence) = not stored, not counted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bypass {
    pub reason: String,
    pub sequence: u64,
}

/// Outcome of one gate evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeGateResult {
    pub pass: bool,
    pub reason: String,
    pub test_count: usize,
    pub commit_count: usize,
    pub bypass_count: usize,
}

/// Derives the observed mode from outcome evidence (packet proposal §6).
pub fn observe_mode(outcomes: &[OutcomeEvidence]) -> ModeObserved {
    if outcomes.iter().any(|o| {
        matches!(
            o.kind,
            OutcomeKind::TestOrBuildPassed
                | OutcomeKind::CommitObserved
                | OutcomeKind::CiOrDeploymentVerified
        )
    }) {
        return ModeObserved::Build;
    }
    if outcomes.is_empty() {
        return ModeObserved::Unknown;
    }
    ModeObserved::Explore
}

/// Extractor: test/build passes. Can return empty — callers assert non-empty
/// input first (extractor rule).
pub fn extract_test_passes(outcomes: &[OutcomeEvidence]) -> Vec<&OutcomeEvidence> {
    outcomes
        .iter()
        .filter(|o| o.kind == OutcomeKind::TestOrBuildPassed)
        .collect()
}

/// Extractor: commit evidence (commit observed or stronger). Can return empty —
/// callers assert non-empty input first (extractor rule).
pub fn extract_commits(outcomes: &[OutcomeEvidence]) -> Vec<&OutcomeEvidence> {
    outcomes
        .iter()
        .filter(|o| {
            matches!(
                o.kind,
                OutcomeKind::CommitObserved | OutcomeKind::CiOrDeploymentVerified
            )
        })
        .collect()
}

/// Finds a receipted `bypass: <reason>` line in message events.
/// Returns the first non-empty reason with its event sequence as receipt.
pub fn find_bypass(events: &[NormalizedEvent]) -> Option<Bypass> {
    for event in events {
        let content = match &event.payload {
            EventPayload::UserMessage { content } => Some(content.as_str()),
            EventPayload::AssistantMessage { content, .. } => Some(content.as_str()),
            _ => None,
        };
        if let Some(text) = content {
            if let Some(reason) = parse_bypass_reason(text) {
                return Some(Bypass {
                    reason,
                    sequence: event.sequence,
                });
            }
        }
    }
    None
}

fn parse_bypass_reason(text: &str) -> Option<String> {
    let idx = find_bypass_marker(text)?;
    let reason = text.get(idx + "bypass:".len()..)?.trim();
    // One line only; a trailing newline starts a new thought, not the reason.
    let reason = reason.lines().next().unwrap_or("").trim();
    if reason.is_empty() {
        return None;
    }
    Some(reason.to_string())
}

/// ASCII case-insensitive search for `bypass:` returning a byte index into the
/// original text (char boundary safe: the match itself is pure ASCII).
fn find_bypass_marker(text: &str) -> Option<usize> {
    const NEEDLE: &[u8] = b"bypass:";
    let hay = text.as_bytes();
    if hay.len() < NEEDLE.len() {
        return None;
    }
    (0..=hay.len() - NEEDLE.len()).find(|&i| {
        hay[i..i + NEEDLE.len()]
            .iter()
            .zip(NEEDLE.iter())
            .all(|(a, b)| a.to_ascii_lowercase() == *b)
    })
}

/// Evaluates the gate for one handoff.
///
/// - Empty `events` -> RED (`empty_trace_no_evidence`), fail-closed.
/// - `Build` + zero test passes -> RED (`build_zero_tests`), unless a
///   receipted bypass is present -> GREEN (`build_bypassed`, counted).
/// - `Build` + tests -> GREEN (`build_tests_present`).
/// - `Explore` -> GREEN (`explore_ok` / `explore_zero_commits_ok`).
pub fn evaluate_mode_gate(
    mode: Mode,
    events: &[NormalizedEvent],
    outcomes: &[OutcomeEvidence],
) -> ModeGateResult {
    if events.is_empty() {
        return ModeGateResult {
            pass: false,
            reason: "empty_trace_no_evidence".to_string(),
            test_count: 0,
            commit_count: 0,
            bypass_count: 0,
        };
    }
    let test_count = extract_test_passes(outcomes).len();
    let commit_count = extract_commits(outcomes).len();
    let bypass = find_bypass(events);
    let bypass_count = usize::from(bypass.is_some());

    match mode {
        Mode::Build => {
            if test_count == 0 {
                if bypass.is_some() {
                    ModeGateResult {
                        pass: true,
                        reason: "build_bypassed".to_string(),
                        test_count,
                        commit_count,
                        bypass_count,
                    }
                } else {
                    ModeGateResult {
                        pass: false,
                        reason: "build_zero_tests".to_string(),
                        test_count,
                        commit_count,
                        bypass_count: 0,
                    }
                }
            } else {
                ModeGateResult {
                    pass: true,
                    reason: "build_tests_present".to_string(),
                    test_count,
                    commit_count,
                    bypass_count,
                }
            }
        }
        Mode::Explore => ModeGateResult {
            pass: true,
            reason: if commit_count == 0 {
                "explore_zero_commits_ok".to_string()
            } else {
                "explore_ok".to_string()
            },
            test_count,
            commit_count,
            bypass_count,
        },
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn bypass_reason_parsing_keeps_first_line_only() {
        assert_eq!(
            parse_bypass_reason("bypass: docs-only, no code"),
            Some("docs-only, no code".to_string())
        );
        assert_eq!(parse_bypass_reason("bypass:"), None);
        assert_eq!(parse_bypass_reason("nothing here"), None);
    }
}
