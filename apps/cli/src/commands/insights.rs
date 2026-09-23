//! `agentworth insights` — the deterministic machine-insights report, straight from the index.
//!
//! The command opens its own `SQLITE_OPEN_READ_ONLY` connection (the completions.rs pattern) and
//! gives up immediately rather than waiting on a write lock: an in-flight scan must not stall a
//! read that reports the index rather than build it. JSON output is the whole contract the deck
//! and MCP lanes render.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OpenFlags};

use crate::ui::Ui;

pub fn run_insights_command(json: bool, db_path: Option<PathBuf>, ui: &Ui) -> Result<()> {
    let db = match db_path {
        Some(p) => p,
        None => agentworth_storage::default_db_dir().map(|dir| dir.join("agentworth.db"))?,
    };

    if !db.exists() {
        bail!(
            "no AgentWorth index found at {}. Run `archie scan` first.",
            db.display()
        );
    }

    let insights = crate::ui::with_status(ui, "computing insights", || {
        let conn = Connection::open_with_flags(
            &db,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("cannot open index read-only at {}", db.display()))?;
        // Zero busy timeout: this read reports the index, it does not wait for its writer.
        conn.busy_timeout(std::time::Duration::from_millis(0)).ok();
        agentworth_storage::compute_insights(&conn)
    })?;

    if json {
        println!("{}", serde_json::to_string_pretty(&insights)?);
    } else {
        print_human_summary(&insights);
    }
    Ok(())
}

/// A single plain-text summary line per headline number, matching `agentworth stats`'s tone.
/// Every finer figure lives under `--json`; this surface never renders tables.
fn print_human_summary(insights: &agentworth_storage::insights::Insights) {
    let pop = &insights.population;
    let vol = &insights.volume;
    println!(
        "Usable sessions: {} (conversation, multi-event)",
        vol.usable_sessions
    );
    println!(
        "Tool calls witnessed: {} · File touches: {}",
        vol.tool_calls_witnessed, insights.file_modifications.total
    );
    println!(
        "Verified outcomes: {} ({:.1}% usable)",
        insights.verified.sessions, insights.verified.share_pct
    );
    if let Some(strict) = insights.calls_per_turn.strict {
        println!("Tool calls per human turn (strict ratio of sums): {strict:.1}");
    }
    if let Some(heavy) = insights.calls_per_turn.heavy_session_average {
        println!("Tool calls per turn, within heavy sessions (>10 events): {heavy:.1}");
    }
    println!(
        "Window: {} → {} · {} raw rows, {} excluded by the clock-bug filter",
        insights
            .window
            .sessions
            .min_started_at
            .as_deref()
            .map(|s| s.get(..10).unwrap_or(s))
            .unwrap_or("?"),
        insights
            .window
            .sessions
            .max_started_at
            .as_deref()
            .map(|s| s.get(..10).unwrap_or(s))
            .unwrap_or("?"),
        pop.sessions_raw,
        pop.sessions_excluded_pre_2020
    );
    println!(
        "Deferred by design: {} groups (reasons under --json)",
        insights.deferred.len()
    );
}
