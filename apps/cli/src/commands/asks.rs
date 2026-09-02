//! `archie session asks` — the questions-to-answers index for a session, rendered for a person.
//!
//! In a long session you ask a question, the answer lands several messages later among tool
//! notifications, and you re-ask it because scrolling costs time and re-asking costs tokens.
//! This finds it instead: `docs/specs/asks.md` is the design, `agentworth_outcomes::asks` is the
//! extraction, `crate::asks` assembles it into a report. This file is presentation only, same
//! division as `forgotten.rs` / `commands/forgotten.rs`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use agentworth_adapter_sdk::SessionSource;
use agentworth_core::Scanner;
use agentworth_outcomes::{Ask, AskStatus};
use agentworth_redaction::Redactor;
use agentworth_schema::{extract_repository_or_workspace, AgentWorthTrace};
use agentworth_storage::{SessionFilter, SessionOrderBy, Storage};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, TimeZone, Utc};

use crate::asks::{load_asks, note, AsksOptions, AsksReport, DEFAULT_LIMIT};
use crate::ui::views::{HandoffSection, HandoffView};
use crate::ui::{compact, Role, Ui};

/// The one adapter the raw-path fallback parses through, per the brief: a `--session` value
/// that isn't an indexed ID or prefix but does exist on disk is read as a Claude Code JSONL
/// transcript, bypassing the index entirely.
const RAW_PATH_ADAPTER: &str = "claude_code";

/// How a session was chosen, so the CLI can say it in a dim line rather than silently guessing.
enum Resolution {
    /// `--session` named an indexed session (exact ID or a unique prefix).
    Explicit,
    /// `--session` named a file, not an indexed session; parsed directly, no index involved.
    RawPath,
    /// Newest session whose derived repo matches the current directory.
    CurrentRepo(String),
    /// No session for this directory; fell back to the newest session anywhere.
    NewestAnywhere,
}

#[allow(clippy::too_many_arguments)]
pub fn run_asks_command(
    session: Option<String>,
    last: bool,
    current: bool,
    since: Option<String>,
    unanswered: bool,
    json: bool,
    db_path: Option<PathBuf>,
    ui: &Ui,
) -> Result<()> {
    let started = Instant::now();
    let since = since.as_deref().map(parse_since_flag).transpose()?;

    let storage = match db_path {
        Some(path) => Arc::new(Storage::open_path(&path)?),
        None => Arc::new(Storage::open_default()?),
    };
    let scanner = Scanner::new(storage.clone());

    let options = AsksOptions {
        since,
        unanswered_only: unanswered,
        limit: DEFAULT_LIMIT,
    };

    let (report, trace, resolution) = resolve_and_load(
        &storage,
        &scanner,
        session.as_deref(),
        last || current,
        json,
        ui,
        &options,
    )?;
    let report = report.redacted(&Redactor::new().for_trace(&trace));

    let elapsed = started.elapsed();

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        eprintln!("took {}ms, session {}", elapsed.as_millis(), trace.session_id);
        return Ok(());
    }

    print_resolution(ui, &resolution);
    print!("{}", render_terminal(&report, ui, &resolution));
    eprintln!(
        "{}",
        ui.paint(
            Role::Label,
            &format!("took {}ms · zero network calls", elapsed.as_millis())
        )
    );
    Ok(())
}

/// Resolves `--session` (an ID, a unique prefix, or a raw JSONL path), `--last`/`--current`,
/// or -- with none of those given -- the shared picker (`crate::ui::picker`), and loads the
/// asks report for whatever it resolves to.
///
/// The raw-JSONL-path fallback is `asks`'s own and the picker doesn't know about it, so it
/// stays a check here rather than moving into the shared resolver: a `--session` value that
/// isn't an indexed ID or prefix is tried as a path before giving up.
fn resolve_and_load(
    storage: &Storage,
    scanner: &Scanner,
    session: Option<&str>,
    wants_last: bool,
    json: bool,
    ui: &Ui,
    options: &AsksOptions,
) -> Result<(AsksReport, AgentWorthTrace, Resolution)> {
    if let Some(input) = session {
        match crate::ui::picker::resolve_explicit(storage, input)? {
            crate::ui::picker::Resolved::Id(id) => {
                let (report, trace) = load_asks(storage, scanner, &id, options)?;
                return Ok((report, trace, Resolution::Explicit));
            }
            crate::ui::picker::Resolved::Ambiguous { input, candidates } => {
                crate::ui::picker::exit_ambiguous(ui, json, &input, &candidates)
            }
            crate::ui::picker::Resolved::NotFound(_) => {
                if Path::new(input).is_file() {
                    let trace = load_trace_from_raw_path(Path::new(input))?;
                    let report = crate::asks::build_asks(&trace, None, options);
                    return Ok((report, trace, Resolution::RawPath));
                }
                print!("{}", not_found(storage, input, ui));
                std::process::exit(1);
            }
        }
    }

    // `--last`/`--current`: the exact current-repo-then-anywhere fallback `asks` always did,
    // kept as its own branch (rather than `crate::ui::picker::resolve_last`) so the CLI can
    // still say which of the two it used -- `resolve_last`'s own message is aimed at a
    // person reading past a not-found screen, not at this receipt-shaped resolution line.
    if wants_last {
        let repo = std::env::current_dir()
            .ok()
            .map(|d| extract_repository_or_workspace(&d.to_string_lossy()));
        if let Some(repo) = repo.as_deref() {
            if let Some(summary) = storage
                .list_sessions_for_repo(repo, 1)?
                .sessions
                .into_iter()
                .next()
            {
                let (report, trace) = load_asks(storage, scanner, &summary.session_id, options)?;
                return Ok((report, trace, Resolution::CurrentRepo(repo.to_string())));
            }
        }

        let newest = storage
            .list_sessions_filtered(&SessionFilter {
                limit: Some(1),
                order_by: Some(SessionOrderBy::StartedAtDesc),
                ..Default::default()
            })?
            .into_iter()
            .next()
            .context("no sessions are indexed; run `archie scan` first")?;
        let (report, trace) = load_asks(storage, scanner, &newest.session_id, options)?;
        return Ok((report, trace, Resolution::NewestAnywhere));
    }

    // Nothing given at all: the shared picker. On a TTY this is interactive; off a TTY, or
    // with `--json`, it prints the same listing and exits 2 rather than silently guessing.
    let arg = crate::ui::picker::SessionArg::new(None, false, false);
    match crate::ui::picker::resolve(storage, ui, json, &arg)? {
        crate::ui::picker::Resolved::Id(id) => {
            let (report, trace) = load_asks(storage, scanner, &id, options)?;
            Ok((report, trace, Resolution::Explicit))
        }
        crate::ui::picker::Resolved::NotFound(_) | crate::ui::picker::Resolved::Ambiguous { .. } => {
            unreachable!("picker::resolve with no id_or_prefix resolves or exits")
        }
    }
}

/// Parses a raw Claude Code JSONL transcript straight off disk, bypassing the index -- the
/// fallback for a `--session` value that names a real file rather than an indexed session.
fn load_trace_from_raw_path(path: &Path) -> Result<AgentWorthTrace> {
    let adapter = agentworth_adapters::all_adapters()
        .into_iter()
        .find(|a| a.name() == RAW_PATH_ADAPTER)
        .context("no claude_code adapter registered")?;
    let source = SessionSource::from_path(path, RAW_PATH_ADAPTER)
        .with_context(|| format!("could not read {}", path.display()))?;
    let parsed = adapter
        .parse(&source)
        .with_context(|| format!("could not parse {} as a Claude Code transcript", path.display()))?;
    Ok(parsed.trace)
}

fn print_resolution(ui: &Ui, resolution: &Resolution) {
    let line = match resolution {
        Resolution::Explicit | Resolution::RawPath => return,
        Resolution::CurrentRepo(repo) => format!("resolved via --current: newest session for {repo}"),
        Resolution::NewestAnywhere => {
            "no indexed session for this directory; using the newest session anywhere".to_string()
        }
    };
    eprintln!("{}", ui.paint(Role::Label, &line));
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
        &format!("archie session asks {session_id}"),
        &format!("No indexed session starts with {session_id}, and it isn't a file on disk either."),
        "Closest three:",
        &nearest,
        &[
            ("archie session asks".to_string(), "the newest session in this repo".to_string()),
            ("archie scan".to_string(), "re-index, if it should be here".to_string()),
        ],
    )
}

/// Parses `--since`: RFC 3339, a bare `YYYY-MM-DD`, or a relative duration like `2h`, `30m`,
/// `1d`, `3w` (subtracted from now).
fn parse_since_flag(value: &str) -> Result<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Some(naive) = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
    {
        return Ok(Utc.from_utc_datetime(&naive));
    }
    if let Some(duration) = parse_relative_duration(value) {
        return Ok(Utc::now() - duration);
    }
    anyhow::bail!(
        "--since must be RFC 3339, YYYY-MM-DD, or a relative duration like 2h/30m/1d/3w (got '{value}')"
    )
}

fn parse_relative_duration(value: &str) -> Option<Duration> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let split_at = value.len() - 1;
    let (num_str, unit) = value.split_at(split_at);
    let n: i64 = num_str.parse().ok()?;
    match unit {
        "s" => Some(Duration::seconds(n)),
        "m" => Some(Duration::minutes(n)),
        "h" => Some(Duration::hours(n)),
        "d" => Some(Duration::days(n)),
        "w" => Some(Duration::weeks(n)),
        _ => None,
    }
}

/// Rows shown in the terminal before the section reports a dropped count -- a screen scrolls,
/// so this is about what stays readable, same reasoning as `forgotten.rs`'s `TERMINAL_ROWS`.
const TERMINAL_ROWS: usize = 15;

fn render_terminal(report: &AsksReport, ui: &Ui, resolution: &Resolution) -> String {
    let all_rows: Vec<(String, String)> = report
        .asks
        .iter()
        .map(|a| (gutter_text(a), claim_text(a)))
        .collect();
    let dropped = all_rows.len().saturating_sub(TERMINAL_ROWS);
    let rows: Vec<(String, String)> = all_rows.into_iter().take(TERMINAL_ROWS).collect();

    let title = if report.asks.is_empty() {
        empty_title(report).to_string()
    } else {
        format!(
            "Questions — {} answered, {} flagged, {} no reply",
            report.answered, report.flagged_back_to_user, report.no_reply_yet
        )
    };

    let sections = vec![HandoffSection {
        title: &title,
        total: report.returned,
        dropped,
        rows,
    }];

    let cost = cost_line(report);
    let receipt = receipt_lines(report);
    let command = command_line(report, resolution);

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
                format!("archie session show {}", short(&report.receipt.session_id)),
                "read the turns these questions and answers came from".to_string(),
            )),
        },
    )
}

fn gutter_text(a: &Ask) -> String {
    let status = match a.status {
        AskStatus::Answered => "answered",
        AskStatus::FlaggedBackToUser => "flagged",
        AskStatus::NoReplyYet => "no-reply",
    };
    format!("seq {} {}", a.pointer.sequence, status)
}

fn claim_text(a: &Ask) -> String {
    match &a.answer_excerpt {
        Some(excerpt) => format!("{} — {}", a.question, excerpt),
        None => a.question.clone(),
    }
}

fn empty_title(report: &AsksReport) -> &'static str {
    if report.notes.iter().any(|n| n == note::NO_QUESTIONS) {
        "This session asked no questions"
    } else {
        "No questions matched --since / --unanswered"
    }
}

fn cost_line(report: &AsksReport) -> String {
    let mut parts = vec![
        format!("{} question{}", report.total_questions, plural(report.total_questions)),
        format!("{} answered", compact(report.answered as u64)),
        format!("{} unanswered", compact((report.flagged_back_to_user + report.no_reply_yet) as u64)),
    ];
    if report.truncated {
        parts.push(format!("showing {} of {}", report.returned, report.total_questions));
    }
    parts.join(" · ")
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn receipt_lines(report: &AsksReport) -> [String; 2] {
    [
        format!(
            "session {} · {} · regex_v1 · no model{}",
            report.receipt.session_id,
            report.receipt.adapter,
            if report.receipt.redacted { " · redacted" } else { "" }
        ),
        match report.receipt.index_last_updated {
            Some(t) => format!("index last updated {}", t.format("%Y-%m-%dT%H:%MZ")),
            None => "index last updated unknown (parsed directly from a raw path)".to_string(),
        },
    ]
}

fn command_line(report: &AsksReport, resolution: &Resolution) -> String {
    match resolution {
        Resolution::RawPath => "archie session asks --session <path>".to_string(),
        _ => format!("archie session asks {}", short(&report.receipt.session_id)),
    }
}

fn short(session_id: &str) -> &str {
    agentworth_schema::text::truncate_chars(session_id, 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn since_parses_relative_shorthand() {
        let now = Utc::now();
        let two_hours_ago = parse_since_flag("2h").unwrap();
        assert!((now - two_hours_ago - Duration::hours(2)).num_seconds().abs() < 5);
    }

    #[test]
    fn since_parses_rfc3339_and_bare_date() {
        assert!(parse_since_flag("2026-09-01T00:00:00Z").is_ok());
        assert!(parse_since_flag("2026-09-01").is_ok());
    }

    #[test]
    fn since_rejects_garbage() {
        assert!(parse_since_flag("not-a-time").is_err());
    }
}
