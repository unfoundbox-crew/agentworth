//! Context-Rot Marker: flags sessions whose agent-output quality plausibly degraded as the
//! session's own context grew, so someone reviewing session history can spot "this should have
//! been split into a fresh session earlier."
//!
//! # Why this shape
//!
//! There is no ground-truth label for "context rot" anywhere in the schema — nothing tags a
//! session afterward as "yes, this one rotted." So this detector does not classify against an
//! absolute bar (e.g. "sessions over 100k tokens are suspect"). The task that produced this file
//! explicitly warns against that: a long session that stays sharp throughout is not context rot,
//! and a short session that degrades quickly might be. Instead, a session is compared against
//! *itself*: split it into three chronological segments (Early / Middle / Late) along its own
//! token-growth curve, compute the same handful of quality proxies in each segment, and check
//! whether Late is the worst point on every proxy that moves. A session that dips in the middle
//! and recovers by the end is deliberately NOT flagged — that is a rough patch handled well, not
//! unresolved rot at the point the session stopped.
//!
//! Segmentation prefers cumulative token count (summed from `ModelInvocation` events) over raw
//! event count, per the task's framing: it is context *growth* that should predict rot, not
//! elapsed turns. Traces with no token-bearing events at all (an adapter that never emits
//! `ModelInvocation`) fall back to equal event-count thirds.
//!
//! Five component signals feed the score, deliberately mixing two kinds of evidence:
//!
//! - **Structural** (read directly off typed fields, no text classification): failure/error
//!   rate, and repeat-edit churn on the same file path within a segment. This is the trustworthy
//!   half of the signal.
//! - **Inferred** (reuses `agentworth_outcomes::OutcomeDetector` / `RecoveryDetector`, which
//!   classify by keyword/regex matching): the ratio of bare "done claimed" self-assertions to
//!   all outcome evidence, decline in the highest verification rung reached, and slowdown in
//!   failure-recovery loops (`steps_to_recover` rising). These inherit whatever false-positive/
//!   false-negative rate those detectors already have — a prior pass over this same project
//!   found a real casing bug in outcome-string matching (`blind_spots.rs` comparing against
//!   lowercase when the index stores PascalCase), so treat this half as directional, not exact.
//!
//! `rot_score` is deliberately not a pure magnitude average. Half of it comes from *how many*
//! of the five components independently moved past their trigger threshold, half from *how far*
//! they moved. Weighting agreement-across-signals this heavily is a direct response to the
//! "inferred" half being noisy: three independent signals moving a little is more trustworthy
//! than one signal moving a lot, given that any single one of them (especially the
//! keyword-matched ones) can be wrong. `flagged` additionally hard-requires at least two
//! components to trigger, so a single noisy metric can never flag a session by itself.
//!
//! # Limitations (read before trusting this in a dashboard)
//!
//! - **No labeled ground truth.** Every threshold and weight below (`ROT_SCORE_THRESHOLD`,
//!   `TRIGGER_EPS`, the per-component weights, `RECOVERY_FRICTION_SCALE`) is a reasoned guess
//!   checked against a handful of hand-built fixture sessions (see the tests in this file), not
//!   fit to real flagged/unflagged sessions, because no such labeled set exists yet. Treat
//!   `rot_score` as a rough prioritization signal ("look at this session before that one"),
//!   never as a calibrated probability.
//! - **Three segments is coarse.** A session that rots and then partially recovers within the
//!   same Late third looks identical to one that never rotted, if the recovery lands before the
//!   segment boundary. More/adaptive segments would sharpen this at the cost of needing more
//!   events to stay statistically meaningful — not worth it until there is real session data to
//!   tune against.
//! - **Small samples inside a segment are noisy.** `MIN_EVENTS_FOR_SIGNAL` guarantees roughly 3
//!   events per segment on average, not per actual segment — a pathological token distribution
//!   (nearly all tokens inside one giant early event) can still leave a segment with only 1-2
//!   events after boundary clamping, where a single error looks like a 100% failure rate.
//! - **Recovery-loop slicing is an approximation.** `avg_recovery_steps` assigns each
//!   `RecoverySignal` (detected once over the *full*, untruncated trace, so cross-boundary
//!   recoveries are not lost) to the segment containing its *failure* event. This differs from
//!   the outcome-evidence and churn metrics, which are recomputed independently per segment
//!   slice — safe only because `OutcomeDetector` classifies one event at a time with no
//!   cross-event memory, unlike `RecoveryDetector`.
//! - **The "inferred" half is only as good as `OutcomeDetector`.** It matches on keywords/regex
//!   (e.g. "successfully implemented", `cargo test` output shapes). A verbose late-session
//!   assistant message that happens to contain a completion phrase counts as a self-claim
//!   whether or not the underlying claim is true.
//! - **Not validated against real sessions.** Everything here is fixture-tested with
//!   deliberately engineered trends, not run against a corpus of real agent sessions with
//!   known-good/known-bad labels. Confidence in the *mechanism* (compare a session to itself,
//!   require the end to be the worst point, require multiple signals to agree) is reasonable;
//!   confidence in the *exact numbers* is low. Say so plainly wherever this signal is surfaced.

use std::collections::HashMap;

use agentworth_outcomes::{outcome_rank, OutcomeDetector, RecoveryDetector};
use agentworth_schema::{
    AgentWorthTrace, EventPayload, FileActionType, NormalizedEvent, OutcomeKind,
};
use serde::{Deserialize, Serialize};

/// Minimum event count before a trend assessment is attempted at all. Below this, splitting
/// into three segments would leave each with too few events for a rate to mean anything (a
/// single error in a 2-event segment is a "100% failure rate"). This gates on sample size only
/// — it does NOT gate on token count or session duration, since a short session that degrades
/// quickly is exactly the case this detector should still be able to catch.
const MIN_EVENTS_FOR_SIGNAL: usize = 9;

/// A component signal must move by at least this much (on its own 0.0..=1.0 scale) between the
/// Early and Late segments to count as "triggered".
const TRIGGER_EPS: f64 = 0.15;

/// `flagged` requires at least this many of the five components to have individually triggered,
/// regardless of `rot_score`. Prevents one noisy metric from flagging a session alone.
const MIN_TRIGGERED_SIGNALS: usize = 2;

/// `rot_score` must clear this to flag. Chosen so that two components moving substantially
/// (roughly 0.5 on their own scale) clears it, but two components barely past `TRIGGER_EPS` do
/// not. See the module doc comment: this is a judgment call anchored on fixture scenarios, not
/// on labeled data.
const ROT_SCORE_THRESHOLD: f64 = 0.30;

/// Half of `rot_score` comes from what fraction of the five components triggered.
const TRIGGER_COUNT_WEIGHT: f64 = 0.5;
/// Half comes from the weighted magnitude of how far each component moved.
const MAGNITUDE_WEIGHT: f64 = 0.5;

/// `steps_to_recover` has no natural upper bound; this many extra steps between Early and Late
/// is treated as a "fully triggered" recovery-friction signal. Heuristic, not measured.
const RECOVERY_FRICTION_SCALE: f64 = 8.0;

const FAILURE_WEIGHT: f64 = 0.25;
const CHURN_WEIGHT: f64 = 0.25;
const SELF_CLAIM_WEIGHT: f64 = 0.15;
const VERIFICATION_WEIGHT: f64 = 0.15;
const RECOVERY_WEIGHT: f64 = 0.20;

/// Which third of the session (by cumulative token growth, or by event count as a fallback) a
/// [`ContextRotSegment`] covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentLabel {
    Early,
    Middle,
    Late,
}

/// Quality snapshot for one third of a session's timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextRotSegment {
    pub label: SegmentLabel,
    pub event_count: usize,
    /// Cumulative session token count (sum of every `ModelInvocation`'s token usage from the
    /// start of the trace) at the last event of this segment. Zero for every segment when the
    /// trace has no `ModelInvocation` events at all — in that case segmentation fell back to
    /// equal event-count thirds instead of token-weighted thirds.
    pub cumulative_tokens_at_end: u64,
    /// Fraction of events in this segment that are a structural failure: an `Error` event, an
    /// `is_error` tool result, or a nonzero shell exit code. Field-based, not text-classified.
    pub failure_rate: f64,
    /// Fraction of file Edit/Write actions in this segment that re-touch a path already edited
    /// earlier in the *same* segment — edits beyond the first touch of each file. Zero when the
    /// segment has no file edits at all.
    pub file_churn_rate: f64,
    /// Fraction of `OutcomeDetector` evidence in this segment that is a bare `DoneClaimed`
    /// self-assertion rather than an observed artifact/test/commit/CI signal. Zero when the
    /// segment has no outcome evidence at all.
    pub self_claim_ratio: f64,
    /// Highest `outcome_rank` reached by any evidence in this segment (0 = no evidence at all,
    /// 5 = `CiOrDeploymentVerified`).
    pub max_verification_rank: u8,
    /// Average `steps_to_recover` (see `agentworth_outcomes::RecoveryDetector`) across
    /// failure-recovery loops whose *failure* event falls in this segment. Zero when this
    /// segment has no detected recoveries — which is not the same as "nothing ever failed here"
    /// (see the module doc comment's limitations on recovery-loop slicing).
    pub avg_recovery_steps: f64,
}

/// Result of running the context-rot check over one session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextRotSignal {
    /// Empty when `insufficient_data` is true; otherwise exactly 3 entries, ordered
    /// Early, Middle, Late.
    pub segments: Vec<ContextRotSegment>,
    /// 0.0 (no evidence the session degraded as it grew) to 1.0 (strong evidence across every
    /// component). Read this as a rough prioritization score, not a calibrated probability —
    /// see the module doc comment.
    pub rot_score: f64,
    /// True when `rot_score` clears [`ROT_SCORE_THRESHOLD`] AND at least
    /// [`MIN_TRIGGERED_SIGNALS`] independent component signals individually cleared their own
    /// trigger threshold.
    pub flagged: bool,
    /// True when the trace had too few events ([`MIN_EVENTS_FOR_SIGNAL`]) to split into three
    /// segments with any statistical meaning. `rot_score` is 0.0 and `flagged` is false in this
    /// case — this means "not assessed", not "assessed as healthy".
    pub insufficient_data: bool,
    /// Human-readable names of the component signals that individually triggered, or a single
    /// explanatory string when none did, or when data was insufficient.
    pub reasons: Vec<String>,
}

/// Detector that compares a session's Early/Middle/Late segments to flag likely context rot.
/// See the module doc comment for the full design rationale and known limitations.
#[derive(Debug, Default, Clone)]
pub struct ContextRotDetector;

impl ContextRotDetector {
    pub fn new() -> Self {
        Self
    }

    /// Run the context-rot check over a full trace.
    pub fn detect(&self, trace: &AgentWorthTrace) -> ContextRotSignal {
        self.detect_from_events(&trace.events)
    }

    /// Run the context-rot check over a raw event slice.
    pub fn detect_from_events(&self, events: &[NormalizedEvent]) -> ContextRotSignal {
        if events.len() < MIN_EVENTS_FOR_SIGNAL {
            return ContextRotSignal {
                segments: Vec::new(),
                rot_score: 0.0,
                flagged: false,
                insufficient_data: true,
                reasons: vec![format!(
                    "trace has only {} event(s); need at least {} to split into three comparable segments",
                    events.len(),
                    MIN_EVENTS_FOR_SIGNAL
                )],
            };
        }

        let (early_end, mid_end, cumulative) = segment_boundaries(events);

        let mut early = compute_segment(&events[0..early_end], SegmentLabel::Early);
        let mut middle = compute_segment(&events[early_end..mid_end], SegmentLabel::Middle);
        let mut late = compute_segment(&events[mid_end..], SegmentLabel::Late);

        early.cumulative_tokens_at_end = cumulative[early_end - 1];
        middle.cumulative_tokens_at_end = cumulative[mid_end - 1];
        late.cumulative_tokens_at_end = cumulative[events.len() - 1];

        attach_recovery_friction(
            events,
            early_end,
            mid_end,
            &mut early,
            &mut middle,
            &mut late,
        );

        let d_failure = worse_at_end(early.failure_rate, middle.failure_rate, late.failure_rate);
        let d_churn = worse_at_end(
            early.file_churn_rate,
            middle.file_churn_rate,
            late.file_churn_rate,
        );
        let d_self_claim = worse_at_end(
            early.self_claim_ratio,
            middle.self_claim_ratio,
            late.self_claim_ratio,
        );
        let d_verification = (declining_at_end(
            early.max_verification_rank as f64,
            middle.max_verification_rank as f64,
            late.max_verification_rank as f64,
        ) / 4.0)
            .min(1.0);
        let d_recovery = (worse_at_end(
            early.avg_recovery_steps,
            middle.avg_recovery_steps,
            late.avg_recovery_steps,
        ) / RECOVERY_FRICTION_SCALE)
            .min(1.0);

        let components: [(&str, f64, f64); 5] = [
            ("rising failure/error rate", d_failure, FAILURE_WEIGHT),
            (
                "rising repeat-edit churn on the same files",
                d_churn,
                CHURN_WEIGHT,
            ),
            (
                "rising self-claimed-done without new verification",
                d_self_claim,
                SELF_CLAIM_WEIGHT,
            ),
            (
                "declining verification-ladder rung reached",
                d_verification,
                VERIFICATION_WEIGHT,
            ),
            ("slower failure-recovery loops", d_recovery, RECOVERY_WEIGHT),
        ];

        let triggered: Vec<String> = components
            .iter()
            .filter(|(_, value, _)| *value >= TRIGGER_EPS)
            .map(|(name, _, _)| name.to_string())
            .collect();

        let magnitude_sum: f64 = components
            .iter()
            .map(|(_, value, weight)| value * weight)
            .sum();
        let trigger_fraction = triggered.len() as f64 / components.len() as f64;
        let rot_score = (TRIGGER_COUNT_WEIGHT * trigger_fraction
            + MAGNITUDE_WEIGHT * magnitude_sum)
            .clamp(0.0, 1.0);

        let flagged = rot_score >= ROT_SCORE_THRESHOLD && triggered.len() >= MIN_TRIGGERED_SIGNALS;

        let reasons = if triggered.is_empty() {
            vec!["no within-session degradation trend detected".to_string()]
        } else {
            triggered
        };

        ContextRotSignal {
            segments: vec![early, middle, late],
            rot_score,
            flagged,
            insufficient_data: false,
            reasons,
        }
    }
}

/// Convenience wrapper equivalent to `ContextRotDetector::new().detect(trace)`.
pub fn detect_context_rot(trace: &AgentWorthTrace) -> ContextRotSignal {
    ContextRotDetector::new().detect(trace)
}

/// Determine `(early_end, mid_end)` half-open slice boundaries splitting `events` into three
/// non-empty, chronologically-ordered segments, preferring cumulative `ModelInvocation` token
/// count as the position axis and falling back to plain event-count thirds when the trace has
/// no token-bearing events. Also returns the per-event running cumulative token total (same
/// length as `events`) so callers do not need to recompute it. Boundaries are clamped so all
/// three segments are always non-empty, however uneven the token distribution.
fn segment_boundaries(events: &[NormalizedEvent]) -> (usize, usize, Vec<u64>) {
    let n = events.len();
    debug_assert!(n >= 3, "caller must gate on MIN_EVENTS_FOR_SIGNAL");

    let mut cumulative = Vec::with_capacity(n);
    let mut running: u64 = 0;
    for event in events {
        if let EventPayload::ModelInvocation { token_usage, .. } = &event.payload {
            running = running.saturating_add(token_usage.total());
        }
        cumulative.push(running);
    }
    let total_tokens = running;

    let (raw_early, raw_mid) = if total_tokens > 0 {
        let t1 = total_tokens / 3;
        let t2 = (total_tokens * 2) / 3;
        // `position` finds the first event whose cumulative total reaches the threshold; that
        // event becomes the first event of the *next* segment, hence the `+ 1` boundary.
        let idx1 = cumulative.iter().position(|&t| t >= t1).unwrap_or(n / 3);
        let idx2 = cumulative
            .iter()
            .position(|&t| t >= t2)
            .unwrap_or(2 * n / 3);
        (idx1 + 1, idx2 + 1)
    } else {
        (n / 3, 2 * n / 3)
    };

    let early_end = raw_early.clamp(1, n - 2);
    let mid_end = raw_mid.clamp(early_end + 1, n - 1);

    (early_end, mid_end, cumulative)
}

fn compute_segment(events: &[NormalizedEvent], label: SegmentLabel) -> ContextRotSegment {
    let event_count = events.len();

    let failure_events = events
        .iter()
        .filter(|event| is_structural_failure(&event.payload))
        .count();
    let failure_rate = failure_events as f64 / event_count as f64;

    let mut edit_counts: HashMap<&str, usize> = HashMap::new();
    let mut total_edits = 0usize;
    for event in events {
        if let EventPayload::FileAction { path, action, .. } = &event.payload {
            if matches!(action, FileActionType::Edit | FileActionType::Write) {
                total_edits += 1;
                *edit_counts.entry(path.as_str()).or_insert(0) += 1;
            }
        }
    }
    let churned_edits: usize = edit_counts
        .values()
        .map(|&count| count.saturating_sub(1))
        .sum();
    let file_churn_rate = if total_edits > 0 {
        churned_edits as f64 / total_edits as f64
    } else {
        0.0
    };

    // Stateless per-event classification, so it is safe to run on an arbitrary sub-slice: unlike
    // RecoveryDetector, OutcomeDetector never looks at neighboring events.
    let outcomes = OutcomeDetector::new().detect_from_events(events);
    let mut self_claims = 0usize;
    let mut max_rank = 0u8;
    for evidence in &outcomes {
        if evidence.kind == OutcomeKind::DoneClaimed {
            self_claims += 1;
        }
        max_rank = max_rank.max(outcome_rank(evidence.kind));
    }
    let self_claim_ratio = if outcomes.is_empty() {
        0.0
    } else {
        self_claims as f64 / outcomes.len() as f64
    };

    ContextRotSegment {
        label,
        event_count,
        cumulative_tokens_at_end: 0,
        failure_rate,
        file_churn_rate,
        self_claim_ratio,
        max_verification_rank: max_rank,
        avg_recovery_steps: 0.0,
    }
}

/// Runs `RecoveryDetector` once over the *full* event slice (so recoveries spanning a segment
/// boundary are still detected), then attributes each recovery's `steps_to_recover` to the
/// segment containing its failure event, by looking up the failure's original array index via
/// its `sequence` field.
fn attach_recovery_friction(
    events: &[NormalizedEvent],
    early_end: usize,
    mid_end: usize,
    early: &mut ContextRotSegment,
    middle: &mut ContextRotSegment,
    late: &mut ContextRotSegment,
) {
    let sequence_to_index: HashMap<u64, usize> = events
        .iter()
        .enumerate()
        .map(|(index, event)| (event.sequence, index))
        .collect();

    let recoveries = RecoveryDetector::new().detect_recoveries_from_events(events);

    let mut steps_sum = [0u64; 3];
    let mut steps_count = [0u64; 3];
    for recovery in &recoveries {
        if let Some(&index) = sequence_to_index.get(&recovery.failure_sequence) {
            let segment = if index < early_end {
                0
            } else if index < mid_end {
                1
            } else {
                2
            };
            steps_sum[segment] += recovery.steps_to_recover as u64;
            steps_count[segment] += 1;
        }
    }

    early.avg_recovery_steps = avg_or_zero(steps_sum[0], steps_count[0]);
    middle.avg_recovery_steps = avg_or_zero(steps_sum[1], steps_count[1]);
    late.avg_recovery_steps = avg_or_zero(steps_sum[2], steps_count[2]);
}

fn avg_or_zero(sum: u64, count: u64) -> f64 {
    if count == 0 {
        0.0
    } else {
        sum as f64 / count as f64
    }
}

fn is_structural_failure(payload: &EventPayload) -> bool {
    match payload {
        EventPayload::Error { .. } => true,
        EventPayload::ToolResult(result) => result.is_error,
        EventPayload::ShellCommand(command) => command.exit_code.is_some_and(|code| code != 0),
        _ => false,
    }
}

/// Returns `late - early` (floored at 0.0) only when `late` is the worst (highest) of the three
/// points, i.e. the segment ends worse than it started AND worse than or equal to its own
/// middle. A dip that recovers by the end (`early -> bad middle -> back to early`) returns 0.0:
/// that is a session that had a rough patch and course-corrected, not one that ended in a
/// degraded state, so it does not count as unresolved context rot.
fn worse_at_end(early: f64, middle: f64, late: f64) -> f64 {
    if late >= early && late >= middle {
        (late - early).max(0.0)
    } else {
        0.0
    }
}

/// Mirror of [`worse_at_end`] for metrics where a LOWER value is worse (e.g. the highest
/// verification rung reached). Returns `early - late` only when `late` is the worst (lowest).
fn declining_at_end(early: f64, middle: f64, late: f64) -> f64 {
    if late <= early && late <= middle {
        (early - late).max(0.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{Provenance, ShellCommand, TokenUsage, ToolCall, ToolResult};
    use chrono::{Duration, Utc};

    fn push(
        events: &mut Vec<NormalizedEvent>,
        seq: u64,
        start: chrono::DateTime<Utc>,
        payload: EventPayload,
    ) {
        events.push(NormalizedEvent::new(
            seq,
            start + Duration::seconds(seq as i64),
            payload,
        ));
    }

    fn make_trace(events: Vec<NormalizedEvent>) -> AgentWorthTrace {
        let prov = Provenance::new("/tmp/test.jsonl", "claude_code", 100, 1000, "fp123");
        let mut trace = AgentWorthTrace::new("sess-context-rot", "claude_code", prov, Utc::now());
        trace.events = events;
        trace
    }

    #[test]
    fn test_worse_at_end_requires_late_to_be_the_worst_point() {
        assert_eq!(worse_at_end(0.1, 0.2, 0.5), 0.4); // monotonic rise -> full credit
        assert_eq!(worse_at_end(0.1, 0.9, 0.2), 0.0); // spike in the middle, recovered -> not counted
        assert_eq!(worse_at_end(0.2, 0.2, 0.2), 0.0); // flat -> no degradation
        assert_eq!(worse_at_end(0.5, 0.1, 0.5), 0.0); // back to baseline by the end -> delta is 0
    }

    #[test]
    fn test_declining_at_end_mirrors_worse_at_end() {
        assert_eq!(declining_at_end(4.0, 3.0, 1.0), 3.0); // monotonic decline -> full credit
        assert_eq!(declining_at_end(4.0, 1.0, 3.0), 0.0); // dipped in the middle, recovered -> not counted
        assert_eq!(declining_at_end(4.0, 4.0, 4.0), 0.0); // flat -> no decline
    }

    /// Core positive case: a session that starts clean (verified tests + commit, no churn) and
    /// ends messy (repeated failures, repeated edits to the same file, only self-claimed "done"
    /// with no fresh verification) must be flagged. No `ModelInvocation` events are used here on
    /// purpose, forcing the event-count fallback segmentation so the expected 9/9/9 split is
    /// exact and hand-verifiable.
    #[test]
    fn test_degrading_session_is_flagged() {
        let start = Utc::now();
        let mut events = Vec::new();
        let mut seq = 1u64;
        macro_rules! push_ev {
            ($payload:expr) => {
                push(&mut events, seq, start, $payload);
                seq += 1;
            };
        }

        // Early (9 events): clean. Verified test pass + commit, two distinct files touched once.
        push_ev!(EventPayload::UserMessage {
            content: "Fix the bug".to_string()
        });
        push_ev!(EventPayload::ToolCall(ToolCall {
            id: Some("t1".to_string()),
            name: "edit_file".to_string(),
            arguments: serde_json::json!({"path": "src/a.rs"}),
        }));
        push_ev!(EventPayload::FileAction {
            path: "src/a.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ fix".to_string()),
            lines_changed: Some(1),
        });
        push_ev!(EventPayload::ShellCommand(ShellCommand {
            command: "cargo test".to_string(),
            cwd: Some("/repo".to_string()),
            exit_code: Some(0),
            output: Some("test result: ok. 3 passed; 0 failed".to_string()),
        }));
        push_ev!(EventPayload::ShellCommand(ShellCommand {
            command: "git commit -m 'fix a'".to_string(),
            cwd: Some("/repo".to_string()),
            exit_code: Some(0),
            output: Some("[main abc123] fix a\n 1 file changed".to_string()),
        }));
        push_ev!(EventPayload::AssistantMessage {
            content: "Fixed the bug in a.rs, tests pass.".to_string(),
            thinking: None,
        });
        push_ev!(EventPayload::FileAction {
            path: "src/b.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ tidy".to_string()),
            lines_changed: Some(1),
        });
        push_ev!(EventPayload::ShellCommand(ShellCommand {
            command: "cargo test".to_string(),
            cwd: Some("/repo".to_string()),
            exit_code: Some(0),
            output: Some("test result: ok. 4 passed; 0 failed".to_string()),
        }));
        push_ev!(EventPayload::AssistantMessage {
            content: "Also cleaned up b.rs while I was there.".to_string(),
            thinking: None,
        });

        // Middle (9 events): one failure appears, otherwise still reasonably healthy.
        push_ev!(EventPayload::UserMessage {
            content: "Now fix the second bug".to_string()
        });
        push_ev!(EventPayload::ShellCommand(ShellCommand {
            command: "cargo test".to_string(),
            cwd: None,
            exit_code: Some(101),
            output: Some("error[E0425]: cannot find value `x`\ntest result: FAILED".to_string()),
        }));
        push_ev!(EventPayload::ToolCall(ToolCall {
            id: Some("t2".to_string()),
            name: "edit_file".to_string(),
            arguments: serde_json::json!({"path": "src/b.rs"}),
        }));
        push_ev!(EventPayload::FileAction {
            path: "src/b.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ let x = 42;".to_string()),
            lines_changed: Some(1),
        });
        push_ev!(EventPayload::ShellCommand(ShellCommand {
            command: "cargo test".to_string(),
            cwd: None,
            exit_code: Some(0),
            output: Some("test result: ok. 1 passed; 0 failed".to_string()),
        }));
        push_ev!(EventPayload::AssistantMessage {
            content: "Fixed it, tests pass now.".to_string(),
            thinking: None,
        });
        push_ev!(EventPayload::ToolCall(ToolCall {
            id: Some("t3".to_string()),
            name: "edit_file".to_string(),
            arguments: serde_json::json!({"path": "src/c.rs"}),
        }));
        push_ev!(EventPayload::FileAction {
            path: "src/c.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ refactor".to_string()),
            lines_changed: Some(1),
        });
        push_ev!(EventPayload::AssistantMessage {
            content: "Small refactor in c.rs.".to_string(),
            thinking: None,
        });

        // Late (9 events): messy. Repeated failures, the same file (d.rs) edited three times,
        // and only bare self-claims with no fresh test/commit evidence.
        push_ev!(EventPayload::ShellCommand(ShellCommand {
            command: "cargo test".to_string(),
            cwd: None,
            exit_code: Some(101),
            output: Some("error[E0308]: mismatched types\ntest result: FAILED".to_string()),
        }));
        push_ev!(EventPayload::ToolCall(ToolCall {
            id: Some("t4".to_string()),
            name: "edit_file".to_string(),
            arguments: serde_json::json!({"path": "src/d.rs"}),
        }));
        push_ev!(EventPayload::FileAction {
            path: "src/d.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ attempt 1".to_string()),
            lines_changed: Some(2),
        });
        push_ev!(EventPayload::ShellCommand(ShellCommand {
            command: "cargo test".to_string(),
            cwd: None,
            exit_code: Some(101),
            output: Some("still failing\ntest result: FAILED. 1 failed".to_string()),
        }));
        push_ev!(EventPayload::FileAction {
            path: "src/d.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ attempt 2".to_string()),
            lines_changed: Some(2),
        });
        push_ev!(EventPayload::ToolResult(ToolResult {
            call_id: Some("t5".to_string()),
            name: Some("Bash".to_string()),
            output: serde_json::json!("permission denied"),
            is_error: true,
        }));
        push_ev!(EventPayload::FileAction {
            path: "src/d.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ attempt 3".to_string()),
            lines_changed: Some(2),
        });
        push_ev!(EventPayload::AssistantMessage {
            content: "I have completed all the requested tasks!".to_string(),
            thinking: None,
        });
        push_ev!(EventPayload::AssistantMessage {
            content: "Everything is in place.".to_string(),
            thinking: None,
        });

        let _ = seq; // last increment is never read again; silences unused_assignments
        assert_eq!(events.len(), 27);
        let trace = make_trace(events);

        let signal = ContextRotDetector::new().detect(&trace);

        assert!(!signal.insufficient_data);
        assert_eq!(signal.segments.len(), 3);

        let (early, middle, late) = (
            &signal.segments[0],
            &signal.segments[1],
            &signal.segments[2],
        );
        assert_eq!(early.event_count, 9);
        assert_eq!(middle.event_count, 9);
        assert_eq!(late.event_count, 9);

        // Structural signals: strictly worse by the end.
        assert_eq!(early.failure_rate, 0.0);
        assert!(late.failure_rate > middle.failure_rate);
        assert!(late.failure_rate > 0.3);

        assert_eq!(early.file_churn_rate, 0.0);
        assert_eq!(middle.file_churn_rate, 0.0);
        assert!(
            late.file_churn_rate > 0.6,
            "d.rs is re-edited twice out of three edits"
        );

        // Inferred signals: self-claims rise, verification rung falls.
        assert_eq!(early.self_claim_ratio, 0.0);
        assert!(late.self_claim_ratio > 0.0);
        assert_eq!(early.max_verification_rank, 4); // CommitObserved
        assert!(late.max_verification_rank < early.max_verification_rank);

        assert!(
            signal.flagged,
            "expected a degrading session to be flagged: rot_score={}, reasons={:?}, segments={:?}",
            signal.rot_score, signal.reasons, signal.segments
        );
        assert!(signal.rot_score >= ROT_SCORE_THRESHOLD);
        assert!(signal
            .reasons
            .contains(&"rising failure/error rate".to_string()));
        assert!(signal
            .reasons
            .contains(&"rising repeat-edit churn on the same files".to_string()));
    }

    /// Negative case, and the direct test of "a long session that stays sharp throughout is not
    /// context rot": 160 events and well over 100k cumulative tokens, but every block of 4
    /// events is structurally identical (distinct file per block, test always passes, no
    /// errors). There is no within-session trend for the detector to find.
    #[test]
    fn test_healthy_long_session_not_flagged() {
        let start = Utc::now();
        let mut events = Vec::new();
        let mut seq = 1u64;

        for i in 0..40 {
            push(
                &mut events,
                seq,
                start,
                EventPayload::ModelInvocation {
                    model: "claude-sonnet-5".to_string(),
                    token_usage: TokenUsage::new(2000, 400, 100, 50),
                    cost_usd: None,
                    latency_ms: Some(900),
                },
            );
            seq += 1;
            push(
                &mut events,
                seq,
                start,
                EventPayload::ToolCall(ToolCall {
                    id: Some(format!("call_{i}")),
                    name: "edit_file".to_string(),
                    arguments: serde_json::json!({"path": format!("src/file_{i}.rs")}),
                }),
            );
            seq += 1;
            push(
                &mut events,
                seq,
                start,
                EventPayload::FileAction {
                    path: format!("src/file_{i}.rs"),
                    action: FileActionType::Edit,
                    diff: Some("+ improvement".to_string()),
                    lines_changed: Some(2),
                },
            );
            seq += 1;
            push(
                &mut events,
                seq,
                start,
                EventPayload::ShellCommand(ShellCommand {
                    command: "cargo test".to_string(),
                    cwd: Some("/repo".to_string()),
                    exit_code: Some(0),
                    output: Some("test result: ok. 1 passed; 0 failed".to_string()),
                }),
            );
            seq += 1;
        }
        assert_eq!(events.len(), 160);

        let trace = make_trace(events);
        let signal = ContextRotDetector::new().detect(&trace);

        assert!(!signal.insufficient_data);
        assert_eq!(signal.segments.len(), 3);

        // Token growth is large and strictly increasing across segments -- this is a genuinely
        // long, context-heavy session, not a trivial one.
        assert!(signal.segments[2].cumulative_tokens_at_end > 100_000);
        assert!(
            signal.segments[0].cumulative_tokens_at_end
                < signal.segments[2].cumulative_tokens_at_end
        );

        for segment in &signal.segments {
            assert_eq!(segment.failure_rate, 0.0);
            assert_eq!(segment.file_churn_rate, 0.0);
            assert_eq!(segment.self_claim_ratio, 0.0);
        }

        assert!(
            !signal.flagged,
            "a long session that stays sharp throughout must not be flagged: rot_score={}, reasons={:?}",
            signal.rot_score, signal.reasons
        );
        assert!(signal.rot_score < 0.1);
    }

    /// Direct test of the other half of the task's framing: a SHORT session (just above the
    /// minimum sample size) that degrades quickly must still be flagged. Size alone is not the
    /// gate -- the within-session trend is.
    #[test]
    fn test_short_but_quickly_degrading_session_is_flagged() {
        let start = Utc::now();
        let mut events = Vec::new();
        let mut seq = 1u64;
        macro_rules! push_ev {
            ($payload:expr) => {
                push(&mut events, seq, start, $payload);
                seq += 1;
            };
        }

        // Early (4): clean, verified.
        push_ev!(EventPayload::ShellCommand(ShellCommand {
            command: "cargo test".to_string(),
            cwd: None,
            exit_code: Some(0),
            output: Some("test result: ok. 2 passed; 0 failed".to_string()),
        }));
        push_ev!(EventPayload::ShellCommand(ShellCommand {
            command: "git commit -m 'feat: initial'".to_string(),
            cwd: None,
            exit_code: Some(0),
            output: Some("[main aaa111] feat: initial\n 1 file changed".to_string()),
        }));
        push_ev!(EventPayload::FileAction {
            path: "src/x.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ x".to_string()),
            lines_changed: Some(1),
        });
        push_ev!(EventPayload::FileAction {
            path: "src/w.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ w".to_string()),
            lines_changed: Some(1),
        });

        // Middle (4): still clean -- the point is that it falls apart only at the very end.
        push_ev!(EventPayload::ShellCommand(ShellCommand {
            command: "cargo test".to_string(),
            cwd: None,
            exit_code: Some(0),
            output: Some("test result: ok. 3 passed; 0 failed".to_string()),
        }));
        push_ev!(EventPayload::FileAction {
            path: "src/y.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ y".to_string()),
            lines_changed: Some(1),
        });
        push_ev!(EventPayload::ShellCommand(ShellCommand {
            command: "cargo test".to_string(),
            cwd: None,
            exit_code: Some(0),
            output: Some("test result: ok. 4 passed; 0 failed".to_string()),
        }));
        push_ev!(EventPayload::FileAction {
            path: "src/v.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ v".to_string()),
            lines_changed: Some(1),
        });

        // Late (5, since 13 events falls back to a 4/4/5 event-count split): two failures and
        // repeated churn on the same file, closed out with a bare self-claim.
        push_ev!(EventPayload::ShellCommand(ShellCommand {
            command: "cargo test".to_string(),
            cwd: None,
            exit_code: Some(101),
            output: Some("test result: FAILED. 1 failed".to_string()),
        }));
        push_ev!(EventPayload::ShellCommand(ShellCommand {
            command: "cargo test".to_string(),
            cwd: None,
            exit_code: Some(101),
            output: Some("still failing\ntest result: FAILED. 1 failed".to_string()),
        }));
        push_ev!(EventPayload::FileAction {
            path: "src/z.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ attempt 1".to_string()),
            lines_changed: Some(1),
        });
        push_ev!(EventPayload::FileAction {
            path: "src/z.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ attempt 2".to_string()),
            lines_changed: Some(1),
        });
        push_ev!(EventPayload::AssistantMessage {
            content: "I have completed all the requested tasks!".to_string(),
            thinking: None,
        });

        let _ = seq; // last increment is never read again; silences unused_assignments
        assert_eq!(events.len(), 13);
        let trace = make_trace(events);
        let signal = ContextRotDetector::new().detect(&trace);

        assert!(!signal.insufficient_data);
        let (early, middle, late) = (
            &signal.segments[0],
            &signal.segments[1],
            &signal.segments[2],
        );
        assert_eq!(early.event_count, 4);
        assert_eq!(middle.event_count, 4);
        assert_eq!(late.event_count, 5);

        assert!(late.failure_rate > 0.0 && early.failure_rate == 0.0 && middle.failure_rate == 0.0);
        assert!(late.file_churn_rate > 0.0);

        assert!(
            signal.flagged,
            "a short session degrading quickly at the end must still be flagged: rot_score={}, reasons={:?}",
            signal.rot_score, signal.reasons
        );
    }

    /// A session that has a rough patch in the middle but is back to its Early-segment quality
    /// by the end must NOT be flagged: the degradation did not persist to the point the session
    /// stopped, which is the whole point of `worse_at_end` / `declining_at_end`.
    #[test]
    fn test_dip_that_recovers_by_end_is_not_flagged() {
        let start = Utc::now();
        let mut events = Vec::new();
        let mut seq = 1u64;
        macro_rules! push_ev {
            ($payload:expr) => {
                push(&mut events, seq, start, $payload);
                seq += 1;
            };
        }

        // Early (4): clean.
        push_ev!(EventPayload::ShellCommand(ShellCommand {
            command: "cargo test".to_string(),
            cwd: None,
            exit_code: Some(0),
            output: Some("test result: ok. 2 passed; 0 failed".to_string()),
        }));
        push_ev!(EventPayload::ShellCommand(ShellCommand {
            command: "git commit -m 'feat: a'".to_string(),
            cwd: None,
            exit_code: Some(0),
            output: Some("[main bbb222] feat: a\n 1 file changed".to_string()),
        }));
        push_ev!(EventPayload::FileAction {
            path: "src/a.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ a".to_string()),
            lines_changed: Some(1),
        });
        push_ev!(EventPayload::FileAction {
            path: "src/b.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ b".to_string()),
            lines_changed: Some(1),
        });

        // Middle (4): a rough patch -- failure, churn, and a bare self-claim.
        push_ev!(EventPayload::ShellCommand(ShellCommand {
            command: "cargo test".to_string(),
            cwd: None,
            exit_code: Some(101),
            output: Some("test result: FAILED. 1 failed".to_string()),
        }));
        push_ev!(EventPayload::FileAction {
            path: "src/c.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ attempt 1".to_string()),
            lines_changed: Some(1),
        });
        push_ev!(EventPayload::FileAction {
            path: "src/c.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ attempt 2".to_string()),
            lines_changed: Some(1),
        });
        push_ev!(EventPayload::AssistantMessage {
            content: "I have completed all the requested tasks!".to_string(),
            thinking: None,
        });

        // Late (4): fully recovered -- verified test pass and commit again, distinct files.
        push_ev!(EventPayload::ShellCommand(ShellCommand {
            command: "cargo test".to_string(),
            cwd: None,
            exit_code: Some(0),
            output: Some("test result: ok. 5 passed; 0 failed".to_string()),
        }));
        push_ev!(EventPayload::ShellCommand(ShellCommand {
            command: "git commit -m 'fix: c'".to_string(),
            cwd: None,
            exit_code: Some(0),
            output: Some("[main ccc333] fix: c\n 1 file changed".to_string()),
        }));
        push_ev!(EventPayload::FileAction {
            path: "src/d.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ d".to_string()),
            lines_changed: Some(1),
        });
        push_ev!(EventPayload::FileAction {
            path: "src/e.rs".to_string(),
            action: FileActionType::Edit,
            diff: Some("+ e".to_string()),
            lines_changed: Some(1),
        });

        let _ = seq; // last increment is never read again; silences unused_assignments
        assert_eq!(events.len(), 12);
        let trace = make_trace(events);
        let signal = ContextRotDetector::new().detect(&trace);

        let (early, middle, late) = (
            &signal.segments[0],
            &signal.segments[1],
            &signal.segments[2],
        );
        // Sanity: the middle segment really was worse than both neighbors.
        assert!(middle.failure_rate > early.failure_rate);
        assert!(middle.file_churn_rate > late.file_churn_rate);
        // Late is back to baseline.
        assert_eq!(late.failure_rate, 0.0);
        assert_eq!(late.file_churn_rate, 0.0);
        assert_eq!(late.max_verification_rank, early.max_verification_rank);

        assert!(
            !signal.flagged,
            "a mid-session dip that recovers by the end must not be flagged: rot_score={}, reasons={:?}",
            signal.rot_score, signal.reasons
        );
    }

    #[test]
    fn test_too_few_events_is_insufficient_data() {
        let start = Utc::now();
        let mut events = Vec::new();
        push(
            &mut events,
            1,
            start,
            EventPayload::UserMessage {
                content: "hi".to_string(),
            },
        );
        push(
            &mut events,
            2,
            start,
            EventPayload::AssistantMessage {
                content: "hello".to_string(),
                thinking: None,
            },
        );
        let trace = make_trace(events);

        let signal = ContextRotDetector::new().detect(&trace);

        assert!(signal.insufficient_data);
        assert!(!signal.flagged);
        assert_eq!(signal.rot_score, 0.0);
        assert!(signal.segments.is_empty());
        assert_eq!(signal.reasons.len(), 1);
    }

    /// Segmentation must never panic or collapse a segment to zero events even when nearly all
    /// cumulative token mass lands on a single early event.
    #[test]
    fn test_token_weighted_segmentation_handles_uneven_distribution() {
        let start = Utc::now();
        let mut events = Vec::new();
        push(
            &mut events,
            1,
            start,
            EventPayload::ModelInvocation {
                model: "m".to_string(),
                token_usage: TokenUsage::new(50_000, 0, 0, 0),
                cost_usd: None,
                latency_ms: None,
            },
        );
        for seq in 2..=15u64 {
            push(
                &mut events,
                seq,
                start,
                EventPayload::UserMessage {
                    content: format!("message {seq}"),
                },
            );
        }
        let trace = make_trace(events.clone());

        let signal = ContextRotDetector::new().detect(&trace);

        assert!(!signal.insufficient_data);
        assert_eq!(signal.segments.len(), 3);
        assert!(signal
            .segments
            .iter()
            .all(|segment| segment.event_count > 0));
        let total: usize = signal
            .segments
            .iter()
            .map(|segment| segment.event_count)
            .sum();
        assert_eq!(total, events.len());
    }

    #[test]
    fn test_context_rot_signal_is_serializable() {
        let signal = ContextRotDetector::new().detect_from_events(&[]);
        let json = serde_json::to_string(&signal).expect("serialize");
        let round_tripped: ContextRotSignal = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(signal, round_tripped);
    }
}
