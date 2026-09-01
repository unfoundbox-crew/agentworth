//! Loop Sentinel real-time watchdog command for AgentWorth.
//!
//! Subcommand: `agentworth watch [--interval-secs S] [--poll-once] [--json]`
//! Tails active session history files and detects destructive doom loops:
//! 1. 3+ identical consecutive tool calls with matching parameters
//! 2. Rapid file edit-revert oscillations on the same file path

use std::collections::HashMap;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

use agentworth_core::Scanner;
use agentworth_schema::{AgentWorthTrace, EventPayload, ToolCall};
use anyhow::Result;
use console::style;
use serde::{Deserialize, Serialize};

/// An alert produced by the Loop Sentinel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopAlertKind {
    IdenticalToolLoop,
    FileOscillation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopSentinelAlert {
    pub session_id: String,
    pub kind: LoopAlertKind,
    pub description: String,
    pub offending_target: String,
    pub repeat_count: usize,
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

    for event in &trace.events {
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
                alerts.push(LoopSentinelAlert {
                    session_id: trace.session_id.clone(),
                    kind: LoopAlertKind::IdenticalToolLoop,
                    description: format!(
                        "Agent executed identical tool call {} times consecutively without modifying parameters.",
                        consecutive_count
                    ),
                    offending_target: tool.name.clone(),
                    repeat_count: consecutive_count,
                });
            }
        }
    }

    // 2. Detect file edit oscillations
    let mut file_action_counts: HashMap<String, usize> = HashMap::new();
    for event in &trace.events {
        if let EventPayload::FileAction { path, .. } = &event.payload {
            let count = file_action_counts.entry(path.clone()).or_insert(0);
            *count += 1;
            if *count >= max_file_revisions {
                alerts.push(LoopSentinelAlert {
                    session_id: trace.session_id.clone(),
                    kind: LoopAlertKind::FileOscillation,
                    description: format!(
                        "File '{}' has been modified/reverted {} times in this session, indicating potential thrashing.",
                        path, *count
                    ),
                    offending_target: path.clone(),
                    repeat_count: *count,
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
) -> Result<()> {
    let scanner = Scanner::default();
    let opts = agentworth_adapter_sdk::ScanOptions {
        custom_paths,
        force: false,
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
        let enumerated = scanner.enumerate_sources(&opts)?;
        // Sort newest first
        let mut sorted = enumerated;
        sorted.sort_by(|a, b| b.mtime_epoch_secs.cmp(&a.mtime_epoch_secs));

        let mut total_alerts = Vec::new();

        // Check top 5 newest active sessions
        for source in sorted.iter().take(5) {
            if let Ok(parsed) = scanner.parse_session(source) {
                let alerts = evaluate_trace_for_loops(&parsed.trace, 3, 4);
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
    use agentworth_schema::{FileActionType, NormalizedEvent, Provenance};
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
    }
}
