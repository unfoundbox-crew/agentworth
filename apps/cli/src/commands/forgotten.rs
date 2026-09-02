//! `agentworth forgotten` — the decisions a session's compaction rounds dropped.
//!
//! The same facts the `forgotten_context` MCP tool returns, rendered for a person. Redaction
//! follows the CLI's own convention rather than the tool's: `--redact` is opt-in here, because
//! this prints to the terminal of the person whose machine the transcript is already on, where
//! `forgotten_context` hands text to something else.

use std::path::PathBuf;
use std::sync::Arc;

use agentworth_core::Scanner;
use agentworth_redaction::Redactor;
use agentworth_schema::extract_repository_or_workspace;
use agentworth_storage::{SessionFilter, SessionOrderBy, Storage};
use anyhow::{Context, Result};

use crate::forgotten::{load_forgotten, ForgottenOptions, ForgottenReport, DEFAULT_LIMIT};
use crate::ui::views::{HandoffSection, HandoffView};
use crate::ui::{compact, Ui};

/// Statements shown per round before the screen starts reporting a dropped count. The MCP tool
/// spends a `limit` because a context window is the constraint there; a terminal scrolls, so
/// this is about what stays readable.
const TERMINAL_ROWS: usize = 10;

/// Resolves what the caller typed to one session.
///
/// An exact id wins, then a unique prefix — `agentworth inspect` (#76) made the prefix the
/// normal way to name a session and this follows it. With nothing typed, the newest session
/// for this directory's repository is what was meant.
fn resolve_session(storage: &Storage, session_id: Option<&str>, ui: &Ui) -> Result<String> {
    let Some(input) = session_id else {
        let repo = std::env::current_dir()
            .ok()
            .map(|d| extract_repository_or_workspace(&d.to_string_lossy()));
        if let Some(repo) = repo.as_deref() {
            if let Some(session) = storage
                .list_sessions_for_repo(repo, 1)?
                .sessions
                .into_iter()
                .next()
            {
                return Ok(session.session_id);
            }
            eprintln!("No indexed session for {repo}; falling back to the newest session anywhere.");
        }
        return storage
            .list_sessions_filtered(&SessionFilter {
                limit: Some(1),
                order_by: Some(SessionOrderBy::StartedAtDesc),
                ..Default::default()
            })?
            .into_iter()
            .next()
            .map(|s| s.session_id)
            .context("no sessions are indexed; run `agentworth scan` first");
    };

    if storage.get_session_by_id(input)?.is_some() {
        return Ok(input.to_string());
    }
    match storage.find_sessions_by_id_prefix(input, 2)?.as_slice() {
        [only] => Ok(only.session_id.clone()),
        _ => {
            print!("{}", not_found(storage, input, ui));
            std::process::exit(1);
        }
    }
}

fn not_found(storage: &Storage, session_id: &str, ui: &Ui) -> String {
    let needle = session_id.to_lowercase();
    let nearest: Vec<String> = storage
        .list_sessions_filtered(&SessionFilter {
            limit: Some(200),
            order_by: Some(SessionOrderBy::StartedAtDesc),
            ..Default::default()
        })
        .unwrap_or_default()
        .iter()
        .filter(|s| s.session_id.to_lowercase().contains(&needle))
        .take(3)
        .map(|s| format!("{}\t{}", s.session_id, s.started_at.format("%b %e %H:%M")))
        .collect();

    crate::ui::views::error(
        ui,
        &format!("agentworth forgotten {session_id}"),
        &format!("No indexed session starts with {session_id}."),
        "Closest three:",
        &nearest,
        &[
            (
                "agentworth forgotten".to_string(),
                "the newest session in this repo".to_string(),
            ),
            (
                "agentworth scan".to_string(),
                "re-index, if it should be here".to_string(),
            ),
        ],
    )
}

pub fn run_forgotten_command(
    session_id: Option<String>,
    round: Option<u32>,
    classes: Vec<String>,
    limit: Option<usize>,
    redact: bool,
    json: bool,
    db_path: Option<PathBuf>,
    ui: &Ui,
) -> Result<()> {
    let storage = match db_path {
        Some(path) => Arc::new(Storage::open_path(&path)?),
        None => Arc::new(Storage::open_default()?),
    };
    let resolved = resolve_session(&storage, session_id.as_deref(), ui)?;
    let scanner = Scanner::new(storage.clone());

    let options = ForgottenOptions {
        round,
        classes: crate::forgotten::parse_classes(&classes)?,
        limit: limit.unwrap_or(DEFAULT_LIMIT),
    };
    let (report, trace) = load_forgotten(&storage, &scanner, &resolved, &options)?;
    let report = if redact {
        report.redacted(&Redactor::new().for_trace(&trace))
    } else {
        report
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    print!("{}", render_terminal(&report, ui));
    Ok(())
}

fn render_terminal(report: &ForgottenReport, ui: &Ui) -> String {
    // One section per round, so the reader can see the shape of the loss round by round rather
    // than as one undifferentiated list. Rounds with nothing returned are still named in the
    // heading count, which is how "round 2 dropped 63 and kept 3" stays visible.
    let mut sections: Vec<HandoffSection> = Vec::new();
    let mut titles: Vec<String> = Vec::new();
    for round in &report.rounds {
        titles.push(format!(
            "Round {} — {} dropped, {} survived",
            round.round, round.dropped_total, round.summary_total
        ));
    }

    for (i, round) in report.rounds.iter().enumerate() {
        let rows: Vec<(String, String)> = report
            .forgotten
            .iter()
            .filter(|s| s.round == round.round)
            .take(TERMINAL_ROWS)
            .map(|s| {
                let acted = if s.followed_by.is_empty() { "" } else { " ·acted" };
                (format!("seq {}{}", s.sequence, acted), s.text.clone())
            })
            .collect();
        let shown = rows.len();
        let total = report
            .forgotten
            .iter()
            .filter(|s| s.round == round.round)
            .count();
        if total == 0 {
            continue;
        }
        sections.push(HandoffSection {
            title: &titles[i],
            total,
            dropped: total.saturating_sub(shown),
            rows,
        });
    }

    let cost = cost_line(report);
    let receipt = receipt_lines(report);
    let command = format!("agentworth forgotten {}", short(&report.receipt.session_id));

    crate::ui::views::handoff(
        ui,
        &HandoffView {
            command: &command,
            repo: &report.receipt.repo,
            task: Some(&report.headline),
            outcome: None,
            cost: &cost,
            sections: &sections,
            skipped: &[],
            receipt,
            next: Some((
                format!("agentworth inspect {}", short(&report.receipt.session_id)),
                "read the turns these sentences came from".to_string(),
            )),
        },
    )
}

fn cost_line(report: &ForgottenReport) -> String {
    let mut parts = vec![
        format!(
            "{} compaction round{}",
            report.compactions,
            if report.compactions == 1 { "" } else { "s" }
        ),
        format!("{} in", compact(report.dropped_total as u64)),
        format!("{} out", compact(report.survived_in_summary as u64)),
    ];
    if report.truncated {
        parts.push(format!("showing {} of {}", report.returned, report.forgotten_total));
    }
    if !report.notes.is_empty() {
        parts.push(report.notes.join(", "));
    }
    parts.join(" · ")
}

fn receipt_lines(report: &ForgottenReport) -> [String; 2] {
    [
        format!(
            "session {} · {} · {} · no model{}",
            report.receipt.session_id,
            report.receipt.adapter,
            report.receipt.method,
            if report.receipt.redacted { " · redacted" } else { "" }
        ),
        match report.receipt.index_last_updated {
            Some(t) => format!("index last updated {}", t.format("%Y-%m-%dT%H:%MZ")),
            None => "index last updated unknown".to_string(),
        },
    ]
}

fn short(session_id: &str) -> &str {
    let cut = session_id
        .char_indices()
        .nth(8)
        .map_or(session_id.len(), |(i, _)| i);
    &session_id[..cut]
}
