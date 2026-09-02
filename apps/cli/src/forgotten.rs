//! `forgotten_context`: the decisions compaction dropped, handed back with receipts.
//!
//! `docs/specs/compaction-diff.md` is the design. The extraction itself lives in
//! `agentworth_outcomes::compaction_diff`; this assembles it into an answer -- the round
//! boundaries from the index, the trace from disk, the filters, the honest empty cases, and the
//! receipt that says how the sentences were found.
//!
//! It lives in `apps/cli` for the same reason `handoff/` does: it composes storage, the
//! extractor, the scanner and redaction, and is consumed by the CLI and the MCP tools, both of
//! which are here.

use agentworth_core::Scanner;
use agentworth_outcomes::{
    diff_compaction_rounds, Evidence, ForgottenStatement, RoundDiff, StatementClass,
    SURVIVAL_JACCARD_THRESHOLD,
};
use agentworth_redaction::Redactor;
use agentworth_schema::{
    compaction_rounds, extract_repository_or_workspace, AgentWorthTrace, CompactionRound,
};
use agentworth_storage::Storage;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// How many statements come back when the caller does not say. Small on purpose: this is read
/// by a session that has a context budget, and the newest few are the ones most likely to still
/// matter.
pub const DEFAULT_LIMIT: usize = 20;

/// Hard ceiling. Past this the caller wants the whole session, which is `session_get`.
pub const LIMIT_CEILING: usize = 200;

/// The extraction method, stamped on every receipt. Bumped when the patterns change, so an
/// answer can be told apart from one produced by a different definition of "decision-shaped".
pub const METHOD: &str = "regex_v1";

/// The machine-readable "I don't know", and the three cases the spec insists are different.
pub mod note {
    /// The session never compacted. Not an error, and not an empty answer dressed as a finding.
    pub const NEVER_COMPACTED: &str = "no_compactions_in_this_session";
    /// It compacted, and no sentence in the dropped spans matched any pattern.
    pub const NOTHING_DECISION_SHAPED: &str = "nothing_decision_shaped_was_dropped";
    /// It compacted, sentences were dropped, and every one of them is restated in a summary.
    pub const EVERYTHING_SURVIVED: &str = "every_dropped_decision_survived_in_a_summary";
    /// The caller asked for a round this session does not have.
    pub const ROUND_OUT_OF_RANGE: &str = "requested_round_does_not_exist";
    /// Rounds came off the trace because the index has none for this session -- an index
    /// written before the boundary table existed, not yet rescanned.
    pub const ROUNDS_NOT_INDEXED: &str = "round_boundaries_derived_from_trace_not_index";
}

/// Where the round boundaries came from. Reported rather than hidden: a derived answer is
/// correct but says the index is stale, and the caller may want to run `agentworth scan`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoundsSource {
    Index,
    Trace,
}

/// What was asked for.
#[derive(Debug, Clone)]
pub struct ForgottenOptions {
    /// One 1-based round, or every round.
    pub round: Option<u32>,
    /// Which classes to return. Empty means all three.
    pub classes: Vec<StatementClass>,
    pub limit: usize,
}

impl Default for ForgottenOptions {
    fn default() -> Self {
        Self {
            round: None,
            classes: Vec::new(),
            limit: DEFAULT_LIMIT,
        }
    }
}

/// How the answer was produced, so it can be checked rather than believed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForgottenReceipt {
    pub session_id: String,
    pub repo: String,
    pub adapter: String,
    pub source_path: String,
    pub generated_at: DateTime<Utc>,
    pub index_last_updated: Option<DateTime<Utc>>,
    /// Always `regex_v1`. Stated so a caller can tell "nothing was decided" from "the regex
    /// found nothing", which the spec is explicit are different answers.
    pub method: String,
    /// Always true, and worth saying out loud: nothing here was paraphrased by a model.
    pub no_model: bool,
    pub rounds_source: RoundsSource,
    pub survival_threshold: f64,
    pub redacted: bool,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForgottenReport {
    pub receipt: ForgottenReceipt,
    /// The line a session actually reads.
    pub headline: String,
    /// Every round's before-and-after, whether or not any of it was returned.
    pub rounds: Vec<RoundDiff>,
    pub compactions: usize,
    pub dropped_total: usize,
    pub survived_in_summary: usize,
    pub forgotten_total: usize,
    /// After class filtering and the limit.
    pub returned: usize,
    pub truncated: bool,
    /// Newest first.
    pub forgotten: Vec<ForgottenStatement>,
    /// Named reasons the body is empty, or the round list is not from the index. Never padded
    /// over, and never the only thing said when there is a real answer.
    pub notes: Vec<String>,
}

impl ForgottenReport {
    /// A copy with every free-text field run through `redactor`.
    ///
    /// One [`Redactor::for_trace`]-augmented instance built from this report's own trace, used
    /// for nothing else -- the same rule the handoff report follows, so the session's
    /// repository identity is masked across quoted sentences, evidence labels and the receipt
    /// path rather than on only one of them.
    pub fn redacted(&self, redactor: &Redactor) -> ForgottenReport {
        ForgottenReport {
            receipt: ForgottenReceipt {
                repo: redactor.redact_text(&self.receipt.repo),
                source_path: redactor.redact_text(&self.receipt.source_path),
                redacted: true,
                ..self.receipt.clone()
            },
            headline: redactor.redact_text(&self.headline),
            forgotten: self
                .forgotten
                .iter()
                .map(|s| ForgottenStatement {
                    text: redactor.redact_text(&s.text),
                    followed_by: s
                        .followed_by
                        .iter()
                        .map(|e| Evidence {
                            what: redactor.redact_text(&e.what),
                            sequence: e.sequence,
                        })
                        .collect(),
                    ..s.clone()
                })
                .collect(),
            ..self.clone()
        }
    }
}

/// Loads a session and diffs its compaction rounds, returning the trace alongside so a caller
/// can build the redactor from it (see [`ForgottenReport::redacted`]).
///
/// Refuses rather than guessing when the raw session file is gone: `sessions.source_path` can
/// point at a file that has since been deleted, and returning a partial diff assembled from an
/// index row would be inventing content the spec is explicit must not be invented.
pub fn load_forgotten(
    storage: &Storage,
    scanner: &Scanner,
    session_id: &str,
    options: &ForgottenOptions,
) -> Result<(ForgottenReport, AgentWorthTrace)> {
    storage
        .get_session_by_id(session_id)?
        .with_context(|| format!("session '{session_id}' is not in the index"))?;
    let trace = scanner.load_trace(session_id).with_context(|| {
        format!("session '{session_id}' could not be re-read from its source file")
    })?;
    let stored = storage.get_compaction_rounds(session_id)?;
    let index_last_updated = storage.last_scanned_at().unwrap_or(None);

    Ok((
        build_forgotten(&trace, stored, index_last_updated, options),
        trace,
    ))
}

/// Assembles the report from things already in memory. Split out from [`load_forgotten`] so it
/// can be tested against a hand-built trace with no storage, scanner or filesystem involved.
pub fn build_forgotten(
    trace: &AgentWorthTrace,
    stored_rounds: Vec<CompactionRound>,
    index_last_updated: Option<DateTime<Utc>>,
    options: &ForgottenOptions,
) -> ForgottenReport {
    let mut notes = Vec::new();

    // Boundaries from the index when they are there, from the trace when they are not. A
    // session indexed before the boundary table existed still answers correctly; it just says
    // where the answer came from.
    let (rounds, rounds_source) = if stored_rounds.is_empty() {
        let derived = compaction_rounds(trace);
        if !derived.is_empty() {
            notes.push(note::ROUNDS_NOT_INDEXED.to_string());
        }
        (derived, RoundsSource::Trace)
    } else {
        (stored_rounds, RoundsSource::Index)
    };

    let compactions = rounds.len();
    if let Some(requested) = options.round {
        if !rounds.iter().any(|r| r.round == requested) {
            notes.push(note::ROUND_OUT_OF_RANGE.to_string());
        }
    }

    let diff = diff_compaction_rounds(trace, &rounds, options.round);
    let forgotten_total = diff.forgotten_total();

    let mut selected: Vec<ForgottenStatement> = diff
        .forgotten
        .iter()
        .filter(|s| {
            options.classes.is_empty() || s.classes.iter().any(|c| options.classes.contains(c))
        })
        .cloned()
        .collect();
    let matched = selected.len();
    let truncated = matched > options.limit;
    selected.truncate(options.limit);

    if compactions == 0 {
        notes.push(note::NEVER_COMPACTED.to_string());
    } else if diff.dropped_total == 0 {
        notes.push(note::NOTHING_DECISION_SHAPED.to_string());
    } else if forgotten_total == 0 {
        notes.push(note::EVERYTHING_SURVIVED.to_string());
    }

    ForgottenReport {
        receipt: ForgottenReceipt {
            session_id: trace.session_id.clone(),
            repo: extract_repository_or_workspace(&trace.provenance.source_path),
            adapter: trace.adapter.clone(),
            source_path: trace.provenance.source_path.clone(),
            generated_at: Utc::now(),
            index_last_updated,
            method: METHOD.to_string(),
            no_model: true,
            rounds_source,
            survival_threshold: SURVIVAL_JACCARD_THRESHOLD,
            redacted: false,
        },
        headline: headline(compactions, forgotten_total, diff.survived_in_summary),
        rounds: diff.rounds,
        compactions,
        dropped_total: diff.dropped_total,
        survived_in_summary: diff.survived_in_summary,
        forgotten_total,
        returned: selected.len(),
        truncated,
        forgotten: selected,
        notes,
    }
}

/// The one line the spec puts at the top, and the two honest variants of it.
fn headline(compactions: usize, forgotten: usize, survived: usize) -> String {
    if compactions == 0 {
        return "This session never compacted, so nothing was dropped.".to_string();
    }
    if forgotten == 0 {
        return format!(
            "This session compacted {compactions} time{}, and nothing decision-shaped was lost.",
            plural(compactions)
        );
    }
    format!(
        "Things you decided and no longer remember — {forgotten} sentence{} dropped across \
         {compactions} compaction round{}, {survived} survived in the summaries.",
        plural(forgotten),
        plural(compactions)
    )
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// Parses a caller's `classes` list, rejecting an unknown name rather than ignoring it -- a
/// silently ignored filter returns more than was asked for and looks like an answer.
pub fn parse_classes(raw: &[String]) -> Result<Vec<StatementClass>> {
    let mut out = Vec::new();
    for name in raw {
        let class = StatementClass::parse(name).with_context(|| {
            format!("unknown class '{name}'; expected decision, rejected or reason")
        })?;
        if !out.contains(&class) {
            out.push(class);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{
        CompactionEvent, EventPayload, NormalizedEvent, Provenance, COMPACT_SUMMARY_KIND,
    };
    use chrono::Duration;
    use serde_json::json;

    fn trace_with(events: Vec<NormalizedEvent>) -> AgentWorthTrace {
        let prov = Provenance::new(
            "/Users/dev/.claude/projects/-Users-dev-code-agentworth/s1.jsonl",
            "claude_code",
            1,
            1,
            "fp",
        );
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

    fn two_round_trace() -> AgentWorthTrace {
        trace_with(vec![
            assistant(1, "We decided to store the round boundaries rather than rescan."),
            assistant(2, "The splitter is hand-rolled because the regex crate has no lookbehind."),
            boundary(3),
            summary(4, "round one summary, restating none of it"),
            assistant(5, "Going with 0.6 Jaccard instead of an exact match on the text."),
            boundary(6),
            summary(7, "round two summary, restating none of it either"),
        ])
    }

    /// A session that never compacted must say so, not return an empty list that reads like a
    /// finding.
    #[test]
    fn a_session_that_never_compacted_says_so() {
        let trace = trace_with(vec![assistant(
            1,
            "We decided to keep the exit-code index out of SQLite entirely.",
        )]);
        let report = build_forgotten(&trace, Vec::new(), None, &ForgottenOptions::default());

        assert_eq!(report.compactions, 0);
        assert!(report.forgotten.is_empty());
        assert!(report.notes.contains(&note::NEVER_COMPACTED.to_string()));
        assert!(report.headline.contains("never compacted"));
    }

    /// Compacted but nothing matched is a different answer, and the note has to say which.
    #[test]
    fn compacted_with_no_matches_is_not_the_same_as_never_compacted() {
        let trace = trace_with(vec![
            assistant(1, "Reading the file now and then moving on to the next one."),
            boundary(2),
            summary(3, "a summary with nothing decision-shaped in it at all"),
        ]);
        let report = build_forgotten(&trace, Vec::new(), None, &ForgottenOptions::default());

        assert_eq!(report.compactions, 1);
        assert_eq!(report.dropped_total, 0);
        assert!(report
            .notes
            .contains(&note::NOTHING_DECISION_SHAPED.to_string()));
        assert!(!report.notes.contains(&note::NEVER_COMPACTED.to_string()));
    }

    #[test]
    fn statements_come_back_newest_first_with_their_round() {
        let trace = two_round_trace();
        let report = build_forgotten(&trace, Vec::new(), None, &ForgottenOptions::default());

        assert_eq!(report.compactions, 2);
        assert_eq!(report.forgotten_total, 3);
        assert_eq!(
            report.forgotten.iter().map(|s| s.round).collect::<Vec<_>>(),
            vec![2, 1, 1]
        );
        assert!(report.headline.starts_with("Things you decided"));
        assert!(report.notes.is_empty(), "a real answer carries no note");
    }

    #[test]
    fn a_class_filter_narrows_without_changing_the_totals() {
        let trace = two_round_trace();
        let report = build_forgotten(
            &trace,
            Vec::new(),
            None,
            &ForgottenOptions {
                classes: vec![StatementClass::Reason],
                ..Default::default()
            },
        );

        assert_eq!(report.forgotten_total, 3, "the count is of the session, not the filter");
        assert_eq!(report.returned, 1);
        assert!(report.forgotten[0].text.contains("because"));
    }

    #[test]
    fn the_limit_truncates_and_says_it_did() {
        let trace = two_round_trace();
        let report = build_forgotten(
            &trace,
            Vec::new(),
            None,
            &ForgottenOptions {
                limit: 1,
                ..Default::default()
            },
        );
        assert_eq!(report.returned, 1);
        assert!(report.truncated);
        assert_eq!(report.forgotten_total, 3);
    }

    #[test]
    fn one_round_can_be_asked_for_and_a_missing_one_is_named() {
        let trace = two_round_trace();
        let report = build_forgotten(
            &trace,
            Vec::new(),
            None,
            &ForgottenOptions {
                round: Some(1),
                ..Default::default()
            },
        );
        assert!(report.forgotten.iter().all(|s| s.round == 1));

        let missing = build_forgotten(
            &trace,
            Vec::new(),
            None,
            &ForgottenOptions {
                round: Some(9),
                ..Default::default()
            },
        );
        assert!(missing.forgotten.is_empty());
        assert!(missing.notes.contains(&note::ROUND_OUT_OF_RANGE.to_string()));
    }

    /// An index written before the boundary table exists must still answer, and must say the
    /// answer did not come from the index.
    #[test]
    fn boundaries_fall_back_to_the_trace_and_the_receipt_admits_it() {
        let trace = two_round_trace();
        let derived = build_forgotten(&trace, Vec::new(), None, &ForgottenOptions::default());
        assert_eq!(derived.receipt.rounds_source, RoundsSource::Trace);
        assert!(derived.notes.contains(&note::ROUNDS_NOT_INDEXED.to_string()));

        let stored = build_forgotten(
            &trace,
            compaction_rounds(&trace),
            None,
            &ForgottenOptions::default(),
        );
        assert_eq!(stored.receipt.rounds_source, RoundsSource::Index);
        assert!(!stored.notes.contains(&note::ROUNDS_NOT_INDEXED.to_string()));
        assert_eq!(stored.forgotten, derived.forgotten, "same answer either way");
    }

    #[test]
    fn redaction_reaches_the_quoted_sentence_and_the_receipt_path() {
        let trace = two_round_trace();
        let report = build_forgotten(&trace, Vec::new(), None, &ForgottenOptions::default());
        let redacted = report.redacted(&Redactor::new().for_trace(&trace));

        assert!(redacted.receipt.redacted);
        assert!(
            !redacted.receipt.source_path.contains("/Users/dev"),
            "the home directory must not survive redaction: {}",
            redacted.receipt.source_path
        );
        assert_eq!(redacted.forgotten.len(), report.forgotten.len());
    }

    #[test]
    fn an_unknown_class_is_rejected_rather_than_ignored() {
        assert!(parse_classes(&["decision".to_string(), "reason".to_string()]).is_ok());
        assert!(parse_classes(&["everything".to_string()]).is_err());
    }
}
