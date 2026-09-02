//! Compaction round boundaries: which slice of a session each round threw away.
//!
//! `compaction_count` and `compaction_tokens_dropped` (#62) say a session compacted and how
//! much it cost. Neither says *where*, and `docs/specs/compaction-diff.md` needs exactly that:
//! the span of events a round dropped, so the decisions in it can be diffed against the
//! summary that replaced them without re-reading a 68 MB JSONL on every call.
//!
//! Derived here rather than in the adapters because the shape is the same in every harness
//! that has the concept: a boundary event, a summary event right after it, and everything
//! since the last round in between. An adapter's only job stays recognising its own markers
//! and emitting `EventPayload::Compaction`.

use serde::{Deserialize, Serialize};

use crate::event::{EventPayload, NormalizedEvent};
use crate::trace::AgentWorthTrace;

/// The `Custom` event kind Claude Code's compaction summary is normalized to (see
/// `crates/adapters/src/claude.rs`, the `isCompactSummary` arm). Kept as a shared constant so
/// the writer here and the extractor in `agentworth-outcomes` cannot drift on the string.
pub const COMPACT_SUMMARY_KIND: &str = "compact_summary";

/// How far past a boundary to look for the summary that replaced the conversation. Claude Code
/// writes it as the very next record; the window exists so a harness that interleaves one
/// bookkeeping event between the two still resolves, and a harness that writes no summary at
/// all resolves to `None` rather than to some unrelated later event.
const SUMMARY_LOOKAHEAD_EVENTS: usize = 4;

/// One compaction round, as the boundaries the diff needs.
///
/// `start_seq..=end_seq` is the span the round dropped: everything from the end of the previous
/// round (or the start of the session) up to the event before the boundary. The span is
/// inclusive, and it is empty -- `start_seq > end_seq` -- when a session compacted with nothing
/// in between, which real sessions do not do but a fixture can.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionRound {
    /// 1-based, in event order.
    pub round: u32,
    pub start_seq: u64,
    pub end_seq: u64,
    /// Sequence of the summary event that replaced the span, when the harness wrote one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_seq: Option<u64>,
    /// Context size before this round, from the harness's own metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_before: Option<u64>,
    /// Context size after it -- the summary, and nothing else.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_tokens: Option<u64>,
}

impl CompactionRound {
    /// Whether the round actually dropped anything.
    pub fn is_empty(&self) -> bool {
        self.start_seq > self.end_seq
    }

    /// Events whose sequence falls in this round's dropped span, in sequence order.
    pub fn dropped_events<'a>(&self, events: &'a [NormalizedEvent]) -> Vec<&'a NormalizedEvent> {
        if self.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<&NormalizedEvent> = events
            .iter()
            .filter(|e| e.sequence >= self.start_seq && e.sequence <= self.end_seq)
            .collect();
        out.sort_by_key(|e| e.sequence);
        out
    }
}

/// Derives every compaction round in a trace, in order.
///
/// Empty for a session that never compacted, which is the overwhelming majority
/// (`docs/specs/compaction.md`: 22 of 543 sessions over 50 KB).
pub fn compaction_rounds(trace: &AgentWorthTrace) -> Vec<CompactionRound> {
    let mut ordered: Vec<&NormalizedEvent> = trace.events.iter().collect();
    ordered.sort_by_key(|e| e.sequence);

    let mut rounds = Vec::new();
    // First sequence not yet claimed by an earlier round. Starts at the session's own first
    // sequence rather than at a hardcoded 1, since adapters differ on whether they number
    // from 0 or 1 and neither is wrong.
    let mut cursor = ordered.first().map_or(1, |e| e.sequence);

    for (i, event) in ordered.iter().enumerate() {
        let EventPayload::Compaction(c) = &event.payload else {
            continue;
        };
        let boundary_seq = event.sequence;
        let summary_seq = ordered
            .iter()
            .skip(i + 1)
            .take(SUMMARY_LOOKAHEAD_EVENTS)
            .find(|e| {
                matches!(&e.payload, EventPayload::Custom { kind, .. } if kind == COMPACT_SUMMARY_KIND)
            })
            .map(|e| e.sequence);

        rounds.push(CompactionRound {
            round: rounds.len() as u32 + 1,
            start_seq: cursor,
            end_seq: boundary_seq.saturating_sub(1),
            summary_seq,
            tokens_before: c.pre_tokens,
            summary_tokens: c.post_tokens,
        });

        cursor = summary_seq.unwrap_or(boundary_seq).saturating_add(1);
    }

    rounds
}

/// The text of a compaction summary event, or `None` if this is not one.
///
/// The summary is stored as the harness's raw record under `EventPayload::Custom`, so the text
/// has to be dug back out of it. Both content shapes Claude Code writes are handled: a plain
/// string, and the block array a normal message carries.
pub fn compact_summary_text(payload: &EventPayload) -> Option<String> {
    let EventPayload::Custom { kind, data } = payload else {
        return None;
    };
    if kind != COMPACT_SUMMARY_KIND {
        return None;
    }
    let content = data
        .get("message")
        .and_then(|m| m.get("content"))
        .or_else(|| data.get("content"))?;

    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }
    let blocks = content.as_array()?;
    let joined = blocks
        .iter()
        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    Some(joined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::CompactionEvent;
    use crate::provenance::Provenance;
    use chrono::{Duration, Utc};
    use serde_json::json;

    fn trace_with(events: Vec<NormalizedEvent>) -> AgentWorthTrace {
        let prov = Provenance::new("/tmp/t.jsonl", "claude_code", 1, 1, "fp");
        let mut trace = AgentWorthTrace::new("s1", "claude_code", prov, Utc::now());
        trace.events = events;
        trace
    }

    fn assistant(seq: u64) -> NormalizedEvent {
        NormalizedEvent::new(
            seq,
            Utc::now() + Duration::seconds(seq as i64),
            EventPayload::AssistantMessage {
                content: format!("turn {seq}"),
                thinking: None,
            },
        )
    }

    fn boundary(seq: u64, pre: u64, post: u64) -> NormalizedEvent {
        NormalizedEvent::new(
            seq,
            Utc::now() + Duration::seconds(seq as i64),
            EventPayload::Compaction(CompactionEvent {
                trigger: "manual".to_string(),
                pre_tokens: Some(pre),
                post_tokens: Some(post),
                dropped_tokens: Some(pre - post),
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
                data: json!({"message": {"role": "user", "content": text}}),
            },
        )
    }

    #[test]
    fn no_compaction_is_no_rounds() {
        let trace = trace_with(vec![assistant(1), assistant(2)]);
        assert!(compaction_rounds(&trace).is_empty());
    }

    #[test]
    fn two_rounds_split_the_session_at_their_summaries() {
        let trace = trace_with(vec![
            assistant(1),
            assistant(2),
            boundary(3, 700_000, 20_000),
            summary(4, "round one summary"),
            assistant(5),
            boundary(6, 800_000, 25_000),
            summary(7, "round two summary"),
            assistant(8),
        ]);

        let rounds = compaction_rounds(&trace);
        assert_eq!(rounds.len(), 2);

        assert_eq!(rounds[0].round, 1);
        assert_eq!((rounds[0].start_seq, rounds[0].end_seq), (1, 2));
        assert_eq!(rounds[0].summary_seq, Some(4));
        assert_eq!(rounds[0].tokens_before, Some(700_000));
        assert_eq!(rounds[0].summary_tokens, Some(20_000));

        // Round 2's span starts after round 1's summary, not after its boundary: the summary
        // itself was never part of the conversation round 2 dropped.
        assert_eq!(rounds[1].round, 2);
        assert_eq!((rounds[1].start_seq, rounds[1].end_seq), (5, 5));
        assert_eq!(rounds[1].summary_seq, Some(7));
    }

    #[test]
    fn a_boundary_with_no_summary_still_bounds_a_round() {
        let trace = trace_with(vec![assistant(1), assistant(2), boundary(3, 100, 10)]);
        let rounds = compaction_rounds(&trace);
        assert_eq!(rounds.len(), 1);
        assert_eq!(rounds[0].summary_seq, None);
        assert_eq!((rounds[0].start_seq, rounds[0].end_seq), (1, 2));
    }

    #[test]
    fn dropped_events_are_the_span_and_nothing_else() {
        let trace = trace_with(vec![
            assistant(1),
            assistant(2),
            boundary(3, 100, 10),
            summary(4, "s"),
            assistant(5),
        ]);
        let rounds = compaction_rounds(&trace);
        let dropped = rounds[0].dropped_events(&trace.events);
        assert_eq!(
            dropped.iter().map(|e| e.sequence).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn summary_text_reads_both_content_shapes() {
        let plain = summary(1, "the summary");
        assert_eq!(
            compact_summary_text(&plain.payload).as_deref(),
            Some("the summary")
        );

        let blocks = NormalizedEvent::new(
            2,
            Utc::now(),
            EventPayload::Custom {
                kind: COMPACT_SUMMARY_KIND.to_string(),
                data: json!({"message": {"content": [
                    {"type": "text", "text": "first"},
                    {"type": "thinking", "thinking": "ignored"},
                    {"type": "text", "text": "second"}
                ]}}),
            },
        );
        assert_eq!(
            compact_summary_text(&blocks.payload).as_deref(),
            Some("first\nsecond")
        );

        assert!(compact_summary_text(&assistant(3).payload).is_none());
    }
}
