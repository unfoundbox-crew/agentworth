//! Loop Sentinel real-time watchdog command for AgentWorth.
//!
//! Subcommand: `agentworth watch [--interval-secs S] [--poll-once] [--json]`
//! Tails active session history files and detects destructive doom loops:
//! 1. 3+ identical consecutive tool calls with matching parameters
//! 2. Rapid file edit-revert oscillations on the same file path
//! 3. Whether each detected loop was self-corrected or needed a human to
//!    step in -- see the doc comment on `classify_resolution` for the
//!    heuristic and its known limitations

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use agentworth_adapter_sdk::ScanOptions;
use agentworth_core::Scanner;
use agentworth_schema::{AgentWorthTrace, EventPayload, NormalizedEvent};
use agentworth_storage::{SessionFilter, SessionOrderBy, Storage};
use anyhow::Result;
use console::style;
use serde::{Deserialize, Serialize};

/// An alert produced by the Loop Sentinel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopAlertKind {
    IdenticalToolLoop,
    FileOscillation,
}

/// How a detected loop's repeating pattern was resolved, if it was resolved
/// at all by the time this trace was evaluated. See the doc comment on
/// [`classify_resolution`] for the heuristic and its known limitations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopResolution {
    /// The agent's own behavior changed (a different tool call, or a touch
    /// on a different file) before any `UserMessage` event was observed.
    SelfCorrected,
    /// A `UserMessage` event was observed before the agent's behavior
    /// changed on its own -- a human said something that interrupted the
    /// pattern.
    HumanRescued,
    /// Neither was observed anywhere between the alert and the end of the
    /// trace collected so far. Expect this to be the common case for
    /// `watch`, which polls a still-growing session -- the loop may simply
    /// still be in progress.
    StillLooping,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopSentinelAlert {
    pub session_id: String,
    pub kind: LoopAlertKind,
    pub description: String,
    pub offending_target: String,
    pub repeat_count: usize,
    /// How this loop was resolved, if at all, as of this evaluation.
    pub resolution: LoopResolution,
}

/// Classify how a flagged loop was resolved, by scanning forward from the
/// event that tripped the alert.
///
/// ## The signal
///
/// Trace events are one time-ordered stream mixing agent actions
/// (`ToolCall`, `FileAction`, `AssistantMessage`, ...) with human turns
/// (`UserMessage`). No field on a `ToolCall` or `FileAction` says who
/// caused it -- there's no actor/role on those payloads (see
/// `crates/schema/src/event.rs`). The only place a human shows up directly
/// in the stream is `EventPayload::UserMessage`, which every adapter
/// constructs from a real user turn (checked all 18 files under
/// `crates/adapters/src/`).
///
/// The schema also has `EventPayload::HumanIntervention`, which sounds like
/// it was built for exactly this. It is deliberately not used here: as of
/// this writing, no adapter ever constructs one from a real transcript
/// (grepped `crates/adapters/src` -- zero hits). It's only ever read
/// downstream (export-atif, redaction, the `main.rs` pretty-printer), so
/// today it is permanently empty on every trace `watch` will ever see.
/// Building this on it would silently never fire. If an adapter later
/// starts emitting real `HumanIntervention` events (say, by parsing a
/// Ctrl+C/ESC interrupt marker out of the raw log), that would be a
/// strictly stronger signal than the one below and should be checked
/// first.
///
/// ## The heuristic
///
/// Starting just after the triggering event, walk forward and see which
/// happens first:
///
/// - a `UserMessage` -> [`LoopResolution::HumanRescued`]
/// - the loop's own condition changing (a different tool-call signature for
///   a tool loop; a touch on some other file for a file oscillation), with
///   no `UserMessage` seen yet -> [`LoopResolution::SelfCorrected`]
/// - neither, through the end of the trace -> [`LoopResolution::StillLooping`]
///
/// A `UserMessage` ahead of the pattern break stands in for "a human had to
/// step in." It's the interruption we're detecting, not proof the message
/// was about the loop, or that whatever followed actually fixed anything.
///
/// ## Known limitations (first-cut heuristic, not tuned against real sessions)
///
/// - **Correlation, not causation.** Any `UserMessage` ahead of the pattern
///   break counts as the rescue, even an unrelated one (answering a
///   permission prompt, queuing up the next task). There's no content
///   signal here, only ordering.
/// - **Misses silent interrupts.** A Ctrl+C/ESC interrupt that an adapter
///   doesn't parse into a `UserMessage` (or drops entirely) leaves no
///   trace here, so a genuinely human-rescued loop can read as
///   `SelfCorrected` or `StillLooping`.
/// - **No distance or time bound.** The scan runs to the end of the trace
///   if it has to. In practice the pattern usually breaks (or the next
///   message shows up) within a handful of events, but in a long run of
///   the exact same call, an unrelated `UserMessage` that actually starts
///   the *next* task later in the session could get misattributed as the
///   rescue for a loop the agent had, in spirit, already dropped. Left
///   unbounded for now -- there's no real trace corpus on hand to tune a
///   distance or time window against, and a made-up one would just be a
///   different, unverified guess.
/// - **`StillLooping` is a snapshot, not a verdict.** `watch` polls a
///   session that is still being written. Re-evaluating the same session a
///   few polls later can turn a `StillLooping` alert into `SelfCorrected`
///   or `HumanRescued` once more events land. Anything that scores on this
///   field should treat `StillLooping` as "not yet known," never as a
///   terminal outcome.
/// - **The pattern-break check is coarse.** Any differing tool-call
///   signature counts as "moved on," even a near-identical retry (e.g. the
///   same read at a different line offset). Any touch on a second file
///   counts as the oscillation ending, even if the agent immediately swings
///   back to the first file afterward -- that swing-back just crosses the
///   threshold again and produces its own, freshly classified alert, so
///   it isn't lost, only split across two alerts.
fn classify_resolution(
    events: &[NormalizedEvent],
    trigger_index: usize,
    is_pattern_break: impl Fn(&NormalizedEvent) -> bool,
) -> LoopResolution {
    for event in &events[trigger_index + 1..] {
        if matches!(&event.payload, EventPayload::UserMessage { .. }) {
            return LoopResolution::HumanRescued;
        }
        if is_pattern_break(event) {
            return LoopResolution::SelfCorrected;
        }
    }
    LoopResolution::StillLooping
}

/// Detect repeating doom loops and file edit oscillations in a trace.
pub fn evaluate_trace_for_loops(
    trace: &AgentWorthTrace,
    max_tool_repeats: usize,
    max_file_revisions: usize,
) -> Vec<LoopSentinelAlert> {
    let mut alerts = Vec::new();

    // 1. Detect consecutive identical tool calls
    let mut last_tool_sig: Option<String> = None;
    let mut consecutive_count = 0usize;

    for (idx, event) in trace.events.iter().enumerate() {
        if let EventPayload::ToolCall(tool) = &event.payload {
            let sig = format!("{}:{}", tool.name, tool.arguments);
            if let Some(prev) = &last_tool_sig {
                if prev == &sig {
                    consecutive_count += 1;
                } else {
                    consecutive_count = 1;
                    last_tool_sig = Some(sig.clone());
                }
            } else {
                consecutive_count = 1;
                last_tool_sig = Some(sig.clone());
            }

            if consecutive_count >= max_tool_repeats {
                let looping_sig = sig.clone();
                let resolution = classify_resolution(&trace.events, idx, |ev| {
                    matches!(&ev.payload, EventPayload::ToolCall(t) if format!("{}:{}", t.name, t.arguments) != looping_sig)
                });
                alerts.push(LoopSentinelAlert {
                    session_id: trace.session_id.clone(),
                    kind: LoopAlertKind::IdenticalToolLoop,
                    description: format!(
                        "Agent executed identical tool call {} times consecutively without modifying parameters.",
                        consecutive_count
                    ),
                    offending_target: tool.name.clone(),
                    repeat_count: consecutive_count,
                    resolution,
                });
            }
        }
    }

    // 2. Detect file edit oscillations
    let mut file_action_counts: HashMap<String, usize> = HashMap::new();
    for (idx, event) in trace.events.iter().enumerate() {
        if let EventPayload::FileAction { path, .. } = &event.payload {
            let count = file_action_counts.entry(path.clone()).or_insert(0);
            *count += 1;
            if *count >= max_file_revisions {
                let oscillating_path = path.clone();
                let resolution = classify_resolution(&trace.events, idx, |ev| {
                    matches!(&ev.payload, EventPayload::FileAction { path: p, .. } if p != &oscillating_path)
                });
                alerts.push(LoopSentinelAlert {
                    session_id: trace.session_id.clone(),
                    kind: LoopAlertKind::FileOscillation,
                    description: format!(
                        "File '{}' has been modified/reverted {} times in this session, indicating potential thrashing.",
                        path, *count
                    ),
                    offending_target: path.clone(),
                    repeat_count: *count,
                    resolution,
                });
            }
        }
    }

    alerts
}

/// Execute the `agentworth watch` subcommand.
pub fn run_watch_command(
    interval_secs: u64,
    poll_once: bool,
    json: bool,
    custom_paths: Vec<PathBuf>,
    db_path: Option<PathBuf>,
) -> Result<()> {
    let storage = Arc::new(match db_path {
        Some(p) => Storage::open_path(&p)?,
        None => Storage::open_default()?,
    });
    let scanner = Scanner::new(storage.clone());
    // include_stubs: true -- Watch polls a still-growing transcript to catch a doom loop as
    // it happens, and queries below with the matching `include_stubs: Some(true)` to see
    // that session even while it's thin. If the scanner skipped storing it as a stub, that
    // query would never find it until it grew past the predicate -- too late for the whole
    // point of watching live.
    let scan_opts = ScanOptions {
        custom_paths,
        force: false,
        include_stubs: true,
    };

    if !json {
        println!();
        println!(
            "{}",
            style("┌─ 🛡️  AgentWorth Loop Sentinel Active ────────────────────┐").bold().cyan()
        );
        println!("│ Polling active agent session transcripts for doom loops...  │");
        println!(
            "{}",
            style("└──────────────────────────────────────────────────────────┘").bold()
        );
        println!();
    }

    loop {
        // Refresh the index so a still-growing transcript (changed mtime/fingerprint) gets
        // re-parsed on this poll -- this is what lets Watch see activity from the current turn,
        // not just whatever was indexed by the last `agentworth scan`.
        let _ = scanner.run_scan(&scan_opts, |_, _| {});

        let recent_sessions = storage.list_sessions_filtered(&SessionFilter {
            limit: Some(5),
            order_by: Some(SessionOrderBy::StartedAtDesc),
            include_stubs: Some(true),
            ..Default::default()
        })?;

        let mut total_alerts = Vec::new();

        // Check the 5 most recently active sessions
        for summary in &recent_sessions {
            if let Ok(trace) = scanner.load_trace(&summary.session_id) {
                let alerts = evaluate_trace_for_loops(&trace, 3, 4);
                total_alerts.extend(alerts);
            }
        }

        if json {
            println!("{}", serde_json::to_string_pretty(&total_alerts)?);
        } else if total_alerts.is_empty() {
            println!(
                "{}",
                style(format!("✓ [{}] All monitored sessions normal (no loops detected).", chrono::Local::now().format("%H:%M:%S"))).dim()
            );
        } else {
            for alert in &total_alerts {
                println!(
                    "{}",
                    style("┌─ 🚨 LOOP SENTINEL ALERT DETECTED ────────────────────────┐")
                        .bold()
                        .red()
                );
                println!(
                    "│ Session: {:<47} │",
                    style(&alert.session_id).bold().yellow()
                );
                println!(
                    "│ Type:    {:<47} │",
                    style(match alert.kind {
                        LoopAlertKind::IdenticalToolLoop => "Identical Consecutive Tool Calls",
                        LoopAlertKind::FileOscillation => "File Edit Thrashing / Oscillation",
                    }).red()
                );
                println!(
                    "│ Target:  {:<47} │",
                    style(&alert.offending_target).cyan()
                );
                println!(
                    "│ Repeats: {:<47} │",
                    style(format!("{} iterations", alert.repeat_count)).bold()
                );
                println!(
                    "│ Outcome: {:<47} │",
                    match alert.resolution {
                        LoopResolution::SelfCorrected =>
                            style("Self-corrected (no user message first)".to_string()).green(),
                        LoopResolution::HumanRescued =>
                            style("Human rescued (a user message interrupted it)".to_string())
                                .yellow(),
                        LoopResolution::StillLooping =>
                            style("Still looping (no resolution observed yet)".to_string()).red(),
                    }
                );
                println!(
                    "{}",
                    style("└──────────────────────────────────────────────────────────┘").bold()
                );
                println!();
            }
        }

        if poll_once {
            break;
        }

        sleep(Duration::from_secs(interval_secs));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{FileActionType, NormalizedEvent, Provenance, ToolCall};
    use chrono::Utc;

    #[test]
    fn test_loop_sentinel_identical_tool_calls() {
        let now = Utc::now();
        let prov = Provenance::new("/tmp/test.jsonl", "claude_code", 100, 1000, "fp1");
        let mut trace = AgentWorthTrace::new("sess-loop-1", "claude_code", prov, now);

        for i in 1..=3 {
            trace.events.push(NormalizedEvent::new(
                i,
                now,
                EventPayload::ToolCall(ToolCall {
                    id: Some(format!("call_{}", i)),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({ "path": "src/main.rs" }),
                }),
            ));
        }

        let alerts = evaluate_trace_for_loops(&trace, 3, 4);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, LoopAlertKind::IdenticalToolLoop);
        assert_eq!(alerts[0].repeat_count, 3);
        // Nothing follows the loop in this fixture, so it must read as
        // still in progress, not as having quietly self-corrected.
        assert_eq!(alerts[0].resolution, LoopResolution::StillLooping);
    }

    #[test]
    fn test_loop_sentinel_file_oscillation() {
        let now = Utc::now();
        let prov = Provenance::new("/tmp/test.jsonl", "claude_code", 100, 1000, "fp1");
        let mut trace = AgentWorthTrace::new("sess-loop-2", "claude_code", prov, now);

        for i in 1..=4 {
            trace.events.push(NormalizedEvent::new(
                i,
                now,
                EventPayload::FileAction {
                    path: "src/lib.rs".to_string(),
                    action: FileActionType::Write,
                    diff: Some("+ diff".to_string()),
                    lines_changed: Some(1),
                },
            ));
        }

        let alerts = evaluate_trace_for_loops(&trace, 3, 4);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, LoopAlertKind::FileOscillation);
        assert_eq!(alerts[0].repeat_count, 4);
        assert_eq!(alerts[0].resolution, LoopResolution::StillLooping);
    }

    #[test]
    fn test_loop_sentinel_tool_loop_self_corrected() {
        let now = Utc::now();
        let prov = Provenance::new("/tmp/test.jsonl", "claude_code", 100, 1000, "fp1");
        let mut trace = AgentWorthTrace::new("sess-loop-self", "claude_code", prov, now);

        for i in 1..=3 {
            trace.events.push(NormalizedEvent::new(
                i,
                now,
                EventPayload::ToolCall(ToolCall {
                    id: Some(format!("call_{}", i)),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({ "path": "src/main.rs" }),
                }),
            ));
        }

        // The agent moves on to a different tool call on its own -- no user
        // message appears anywhere in this trace.
        trace.events.push(NormalizedEvent::new(
            4,
            now,
            EventPayload::ToolCall(ToolCall {
                id: Some("call_4".to_string()),
                name: "read_file".to_string(),
                arguments: serde_json::json!({ "path": "src/lib.rs" }),
            }),
        ));

        let alerts = evaluate_trace_for_loops(&trace, 3, 4);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].resolution, LoopResolution::SelfCorrected);
    }

    #[test]
    fn test_loop_sentinel_tool_loop_human_rescued() {
        let now = Utc::now();
        let prov = Provenance::new("/tmp/test.jsonl", "claude_code", 100, 1000, "fp1");
        let mut trace = AgentWorthTrace::new("sess-loop-human", "claude_code", prov, now);

        for i in 1..=3 {
            trace.events.push(NormalizedEvent::new(
                i,
                now,
                EventPayload::ToolCall(ToolCall {
                    id: Some(format!("call_{}", i)),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({ "path": "src/main.rs" }),
                }),
            ));
        }

        // A human steps in before the agent's own behavior changes.
        trace.events.push(NormalizedEvent::new(
            4,
            now,
            EventPayload::UserMessage {
                content: "Stop re-reading that file, check the error output instead."
                    .to_string(),
            },
        ));

        trace.events.push(NormalizedEvent::new(
            5,
            now,
            EventPayload::ToolCall(ToolCall {
                id: Some("call_5".to_string()),
                name: "read_file".to_string(),
                arguments: serde_json::json!({ "path": "target/debug/build.log" }),
            }),
        ));

        let alerts = evaluate_trace_for_loops(&trace, 3, 4);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].resolution, LoopResolution::HumanRescued);
    }

    #[test]
    fn test_loop_sentinel_file_oscillation_self_corrected() {
        let now = Utc::now();
        let prov = Provenance::new("/tmp/test.jsonl", "claude_code", 100, 1000, "fp1");
        let mut trace = AgentWorthTrace::new("sess-osc-self", "claude_code", prov, now);

        for i in 1..=4 {
            trace.events.push(NormalizedEvent::new(
                i,
                now,
                EventPayload::FileAction {
                    path: "src/lib.rs".to_string(),
                    action: FileActionType::Write,
                    diff: Some("+ diff".to_string()),
                    lines_changed: Some(1),
                },
            ));
        }

        // The agent abandons the thrashed file for a different one, on its
        // own -- no user message anywhere in this trace.
        trace.events.push(NormalizedEvent::new(
            5,
            now,
            EventPayload::FileAction {
                path: "src/other.rs".to_string(),
                action: FileActionType::Edit,
                diff: Some("+ diff".to_string()),
                lines_changed: Some(2),
            },
        ));

        let alerts = evaluate_trace_for_loops(&trace, 3, 4);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].resolution, LoopResolution::SelfCorrected);
    }

    #[test]
    fn test_loop_sentinel_file_oscillation_human_rescued() {
        let now = Utc::now();
        let prov = Provenance::new("/tmp/test.jsonl", "claude_code", 100, 1000, "fp1");
        let mut trace = AgentWorthTrace::new("sess-osc-human", "claude_code", prov, now);

        for i in 1..=4 {
            trace.events.push(NormalizedEvent::new(
                i,
                now,
                EventPayload::FileAction {
                    path: "src/lib.rs".to_string(),
                    action: FileActionType::Write,
                    diff: Some("+ diff".to_string()),
                    lines_changed: Some(1),
                },
            ));
        }

        trace.events.push(NormalizedEvent::new(
            5,
            now,
            EventPayload::UserMessage {
                content: "You're going in circles on lib.rs. Revert it and try the config \
                          instead."
                    .to_string(),
            },
        ));

        trace.events.push(NormalizedEvent::new(
            6,
            now,
            EventPayload::FileAction {
                path: "src/config.rs".to_string(),
                action: FileActionType::Edit,
                diff: Some("+ diff".to_string()),
                lines_changed: Some(3),
            },
        ));

        let alerts = evaluate_trace_for_loops(&trace, 3, 4);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].resolution, LoopResolution::HumanRescued);
    }

    /// Presence of a later `UserMessage` must not retroactively flip a loop
    /// that had already self-corrected. The heuristic is a race -- whichever
    /// comes first, the pattern break or the human message -- not "was a
    /// user message seen anywhere afterward."
    #[test]
    fn test_loop_sentinel_resolution_order_matters_not_just_presence() {
        let now = Utc::now();
        let prov = Provenance::new("/tmp/test.jsonl", "claude_code", 100, 1000, "fp1");
        let mut trace = AgentWorthTrace::new("sess-order", "claude_code", prov, now);

        for i in 1..=3 {
            trace.events.push(NormalizedEvent::new(
                i,
                now,
                EventPayload::ToolCall(ToolCall {
                    id: Some(format!("call_{}", i)),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({ "path": "src/main.rs" }),
                }),
            ));
        }

        // The agent already moved on by itself...
        trace.events.push(NormalizedEvent::new(
            4,
            now,
            EventPayload::ToolCall(ToolCall {
                id: Some("call_4".to_string()),
                name: "read_file".to_string(),
                arguments: serde_json::json!({ "path": "src/lib.rs" }),
            }),
        ));

        // ...and only afterward does the user say something unrelated, e.g.
        // kicking off the next task.
        trace.events.push(NormalizedEvent::new(
            5,
            now,
            EventPayload::UserMessage {
                content: "Great, now add a test for the new parser.".to_string(),
            },
        ));

        let alerts = evaluate_trace_for_loops(&trace, 3, 4);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].resolution, LoopResolution::SelfCorrected);
    }
}
