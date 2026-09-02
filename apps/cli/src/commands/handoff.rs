//! `agentworth handoff` and `agentworth loose-ends`.
//!
//! The same facts the `session_handoff` MCP tool returns, rendered for a person instead of an
//! agent. `README.md` and `SKILL.md` have claimed `agentworth loose-ends` existed since #44;
//! it did not — the detector shipped in the dashboard's TypeScript and nothing on the CLI
//! could reach it. It exists now, as a view onto the one section of the handoff it names.

use std::path::PathBuf;
use std::sync::Arc;

use agentworth_core::Scanner;
use agentworth_redaction::Redactor;
use agentworth_schema::extract_repository_or_workspace;
use agentworth_storage::{SessionFilter, SessionOrderBy, Storage};
use anyhow::{Context, Result};

use crate::handoff::{
    load_handoff, loose_ends_prompt_for, render_markdown, HandoffOptions, HandoffReport,
    DEFAULT_MAX_LINES, MAX_LINES_CEILING,
};
use crate::ui::views::{HandoffSection, HandoffView};
use crate::ui::{compact, thousands, Ui};

/// Rows each section shows in the terminal before it starts reporting a dropped count. The
/// markdown renderer spends a line budget because 60 lines is the contract there; a terminal
/// scrolls, so these are about what stays readable rather than what fits.
const TERMINAL_ROWS: usize = 12;

/// Resolves which session to hand over.
///
/// With no id and no `--last`, `--last` is what was meant: an agent or a person asking for
/// "the handoff" in a repo means the newest session there. Falls back to the newest session
/// anywhere only when this directory has none indexed, and says which it used.
fn resolve_session(storage: &Storage, session_id: Option<&str>, ui: &Ui) -> Result<String> {
    if let Some(id) = session_id {
        return match storage.get_session_by_id(id)? {
            Some(s) => Ok(s.session_id),
            None => {
                print!("{}", not_found(storage, id, ui));
                std::process::exit(1);
            }
        };
    }

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

    storage
        .list_sessions_filtered(&SessionFilter {
            limit: Some(1),
            order_by: Some(SessionOrderBy::StartedAtDesc),
            ..Default::default()
        })?
        .into_iter()
        .next()
        .map(|s| s.session_id)
        .context("no sessions are indexed; run `agentworth scan` first")
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
        &format!("agentworth handoff {session_id}"),
        &format!("No indexed session starts with {session_id}."),
        "Closest three:",
        &nearest,
        &[
            (
                "agentworth handoff".to_string(),
                "hand over the newest session in this repo".to_string(),
            ),
            (
                "agentworth scan".to_string(),
                "re-index, if it should be here".to_string(),
            ),
        ],
    )
}

fn open_storage(db_path: Option<PathBuf>) -> Result<Arc<Storage>> {
    if let Some(path) = db_path {
        Ok(Arc::new(Storage::open_path(&path)?))
    } else {
        Ok(Arc::new(Storage::open_default()?))
    }
}

/// Loads one session's report, applying `--redact` the same way the MCP tools apply
/// `include_raw: false` — one `Redactor::for_trace` instance across every field.
fn report_for(
    db_path: Option<PathBuf>,
    session_id: Option<&str>,
    redact: bool,
    options: HandoffOptions,
    ui: &Ui,
) -> Result<HandoffReport> {
    let storage = open_storage(db_path)?;
    let resolved = resolve_session(&storage, session_id, ui)?;
    let scanner = Scanner::new(storage.clone());
    let (report, trace) = load_handoff(&storage, &scanner, &resolved, options)?;
    Ok(if redact {
        report.redacted(&Redactor::new().for_trace(&trace))
    } else {
        report
    })
}

#[allow(clippy::too_many_arguments)]
pub fn run_handoff_command(
    session_id: Option<String>,
    redact: bool,
    max_lines: Option<usize>,
    markdown: bool,
    json: bool,
    db_path: Option<PathBuf>,
    ui: &Ui,
) -> Result<()> {
    let report = report_for(
        db_path,
        session_id.as_deref(),
        redact,
        HandoffOptions::default(),
        ui,
    )?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    if markdown {
        println!(
            "{}",
            render_markdown(&report, max_lines.unwrap_or(DEFAULT_MAX_LINES).min(MAX_LINES_CEILING))
        );
        return Ok(());
    }

    print!("{}", render_terminal(&report, ui));
    Ok(())
}

pub fn run_loose_ends_command(
    session_id: Option<String>,
    redact: bool,
    prompt: bool,
    json: bool,
    db_path: Option<PathBuf>,
    ui: &Ui,
) -> Result<()> {
    let report = report_for(
        db_path,
        session_id.as_deref(),
        redact,
        HandoffOptions::default(),
        ui,
    )?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report.loose_ends)?);
        return Ok(());
    }
    if prompt {
        // The copyable prompt `docs/specs/dropped-commitments.md` designs: hand the gap to
        // whatever already has the repository open, rather than writing the fix here.
        println!(
            "{}",
            loose_ends_prompt_for(&report.loose_ends, &report.receipt.session_id)
        );
        return Ok(());
    }

    let rows: Vec<(String, String)> = report
        .loose_ends
        .iter()
        .take(TERMINAL_ROWS)
        .map(|e| (format!("seq {}", e.sequence), e.text.clone()))
        .collect();
    let dropped = report.loose_ends.len().saturating_sub(rows.len());
    let sections = if report.loose_ends.is_empty() {
        Vec::new()
    } else {
        vec![HandoffSection {
            title: "Said it would, no evidence it did",
            total: report.loose_ends.len(),
            rows,
            dropped,
        }]
    };

    let cost = cost_line(&report);
    let receipt = receipt_lines(&report);
    let command = format!("agentworth loose-ends {}", short(&report.receipt.session_id));
    print!(
        "{}",
        crate::ui::views::handoff(
            ui,
            &HandoffView {
                command: &command,
                repo: &report.receipt.repo,
                task: report.task.as_deref(),
                outcome: report.outcome.as_ref().map(|o| (o.rung as usize, o.kind.clone())),
                cost: &cost,
                sections: &sections,
                skipped: &[],
                receipt,
                next: Some((
                    format!("agentworth handoff {}", short(&report.receipt.session_id)),
                    "the whole handoff, not just this section".to_string(),
                )),
            }
        )
    );
    Ok(())
}

fn render_terminal(report: &HandoffReport, ui: &Ui) -> String {
    let mut sections = Vec::new();

    if !report.loose_ends.is_empty() {
        sections.push(section(
            "Said it would, no evidence it did",
            report.loose_ends.len(),
            report
                .loose_ends
                .iter()
                .map(|e| (format!("seq {}", e.sequence), e.text.clone())),
        ));
    }
    if !report.ran.is_empty() {
        sections.push(section(
            "Ran",
            report.ran_total,
            report.ran.iter().map(|c| {
                (
                    c.at.format("%H:%M").to_string(),
                    format!("{}  {} {}", c.command, ui.dash(), c.ending()),
                )
            }),
        ));
    }
    if !report.files.is_empty() {
        sections.push(section(
            "Files touched",
            report.files_total,
            report.files.iter().map(|f| {
                (
                    f.last_at.format("%H:%M").to_string(),
                    format!(
                        "{}  {} {} edit{}",
                        f.path,
                        ui.dash(),
                        f.edits,
                        if f.edits == 1 { "" } else { "s" }
                    ),
                )
            }),
        ));
    }
    if !report.decided.is_empty() {
        sections.push(section(
            "Said it decided",
            report.decided.len(),
            report
                .decided
                .iter()
                .map(|s| (format!("seq {}", s.sequence), s.text.clone())),
        ));
    }

    let cost = cost_line(report);
    let receipt = receipt_lines(report);
    let command = format!("agentworth handoff {}", short(&report.receipt.session_id));
    crate::ui::views::handoff(
        ui,
        &HandoffView {
            command: &command,
            repo: &report.receipt.repo,
            task: report.task.as_deref(),
            outcome: report
                .outcome
                .as_ref()
                .map(|o| (o.rung as usize, o.kind.clone())),
            cost: &cost,
            sections: &sections,
            skipped: &[],
            receipt,
            next: Some((
                format!("agentworth inspect {}", short(&report.receipt.session_id)),
                "read the turns these lines came from".to_string(),
            )),
        },
    )
}

fn section<'a>(
    title: &'a str,
    total: usize,
    rows: impl Iterator<Item = (String, String)>,
) -> HandoffSection<'a> {
    let rows: Vec<(String, String)> = rows.take(TERMINAL_ROWS).collect();
    HandoffSection {
        title,
        total,
        dropped: total.saturating_sub(rows.len()),
        rows,
    }
}

fn cost_line(report: &HandoffReport) -> String {
    let mut parts = vec![
        format!("{} tokens", compact(report.total_tokens)),
        format!("{} events", thousands(report.total_events as u64)),
    ];
    if let Some(seconds) = report.duration_seconds {
        parts.push(crate::ui::duration(seconds));
    }
    if report.compactions > 0 {
        parts.push(format!(
            "{} compaction{} ({} dropped)",
            report.compactions,
            if report.compactions == 1 { "" } else { "s" },
            compact(report.compaction_tokens_dropped)
        ));
    }
    parts.join(" · ")
}

fn receipt_lines(report: &HandoffReport) -> [String; 2] {
    [
        format!(
            "session {} · {} · generated {}{}",
            report.receipt.session_id,
            report.receipt.adapter,
            report.receipt.generated_at.format("%Y-%m-%dT%H:%MZ"),
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
