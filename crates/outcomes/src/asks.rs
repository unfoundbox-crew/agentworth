//! Asks: a questions-to-answers index over a session.
//!
//! `docs/specs/asks.md` is the design. In a long session Saurabh asks a question, the answer
//! lands several messages later among tool notifications, and he re-asks it because scrolling
//! costs time and re-asking costs tokens. This module finds every question in a trace and, for
//! each, the first substantive assistant text that plausibly answers it -- deterministic, no
//! model involved, same spirit as `loose_ends.rs` next to it.
//!
//! A question comes from one of two places:
//! - **A user turn.** Any sentence in a `UserMessage` containing `?` is a question Saurabh asked.
//! - **An assistant flag line.** A line in an `AssistantMessage` starting with `⚑` or `🚩` is a
//!   question the assistant asked back to him -- the convention `~/.claude/CLAUDE.md` calls out
//!   as "prefix the line with a flag character" when a decision needs him specifically.
//!
//! For a user-asked question, the answer is the first assistant text event afterward (skipping
//! tool calls, tool results, and everything else that isn't `AssistantMessage`) that carries a
//! line of at least 20 characters not itself filler ("waiting", "on it", ...). If that answer's
//! own first sentence ends in `?`, the assistant handed the question back rather than answering
//! it, which is a `flagged_back_to_user` outcome, same as the flag-line case itself. If nothing
//! substantive appears before the next user turn (or the trace ends), the question is
//! `no_reply_yet`.
//!
//! A flag-line question is never scanned forward for an answer: it is, by construction, the
//! assistant asking Saurabh something, so there is no assistant text later in the trace that
//! would count as answering it -- the reply worth showing is his own next message, which this
//! index does not reach for (it only follows *assistant* text). It is always `flagged_back_to_user`.

use std::sync::OnceLock;

use agentworth_schema::{AgentWorthTrace, EventPayload, NormalizedEvent};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::loose_ends::split_sentences;

/// Question text is trimmed to this many characters.
const QUESTION_TRIM: usize = 120;
/// Answer excerpts are trimmed to this many characters.
const ANSWER_TRIM: usize = 200;
/// A line shorter than this cannot carry a substantive answer.
const MIN_ANSWER_LINE_LEN: usize = 20;

/// A whole trimmed line that is nothing but a "still working" placeholder -- not a real answer.
const FILLER_PATTERN: &str = r"(?i)^(waiting|still waiting|working on it|on it|one moment|hold on|almost done|let me check|checking|stand by|just a (?:moment|sec|second)|give me a (?:moment|sec|second))\.*!?$";

static FILLER: OnceLock<regex::Regex> = OnceLock::new();

fn filler_re() -> &'static regex::Regex {
    FILLER.get_or_init(|| regex::Regex::new(FILLER_PATTERN).expect("valid regex"))
}

/// The two flag glyphs `~/.claude/CLAUDE.md` recognizes for "this line asks Saurabh a decision".
const FLAG_CHARS: [char; 2] = ['⚑', '🚩'];

/// Who asked the question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskedBy {
    /// A `?` sentence in a user turn.
    User,
    /// A flag-prefixed line in an assistant turn -- the assistant asking back.
    Assistant,
}

/// Whether a question got answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskStatus {
    /// A substantive assistant answer was found, and it wasn't itself another question.
    Answered,
    /// Either a flag-line question (always this), or the reply found was itself a question --
    /// both mean the ball is back in Saurabh's court.
    FlaggedBackToUser,
    /// No assistant text before the next user turn, or before the trace ends.
    NoReplyYet,
}

/// Where to jump to see the outcome: the answer's location when there is one, otherwise the
/// question's own location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskPointer {
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
}

/// One question and what became of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ask {
    /// The question, verbatim, trimmed to 120 characters.
    pub question: String,
    pub timestamp: DateTime<Utc>,
    pub event_id: String,
    pub sequence: u64,
    pub asked_by: AskedBy,
    pub status: AskStatus,
    /// First 200 characters of the answer, when one was found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer_excerpt: Option<String>,
    /// Jump target: the answer's event when one was found, else the question's own event.
    pub pointer: AskPointer,
}

/// Length in characters, trimmed at a char boundary and marked with `..` if anything was cut.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else if max <= 2 {
        s.chars().take(max).collect()
    } else {
        let head: String = s.chars().take(max - 2).collect();
        format!("{head}..")
    }
}

/// Strips a common markdown list prefix (`- `, `* `, `1. `, bold markers) so a flag glyph inside
/// a bulleted or numbered line is still recognized.
fn strip_list_prefix(line: &str) -> &str {
    let mut s = line.trim_start();
    for prefix in ["- ", "* ", "**"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.trim_start();
        }
    }
    if let Some(dot) = s.find(". ") {
        #[allow(
            clippy::string_slice,
            reason = "dot comes from find(\". \"), offset by its own byte length: always a char boundary"
        )]
        if s[..dot].chars().all(|c| c.is_ascii_digit()) && !s[..dot].is_empty() {
            s = s[dot + 2..].trim_start();
        }
    }
    s
}

/// Does this line start with a flag glyph, once common list/markdown decoration is stripped?
fn is_flagged_line(line: &str) -> bool {
    strip_list_prefix(line)
        .chars()
        .next()
        .is_some_and(|c| FLAG_CHARS.contains(&c))
}

/// Does this line carry enough text to be a real answer, as opposed to a placeholder?
fn is_substantive_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.chars().count() >= MIN_ANSWER_LINE_LEN && !filler_re().is_match(trimmed)
}

/// Does this text read as a question -- its first sentence ends in `?`?
fn reads_as_question(text: &str) -> bool {
    split_sentences(text)
        .first()
        .is_some_and(|s| s.trim_end().ends_with('?'))
}

/// One candidate answer: the text to show, and where it was said.
struct Candidate<'a> {
    text: &'a str,
    sequence: u64,
    timestamp: DateTime<Utc>,
}

/// Scans forward from `after` (exclusive) for the first substantive assistant text, stopping at
/// the next user turn. Everything that isn't `AssistantMessage` -- tool calls, tool results,
/// task notifications, compaction records, everything else -- is skipped, not a stopping point;
/// only a `UserMessage` ends the search, matching `loose_ends.rs`'s own fulfilment scan.
fn find_answer<'a>(ordered: &'a [&'a NormalizedEvent], after: usize) -> Option<Candidate<'a>> {
    for event in &ordered[after + 1..] {
        match &event.payload {
            EventPayload::AssistantMessage { content, .. } => {
                if let Some(text) = first_substantive_span(content) {
                    return Some(Candidate {
                        text,
                        sequence: event.sequence,
                        timestamp: event.timestamp,
                    });
                }
            }
            EventPayload::UserMessage { .. } => return None,
            _ => {}
        }
    }
    None
}

/// From the first line carrying at least [`MIN_ANSWER_LINE_LEN`] non-filler characters, the rest
/// of the message (trimmed). `None` if no line in `content` qualifies -- a short preamble like
/// "One moment." followed by nothing else doesn't count as an answer.
///
/// Tracks byte offsets by hand rather than joining `str::lines()` back into an owned `String`,
/// so the returned slice borrows straight from `content` instead of a search-for-the-substring
/// hack that would silently fall back to the whole message on any mismatch (trailing
/// whitespace, `\r\n`, ...).
#[allow(
    clippy::string_slice,
    reason = "offset accumulates split('\\n') line lengths, always a char boundary"
)]
fn first_substantive_span(content: &str) -> Option<&str> {
    let mut offset = 0usize;
    for line in content.split('\n') {
        if is_substantive_line(line) {
            let rest = content[offset..].trim();
            return (!rest.is_empty()).then_some(rest);
        }
        offset += line.len() + 1;
    }
    None
}

/// Finds every question in a session and what became of it. Newest first.
pub fn find_asks(events: &[NormalizedEvent]) -> Vec<Ask> {
    let mut ordered: Vec<&NormalizedEvent> = events.iter().collect();
    ordered.sort_by_key(|e| e.sequence);

    let mut out = Vec::new();

    for (i, event) in ordered.iter().enumerate() {
        match &event.payload {
            EventPayload::UserMessage { content } => {
                for sentence in split_sentences(content) {
                    if !sentence.contains('?') {
                        continue;
                    }
                    out.push(build_ask(event, sentence, AskedBy::User, &ordered, i));
                }
            }
            EventPayload::AssistantMessage { content, .. } => {
                for line in content.lines() {
                    if !is_flagged_line(line) {
                        continue;
                    }
                    out.push(Ask {
                        question: truncate_chars(line.trim(), QUESTION_TRIM),
                        timestamp: event.timestamp,
                        event_id: event.id.clone(),
                        sequence: event.sequence,
                        asked_by: AskedBy::Assistant,
                        status: AskStatus::FlaggedBackToUser,
                        answer_excerpt: None,
                        pointer: AskPointer {
                            sequence: event.sequence,
                            timestamp: event.timestamp,
                        },
                    });
                }
            }
            _ => {}
        }
    }

    out.sort_by_key(|a| std::cmp::Reverse(a.sequence));
    out
}

fn build_ask(
    event: &NormalizedEvent,
    question_text: &str,
    asked_by: AskedBy,
    ordered: &[&NormalizedEvent],
    index: usize,
) -> Ask {
    let question = truncate_chars(question_text, QUESTION_TRIM);

    match find_answer(ordered, index) {
        Some(candidate) if reads_as_question(candidate.text) => Ask {
            question,
            timestamp: event.timestamp,
            event_id: event.id.clone(),
            sequence: event.sequence,
            asked_by,
            status: AskStatus::FlaggedBackToUser,
            answer_excerpt: Some(truncate_chars(candidate.text, ANSWER_TRIM)),
            pointer: AskPointer {
                sequence: candidate.sequence,
                timestamp: candidate.timestamp,
            },
        },
        Some(candidate) => Ask {
            question,
            timestamp: event.timestamp,
            event_id: event.id.clone(),
            sequence: event.sequence,
            asked_by,
            status: AskStatus::Answered,
            answer_excerpt: Some(truncate_chars(candidate.text, ANSWER_TRIM)),
            pointer: AskPointer {
                sequence: candidate.sequence,
                timestamp: candidate.timestamp,
            },
        },
        None => Ask {
            question,
            timestamp: event.timestamp,
            event_id: event.id.clone(),
            sequence: event.sequence,
            asked_by,
            status: AskStatus::NoReplyYet,
            answer_excerpt: None,
            pointer: AskPointer {
                sequence: event.sequence,
                timestamp: event.timestamp,
            },
        },
    }
}

/// Convenience wrapper over [`find_asks`] for a whole trace.
pub fn find_asks_in_trace(trace: &AgentWorthTrace) -> Vec<Ask> {
    find_asks(&trace.events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{Provenance, ToolCall};
    use chrono::Duration;

    fn trace() -> AgentWorthTrace {
        let prov = Provenance::new("/tmp/asks.jsonl", "claude_code", 10, 1, "fp");
        AgentWorthTrace::new("sess_asks", "claude_code", prov, Utc::now())
    }

    fn push(t: &mut AgentWorthTrace, seq: u64, payload: EventPayload) {
        let ts = t.started_at + Duration::seconds(seq as i64);
        t.events.push(NormalizedEvent::new(seq, ts, payload));
    }

    fn user(content: &str) -> EventPayload {
        EventPayload::UserMessage {
            content: content.to_string(),
        }
    }

    fn assistant(content: &str) -> EventPayload {
        EventPayload::AssistantMessage {
            content: content.to_string(),
            thinking: None,
        }
    }

    #[test]
    fn a_question_with_a_real_answer_is_answered() {
        let mut t = trace();
        push(&mut t, 1, user("Does the fps drop when we hit compaction?"));
        push(
            &mut t,
            2,
            assistant("Yes, frame pacing degrades right after a compaction round lands."),
        );

        let asks = find_asks_in_trace(&t);
        assert_eq!(asks.len(), 1);
        assert_eq!(asks[0].status, AskStatus::Answered);
        assert_eq!(asks[0].asked_by, AskedBy::User);
        assert_eq!(asks[0].pointer.sequence, 2);
        assert!(asks[0]
            .answer_excerpt
            .as_deref()
            .unwrap()
            .contains("frame pacing degrades"));
    }

    #[test]
    fn tool_calls_and_notifications_are_skipped_not_stopping_points() {
        let mut t = trace();
        push(&mut t, 1, user("What broke the build?"));
        push(
            &mut t,
            2,
            EventPayload::ToolCall(ToolCall {
                id: None,
                name: "Bash".to_string(),
                arguments: serde_json::json!({}),
            }),
        );
        push(
            &mut t,
            3,
            EventPayload::Custom {
                kind: "subagent_delegation".to_string(),
                data: serde_json::json!({}),
            },
        );
        push(
            &mut t,
            4,
            assistant("A stale lockfile pinned an incompatible version of the schema crate."),
        );

        let asks = find_asks_in_trace(&t);
        assert_eq!(asks.len(), 1);
        assert_eq!(asks[0].status, AskStatus::Answered);
        assert_eq!(asks[0].pointer.sequence, 4);
    }

    #[test]
    fn a_flag_line_is_always_flagged_back_to_user() {
        let mut t = trace();
        push(
            &mut t,
            1,
            assistant("Ran the migration cleanly.\n⚑ Merge to main now, or wait for review?"),
        );
        push(&mut t, 2, user("go ahead"));

        let asks = find_asks_in_trace(&t);
        assert_eq!(asks.len(), 1);
        assert_eq!(asks[0].asked_by, AskedBy::Assistant);
        assert_eq!(asks[0].status, AskStatus::FlaggedBackToUser);
        assert!(asks[0].answer_excerpt.is_none());
        assert_eq!(asks[0].pointer.sequence, 1);
        assert!(asks[0].question.starts_with('⚑'));
    }

    #[test]
    fn the_alternate_flag_glyph_is_also_recognized() {
        let mut t = trace();
        push(&mut t, 1, assistant("🚩 Ship the risky migration tonight?"));
        let asks = find_asks_in_trace(&t);
        assert_eq!(asks.len(), 1);
        assert_eq!(asks[0].asked_by, AskedBy::Assistant);
    }

    #[test]
    fn a_reply_that_is_itself_a_question_is_flagged_back() {
        let mut t = trace();
        push(&mut t, 1, user("Should I rebase or merge this branch?"));
        push(
            &mut t,
            2,
            assistant("Depends on the history you want kept -- do you care about a linear log?"),
        );

        let asks = find_asks_in_trace(&t);
        assert_eq!(asks.len(), 1);
        assert_eq!(asks[0].status, AskStatus::FlaggedBackToUser);
        assert_eq!(asks[0].asked_by, AskedBy::User);
        assert!(asks[0].answer_excerpt.is_some());
    }

    #[test]
    fn no_assistant_text_before_the_next_user_turn_is_no_reply_yet() {
        let mut t = trace();
        push(&mut t, 1, user("Is the flaky test fixed yet?"));
        // No question mark -- this message must not itself become an ask; it exists only to
        // end the scan before any assistant text appears.
        push(&mut t, 2, user("never mind, I'll check myself"));

        let asks = find_asks_in_trace(&t);
        assert_eq!(asks.len(), 1, "the second message has no '?' and isn't a question");
        assert_eq!(asks[0].status, AskStatus::NoReplyYet);
        assert!(asks[0].answer_excerpt.is_none());
        assert_eq!(asks[0].pointer.sequence, 1, "points at itself with nowhere else to jump");
    }

    #[test]
    fn no_assistant_text_before_the_trace_ends_is_also_no_reply_yet() {
        let mut t = trace();
        push(&mut t, 1, user("Did the release script actually run?"));

        let asks = find_asks_in_trace(&t);
        assert_eq!(asks.len(), 1);
        assert_eq!(asks[0].status, AskStatus::NoReplyYet);
    }

    #[test]
    fn filler_and_short_lines_are_skipped_in_favor_of_the_real_answer() {
        let mut t = trace();
        push(&mut t, 1, user("Which adapter recovers best from failure?"));
        push(&mut t, 2, assistant("One moment."));
        push(&mut t, 3, assistant("ok"));
        push(
            &mut t,
            4,
            assistant("Codex recovers in the fewest steps across the sample measured so far."),
        );

        let asks = find_asks_in_trace(&t);
        assert_eq!(asks.len(), 1);
        assert_eq!(asks[0].status, AskStatus::Answered);
        assert_eq!(asks[0].pointer.sequence, 4);
    }

    /// The length gate alone would let a padded filler line through once it crosses 20
    /// characters (real filler phrases are all short); the regex is what still catches it.
    #[test]
    fn a_padded_filler_line_is_still_recognized_past_the_length_gate() {
        let mut t = trace();
        push(&mut t, 1, user("Is the release script done running?"));
        push(&mut t, 2, assistant("Just a moment.................."));
        push(
            &mut t,
            3,
            assistant("Done -- it finished with exit code 0, no errors reported."),
        );

        let asks = find_asks_in_trace(&t);
        assert_eq!(asks.len(), 1);
        assert_eq!(asks[0].status, AskStatus::Answered);
        assert_eq!(asks[0].pointer.sequence, 3, "the padded filler line must not count as the answer");
    }

    #[test]
    fn question_and_answer_text_are_trimmed() {
        let mut t = trace();
        let long_question = format!("Is this fine {}?", "x".repeat(200));
        let long_answer = "y".repeat(400);
        push(&mut t, 1, user(&long_question));
        push(&mut t, 2, assistant(&long_answer));

        let asks = find_asks_in_trace(&t);
        assert_eq!(asks[0].question.chars().count(), QUESTION_TRIM);
        assert!(asks[0].question.ends_with(".."));
        let excerpt = asks[0].answer_excerpt.as_deref().unwrap();
        assert_eq!(excerpt.chars().count(), ANSWER_TRIM);
    }

    #[test]
    fn results_come_back_newest_first() {
        let mut t = trace();
        push(&mut t, 1, user("First question here?"));
        push(&mut t, 2, assistant("Answering the first one now with real content."));
        push(&mut t, 3, user("Second question here?"));
        push(&mut t, 4, assistant("Answering the second one now with real content."));

        let asks = find_asks_in_trace(&t);
        assert_eq!(asks.len(), 2);
        assert_eq!(asks[0].sequence, 3);
        assert_eq!(asks[1].sequence, 1);
    }

    #[test]
    fn multiple_questions_in_one_user_turn_each_become_an_ask() {
        let mut t = trace();
        push(
            &mut t,
            1,
            user("Did the build pass? And did the deploy also succeed?"),
        );
        push(&mut t, 2, assistant("Both passed cleanly, no issues found."));

        let asks = find_asks_in_trace(&t);
        assert_eq!(asks.len(), 2);
    }
}
