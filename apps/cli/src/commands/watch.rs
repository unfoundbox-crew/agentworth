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
    ui: &crate::ui::Ui,
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
        print!("{}", crate::ui::views::watch_banner(ui));
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
            let at = chrono::Local::now().format("%H:%M:%S").to_string();
            print!("{}", crate::ui::views::watch_clean(ui, &at));
        } else {
            let rows: Vec<crate::ui::views::WatchAlertRow> = total_alerts
                .iter()
                .map(|alert| {
                    let (outcome, outcome_role) = match alert.resolution {
                        LoopResolution::SelfCorrected => {
                            ("self-corrected (no user message first)", crate::ui::Role::Verified)
                        }
                        LoopResolution::HumanRescued => {
                            ("human rescued (a user message interrupted it)", crate::ui::Role::Warn)
                        }
                        LoopResolution::StillLooping => {
                            ("still looping (no resolution observed yet)", crate::ui::Role::Error)
                        }
                    };
                    crate::ui::views::WatchAlertRow {
                        session_id: &alert.session_id,
                        kind: match alert.kind {
                            LoopAlertKind::IdenticalToolLoop => "identical consecutive tool calls",
                            LoopAlertKind::FileOscillation => "file edit thrashing / oscillation",
                        },
                        target: &alert.offending_target,
                        repeat_count: alert.repeat_count,
                        outcome,
                        outcome_role,
                    }
                })
                .collect();
            print!("{}", crate::ui::views::watch_alerts(ui, &rows));
        }

        if poll_once {
            break;
        }

        sleep(Duration::from_secs(interval_secs));
    }

    Ok(())
}
