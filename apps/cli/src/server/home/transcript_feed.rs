//! Turns a harness transcript into `speech`/`work`/`stop` frames, replacing the `herdr agent
//! read` terminal-snapshot diff heuristic the first `home-gateway` lane shipped with (see that
//! module's doc comment on `run_prompt_and_reply` for why it was low-reliability).
//!
//! The source of truth is the same one `agentworth scan`/`Scanner::load_trace` already use:
//! the harness's own transcript file, parsed through the session's adapter into a full
//! `AgentWorthTrace`. This module never re-implements parsing -- it re-parses the whole trace
//! on every change (bounded by however large that session's transcript already is; see the
//! report this lane shipped with for the accepted cost/latency tradeoff) and diffs the new
//! trace's events against a per-session cursor to find what's new since the last read.
//!
//! Mapping a session to a rider: see `gateway.rs`'s `pane_for_session` -- the actual join used
//! is `agent_state.pane_id` (set by the Claude Code hook loop at session start,
//! `apps/cli/src/loop_runtime.rs`), matched against `herdr agent list`'s own `pane_id` field.
//! NOT CONFIRMED: the brief for this lane named `agent_session.value` as the field to join on;
//! no such field exists anywhere in this codebase or in the herdr CLI output this module reads
//! (`HerdrAgent`, `apps/cli/src/server/home/protocol.rs`), and there is no live herdr instance
//! in this sandbox to check its actual JSON shape against. `agent_state.pane_id` is the closest
//! verified join this codebase already relies on for the same purpose (`trace_anchors`'
//! `pane_id` anchor kind, `docs/specs/loop.md` section 2).

use agentworth_outcomes::{outcome_rank, OutcomeDetector};
use agentworth_schema::{AgentWorthTrace, EventPayload, EventType, NormalizedEvent};
use chrono::{DateTime, Utc};

use super::protocol::{new_id, Message, MessageKind, Rung};

/// Per-session read position: how far into the trace's events this feed has already turned
/// into frames, and the highest outcome rank already reported as a `stop` (0 = none yet, so a
/// session's very first `done_claimed` still crosses the "increased" threshold).
#[derive(Debug, Clone, Copy, Default)]
pub struct SessionCursor {
    pub last_sequence: u64,
    pub last_outcome_rank: u8,
}

/// One session's newly-observed activity: zero or more chat-stream messages, in order, and at
/// most one outcome crossing (the caller decides whether/where to route it as a `Stop`, since
/// that needs a direction id this module has no way to know).
pub struct FeedOutput {
    pub messages: Vec<Message>,
    /// `(rung, evidence summary)` when this trace's strongest outcome just increased past what
    /// `cursor.last_outcome_rank` had already reported.
    pub new_outcome: Option<(Rung, String)>,
}

/// Counts collapsed from one run of tool-shaped events between two chat turns, per the job
/// brief's example: "edited 3 files, ran 2 commands".
#[derive(Debug, Default)]
struct WorkRun {
    files_touched: usize,
    commands_run: usize,
    other_tools: usize,
    last_timestamp: Option<DateTime<Utc>>,
}

impl WorkRun {
    fn is_empty(&self) -> bool {
        self.files_touched == 0 && self.commands_run == 0 && self.other_tools == 0
    }

    fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.files_touched > 0 {
            parts.push(format!(
                "edited {} file{}",
                self.files_touched,
                if self.files_touched == 1 { "" } else { "s" }
            ));
        }
        if self.commands_run > 0 {
            parts.push(format!(
                "ran {} command{}",
                self.commands_run,
                if self.commands_run == 1 { "" } else { "s" }
            ));
        }
        if parts.is_empty() && self.other_tools > 0 {
            parts.push(format!(
                "used {} tool{}",
                self.other_tools,
                if self.other_tools == 1 { "" } else { "s" }
            ));
        }
        parts.join(", ")
    }
}

fn is_work_event(payload: &EventPayload) -> bool {
    matches!(
        payload.event_type(),
        EventType::ToolCall | EventType::ToolResult | EventType::ShellCommand | EventType::FileAction
    )
}

fn fold_into_run(run: &mut WorkRun, event: &NormalizedEvent) {
    run.last_timestamp = Some(event.timestamp);
    match &event.payload {
        EventPayload::FileAction { .. } => run.files_touched += 1,
        EventPayload::ShellCommand(_) => run.commands_run += 1,
        EventPayload::ToolCall(tool) => {
            let name = tool.name.to_lowercase();
            if name.contains("bash") || name.contains("shell") || name.contains("exec") {
                run.commands_run += 1;
            } else if name.contains("edit") || name.contains("write") || name.contains("patch") {
                run.files_touched += 1;
            } else {
                run.other_tools += 1;
            }
        }
        // ToolResult carries no new information this summary needs -- the matching ToolCall
        // already counted it.
        EventPayload::ToolResult(_) => {}
        _ => {}
    }
}

/// Builds the frames for events newly appended to `trace` since `cursor`, and advances
/// `cursor` in place. `space_id` and `from_persona` are the rider's own office/space and
/// persona id -- both resolved by the caller (`gateway.rs`) before calling this, since this
/// module has no notion of personas.
pub fn feed_from_trace(
    trace: &AgentWorthTrace,
    cursor: &mut SessionCursor,
    space_id: &str,
    from_persona: &str,
) -> FeedOutput {
    let mut messages = Vec::new();
    let mut run = WorkRun::default();

    let mut flush_run = |run: &mut WorkRun, messages: &mut Vec<Message>| {
        if run.is_empty() {
            return;
        }
        messages.push(Message {
            id: new_id(),
            space_id: space_id.to_string(),
            from: from_persona.to_string(),
            kind: MessageKind::Work,
            text: run.summary(),
            at: run
                .last_timestamp
                .unwrap_or_else(Utc::now)
                .to_rfc3339(),
            artifacts: None,
            mentions: None,
        });
        *run = WorkRun::default();
    };

    for event in trace.events.iter().filter(|e| e.sequence > cursor.last_sequence) {
        match &event.payload {
            EventPayload::AssistantMessage { content, .. } => {
                flush_run(&mut run, &mut messages);
                if !content.trim().is_empty() {
                    messages.push(Message {
                        id: new_id(),
                        space_id: space_id.to_string(),
                        from: from_persona.to_string(),
                        kind: MessageKind::Speech,
                        text: content.clone(),
                        at: event.timestamp.to_rfc3339(),
                        artifacts: None,
                        mentions: None,
                    });
                }
            }
            payload if is_work_event(payload) => fold_into_run(&mut run, event),
            _ => flush_run(&mut run, &mut messages),
        }
        cursor.last_sequence = cursor.last_sequence.max(event.sequence);
    }
    flush_run(&mut run, &mut messages);

    let detector = OutcomeDetector::new();
    let outcomes = detector.detect_outcomes(trace);
    let new_outcome = detector.strongest_outcome(&outcomes).and_then(|evidence| {
        let rank = outcome_rank(evidence.kind);
        if rank > cursor.last_outcome_rank {
            cursor.last_outcome_rank = rank;
            Rung::from_outcome_rank(rank).map(|rung| (rung, evidence.summary.clone()))
        } else {
            None
        }
    });

    FeedOutput { messages, new_outcome }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_adapter_sdk::{AgentAdapter, SessionSource};
    use agentworth_adapters::ClaudeCodeAdapter;
    use std::io::Write;

    fn adapter() -> ClaudeCodeAdapter {
        ClaudeCodeAdapter::new()
    }

    /// Writes a minimal Claude Code transcript (same shape as the fixture already in
    /// `crates/adapters/src/claude.rs`'s own tests) and parses it through the real adapter, so
    /// this module is tested against the same event shapes production sees.
    fn parse_fixture(lines: &[&str]) -> AgentWorthTrace {
        let mut file = tempfile::NamedTempFile::new().expect("tempfile");
        for line in lines {
            writeln!(file, "{line}").expect("write fixture line");
        }
        let path = file.path().to_path_buf();
        let source = SessionSource::from_path(&path, "claude_code").expect("session source");
        adapter().parse(&source).expect("parse fixture").trace
    }

    #[test]
    fn first_pass_emits_speech_and_collapsed_work_then_advances_cursor() {
        let trace = parse_fixture(&[
            r#"{"type":"user","timestamp":"2026-09-07T10:00:00Z","content":"Fix the bug in src/main.rs"}"#,
            r#"{"type":"assistant","timestamp":"2026-09-07T10:00:05Z","model":"claude-3-5-sonnet-20241022","usage":{"input_tokens":500,"output_tokens":120,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"I will check the file."},{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"cargo test"}}]}"#,
            r#"{"type":"tool_result","timestamp":"2026-09-07T10:00:07Z","tool_use_id":"toolu_1","content":"test result: ok. 4 passed; 0 failed","is_error":false}"#,
            r#"{"type":"assistant","timestamp":"2026-09-07T10:00:10Z","content":[{"type":"text","text":"All tests pass now!"}]}"#,
        ]);

        let mut cursor = SessionCursor::default();
        let out = feed_from_trace(&trace, &mut cursor, "office-harvey", "harvey");

        let kinds: Vec<(MessageKind, &str)> = out
            .messages
            .iter()
            .map(|m| (m.kind, m.text.as_str()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (MessageKind::Speech, "I will check the file."),
                (MessageKind::Speech, "All tests pass now!"),
            ],
            "assistant text becomes speech; the bash tool call between them has no matching \
             ToolResult boundary before the second AssistantMessage, so it folds into a work \
             run flushed at that boundary -- asserted separately below"
        );
        assert!(cursor.last_sequence >= 3, "cursor advanced to the last event's sequence");

        // A second call with the same trace and the now-advanced cursor sees nothing new.
        let again = feed_from_trace(&trace, &mut cursor, "office-harvey", "harvey");
        assert!(again.messages.is_empty(), "no new events -> no new messages");
        assert!(again.new_outcome.is_none(), "outcome rank did not increase on the second pass");
    }

    #[test]
    fn work_run_collapses_multiple_tool_calls_into_one_message() {
        let trace = parse_fixture(&[
            r#"{"type":"assistant","timestamp":"2026-09-07T10:00:00Z","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"src/a.rs","old_string":"a","new_string":"b"}}]}"#,
            r#"{"type":"assistant","timestamp":"2026-09-07T10:00:01Z","content":[{"type":"tool_use","id":"t2","name":"Write","input":{"file_path":"src/b.rs","content":"fn b(){}"}}]}"#,
            r#"{"type":"assistant","timestamp":"2026-09-07T10:00:02Z","content":[{"type":"tool_use","id":"t3","name":"Bash","input":{"command":"cargo test"}}]}"#,
            r#"{"type":"assistant","timestamp":"2026-09-07T10:00:03Z","content":[{"type":"text","text":"done"}]}"#,
        ]);

        let mut cursor = SessionCursor::default();
        let out = feed_from_trace(&trace, &mut cursor, "office-harvey", "harvey");

        let work: Vec<&Message> = out.messages.iter().filter(|m| m.kind == MessageKind::Work).collect();
        assert_eq!(work.len(), 1, "one collapsed work message, not one per tool call");
        assert_eq!(work[0].text, "edited 2 files, ran 1 command");

        let speech: Vec<&Message> = out.messages.iter().filter(|m| m.kind == MessageKind::Speech).collect();
        assert_eq!(speech.len(), 1);
        assert_eq!(speech[0].text, "done");
    }

    #[test]
    fn new_test_pass_outcome_emits_a_stop_at_the_test_rung() {
        let trace = parse_fixture(&[
            r#"{"type":"assistant","timestamp":"2026-09-07T10:00:00Z","message":{"content":[{"type":"tool_use","id":"toolu_c","name":"Bash","input":{"command":"cargo test"}}]}}"#,
            r#"{"type":"user","timestamp":"2026-09-07T10:00:20Z","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_c","is_error":false,"content":[{"type":"text","text":"test result: ok. 12 passed; 0 failed"}]}]},"toolUseResult":{"stdout":"test result: ok. 12 passed; 0 failed","stderr":"","interrupted":false,"isImage":false,"noOutputExpected":false}}"#,
        ]);

        let mut cursor = SessionCursor::default();
        let out = feed_from_trace(&trace, &mut cursor, "office-harvey", "harvey");
        let (rung, _summary) = out.new_outcome.expect("a passing test run is outcome evidence");
        assert_eq!(rung, Rung::Test);
        assert_eq!(cursor.last_outcome_rank, 3);

        // Re-running against the same trace does not re-report the same rung.
        let again = feed_from_trace(&trace, &mut cursor, "office-harvey", "harvey");
        assert!(again.new_outcome.is_none());
    }
}
