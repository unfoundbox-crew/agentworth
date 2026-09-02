//! `session_asks`: the questions-to-answers index for one session, with receipts.
//!
//! `docs/specs/asks.md` is the design; the extraction itself lives in
//! `agentworth_outcomes::asks`. This assembles it into an answer the same way `forgotten.rs`
//! does for compaction diffs -- the trace from disk, the filters, the honest empty cases, and a
//! receipt -- and it lives in `apps/cli` for the same reason: it composes storage, the
//! extractor, the scanner and redaction, and both the CLI and the MCP tool consume it from here.

use agentworth_core::Scanner;
use agentworth_outcomes::{find_asks_in_trace, Ask, AskStatus};
use agentworth_redaction::Redactor;
use agentworth_schema::{extract_repository_or_workspace, AgentWorthTrace};
use agentworth_storage::Storage;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// How many asks come back when the caller does not say.
pub const DEFAULT_LIMIT: usize = 50;

/// Hard ceiling. Past this the caller wants `session_get` and the raw trace.
pub const LIMIT_CEILING: usize = 500;

/// The extraction method, stamped on every receipt -- three deterministic patterns (a `?`
/// sentence, a flag-prefixed line, a reply that is itself a question), no model involved.
pub const METHOD: &str = "regex_v1";

/// The machine-readable "nothing here", and the two cases worth telling apart.
pub mod note {
    /// The session asked no questions at all -- no `?` sentence, no flag line.
    pub const NO_QUESTIONS: &str = "no_questions_in_this_session";
    /// Questions exist, but none match the filters given (`since`, `unanswered_only`).
    pub const NOTHING_MATCHED_FILTERS: &str = "no_questions_matched_the_filters";
}

/// What was asked for.
#[derive(Debug, Clone, Default)]
pub struct AsksOptions {
    /// Only questions asked at or after this instant.
    pub since: Option<DateTime<Utc>>,
    /// Only questions whose status isn't `answered` -- still open, or handed back to the user.
    pub unanswered_only: bool,
    pub limit: usize,
}

/// How the answer was produced, so it can be checked rather than believed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsksReceipt {
    pub session_id: String,
    pub repo: String,
    pub adapter: String,
    pub source_path: String,
    pub generated_at: DateTime<Utc>,
    pub index_last_updated: Option<DateTime<Utc>>,
    /// Always `regex_v1`. No model reads the transcript to produce this.
    pub method: String,
    pub no_model: bool,
    pub redacted: bool,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsksReport {
    pub receipt: AsksReceipt,
    pub headline: String,
    /// Every question the session asked, before `since`/`unanswered_only`/`limit`.
    pub total_questions: usize,
    pub answered: usize,
    pub flagged_back_to_user: usize,
    pub no_reply_yet: usize,
    /// After filtering and the limit.
    pub returned: usize,
    pub truncated: bool,
    /// Newest first.
    pub asks: Vec<Ask>,
    pub notes: Vec<String>,
}

impl AsksReport {
    /// A copy with every free-text field run through `redactor`.
    ///
    /// One [`Redactor::for_trace`]-augmented instance, same convention as
    /// [`crate::forgotten::ForgottenReport::redacted`]: the session's own repository identity
    /// gets masked across quoted questions, answer excerpts and the receipt path, not just one
    /// of them.
    pub fn redacted(&self, redactor: &Redactor) -> AsksReport {
        AsksReport {
            receipt: AsksReceipt {
                repo: redactor.redact_text(&self.receipt.repo),
                source_path: redactor.redact_text(&self.receipt.source_path),
                redacted: true,
                ..self.receipt.clone()
            },
            headline: redactor.redact_text(&self.headline),
            asks: self
                .asks
                .iter()
                .map(|a| Ask {
                    question: redactor.redact_text(&a.question),
                    answer_excerpt: a.answer_excerpt.as_deref().map(|t| redactor.redact_text(t)),
                    ..a.clone()
                })
                .collect(),
            ..self.clone()
        }
    }
}

/// Loads a session and indexes its questions, returning the trace alongside so a caller can
/// build the redactor from it (see [`AsksReport::redacted`]).
///
/// Refuses rather than guessing when the raw session file is gone -- same rule
/// `forgotten::load_forgotten` follows, for the same reason.
pub fn load_asks(
    storage: &Storage,
    scanner: &Scanner,
    session_id: &str,
    options: &AsksOptions,
) -> Result<(AsksReport, AgentWorthTrace)> {
    storage
        .get_session_by_id(session_id)?
        .with_context(|| format!("session '{session_id}' is not in the index"))?;
    let trace = scanner.load_trace(session_id).with_context(|| {
        format!("session '{session_id}' could not be re-read from its source file")
    })?;
    let index_last_updated = storage.last_scanned_at().unwrap_or(None);

    Ok((build_asks(&trace, index_last_updated, options), trace))
}

/// Assembles the report from things already in memory. Split out from [`load_asks`] so it can
/// be tested against a hand-built trace with no storage, scanner or filesystem involved.
pub fn build_asks(
    trace: &AgentWorthTrace,
    index_last_updated: Option<DateTime<Utc>>,
    options: &AsksOptions,
) -> AsksReport {
    let all = find_asks_in_trace(trace);
    let total_questions = all.len();

    let answered = all.iter().filter(|a| a.status == AskStatus::Answered).count();
    let flagged_back_to_user = all
        .iter()
        .filter(|a| a.status == AskStatus::FlaggedBackToUser)
        .count();
    let no_reply_yet = all.iter().filter(|a| a.status == AskStatus::NoReplyYet).count();

    let mut selected: Vec<Ask> = all
        .into_iter()
        .filter(|a| options.since.is_none_or(|since| a.timestamp >= since))
        .filter(|a| !options.unanswered_only || a.status != AskStatus::Answered)
        .collect();
    let matched = selected.len();
    let truncated = matched > options.limit;
    selected.truncate(options.limit);

    let mut notes = Vec::new();
    if total_questions == 0 {
        notes.push(note::NO_QUESTIONS.to_string());
    } else if matched == 0 {
        notes.push(note::NOTHING_MATCHED_FILTERS.to_string());
    }

    AsksReport {
        receipt: AsksReceipt {
            session_id: trace.session_id.clone(),
            repo: extract_repository_or_workspace(&trace.provenance.source_path),
            adapter: trace.adapter.clone(),
            source_path: trace.provenance.source_path.clone(),
            generated_at: Utc::now(),
            index_last_updated,
            method: METHOD.to_string(),
            no_model: true,
            redacted: false,
        },
        headline: headline(total_questions, answered, flagged_back_to_user, no_reply_yet),
        total_questions,
        answered,
        flagged_back_to_user,
        no_reply_yet,
        returned: selected.len(),
        truncated,
        asks: selected,
        notes,
    }
}

fn headline(total: usize, answered: usize, flagged: usize, no_reply: usize) -> String {
    if total == 0 {
        return "This session asked no questions -- nothing to index.".to_string();
    }
    format!(
        "{total} question{} asked -- {answered} answered, {flagged} flagged back to you, \
         {no_reply} still without a reply.",
        plural(total)
    )
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{EventPayload, NormalizedEvent, Provenance};
    use chrono::Duration;

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

    fn user(seq: u64, content: &str) -> NormalizedEvent {
        NormalizedEvent::new(
            seq,
            Utc::now() + Duration::seconds(seq as i64),
            EventPayload::UserMessage {
                content: content.to_string(),
            },
        )
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

    #[test]
    fn a_session_with_no_questions_says_so() {
        let trace = trace_with(vec![
            user(1, "please fix the build"),
            assistant(2, "Fixed it, cargo check is clean now."),
        ]);
        let report = build_asks(&trace, None, &AsksOptions { limit: DEFAULT_LIMIT, ..Default::default() });

        assert_eq!(report.total_questions, 0);
        assert!(report.notes.contains(&note::NO_QUESTIONS.to_string()));
        assert!(report.headline.contains("no questions"));
    }

    #[test]
    fn counts_and_headline_cover_all_three_statuses() {
        let trace = trace_with(vec![
            user(1, "Does the fps drop after compaction?"),
            assistant(2, "Yes, frame pacing degrades right after a round lands."),
            assistant(3, "⚑ Ship the fix now, or wait for the next release?"),
            user(4, "Is the flaky test fixed yet?"),
        ]);
        let report = build_asks(&trace, None, &AsksOptions { limit: DEFAULT_LIMIT, ..Default::default() });

        assert_eq!(report.total_questions, 3);
        assert_eq!(report.answered, 1);
        assert_eq!(report.flagged_back_to_user, 1);
        assert_eq!(report.no_reply_yet, 1);
        assert!(report.notes.is_empty(), "a real answer carries no note");
    }

    #[test]
    fn unanswered_only_keeps_flagged_and_no_reply_but_not_answered() {
        let trace = trace_with(vec![
            user(1, "Does the fps drop after compaction?"),
            assistant(2, "Yes, frame pacing degrades right after a round lands."),
            user(3, "Is the flaky test fixed yet?"),
        ]);
        let report = build_asks(
            &trace,
            None,
            &AsksOptions {
                unanswered_only: true,
                limit: DEFAULT_LIMIT,
                ..Default::default()
            },
        );

        assert_eq!(report.returned, 1);
        assert_eq!(report.asks[0].status, AskStatus::NoReplyYet);
        // Totals describe the whole session regardless of the filter.
        assert_eq!(report.total_questions, 2);
    }

    #[test]
    fn since_filters_by_the_questions_own_timestamp() {
        let trace = trace_with(vec![
            user(1, "First one, does this work?"),
            assistant(2, "Yes it works fine, tested it myself."),
            user(10, "Second one, does this also work?"),
            assistant(11, "Also works, confirmed just now."),
        ]);
        let cutoff = trace.started_at + Duration::seconds(5);
        let report = build_asks(
            &trace,
            None,
            &AsksOptions {
                since: Some(cutoff),
                limit: DEFAULT_LIMIT,
                ..Default::default()
            },
        );

        assert_eq!(report.returned, 1);
        assert_eq!(report.asks[0].sequence, 10);
        assert_eq!(report.total_questions, 2, "totals ignore the filter");
    }

    #[test]
    fn the_limit_truncates_and_says_it_did() {
        let trace = trace_with(vec![
            user(1, "Question one, ready?"),
            assistant(2, "Yes, this one is answered in full right here."),
            user(3, "Question two, ready?"),
            assistant(4, "Yes, this one is answered in full right here too."),
        ]);
        let report = build_asks(&trace, None, &AsksOptions { limit: 1, ..Default::default() });

        assert_eq!(report.returned, 1);
        assert!(report.truncated);
        assert_eq!(report.total_questions, 2);
    }

    #[test]
    fn redaction_reaches_the_quoted_question_and_the_receipt_path() {
        let trace = trace_with(vec![
            user(1, "Does /Users/dev/code/agentworth build cleanly?"),
        ]);
        let report = build_asks(&trace, None, &AsksOptions { limit: DEFAULT_LIMIT, ..Default::default() });
        let redacted = report.redacted(&Redactor::new().for_trace(&trace));

        assert!(redacted.receipt.redacted);
        assert!(
            !redacted.receipt.source_path.contains("/Users/dev"),
            "the home directory must not survive redaction: {}",
            redacted.receipt.source_path
        );
        assert_eq!(redacted.asks.len(), report.asks.len());
    }
}
