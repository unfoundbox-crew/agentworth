//! What compaction dropped: decision-shaped sentences that went into a round and did not come
//! out of the summary.
//!
//! `docs/specs/compaction-diff.md` measured this on one real session: 402 decision-shaped
//! sentences went into eight compaction rounds and 28 came out. Reasons survive at 1.7%, which
//! is the shape that makes a session re-litigate a settled question -- it kept the answer's
//! shadow and lost the argument.
//!
//! **No model, on purpose.** The output is fed to an agent that cannot verify it. A model
//! paraphrasing the dropped span would make this a second summariser, which is the lossy step
//! the whole thing exists to undo, and the receipt would stop pointing at words anyone said.
//! Three regexes return the sentence verbatim with a sequence number, which is a quotable fact.
//! 374 verbatim sentences with false positives in them beats 40 fluent ones nobody can check.
//!
//! Runs on demand from a loaded trace, never at scan time: storing 400 sentences per compacted
//! session would duplicate transcript content into the index, which `AGENTS.md` forbids.

use std::collections::HashSet;
use std::sync::OnceLock;

use agentworth_schema::{
    compact_summary_text, AgentWorthTrace, CompactionRound, EventPayload, NormalizedEvent,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::loose_ends::split_sentences;

/// A choice already made. Verbatim from the spec's table.
const DECISION_PATTERN: &str =
    r"(?i)\b(decided|decide to|chose|choosing|opted for|we'?ll use|going with)\b";

/// An alternative that was considered and dropped.
const REJECTED_PATTERN: &str =
    r"(?i)\b(instead of|rejected|ruled out|won'?t|will not|not going to)\b";

/// The scarcest class, and the reason this tool exists: 174 reasons went into the measured
/// session's eight rounds and three came out.
const REASON_PATTERN: &str = r"(?i)\bbecause\b";

/// Too short to carry a decision. Same floor as the loose-ends detector, and measured in UTF-16
/// units for the same reason: a CJK transcript must get the same window in every process.
const MIN_LEN: usize = 25;

/// The spec's ceiling, and it raises the loose-ends detector's 240 on purpose -- a long
/// decision paragraph is the most valuable thing here. The spec's own open questions ask
/// whether 400 is still too low; it is not raised further until that has a measurement.
const MAX_LEN: usize = 400;

/// How similar a summary sentence has to be to a dropped one before the dropped one counts as
/// having survived. `compaction-diff.md` names no threshold, so this is 0.6 Jaccard over
/// normalised content tokens, chosen and stated here rather than inherited.
///
/// Measured on the spec's own session (452c23fd, eight rounds): the highest overlap between any
/// dropped sentence and any surviving one is **0.29**, in round 6, and five of the eight rounds
/// peak below 0.10. Summaries paraphrase; they do not quote. So on real data this threshold is
/// nowhere near the decision boundary, and moving it anywhere in 0.35..=0.95 changes nothing on
/// that session. It exists to stop a summary that *does* quote a sentence back from having it
/// reported as forgotten -- which is exactly what a manual `/compact` with a verbatim
/// instruction produces.
pub const SURVIVAL_JACCARD_THRESHOLD: f64 = 0.6;

/// How far past a statement to look for evidence it was acted on. Eight events covers the
/// tool call, its result, and a follow-up or two without reaching the next turn's work.
const EVIDENCE_LOOKAHEAD_EVENTS: usize = 8;

/// Words carrying no topical signal, dropped before overlap is measured so two sentences are
/// not judged similar for sharing "the" and "of".
const STOPWORDS: &[&str] = &[
    "the", "a", "an", "and", "or", "of", "to", "in", "is", "are", "was", "were", "be", "been",
    "it", "this", "that", "for", "on", "with", "as", "at", "by",
];

static DECISION: OnceLock<regex::Regex> = OnceLock::new();
static REJECTED: OnceLock<regex::Regex> = OnceLock::new();
static REASON: OnceLock<regex::Regex> = OnceLock::new();
static WORD: OnceLock<regex::Regex> = OnceLock::new();

fn decision_re() -> &'static regex::Regex {
    DECISION.get_or_init(|| regex::Regex::new(DECISION_PATTERN).expect("valid regex"))
}
fn rejected_re() -> &'static regex::Regex {
    REJECTED.get_or_init(|| regex::Regex::new(REJECTED_PATTERN).expect("valid regex"))
}
fn reason_re() -> &'static regex::Regex {
    REASON.get_or_init(|| regex::Regex::new(REASON_PATTERN).expect("valid regex"))
}
fn word_re() -> &'static regex::Regex {
    WORD.get_or_init(|| regex::Regex::new(r"[a-z0-9]+").expect("valid regex"))
}

/// Which of the three patterns a sentence matched. A sentence can match more than one, which is
/// why the spec counts distinct sentences separately from class matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatementClass {
    Decision,
    Rejected,
    Reason,
}

impl StatementClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::Rejected => "rejected",
            Self::Reason => "reason",
        }
    }

    /// Parses a caller-supplied class name. `None` for anything else, so a tool can reject an
    /// unknown filter rather than silently returning everything.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "decision" => Some(Self::Decision),
            "rejected" => Some(Self::Rejected),
            "reason" => Some(Self::Reason),
            _ => None,
        }
    }

    pub const ALL: [StatementClass; 3] = [Self::Decision, Self::Rejected, Self::Reason];
}

/// One decision-shaped sentence that went into a compaction round and did not come out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgottenStatement {
    pub classes: Vec<StatementClass>,
    /// 1-based compaction round that dropped it.
    pub round: u32,
    /// The sentence, verbatim. Paraphrasing it here would defeat the point.
    pub text: String,
    pub event_id: String,
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// What the session did in the next few events. This is what makes a sentence checkable: a
    /// stated decision with a tool call after it was acted on; one with nothing after it is a
    /// claim. Both are returned, labelled, and the caller decides.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub followed_by: Vec<Evidence>,
}

/// One action that followed a statement, with the sequence to go and read it at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// `tool_call:Edit crates/storage/src/lib.rs`, `shell_command:cargo test`, `file_action:...`.
    pub what: String,
    pub sequence: u64,
}

/// One round's before-and-after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundDiff {
    pub round: u32,
    pub start_seq: u64,
    pub end_seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_before: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_tokens: Option<u64>,
    /// Distinct decision-shaped sentences in the dropped span.
    pub dropped_total: usize,
    /// Distinct decision-shaped sentences in the summary that replaced it.
    pub summary_total: usize,
    /// Dropped sentences with no near-match in the summary.
    pub forgotten_total: usize,
}

/// The whole diff for one session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionDiff {
    pub rounds: Vec<RoundDiff>,
    /// Every forgotten statement, newest first.
    pub forgotten: Vec<ForgottenStatement>,
    pub dropped_total: usize,
    pub survived_in_summary: usize,
}

impl CompactionDiff {
    pub fn forgotten_total(&self) -> usize {
        self.rounds.iter().map(|r| r.forgotten_total).sum()
    }
}

/// A sentence and where it came from, before the survival test.
struct Candidate {
    classes: Vec<StatementClass>,
    text: String,
    event_id: String,
    sequence: u64,
    timestamp: DateTime<Utc>,
    model: Option<String>,
}

fn classify(sentence: &str) -> Vec<StatementClass> {
    let mut out = Vec::new();
    if decision_re().is_match(sentence) {
        out.push(StatementClass::Decision);
    }
    if rejected_re().is_match(sentence) {
        out.push(StatementClass::Rejected);
    }
    if reason_re().is_match(sentence) {
        out.push(StatementClass::Reason);
    }
    out
}

/// Content tokens of a sentence, lowercased and stripped of stopwords.
fn content_tokens(s: &str) -> HashSet<String> {
    let lower = s.to_lowercase();
    word_re()
        .find_iter(&lower)
        .map(|m| m.as_str().to_string())
        .filter(|w| !STOPWORDS.contains(&w.as_str()))
        .collect()
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let intersection = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    intersection / union
}

/// Every decision-shaped sentence in a block of assistant text, in order.
fn decision_shaped(text: &str) -> Vec<(String, Vec<StatementClass>)> {
    let mut out = Vec::new();
    for sentence in split_sentences(text) {
        let len = sentence.encode_utf16().count();
        if !(MIN_LEN..=MAX_LEN).contains(&len) {
            continue;
        }
        let classes = classify(sentence);
        if !classes.is_empty() {
            out.push((sentence.to_string(), classes));
        }
    }
    out
}

fn evidence_label(payload: &EventPayload) -> Option<String> {
    match payload {
        EventPayload::ToolCall(tc) => Some(format!("tool_call:{}", tc.name)),
        EventPayload::ShellCommand(sc) => Some(format!("shell_command:{}", sc.command)),
        EventPayload::FileAction { path, action, .. } => {
            Some(format!("file_action:{action:?} {path}"))
        }
        _ => None,
    }
}

/// Diffs every compaction round in a trace: what went in against what came out.
///
/// `rounds` comes from storage (`get_compaction_rounds`) so a caller does not re-derive the
/// boundaries, but any slice with the same shape works -- which is what the fixture tests use.
pub fn diff_compaction_rounds(
    trace: &AgentWorthTrace,
    rounds: &[CompactionRound],
    only_round: Option<u32>,
) -> CompactionDiff {
    let mut ordered: Vec<&NormalizedEvent> = trace.events.iter().collect();
    ordered.sort_by_key(|e| e.sequence);

    let mut round_diffs = Vec::new();
    let mut forgotten = Vec::new();
    let mut dropped_total = 0usize;
    let mut survived_total = 0usize;

    for round in rounds {
        if only_round.is_some_and(|r| r != round.round) {
            continue;
        }

        // The model in force at the start of the span, so a statement can name who said it even
        // when the round's own span opens after the last ModelInvocation.
        let mut current_model = ordered
            .iter()
            .take_while(|e| e.sequence < round.start_seq)
            .filter_map(|e| match &e.payload {
                EventPayload::ModelInvocation { model, .. } => Some(model.clone()),
                _ => None,
            })
            .last();

        let mut candidates: Vec<Candidate> = Vec::new();
        for event in ordered
            .iter()
            .filter(|e| e.sequence >= round.start_seq && e.sequence <= round.end_seq)
        {
            let content = match &event.payload {
                EventPayload::ModelInvocation { model, .. } => {
                    current_model = Some(model.clone());
                    continue;
                }
                EventPayload::AssistantMessage { content, .. } => content,
                _ => continue,
            };
            for (text, classes) in decision_shaped(content) {
                candidates.push(Candidate {
                    classes,
                    text,
                    event_id: event.id.clone(),
                    sequence: event.sequence,
                    timestamp: event.timestamp,
                    model: current_model.clone(),
                });
            }
        }

        let summary = round
            .summary_seq
            .and_then(|seq| ordered.iter().find(|e| e.sequence == seq))
            .and_then(|e| compact_summary_text(&e.payload))
            .unwrap_or_default();
        let survivors: Vec<HashSet<String>> = decision_shaped(&summary)
            .into_iter()
            .map(|(text, _)| content_tokens(&text))
            .collect();

        let mut round_forgotten = 0usize;
        for candidate in &candidates {
            let tokens = content_tokens(&candidate.text);
            let survived = survivors
                .iter()
                .any(|s| jaccard(&tokens, s) >= SURVIVAL_JACCARD_THRESHOLD);
            if survived {
                continue;
            }
            round_forgotten += 1;
            forgotten.push(ForgottenStatement {
                classes: candidate.classes.clone(),
                round: round.round,
                text: candidate.text.clone(),
                event_id: candidate.event_id.clone(),
                sequence: candidate.sequence,
                timestamp: candidate.timestamp,
                model: candidate.model.clone(),
                followed_by: evidence_after(&ordered, candidate.sequence),
            });
        }

        dropped_total += candidates.len();
        survived_total += survivors.len();
        round_diffs.push(RoundDiff {
            round: round.round,
            start_seq: round.start_seq,
            end_seq: round.end_seq,
            summary_seq: round.summary_seq,
            tokens_before: round.tokens_before,
            summary_tokens: round.summary_tokens,
            dropped_total: candidates.len(),
            summary_total: survivors.len(),
            forgotten_total: round_forgotten,
        });
    }

    // Newest first: the most recent thing a session decided and forgot is the one most likely
    // to still matter.
    forgotten.sort_by(|a, b| b.sequence.cmp(&a.sequence));

    CompactionDiff {
        rounds: round_diffs,
        forgotten,
        dropped_total,
        survived_in_summary: survived_total,
    }
}

fn evidence_after(ordered: &[&NormalizedEvent], sequence: u64) -> Vec<Evidence> {
    ordered
        .iter()
        .skip_while(|e| e.sequence <= sequence)
        .take(EVIDENCE_LOOKAHEAD_EVENTS)
        .filter_map(|e| {
            evidence_label(&e.payload).map(|what| Evidence {
                what,
                sequence: e.sequence,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{
        compaction_rounds, CompactionEvent, Provenance, ShellCommand, ToolCall,
        COMPACT_SUMMARY_KIND,
    };
    use chrono::Duration;
    use serde_json::json;

    fn trace_with(events: Vec<NormalizedEvent>) -> AgentWorthTrace {
        let prov = Provenance::new("/tmp/t.jsonl", "claude_code", 1, 1, "fp");
        let mut trace = AgentWorthTrace::new("s1", "claude_code", prov, Utc::now());
        trace.events = events;
        trace
    }

    fn assistant(seq: u64, content: &str) -> NormalizedEvent {
        NormalizedEvent::new(
            seq,
            Utc::now() + Duration::seconds(seq as i64),
            EventPayload::AssistantMessage {
                content: content.to_string(),
                thinking: None,
            },
        )
    }

    fn boundary(seq: u64) -> NormalizedEvent {
        NormalizedEvent::new(
            seq,
            Utc::now() + Duration::seconds(seq as i64),
            EventPayload::Compaction(CompactionEvent {
                trigger: "manual".to_string(),
                pre_tokens: Some(700_000),
                post_tokens: Some(21_000),
                dropped_tokens: Some(679_000),
                duration_ms: None,
            }),
        )
    }

    fn summary(seq: u64, text: &str) -> NormalizedEvent {
        NormalizedEvent::new(
            seq,
            Utc::now() + Duration::seconds(seq as i64),
            EventPayload::Custom {
                kind: COMPACT_SUMMARY_KIND.to_string(),
                data: json!({"message": {"content": text}}),
            },
        )
    }

    fn classes_of(text: &str) -> Vec<StatementClass> {
        let shaped = decision_shaped(text);
        shaped.first().map(|(_, c)| c.clone()).unwrap_or_default()
    }

    /// The spec's own three-row pattern table, one example per row.
    #[test]
    fn the_three_classes_are_the_ones_the_spec_names() {
        assert_eq!(
            classes_of("We decided to keep the exit-code index out of SQLite."),
            vec![StatementClass::Decision]
        );
        assert_eq!(
            classes_of("Going with a marker table rather than a second pass over the file."),
            vec![StatementClass::Decision]
        );
        assert_eq!(
            classes_of("We ruled out persisting exit codes for every command in the session."),
            vec![StatementClass::Rejected]
        );
        assert_eq!(
            classes_of("That approach won't survive the second compaction round, so drop it."),
            vec![StatementClass::Rejected]
        );
        assert_eq!(
            classes_of("The splitter is hand-rolled because the regex crate has no lookbehind."),
            vec![StatementClass::Reason]
        );
    }

    /// The spec counts distinct sentences separately from class matches, precisely because one
    /// sentence hits two patterns. Pin that a sentence is one row carrying both labels.
    #[test]
    fn one_sentence_carrying_two_patterns_is_one_sentence() {
        let shaped = decision_shaped(
            "Going with a marker table instead of a second pass -- the second pass \
             re-reads 68 MB for one boolean.",
        );
        assert_eq!(shaped.len(), 1);
        assert_eq!(
            shaped[0].1,
            vec![StatementClass::Decision, StatementClass::Rejected]
        );
    }

    #[test]
    fn the_length_window_excludes_fragments_and_paragraphs() {
        assert!(decision_shaped("We decided.").is_empty(), "under 25 units");
        let long = format!("We decided to {} the whole thing.", "x".repeat(400));
        assert!(decision_shaped(&long).is_empty(), "over 400 units");
    }

    #[test]
    fn only_assistant_text_is_read() {
        let trace = trace_with(vec![
            NormalizedEvent::new(
                1,
                Utc::now(),
                EventPayload::UserMessage {
                    content: "We decided to keep the exit-code index out of SQLite.".to_string(),
                },
            ),
            boundary(2),
            summary(3, "nothing relevant here"),
        ]);
        let rounds = compaction_rounds(&trace);
        let diff = diff_compaction_rounds(&trace, &rounds, None);
        assert_eq!(diff.dropped_total, 0, "a user's own words were never dropped");
    }

    /// The diff itself: a sentence restated in the summary survives, one that is not is
    /// forgotten. Both in the same round, so the test cannot pass by returning everything or
    /// nothing.
    #[test]
    fn a_restated_sentence_survives_and_the_rest_is_forgotten() {
        let kept = "We decided to keep the exit-code index out of SQLite entirely.";
        let lost = "Going with a marker table instead of a second pass over the transcript.";
        let trace = trace_with(vec![
            assistant(1, &format!("{kept} {lost}")),
            boundary(2),
            summary(3, &format!("Carried forward: {kept}")),
        ]);
        let rounds = compaction_rounds(&trace);
        let diff = diff_compaction_rounds(&trace, &rounds, None);

        assert_eq!(diff.dropped_total, 2);
        assert_eq!(diff.survived_in_summary, 1);
        assert_eq!(diff.forgotten.len(), 1);
        assert_eq!(diff.forgotten[0].text, lost);
        assert_eq!(diff.forgotten[0].round, 1);
    }

    /// A sentence with a tool call after it was acted on; one with nothing after it is a claim.
    #[test]
    fn evidence_after_a_statement_is_returned_with_its_sequence() {
        let trace = trace_with(vec![
            assistant(1, "Going with a marker table instead of a second pass over it."),
            NormalizedEvent::new(
                2,
                Utc::now(),
                EventPayload::ToolCall(ToolCall {
                    id: None,
                    name: "Edit".to_string(),
                    arguments: serde_json::Value::Null,
                }),
            ),
            NormalizedEvent::new(
                3,
                Utc::now(),
                EventPayload::ShellCommand(ShellCommand {
                    command: "cargo test -p agentworth-storage".to_string(),
                    cwd: None,
                    exit_code: Some(0),
                    output: None,
                }),
            ),
            boundary(4),
            summary(5, "unrelated"),
        ]);
        let rounds = compaction_rounds(&trace);
        let diff = diff_compaction_rounds(&trace, &rounds, None);

        assert_eq!(diff.forgotten.len(), 1);
        let evidence: Vec<&str> = diff.forgotten[0]
            .followed_by
            .iter()
            .map(|e| e.what.as_str())
            .collect();
        assert_eq!(
            evidence,
            vec!["tool_call:Edit", "shell_command:cargo test -p agentworth-storage"]
        );
        assert_eq!(diff.forgotten[0].followed_by[0].sequence, 2);
    }

    /// A synthetic two-round session: each round's statements are attributed to that round, the
    /// second round's span does not reach back over the first round's summary, and the results
    /// come back newest first.
    #[test]
    fn a_two_round_session_attributes_each_sentence_to_its_own_round() {
        let r1 = "We decided to store the round boundaries rather than rescan the file.";
        let r2 = "Going with 0.6 Jaccard instead of an exact match on the sentence text.";
        let trace = trace_with(vec![
            assistant(1, r1),
            boundary(2),
            summary(3, "round one summary, restating nothing in particular"),
            assistant(4, r2),
            boundary(5),
            summary(6, "round two summary, also restating nothing"),
            assistant(7, "We decided this one is after every round and must not appear."),
        ]);
        let rounds = compaction_rounds(&trace);
        assert_eq!(rounds.len(), 2);

        let diff = diff_compaction_rounds(&trace, &rounds, None);
        assert_eq!(diff.rounds.len(), 2);
        assert_eq!(diff.rounds[0].dropped_total, 1);
        assert_eq!(diff.rounds[1].dropped_total, 1);
        assert_eq!(
            diff.forgotten.iter().map(|f| f.round).collect::<Vec<_>>(),
            vec![2, 1],
            "newest round first"
        );
        assert_eq!(diff.forgotten[0].text, r2);
        assert_eq!(diff.forgotten[1].text, r1);

        // Nothing after the last boundary is in any round: it is still in the model's context.
        assert!(
            !diff.forgotten.iter().any(|f| f.sequence == 7),
            "a statement made after the last compaction was never dropped"
        );

        let just_round_one = diff_compaction_rounds(&trace, &rounds, Some(1));
        assert_eq!(just_round_one.rounds.len(), 1);
        assert_eq!(just_round_one.forgotten.len(), 1);
        assert_eq!(just_round_one.forgotten[0].text, r1);
    }

    #[test]
    fn a_session_that_never_compacted_diffs_to_nothing() {
        let trace = trace_with(vec![assistant(
            1,
            "We decided to keep the exit-code index out of SQLite entirely.",
        )]);
        let diff = diff_compaction_rounds(&trace, &compaction_rounds(&trace), None);
        assert!(diff.rounds.is_empty());
        assert!(diff.forgotten.is_empty());
        assert_eq!(diff.dropped_total, 0);
    }

    #[test]
    fn class_names_round_trip_through_the_parser() {
        for class in StatementClass::ALL {
            assert_eq!(StatementClass::parse(class.as_str()), Some(class));
        }
        assert_eq!(StatementClass::parse("Decision"), Some(StatementClass::Decision));
        assert_eq!(StatementClass::parse("everything"), None);
    }
}
