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

use super::protocol::{new_id, Message, MessageKind, MomentKind, Rung, TimelineMoment};

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
    last_sequence: u64,
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
    run.last_sequence = run.last_sequence.max(event.sequence);
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

/// Baselines `cursor` the same way [`seed_cursor`] does, but only against events strictly
/// before `seat_time` -- for a session whose persona was just seated (`gateway.rs`'s
/// `dispatch_start_rider`) at a known moment, so its very first reply after that point is
/// treated as new activity instead of being swallowed by a full-history baseline. The bug this
/// fixes: by the time the transcript feed's registration path (`Scanner::scan_one_source`)
/// first sees a freshly-seated rider's session, its transcript can already contain that first
/// reply -- `seed_cursor` would then baseline straight past it, and the reply never reaches the
/// wire. Events at or after `seat_time` are excluded from the *baseline* only; the very next
/// `feed_from_trace` call still reads them fresh, same as any other new activity.
///
/// A rider re-seated after already producing history keeps the same invariant: only events
/// before its new seat time count toward the baseline (including the outcome-rank baseline, so
/// a rung the session already reached before this reseat is not re-reported as freshly crossed
/// the moment the feed starts watching again).
pub fn seed_cursor_before(trace: &AgentWorthTrace, cursor: &mut SessionCursor, seat_time: DateTime<Utc>) {
    let prior_events: Vec<NormalizedEvent> =
        trace.events.iter().filter(|e| e.timestamp < seat_time).cloned().collect();
    cursor.last_sequence = prior_events.iter().map(|e| e.sequence).max().unwrap_or(0);

    let mut prior_trace = trace.clone();
    prior_trace.events = prior_events;
    let detector = OutcomeDetector::new();
    let outcomes = detector.detect_outcomes(&prior_trace);
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
            client_id: None,
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
                        client_id: None,
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

/// Cap on the moments a timeline carries. A multi-million-event session distills to far
/// fewer moments, but not to a bounded number -- so the cap keeps one giant session from
/// pushing megabytes over one websocket frame and thousands of DOM nodes into the deck.
/// Oldest dropped, most recent kept: scrubbing is about where the session is heading.
pub const MAX_TIMELINE_MOMENTS: usize = 2000;

/// Distills a whole trace into the seekable timeline the deck's scrubber scrubs over: the
/// same speech/work collapse `feed_from_trace` applies (shared `WorkRun` rules), plus the
/// two marks the live stream has no place for --
///
/// * `handoff`: an `EventPayload::ModelSwitch` carrying a `from_model`. The Claude adapter
///   emits exactly this for a delegation to a subagent model and the hand-back (its
///   multi-model test states that intent), so it is the delegation signal this codebase
///   already has. A same-session harness-side model flip also lands here; the text says
///   which models, so the mark never claims more than the event does.
/// * `error`: an `EventPayload::Error` -- the fork point the deck renders as fork-on-retry
///   when later moments follow it.
///
/// Ordered by the events' own sequence, which is already monotonic. Pure; no cursor, no
/// mutation -- the caller re-runs it whenever it wants the current timeline. Not capped;
/// [`timeline_with_cap`] applies [`MAX_TIMELINE_MOMENTS`] and reports truncation.
pub fn timeline_from_trace(trace: &AgentWorthTrace) -> Vec<TimelineMoment> {
    let mut moments: Vec<TimelineMoment> = Vec::new();
    let mut run = WorkRun::default();

    let flush_run = |run: &mut WorkRun, moments: &mut Vec<TimelineMoment>| {
        if run.is_empty() {
            return;
        }
        let at = run
            .last_timestamp
            .unwrap_or_else(Utc::now)
            .to_rfc3339();
        moments.push(TimelineMoment {
            seq: run.last_sequence,
            kind: MomentKind::Work,
            text: run.summary(),
            at,
        });
        *run = WorkRun::default();
    };

    for event in &trace.events {
        match &event.payload {
            EventPayload::AssistantMessage { content, .. } => {
                flush_run(&mut run, &mut moments);
                if !content.trim().is_empty() {
                    moments.push(TimelineMoment {
                        seq: event.sequence,
                        kind: MomentKind::Speech,
                        text: content.clone(),
                        at: event.timestamp.to_rfc3339(),
                    });
                }
            }
            EventPayload::ModelSwitch(ms) => {
                flush_run(&mut run, &mut moments);
                let text = match &ms.from_model {
                    Some(from) => format!("{from} → {}", ms.to_model),
                    None => ms.to_model.clone(),
                };
                moments.push(TimelineMoment {
                    seq: event.sequence,
                    kind: MomentKind::Handoff,
                    text,
                    at: event.timestamp.to_rfc3339(),
                });
            }
            EventPayload::Error { message, .. } => {
                flush_run(&mut run, &mut moments);
                moments.push(TimelineMoment {
                    seq: event.sequence,
                    kind: MomentKind::Error,
                    text: message.clone(),
                    at: event.timestamp.to_rfc3339(),
                });
            }
            payload if is_work_event(payload) => fold_into_run(&mut run, event),
            _ => flush_run(&mut run, &mut moments),
        }
    }
    flush_run(&mut run, &mut moments);
    moments
}

/// `timeline_from_trace` with the [`MAX_TIMELINE_MOMENTS`] cap applied: the shape the
/// gateway ships, with its `truncated` flag, so the UI can say "showing the last 2000
/// moments" instead of silently losing the session's head.
pub fn timeline_with_cap(trace: &AgentWorthTrace) -> (Vec<TimelineMoment>, bool) {
    let mut moments = timeline_from_trace(trace);
    if moments.len() > MAX_TIMELINE_MOMENTS {
        let drop = moments.len() - MAX_TIMELINE_MOMENTS;
        moments.drain(..drop);
        return (moments, true);
    }
    (moments, false)
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

    /// Pins down `seed_cursor_before` against the exact shape of the reported bug: a rider is
    /// seated, replies once, and by the time the feed's registration path first sees the
    /// session, the reply is already in the transcript. `seed_cursor` (baseline to trace end)
    /// would swallow it; `seed_cursor_before(seat_time)` must not, because the reply's
    /// timestamp is after `seat_time`.
    #[test]
    fn seed_cursor_before_seat_time_still_emits_a_reply_already_on_disk_at_first_sighting() {
        let trace = parse_fixture(&[
            r#"{"type":"user","timestamp":"2026-09-07T10:00:00Z","content":"say pong and nothing else"}"#,
            r#"{"type":"assistant","timestamp":"2026-09-07T10:00:02Z","content":[{"type":"text","text":"pong"}]}"#,
        ]);
        let seat_time = "2026-09-07T10:00:01Z".parse::<DateTime<Utc>>().expect("valid timestamp");

        let mut cursor = SessionCursor::default();
        seed_cursor_before(&trace, &mut cursor, seat_time);
        let out = feed_from_trace(&trace, &mut cursor, "office-probe-haiku", "probe-haiku");

        let speech: Vec<&str> = out
            .messages
            .iter()
            .filter(|m| m.kind == MessageKind::Speech)
            .map(|m| m.text.as_str())
            .collect();
        assert_eq!(speech, vec!["pong"], "the reply after seat time must reach the wire");

        // A second pass with the same (now-advanced) cursor sees nothing new -- the reply is
        // emitted exactly once, not on every poll.
        let again = feed_from_trace(&trace, &mut cursor, "office-probe-haiku", "probe-haiku");
        assert!(again.messages.is_empty(), "the reply is not re-emitted on a later pass");
    }

    /// Nothing before `seat_time` is replayed, even when it would otherwise look like new
    /// activity -- the user's own goal prompt (a `user`, not `assistant`, event, so
    /// `feed_from_trace` never turns it into a `Message` anyway) must not shift the baseline
    /// forward past events that came before the rider was seated.
    #[test]
    fn seed_cursor_before_seat_time_excludes_everything_at_or_after_it_from_the_baseline_only() {
        let trace = parse_fixture(&[
            r#"{"type":"assistant","timestamp":"2026-09-07T09:00:00Z","content":[{"type":"text","text":"earlier, unrelated turn"}]}"#,
        ]);
        let seat_time = "2026-09-07T10:00:00Z".parse::<DateTime<Utc>>().expect("valid timestamp");

        let mut cursor = SessionCursor::default();
        seed_cursor_before(&trace, &mut cursor, seat_time);
        assert!(cursor.last_sequence > 0, "the prior turn is baselined away");

        let out = feed_from_trace(&trace, &mut cursor, "office-probe-haiku", "probe-haiku");
        assert!(out.messages.is_empty(), "nothing before seat time is replayed");
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

    /// The timeline distiller must apply the same collapse rules the live feed applies, so
    /// a scrubbed-to-the-end timeline reads like the stream that got you there.
    #[test]
    fn timeline_matches_the_live_feed_collapse_rules() {
        let trace = parse_fixture(&[
            r#"{"type":"assistant","timestamp":"2026-09-07T10:00:00Z","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"src/a.rs","old_string":"a","new_string":"b"}}]}"#,
            r#"{"type":"assistant","timestamp":"2026-09-07T10:00:01Z","content":[{"type":"tool_use","id":"t2","name":"Bash","input":{"command":"cargo test"}}]}"#,
            r#"{"type":"assistant","timestamp":"2026-09-07T10:00:03Z","content":[{"type":"text","text":"done"}]}"#,
        ]);

        let moments = timeline_from_trace(&trace);
        let kinds: Vec<(MomentKind, &str)> =
            moments.iter().map(|m| (m.kind, m.text.as_str())).collect();
        assert_eq!(
            kinds,
            vec![
                (MomentKind::Work, "edited 1 file, ran 1 command"),
                (MomentKind::Speech, "done"),
            ]
        );
        // Sequence is the trace's own, so the UI's seek order is deterministic.
        assert!(moments.windows(2).all(|w| w[0].seq < w[1].seq), "moments are strictly ordered by seq");
    }

    /// A delegation to a subagent model lands as a `handoff` moment, and the hand-back as a
    /// second one -- the exact shape `crates/adapters/src/claude.rs`'s multi-model fixture
    /// produces (primary delegates, subagent replies, primary resumes).
    #[test]
    fn timeline_marks_delegation_and_handback_as_handoff_moments() {
        let trace = parse_fixture(&[
            r#"{"type":"assistant","timestamp":"2026-09-07T10:00:00Z","model":"claude-opus-5","usage":{"input_tokens":500,"output_tokens":120,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"Delegating."}]}"#,
            r#"{"type":"assistant","timestamp":"2026-09-07T10:00:10Z","model":"claude-fable-5","usage":{"input_tokens":300,"output_tokens":80,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"Subagent result."}]}"#,
            r#"{"type":"assistant","timestamp":"2026-09-07T10:00:20Z","model":"claude-opus-5","usage":{"input_tokens":100,"output_tokens":40,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"Done."}]}"#,
        ]);

        let moments = timeline_from_trace(&trace);
        let handoffs: Vec<&str> = moments
            .iter()
            .filter(|m| m.kind == MomentKind::Handoff)
            .map(|m| m.text.as_str())
            .collect();
        assert_eq!(
            handoffs,
            vec![
                "claude-opus-5 → claude-fable-5",
                "claude-fable-5 → claude-opus-5",
            ]
        );
    }

    /// An error record is the fork point: it becomes an `error` moment, and the collapse
    /// run it interrupted is flushed ahead of it so the failed work stays before the mark.
    #[test]
    fn timeline_marks_error_records_as_fork_points() {
        let trace = parse_fixture(&[
            r#"{"type":"assistant","timestamp":"2026-09-07T10:00:00Z","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test"}}]}"#,
            r#"{"type":"system","timestamp":"2026-09-07T10:00:05Z","error":"cargo test exited 101"}"#,
            r#"{"type":"assistant","timestamp":"2026-09-07T10:00:10Z","content":[{"type":"text","text":"retrying with a fix"}]}"#,
        ]);

        let moments = timeline_from_trace(&trace);
        let kinds: Vec<(MomentKind, &str)> =
            moments.iter().map(|m| (m.kind, m.text.as_str())).collect();
        assert_eq!(
            kinds,
            vec![
                (MomentKind::Work, "ran 1 command"),
                (MomentKind::Error, "cargo test exited 101"),
                (MomentKind::Speech, "retrying with a fix"),
            ]
        );
    }

    /// An empty or speechless trace yields an empty timeline -- not an error. The deck
    /// degrades to its stops-only strip on exactly this.
    #[test]
    fn timeline_of_an_empty_trace_is_empty() {
        let trace = parse_fixture(&[]);
        assert!(timeline_from_trace(&trace).is_empty());
    }

    /// A session past the cap keeps its most recent moments and drops the oldest, so the
    /// scrubber shows where the session is heading and one giant transcript cannot flood
    /// the frame.
    #[test]
    fn timeline_keeps_the_most_recent_moments_past_the_cap() {
        let mut lines: Vec<String> = Vec::new();
        for i in 0..(MAX_TIMELINE_MOMENTS + 50) {
            lines.push(format!(
                r#"{{"type":"assistant","timestamp":"2026-09-07T10:00:{:02}Z","content":[{{"type":"text","text":"line {i}"}}]}}"#,
                i % 60
            ));
        }
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let trace = parse_fixture(&refs);

        let (moments, truncated) = timeline_with_cap(&trace);
        assert!(truncated);
        assert_eq!(moments.len(), MAX_TIMELINE_MOMENTS);
        assert_eq!(moments[0].text, "line 50", "oldest moments dropped, newest kept");
        assert_eq!(moments.last().unwrap().text, format!("line {}", MAX_TIMELINE_MOMENTS + 49));

        // Under the cap, truncation is stated false, never implied.
        let under: Vec<&str> = lines.iter().take(MAX_TIMELINE_MOMENTS).map(|s| s.as_str()).collect();
        let (_, truncated2) = timeline_with_cap(&parse_fixture(&under));
        assert!(!truncated2);
    }
}
