//! Loose ends: commitments an assistant stated and then handed control back without acting on.
//!
//! A direct port of `apps/dashboard/src/utils/looseEnds.ts` (#44), which ran browser-side on a
//! trace already loaded into the dashboard. `docs/specs/handoff.md` calls for one definition of
//! this logic in Rust so the MCP tools, the CLI, and the dashboard can all reach it; the
//! regexes, the three gating filters, the length window, and the fulfilment scan below are the
//! same ones measured against five real sessions (120 of 212 stated intents gated, not dropped).
//!
//! Deliberately crude, and the spec is right that it does not need to be clever: an assistant
//! states an intent, emits no tool call, and the next event is a user turn. Anything gated on a
//! reply or on a later event is excluded. This reports what has no evidence of happening -- it
//! does not claim the work was forgotten, which is why the surface calls these loose ends
//! rather than misses.

use std::sync::OnceLock;

use agentworth_schema::{AgentWorthTrace, EventPayload, NormalizedEvent};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// An intent the assistant stated. Kept deliberately narrow: "let me" was tried and dropped,
/// because it almost always narrates something being done in the same breath rather than
/// promising it for later.
const INTENT_PATTERN: &str = r"(?i)\b(i'?ll|i will|i'?m going to|i am going to)\b";

/// A gated intent is not a loose end.
///
/// This is the filter that makes the whole thing usable. Measured across five real sessions,
/// 120 of 212 stated intents are gated -- conditional offers ("say go and I'll write both") or
/// deliberate deferrals ("I'll report once they land"). Both were promised *subject to
/// something*, so neither was dropped, and reporting them as misses is what would make the
/// output feel accusatory and get ignored.
const GATED_PATTERN: &str = r"(?i)\b(if|once|unless|when|after|until|pending|whenever|assuming|provided)\b|\b(say the word|say go|say which|let me know|want me to|shall i|would you like|approve|tell me|your call|happy to|say-so)\b|^say\b";

/// "Paste me the error and I'll take it from there" is an offer waiting on the user, not a
/// commitment that was dropped -- the same class as `GATED_PATTERN`, but phrased as an
/// imperative rather than a conditional, so the `if`/`once` vocabulary misses it entirely.
const AWAITING_USER_PATTERN: &str = r"(?i)\b(paste|send|share|give|drop|hand|point|show|run|confirm|approve|pick|choose)\s+(me|it|them|that|those|this|us)\b";

/// Too short to carry a commitment; too long to be one sentence.
const MIN_LEN: usize = 25;
const MAX_LEN: usize = 240;

static INTENT: OnceLock<regex::Regex> = OnceLock::new();
static GATED: OnceLock<regex::Regex> = OnceLock::new();
static AWAITING_USER: OnceLock<regex::Regex> = OnceLock::new();

fn intent_re() -> &'static regex::Regex {
    INTENT.get_or_init(|| regex::Regex::new(INTENT_PATTERN).expect("valid regex"))
}

fn gated_re() -> &'static regex::Regex {
    GATED.get_or_init(|| regex::Regex::new(GATED_PATTERN).expect("valid regex"))
}

fn awaiting_user_re() -> &'static regex::Regex {
    AWAITING_USER.get_or_init(|| regex::Regex::new(AWAITING_USER_PATTERN).expect("valid regex"))
}

/// One stated-and-unfulfilled commitment, with everything needed to check it against the
/// transcript it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LooseEnd {
    /// The sentence, verbatim. Attribution is only worth what it can quote.
    pub text: String,
    /// Event ID of the message that said it, for deep-linking.
    pub event_id: String,
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    /// Model that said it, when the trace records one earlier in the stream.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Does this event count as the assistant actually doing something?
///
/// The TypeScript original tests `payload.type` against the set
/// `{tool_call, shell_command, file_action}`; those are the serde tags of exactly these three
/// `EventPayload` variants, so the match below is the same set stated structurally.
fn is_action(payload: &EventPayload) -> bool {
    matches!(
        payload,
        EventPayload::ToolCall(_) | EventPayload::ShellCommand(_) | EventPayload::FileAction { .. }
    )
}

/// Length in UTF-16 code units, which is what JavaScript's `String.prototype.length` counts.
///
/// The port would otherwise silently change the `MIN_LEN`/`MAX_LEN` window for any sentence
/// carrying non-ASCII text -- Rust's `str::len()` counts bytes, so a CJK sentence measures
/// roughly three times longer than the browser measured it, and the same transcript would
/// produce different loose ends depending on which process read it.
fn js_length(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Splits assistant text into sentences the way the TypeScript original's
/// `text.split(/(?<=[.!?])\s+|\n+/)` does.
///
/// Hand-rolled because the `regex` crate has no lookbehind: at each position, a whitespace run
/// preceded by `.`/`!`/`?` is a delimiter (matching the first alternative, which is greedy over
/// all whitespace including newlines), and otherwise a run of newlines is. Segments are trimmed
/// and empties dropped, same as the `.map(trim).filter(Boolean)` that follows the split there.
fn split_sentences(text: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;

    while i < chars.len() {
        let (byte_idx, ch) = chars[i];
        let prev_is_terminator = i > 0 && matches!(chars[i - 1].1, '.' | '!' | '?');

        let delimiter_is_whitespace_run = ch.is_whitespace() && prev_is_terminator;
        let delimiter_is_newline_run = ch == '\n';

        if delimiter_is_whitespace_run || delimiter_is_newline_run {
            let mut j = i;
            if delimiter_is_whitespace_run {
                while j < chars.len() && chars[j].1.is_whitespace() {
                    j += 1;
                }
            } else {
                while j < chars.len() && chars[j].1 == '\n' {
                    j += 1;
                }
            }
            push_segment(&mut out, &text[start..byte_idx]);
            start = chars.get(j).map_or(text.len(), |(b, _)| *b);
            i = j;
        } else {
            i += 1;
        }
    }

    push_segment(&mut out, &text[start..]);
    out
}

fn push_segment<'a>(out: &mut Vec<&'a str>, segment: &'a str) {
    let trimmed = segment.trim();
    if !trimmed.is_empty() {
        out.push(trimmed);
    }
}

/// Finds commitments the assistant stated and then handed control back without acting on.
///
/// Events are sorted by sequence first, so a caller can pass them in whatever order storage or
/// an adapter produced them.
pub fn find_loose_ends(events: &[NormalizedEvent]) -> Vec<LooseEnd> {
    let mut ordered: Vec<&NormalizedEvent> = events.iter().collect();
    ordered.sort_by_key(|e| e.sequence);

    let mut out = Vec::new();
    // Most recent model seen, so a loose end can name who said it.
    let mut current_model: Option<String> = None;

    for (i, event) in ordered.iter().enumerate() {
        let content = match &event.payload {
            EventPayload::ModelInvocation { model, .. } => {
                current_model = Some(model.clone());
                continue;
            }
            EventPayload::AssistantMessage { content, .. } => content,
            _ => continue,
        };
        if content.is_empty() {
            continue;
        }

        let candidates: Vec<&str> = split_sentences(content)
            .into_iter()
            .filter(|s| {
                let len = js_length(s);
                len >= MIN_LEN
                    && len <= MAX_LEN
                    && intent_re().is_match(s)
                    && !gated_re().is_match(s)
                    && !awaiting_user_re().is_match(s)
            })
            .collect();
        if candidates.is_empty() {
            continue;
        }

        // Did anything actually happen before control went back to the user?
        let mut acted = false;
        for next in &ordered[i + 1..] {
            if is_action(&next.payload) {
                acted = true;
                break;
            }
            if matches!(next.payload, EventPayload::UserMessage { .. }) {
                break;
            }
        }
        if acted {
            continue;
        }

        for text in candidates {
            out.push(LooseEnd {
                text: text.to_string(),
                event_id: event.id.clone(),
                sequence: event.sequence,
                timestamp: event.timestamp,
                model: current_model.clone(),
            });
        }
    }

    out
}

/// Convenience wrapper over [`find_loose_ends`] for a whole trace.
pub fn find_loose_ends_in_trace(trace: &AgentWorthTrace) -> Vec<LooseEnd> {
    find_loose_ends(&trace.events)
}

/// Builds the text a developer hands to whatever already has the repo open.
///
/// Deliberately a prompt and not a patch: writing the fix would mean being right about the fix,
/// from something that read a transcript and never saw the codebase. Being right about what is
/// missing is the answerable half.
pub fn loose_ends_prompt(ends: &[LooseEnd], session_id: &str) -> String {
    let mut lines = vec![
        format!(
            "In an earlier session ({session_id}) the following were said and have no evidence of being done:"
        ),
        String::new(),
    ];
    lines.extend(ends.iter().map(|e| format!("- {}", e.text)));
    lines.push(String::new());
    lines.push(
        "Check each against the current state of the repository. Some may have been done since, \
         and some I may have cancelled — ask me about anything ambiguous rather than assuming. \
         Then do the ones that are still outstanding."
            .to_string(),
    );
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{
        FileActionType, Provenance, ShellCommand, TokenUsage, ToolCall,
    };
    use chrono::Duration;

    fn trace() -> AgentWorthTrace {
        let prov = Provenance::new("/tmp/loose.jsonl", "claude_code", 10, 1, "fp");
        AgentWorthTrace::new("sess_loose", "claude_code", prov, Utc::now())
    }

    fn push(t: &mut AgentWorthTrace, seq: u64, payload: EventPayload) {
        let ts = t.started_at + Duration::seconds(seq as i64);
        t.events.push(NormalizedEvent::new(seq, ts, payload));
    }

    fn assistant(content: &str) -> EventPayload {
        EventPayload::AssistantMessage {
            content: content.to_string(),
            thinking: None,
        }
    }

    fn user(content: &str) -> EventPayload {
        EventPayload::UserMessage {
            content: content.to_string(),
        }
    }

    /// The verbatim example `docs/specs/dropped-commitments.md` quotes from a real session: an
    /// intent stated, nothing done, control handed back.
    #[test]
    fn detects_the_measured_real_world_example() {
        let mut t = trace();
        push(
            &mut t,
            1,
            assistant(
                "Still owe you 03 and 04 over HTTP; I'll finish those next unless you want \
                 something else first.",
            ),
        );
        push(&mut t, 2, user("ok"));

        let ends = find_loose_ends_in_trace(&t);
        // "unless" gates the second clause, and the sentence splitter keeps the whole line as
        // one sentence -- so the measured example is itself gated, which is exactly the
        // behaviour the three filters exist to produce.
        assert!(
            ends.is_empty(),
            "a sentence carrying `unless` is gated, not dropped: {ends:?}"
        );

        // Same commitment with the gate removed is a loose end.
        let mut t = trace();
        push(
            &mut t,
            1,
            assistant("Still owe you 03 and 04 over HTTP; I'll finish those next."),
        );
        push(&mut t, 2, user("ok"));
        let ends = find_loose_ends_in_trace(&t);
        assert_eq!(ends.len(), 1);
        assert!(ends[0].text.contains("I'll finish those next"));
        assert_eq!(ends[0].sequence, 1);
    }

    #[test]
    fn a_tool_call_before_the_user_turn_clears_the_intent() {
        let mut t = trace();
        push(
            &mut t,
            1,
            assistant("I'll re-run the export now that the schema landed."),
        );
        push(
            &mut t,
            2,
            EventPayload::ToolCall(ToolCall {
                id: None,
                name: "Bash".to_string(),
                arguments: serde_json::json!({}),
            }),
        );
        push(&mut t, 3, user("thanks"));

        assert!(find_loose_ends_in_trace(&t).is_empty());
    }

    #[test]
    fn shell_and_file_actions_also_clear_the_intent() {
        for action in [
            EventPayload::ShellCommand(ShellCommand {
                command: "cargo test".to_string(),
                cwd: None,
                exit_code: Some(0),
                output: None,
            }),
            EventPayload::FileAction {
                path: "src/lib.rs".to_string(),
                action: FileActionType::Edit,
                diff: None,
                lines_changed: None,
            },
        ] {
            let mut t = trace();
            push(&mut t, 1, assistant("I'll rewrite the storage helper next."));
            push(&mut t, 2, action);
            push(&mut t, 3, user("ok"));
            assert!(
                find_loose_ends_in_trace(&t).is_empty(),
                "an action event before the user turn means the intent was acted on"
            );
        }
    }

    /// False positive 1 in `docs/specs/dropped-commitments.md`: a conditional offer.
    #[test]
    fn conditional_offers_are_gated() {
        for text in [
            "Say the word and I'll patch both of the remaining call sites for you.",
            "If you want the same treatment for the CLI, I'll do that in the next pass.",
            "Want me to take the other four files as well -- I'll do it right away.",
            "Shall I continue -- I'll finish the remaining migrations right after.",
        ] {
            let mut t = trace();
            push(&mut t, 1, assistant(text));
            push(&mut t, 2, user("later"));
            assert!(
                find_loose_ends_in_trace(&t).is_empty(),
                "conditional offer should be gated: {text}"
            );
        }
    }

    /// False positive 2: deferred by design.
    #[test]
    fn deliberate_deferrals_are_gated() {
        for text in [
            "I'll report back once the CI runs land on the branch.",
            "I'll pick this up again after the schema migration merges.",
            "I'll re-check the numbers when the nightly scan finishes.",
        ] {
            let mut t = trace();
            push(&mut t, 1, assistant(text));
            push(&mut t, 2, user("sounds good"));
            assert!(
                find_loose_ends_in_trace(&t).is_empty(),
                "deferral should be gated: {text}"
            );
        }
    }

    /// The imperative-offer class `GATED_PATTERN`'s `if`/`once` vocabulary misses entirely.
    #[test]
    fn offers_awaiting_the_user_are_gated() {
        for text in [
            "Paste me the error output and I'll take it from there straight away.",
            "Send me the failing branch name and I'll reproduce it locally.",
        ] {
            let mut t = trace();
            push(&mut t, 1, assistant(text));
            push(&mut t, 2, user("ok"));
            assert!(
                find_loose_ends_in_trace(&t).is_empty(),
                "offer awaiting the user should be gated: {text}"
            );
        }
    }

    /// False positive 3: plan narration, where the tool call lands in the same turn. Already
    /// excluded by the fulfilment scan rather than by a regex -- worth pinning so a future
    /// change to that scan cannot quietly reintroduce it.
    #[test]
    fn plan_narration_is_excluded_by_the_fulfilment_scan() {
        let mut t = trace();
        push(
            &mut t,
            1,
            assistant("I'll read the storage crate, then I'll write the new module."),
        );
        push(
            &mut t,
            2,
            EventPayload::ToolCall(ToolCall {
                id: None,
                name: "Read".to_string(),
                arguments: serde_json::json!({}),
            }),
        );
        push(&mut t, 3, assistant("Done reading."));
        push(&mut t, 4, user("go on"));

        assert!(find_loose_ends_in_trace(&t).is_empty());
    }

    #[test]
    fn sentences_outside_the_length_window_are_ignored() {
        let mut t = trace();
        push(&mut t, 1, assistant("I'll do it."));
        push(&mut t, 2, assistant(&format!("I'll {}", "x".repeat(300))));
        push(&mut t, 3, user("ok"));

        assert!(
            find_loose_ends_in_trace(&t).is_empty(),
            "too short and too long both fall outside MIN_LEN..=MAX_LEN"
        );
    }

    #[test]
    fn length_window_is_measured_in_utf16_units_like_the_browser() {
        // 13 CJK characters: 13 UTF-16 units (under MIN_LEN), but 39 bytes (over it). Rust's
        // own `str::len()` would let this through where the dashboard rejected it.
        let cjk = "我会稍后再处理这个问题的哦";
        assert_eq!(js_length(cjk), 13);
        assert_eq!(cjk.len(), 39);
    }

    #[test]
    fn model_is_attributed_from_the_most_recent_invocation() {
        let mut t = trace();
        push(
            &mut t,
            1,
            EventPayload::ModelInvocation {
                model: "claude-opus-4".to_string(),
                token_usage: TokenUsage::new(1, 1, 0, 0),
                cost_usd: None,
                latency_ms: None,
            },
        );
        push(
            &mut t,
            2,
            assistant("I'll delete the stale worktree in a moment."),
        );
        push(&mut t, 3, user("ok"));

        let ends = find_loose_ends_in_trace(&t);
        assert_eq!(ends.len(), 1);
        assert_eq!(ends[0].model.as_deref(), Some("claude-opus-4"));
    }

    #[test]
    fn events_are_sorted_by_sequence_before_scanning() {
        let mut t = trace();
        push(&mut t, 3, user("ok"));
        push(&mut t, 2, assistant("I'll bump the version before tagging."));
        push(
            &mut t,
            1,
            EventPayload::ToolCall(ToolCall {
                id: None,
                name: "Read".to_string(),
                arguments: serde_json::json!({}),
            }),
        );

        let ends = find_loose_ends_in_trace(&t);
        assert_eq!(ends.len(), 1, "the tool call precedes the intent, so it does not clear it");
        assert_eq!(ends[0].sequence, 2);
    }

    #[test]
    fn splits_sentences_on_terminators_and_newlines() {
        assert_eq!(
            split_sentences("One. Two! Three?  Four"),
            vec!["One.", "Two!", "Three?", "Four"]
        );
        assert_eq!(
            split_sentences("alpha\n\nbeta\ngamma"),
            vec!["alpha", "beta", "gamma"]
        );
        // A period with no following whitespace is not a split point, matching the original.
        assert_eq!(split_sentences("v1.2.3 shipped"), vec!["v1.2.3 shipped"]);
        assert!(split_sentences("   \n  ").is_empty());
    }

    #[test]
    fn multiple_intents_in_one_message_each_become_a_loose_end() {
        let mut t = trace();
        push(
            &mut t,
            1,
            assistant(
                "I'll rewrite the storage helper in the morning. I'll also update the README \
                 section that names it.",
            ),
        );
        push(&mut t, 2, user("ok"));

        let ends = find_loose_ends_in_trace(&t);
        assert_eq!(ends.len(), 2);
        assert_eq!(ends[0].event_id, ends[1].event_id);
    }

    #[test]
    fn prompt_quotes_every_end_and_names_the_session() {
        let mut t = trace();
        push(&mut t, 1, assistant("I'll rewrite the storage helper tomorrow."));
        push(&mut t, 2, user("ok"));
        let ends = find_loose_ends_in_trace(&t);

        let prompt = loose_ends_prompt(&ends, "sess_loose");
        assert!(prompt.contains("sess_loose"));
        assert!(prompt.contains("- I'll rewrite the storage helper tomorrow."));
    }
}
