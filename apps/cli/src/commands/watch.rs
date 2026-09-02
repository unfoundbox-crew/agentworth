//! Loop Sentinel real-time watchdog command for AgentWorth.
//!
//! Subcommand: `agentworth watch [--interval-secs S] [--poll-once] [--json]`
//! Tails active session history files and detects destructive doom loops:
//! 1. 3+ identical consecutive tool calls with matching parameters
//! 2. Rapid file edit-revert oscillations on the same file path
//! 3. Whether each detected loop was self-corrected or needed a human to
//!    step in -- see the doc comment on `classify_resolution` for the
//!    heuristic and its known limitations

use std::path::PathBuf;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use agentworth_adapter_sdk::ScanOptions;
use agentworth_core::Scanner;
use agentworth_storage::{SessionFilter, SessionOrderBy, Storage};
use anyhow::Result;
use console::style;

// Loop detection itself now lives in `agentworth_outcomes::loops`, so the scanner can run the
// same detector while it indexes and persist what it finds (`session_risk`). This command
// still owns the live polling and the terminal rendering.
use agentworth_outcomes::loops::{
    evaluate_trace_for_loops, LoopAlertKind, LoopResolution, DEFAULT_MAX_FILE_REVISIONS,
    DEFAULT_MAX_TOOL_REPEATS,
};

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
                let alerts = evaluate_trace_for_loops(
                    &trace,
                    DEFAULT_MAX_TOOL_REPEATS,
                    DEFAULT_MAX_FILE_REVISIONS,
                );
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
