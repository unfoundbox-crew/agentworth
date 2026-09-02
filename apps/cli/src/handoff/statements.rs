//! Sentences the assistant said were decisions, quoted verbatim.
//!
//! `docs/specs/handoff.md` is blunt that the machine owns the inventory and the human owns the
//! judgment, and it lists open decisions among the things the index cannot supply. That is
//! still true, and this does not contradict it: it does not decide anything, summarise
//! anything, or claim a decision is current. It finds sentences that *say* a choice was made
//! and hands them back word for word with a sequence number, so the next session can go and
//! read the turn it came from.
//!
//! The distinction matters enough to keep in the section heading itself, which reads "Said it
//! decided" rather than "Decisions". A generated handoff that silently replaces the human's
//! decision list is the risk the spec names; this one cannot, because it never claims to be
//! that list.

use agentworth_outcomes::loose_ends::split_sentences;
use agentworth_schema::{EventPayload, NormalizedEvent};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Phrases that name a choice already made, rather than one being weighed. `we should` and
/// `maybe we` are deliberately absent -- they are deliberation, and quoting them as decisions
/// is the failure this whole section is written to avoid.
const DECISION_PATTERN: &str = r"(?i)\b(decided (?:to|on|against|not to)|we'?re going with|going with|settled on|opted for|opting for|chose (?:to|the)|choosing (?:to|the)|ruled out|ruling out|the decision is|decision:)\b";

/// Same window the loose-ends detector uses, and for the same reasons: too short to carry a
/// decision, too long to be one sentence. Measured in UTF-16 units so a non-ASCII transcript
/// gets the same treatment in every process (see `agentworth_outcomes::loose_ends`).
const MIN_LEN: usize = 25;
const MAX_LEN: usize = 240;

static DECISION: OnceLock<regex::Regex> = OnceLock::new();

fn decision_re() -> &'static regex::Regex {
    DECISION.get_or_init(|| regex::Regex::new(DECISION_PATTERN).expect("valid regex"))
}

/// One sentence, quoted, with everything needed to find the turn it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statement {
    pub text: String,
    pub event_id: String,
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
}

/// Finds sentences in assistant messages that state a decision was made.
pub fn find_decisions(events: &[&NormalizedEvent]) -> Vec<Statement> {
    let mut out = Vec::new();

    for event in events {
        let EventPayload::AssistantMessage { content, .. } = &event.payload else {
            continue;
        };
        for sentence in split_sentences(content) {
            let len = sentence.encode_utf16().count();
            if len < MIN_LEN || len > MAX_LEN || !decision_re().is_match(sentence) {
                continue;
            }
            out.push(Statement {
                text: sentence.to_string(),
                event_id: event.id.clone(),
                sequence: event.sequence,
                timestamp: event.timestamp,
            });
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

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

    fn found(content: &str) -> Vec<String> {
        let event = assistant(7, content);
        find_decisions(&[&event])
            .into_iter()
            .map(|s| s.text)
            .collect()
    }

    #[test]
    fn quotes_a_stated_decision_verbatim_with_its_sequence() {
        let event = assistant(41, "We decided to keep the exit-code index out of SQLite.");
        let found = find_decisions(&[&event]);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].text,
            "We decided to keep the exit-code index out of SQLite."
        );
        assert_eq!(found[0].sequence, 41);
        assert_eq!(found[0].event_id, event.id);
    }

    #[test]
    fn deliberation_is_not_a_decision() {
        for text in [
            "We should probably keep the exit-code index out of SQLite for now.",
            "Maybe we move the renderer into its own crate later on, or maybe not.",
            "I could go either way on whether the budget belongs in the renderer.",
        ] {
            assert!(found(text).is_empty(), "deliberation must not be quoted: {text}");
        }
    }

    #[test]
    fn recognises_the_common_ways_a_choice_gets_stated() {
        for text in [
            "Going with the read-time derivation rather than a new stored column.",
            "We settled on 60 lines as the default budget for the rendered handoff.",
            "Ruled out persisting the exit codes, since the trace is already in memory.",
            "Opted for a hand-rolled splitter because the regex crate has no lookbehind.",
        ] {
            assert_eq!(found(text).len(), 1, "should be quoted: {text}");
        }
    }

    #[test]
    fn only_assistant_messages_are_read() {
        let user = NormalizedEvent::new(
            1,
            Utc::now(),
            EventPayload::UserMessage {
                content: "We decided to keep the exit-code index out of SQLite.".to_string(),
            },
        );
        assert!(find_decisions(&[&user]).is_empty());
    }

    #[test]
    fn each_sentence_of_a_multi_decision_message_is_quoted_separately() {
        let quoted = found(
            "We decided to keep the exit-code index out of SQLite. \
             Going with the read-time derivation for file counts as well.",
        );
        assert_eq!(quoted.len(), 2);
        assert!(quoted[0].ends_with("out of SQLite."));
    }
}
