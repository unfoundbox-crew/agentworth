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
//! Mapping a session to a rider: see `gateway.rs`'s `HomeRuntime::pane_for_session`. Confirmed
//! live on 2026-09-07 against a real 8-pane herdr fleet: `herdr agent list`'s
//! `agent_session.value` (a field the previous lane could not confirm existed -- it does,
//! `HerdrAgent.agent_session` in `apps/cli/src/server/home/protocol.rs`) matches this index's
//! `sessions.session_id` exactly for claude_code, codex, and antigravity panes, and is now the
//! preferred join. `agent_state.pane_id` (set by the Claude Code hook loop at session start,
//! `apps/cli/src/loop_runtime.rs`, matched against `herdr agent list`'s own `pane_id` field) is
//! the fallback for sessions `agent_session` doesn't resolve -- Grok's adapter names sessions
//! after files ("events", "chat_history") rather than an id herdr recognizes, so a Grok pane's
//! `agent_session.value` never matches either way; that's a gap in Grok's own adapter, not
//! something routed around here.

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

/// `crates/adapters/src/claude.rs` emits a `ToolCall` for every `tool_use` block, and then,
/// for the tool names it specifically recognizes (Bash-family, Edit/Write-family), an
/// additional `ShellCommand`/`FileAction` event for the *same* invocation right after it. So a
/// `ToolCall` only ever counts here as `other_tools` -- counting it toward `commands_run`/
/// `files_touched` as well as the paired `ShellCommand`/`FileAction` would double-count every
/// recognized tool call. An unrecognized tool (no paired event) still shows up, just under
/// "used N tools" rather than a specific verb -- see `WorkRun::summary`.
fn fold_into_run(run: &mut WorkRun, event: &NormalizedEvent) {
    run.last_timestamp = Some(event.timestamp);
    match &event.payload {
        EventPayload::FileAction { .. } => run.files_touched += 1,
        EventPayload::ShellCommand(_) => run.commands_run += 1,
        EventPayload::ToolCall(_) => run.other_tools += 1,
        // ToolResult carries no new information this summary needs -- the matching ToolCall
        // already counted it.
        EventPayload::ToolResult(_) => {}
        _ => {}
    }
}

/// Baselines `cursor` to `trace`'s current end -- its highest event sequence and its strongest
/// outcome rank so far -- without producing any frames. Call this the first time the feed sees
/// a session, so a session that is already hundreds of turns deep doesn't replay its whole
/// history as a live burst the moment the feed starts watching it; only activity after this
/// point turns into a frame.
pub fn seed_cursor(trace: &AgentWorthTrace, cursor: &mut SessionCursor) {
    cursor.last_sequence = trace.events.iter().map(|e| e.sequence).max().unwrap_or(0);
    let detector = OutcomeDetector::new();
    let outcomes = detector.detect_outcomes(trace);
    cursor.last_outcome_rank = detector
        .strongest_outcome(&outcomes)
        .map(|evidence| outcome_rank(evidence.kind))
        .unwrap_or(0);
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

    let flush_run = |run: &mut WorkRun, messages: &mut Vec<Message>| {
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

    let start_sequence = cursor.last_sequence;
    for event in trace.events.iter().filter(|e| e.sequence > start_sequence) {
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
        // The adapter emits ToolCall + ShellCommand for the Bash call *before* the line's own
        // AssistantMessage (text blocks are collected but only turned into an event after the
        // whole content array is walked -- crates/adapters/src/claude.rs), so the collapsed
        // work run for that Bash call is flushed right at that AssistantMessage boundary, ahead
        // of both chat lines in emission order.
        assert_eq!(
            kinds,
            vec![
                (MessageKind::Work, "ran 1 command"),
                (MessageKind::Speech, "I will check the file."),
                (MessageKind::Speech, "All tests pass now!"),
            ]
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

    /// The bug this pins down: the first time the feed sees a session, `SessionCursor::default`
    /// starts at zero, so every event already in a transcript that's hundreds of turns deep gets
    /// replayed as speech/work frames in one burst. `seed_cursor` should baseline the cursor to
    /// the trace's current end instead, so a subsequent `feed_from_trace` call sees only what's
    /// genuinely new -- exactly like `gateway.rs`'s `transcript_feed_loop` now does on first
    /// sighting of a session.
    #[test]
    fn seed_cursor_baselines_existing_history_so_it_never_replays_as_frames() {
        let trace = parse_fixture(&[
            r#"{"type":"user","timestamp":"2026-09-07T10:00:00Z","content":"do the thing"}"#,
            r#"{"type":"assistant","timestamp":"2026-09-07T10:00:05Z","content":[{"type":"text","text":"working on it"}]}"#,
            r#"{"type":"assistant","timestamp":"2026-09-07T10:00:10Z","content":[{"type":"text","text":"done"}]}"#,
        ]);

        let mut cursor = SessionCursor::default();
        seed_cursor(&trace, &mut cursor);
        assert!(cursor.last_sequence > 0, "cursor baselined past the fixture's existing events");

        let out = feed_from_trace(&trace, &mut cursor, "office-harvey", "harvey");
        assert!(out.messages.is_empty(), "seeded cursor must not replay pre-existing history");
        assert!(out.new_outcome.is_none(), "seeded cursor must not re-report a pre-existing outcome");
    }
}
