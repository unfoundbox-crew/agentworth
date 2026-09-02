//! The one place every session-taking command turns "no id, a fuzzy one, or nothing" into
//! a concrete session id.
//!
//! `inspect`, `handoff`, `forgotten`, `export`, `receipt`, and `blunder-blame --session` all
//! hit the same three shapes: an exact id, a unique prefix, or nothing at all. Before this
//! module each command grew its own copy of "exact match, else prefix, else the newest
//! session for this repo" -- `handoff.rs` and `forgotten.rs` had already drifted, since only
//! one of the two did prefix matching. This is that logic, once, plus the interactive list a
//! TTY gets when nothing was typed and the JSON/plain fallback everything else gets.
//!
//! Rendering follows the rest of `ui`: one grid, `MAX_CONTENT` columns, the allowed glyph
//! set, `--plain`/`NO_COLOR`/non-TTY at byte-identical column positions.

use std::io::{self, BufRead, Write as _};

use agentworth_schema::extract_repository_or_workspace;
use agentworth_storage::{SessionFilter, SessionOrderBy, SessionSummary, Storage};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;

use super::{display_width, lpad, rpad, truncate, Role, Ui};

/// Rows shown per page, both in the interactive list and in the plain/JSON fallback.
pub const PAGE_SIZE: usize = 15;

/// What the caller typed, before resolution. Every session-taking command builds one of
/// these from its own `session_id: Option<String>`, `--last`, and `--current` flags.
#[derive(Debug, Clone, Default)]
pub struct SessionArg {
    pub id_or_prefix: Option<String>,
    pub last: bool,
    pub current: bool,
}

impl SessionArg {
    pub fn new(id_or_prefix: Option<String>, last: bool, current: bool) -> Self {
        Self {
            id_or_prefix,
            last,
            current,
        }
    }

    fn wants_last(&self) -> bool {
        self.last || self.current
    }
}

/// The outcome of resolving an explicit id or prefix.
pub enum Resolved {
    Id(String),
    /// The prefix matched more than one session. Never guessed at: `resolve` prints the
    /// candidates and exits 2, because picking one of several for the caller is how the
    /// wrong session gets exported.
    Ambiguous {
        input: String,
        candidates: Vec<SessionSummary>,
    },
    /// Nothing matched. The caller renders its own command-specific not-found screen
    /// (see `not_found` below).
    NotFound(String),
}

/// How many matches a prefix lookup fetches: enough to show the caller what it collided
/// with, bounded so an index of thousands never renders a wall.
const AMBIGUOUS_CANDIDATE_LIMIT: usize = 10;

/// Resolve a session for a command. This is the one entry point every session-taking
/// command calls.
///
/// - An explicit id or prefix (`arg.id_or_prefix`): exact match wins, else a unique
///   prefix, else `Resolved::NotFound`.
/// - `--last`/`--current`: the newest non-stub session for this directory's repository,
///   falling back to the newest session anywhere.
/// - Nothing at all, and stdout is not a TTY or `json` was asked for: the picker's
///   non-interactive contract -- print the same listing as JSON or a plain table and
///   exit 2 (`pass a session id or prefix`). This function does not return in that case.
/// - Nothing at all, on a TTY, without `--json`: the interactive picker.
pub fn resolve(storage: &Storage, ui: &Ui, json: bool, arg: &SessionArg) -> Result<Resolved> {
    if let Some(input) = &arg.id_or_prefix {
        return match resolve_explicit(storage, input)? {
            Resolved::Ambiguous { input, candidates } => {
                exit_ambiguous(ui, json, &input, &candidates)
            }
            other => Ok(other),
        };
    }
    if arg.wants_last() {
        return match resolve_last(storage)? {
            Some(id) => Ok(Resolved::Id(id)),
            None => anyhow::bail!("no sessions are indexed; run `agentworth scan` first"),
        };
    }
    if !is_tty() || json {
        let list = candidates(storage, PAGE_SIZE, 0)?;
        if json {
            println!("{}", render_json(&list));
        } else {
            print!("{}", render_list(ui, &list, "no session id given"));
        }
        eprintln!("pass a session id or prefix");
        std::process::exit(2);
    }
    Ok(Resolved::Id(interactive_pick(storage, ui)?))
}

/// The one call every show-style verb makes: `resolve` plus the two exits that follow from
/// it. `command_name` is the noun-verb spelling this command answers to (`session show`),
/// which is what the not-found screen echoes back.
pub fn resolve_or_exit(
    storage: &Storage,
    ui: &Ui,
    json: bool,
    command_name: &str,
    arg: &SessionArg,
) -> Result<String> {
    match resolve(storage, ui, json, arg)? {
        Resolved::Id(id) => Ok(id),
        Resolved::Ambiguous { input, candidates } => exit_ambiguous(ui, json, &input, &candidates),
        Resolved::NotFound(input) => {
            print!(
                "{}",
                not_found(
                    ui,
                    storage,
                    &format!("archie {command_name} {input}"),
                    &input,
                    &[(
                        format!("archie {command_name} --last"),
                        "the newest session in this repo".to_string(),
                    )],
                )
            );
            std::process::exit(1);
        }
    }
}

/// Exact match wins; failing that, a *unique* prefix resolves silently. An absent or
/// ambiguous prefix is left unresolved so the caller's own not-found screen (which already
/// lists the nearest ids) can take over.
pub fn resolve_explicit(storage: &Storage, input: &str) -> Result<Resolved> {
    if storage.get_session_by_id(input)?.is_some() {
        return Ok(Resolved::Id(input.to_string()));
    }
    let matches = storage.find_sessions_by_id_prefix(input, AMBIGUOUS_CANDIDATE_LIMIT)?;
    match matches.as_slice() {
        [] => Ok(Resolved::NotFound(input.to_string())),
        [only] => Ok(Resolved::Id(only.session_id.clone())),
        _ => Ok(Resolved::Ambiguous {
            input: input.to_string(),
            candidates: matches,
        }),
    }
}

/// Prints what a prefix collided with and exits 2. Exit 2 is the same code the
/// nothing-given-off-a-TTY path uses: both mean "this invocation named no single session",
/// which a script needs to tell apart from a session that simply is not there (exit 1).
pub fn exit_ambiguous(ui: &Ui, json: bool, input: &str, candidates: &[SessionSummary]) -> ! {
    let rows: Vec<Candidate> = candidates.iter().cloned().map(Candidate::from).collect();
    if json {
        println!("{}", render_json(&rows));
    } else {
        print!(
            "{}",
            render_list(ui, &rows, &format!("{input} matches {} sessions", rows.len()))
        );
    }
    eprintln!("ambiguous session prefix {input:?}; pass more characters");
    std::process::exit(2);
}

/// The newest non-stub session for the current directory's repository, falling back to the
/// newest session anywhere. `Ok(None)` means nothing is indexed at all.
pub fn resolve_last(storage: &Storage) -> Result<Option<String>> {
    if let Some(repo) = current_repo() {
        if let Some(session) = storage
            .list_sessions_for_repo(&repo, 1)?
            .sessions
            .into_iter()
            .next()
        {
            return Ok(Some(session.session_id));
        }
        eprintln!("No indexed session for {repo}; falling back to the newest session anywhere.");
    }
    Ok(storage
        .list_sessions_filtered(&SessionFilter {
            limit: Some(1),
            order_by: Some(SessionOrderBy::StartedAtDesc),
            ..Default::default()
        })?
        .into_iter()
        .next()
        .map(|s| s.session_id))
}

/// A generic not-found screen for a command whose own `Resolved::NotFound` came back.
/// `command_echo` is the header line (e.g. `agentworth export a1b2c3`); `hints` are the
/// command-specific next steps shown before the standing `agentworth scan` one.
pub fn not_found(
    ui: &Ui,
    storage: &Storage,
    command_echo: &str,
    input: &str,
    hints: &[(String, String)],
) -> String {
    let needle = input.to_lowercase();
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

    let mut next = hints.to_vec();
    next.push((
        "agentworth scan".to_string(),
        "re-index, if it should be here".to_string(),
    ));

    super::views::error(
        ui,
        command_echo,
        &format!("No indexed session starts with {input}."),
        "Closest three:",
        &nearest,
        &next,
    )
}

fn current_repo() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|d| extract_repository_or_workspace(&d.to_string_lossy()))
}

fn is_tty() -> bool {
    console::Term::stdout().is_term()
}

// -----------------------------------------------------------------------------
// candidates
// -----------------------------------------------------------------------------

/// One row of the picker: enough of `SessionSummary` to list and filter on, plus the
/// repository it was derived from.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub session_id: String,
    pub adapter: String,
    pub started_at: DateTime<Utc>,
    pub duration_seconds: Option<f64>,
    pub total_tokens: u64,
    pub rung: usize,
    pub prompt_preview: Option<String>,
    pub repo: String,
}

impl From<SessionSummary> for Candidate {
    fn from(s: SessionSummary) -> Self {
        let repo = extract_repository_or_workspace(&s.source_path);
        Candidate {
            session_id: s.session_id,
            adapter: s.adapter,
            started_at: s.started_at,
            duration_seconds: s.duration_seconds,
            total_tokens: s.total_tokens,
            rung: rung_from_outcome(s.primary_outcome.as_deref()),
            prompt_preview: s.prompt_preview,
            repo,
        }
    }
}

/// `sessions.primary_outcome` is stored as the snake_case name of `OutcomeKind`; a `NULL`
/// row is rung 0 -- unverified, not missing.
fn rung_from_outcome(outcome: Option<&str>) -> usize {
    match outcome {
        Some("ci_or_deployment_verified") => 5,
        Some("commit_observed") => 4,
        Some("test_or_build_passed") => 3,
        Some("artifact_changed") => 2,
        Some("done_claimed") => 1,
        _ => 0,
    }
}

/// The `limit` newest non-stub sessions starting at `offset`. On the first page only
/// (`offset == 0`), sessions matching the current directory's repository are stably
/// sorted ahead of the rest -- recency order is preserved within each group, so this is
/// a partition, not a re-sort by relevance.
pub fn candidates(storage: &Storage, limit: usize, offset: usize) -> Result<Vec<Candidate>> {
    let rows = storage.list_sessions_filtered(&SessionFilter {
        limit: Some(limit),
        offset: Some(offset),
        order_by: Some(SessionOrderBy::StartedAtDesc),
        ..Default::default()
    })?;
    let mut out: Vec<Candidate> = rows.into_iter().map(Candidate::from).collect();
    if offset == 0 {
        if let Some(here) = current_repo() {
            out.sort_by_key(|c| c.repo != here);
        }
    }
    Ok(out)
}

fn matches_filter(c: &Candidate, needle: &str) -> bool {
    c.session_id.to_lowercase().contains(needle)
        || c.repo.to_lowercase().contains(needle)
        || c.adapter.to_lowercase().contains(needle)
        || c
            .prompt_preview
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(needle)
}

// -----------------------------------------------------------------------------
// rendering
// -----------------------------------------------------------------------------

fn relative_time(when: DateTime<Utc>) -> String {
    let secs = (Utc::now() - when).num_seconds().max(0);
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else if secs < 604_800 {
        format!("{}d ago", secs / 86_400)
    } else {
        format!("{}w ago", secs / 604_800)
    }
}

fn rung_glyph(ui: &Ui, rung: usize) -> String {
    let on = super::views::EVIDENCE_FLOOR <= rung;
    let role = if on { Role::Verified } else { Role::Unverified };
    let glyph = if ui.ascii() {
        if on {
            "#"
        } else {
            "."
        }
    } else if on {
        "\u{25CF}"
    } else {
        "\u{25CB}"
    };
    ui.paint(role, glyph)
}

fn trim_row(line: &str) -> String {
    // Trailing spaces can hide inside a colour span (the reset sequence stops
    // `trim_end`), so this walks in from the end looking for an actually-empty span.
    let mut s = line.trim_end().to_string();
    while let Some(stripped) = s.strip_suffix("\x1b[0m") {
        let before = stripped.trim_end();
        if before.len() == stripped.len() {
            break;
        }
        s = format!("{}\x1b[0m", before);
    }
    s
}

fn push(out: &mut String, ui: &Ui, line: String) {
    let fitted = if display_width(&line) <= ui.width() {
        line
    } else {
        truncate(&line, ui.width())
    };
    out.push_str(&trim_row(&fitted));
    out.push('\n');
}

/// Render one page of the picker as a human-readable list: a numbered table plus the
/// prompt line. Used both for the interactive display and the non-interactive
/// plain-table fallback (`Ui::detect` has already turned off colour and switched to
/// ASCII for the latter, so this one function serves both).
pub fn render_list(ui: &Ui, rows: &[Candidate], subtitle: &str) -> String {
    let mut out = String::new();
    let i = ui.inner();

    out.push_str(&ui.header("agentworth", subtitle));
    out.push('\n');

    const NUM: usize = 2;
    const ID: usize = 8;
    const STARTED: usize = 8;
    const DUR: usize = 7;
    const TOK: usize = 8;
    const RUNG: usize = 2;
    const ADAPTER: usize = 8;
    let fixed = NUM + ID + STARTED + DUR + TOK + RUNG + ADAPTER;
    let gaps = 7 * 2; // one gap before each of the 7 fixed columns
    // The plain/non-TTY fallback appends the untruncated id after the short one (see
    // below), and that has to come out of the PROMPT column's budget up front -- `push`
    // truncates whatever doesn't fit the line, and a truncated "copyable" id would be
    // worse than no id at all.
    let full_id_w = if ui.ascii() {
        rows.iter().map(|c| display_width(&c.session_id)).max().unwrap_or(0) + 2
    } else {
        0
    };
    // No floor here on purpose: the appended full id (below) must never be the part that
    // gets cut when `push`'s width fit kicks in, so a long id is allowed to shrink the
    // prompt column all the way to nothing rather than reclaim space from it. If even a
    // zero-width prompt can't make the longest id fit (a very narrow terminal, or a very
    // long id), the id is dropped instead of appended and then clipped by `push`.
    let show_full_id = ui.ascii() && fixed + gaps + full_id_w <= i;
    let prompt_w = i.saturating_sub(fixed + gaps + full_id_w);

    let head = format!(
        "{}  {}  {}  {}  {}  {}  {}  {}",
        rpad("#", NUM),
        lpad("SESSION", ID),
        lpad("ADAPTER", ADAPTER),
        lpad("STARTED", STARTED),
        rpad("DUR", DUR),
        rpad("TOKENS", TOK),
        rpad("R", RUNG),
        "PROMPT",
    );
    push(&mut out, ui, format!("  {}", ui.paint(Role::Label, &head)));
    push(
        &mut out,
        ui,
        format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))),
    );

    for (n, c) in rows.iter().enumerate() {
        let short = truncate(&c.session_id, ID);
        let preview = c.prompt_preview.as_deref().unwrap_or("");
        let mut row = format!(
            "{}  {}  {}  {}  {}  {}  {}  {}",
            ui.paint(Role::Emphasis, &rpad(&(n + 1).to_string(), NUM)),
            ui.paint(Role::Value, &lpad(&short, ID)),
            ui.paint(Role::Value, &lpad(&truncate(&c.adapter, ADAPTER), ADAPTER)),
            ui.paint(Role::Label, &lpad(&relative_time(c.started_at), STARTED)),
            ui.paint(
                Role::Label,
                &rpad(
                    &c.duration_seconds
                        .map(super::duration)
                        .unwrap_or_else(|| ui.dash().into()),
                    DUR
                )
            ),
            ui.paint(Role::Value, &rpad(&super::compact(c.total_tokens), TOK)),
            rung_glyph(ui, c.rung),
            ui.paint(Role::Value, &truncate(preview, prompt_w)),
        );
        if show_full_id {
            // The plain/non-TTY path has no interactive selection, so the full id has to
            // be copyable straight off the line -- appended after the short one rather
            // than replacing it, so column positions stay identical to the coloured form.
            row.push_str(&format!("  {}", ui.paint(Role::Label, &c.session_id)));
        }
        push(&mut out, ui, format!("  {}", row));
    }

    push(
        &mut out,
        ui,
        format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))),
    );
    out
}

/// The prompt line shown under an interactive listing.
fn prompt_line(n_shown: usize) -> String {
    format!("pick 1-{n_shown}, type to filter, Enter for 1, q to quit")
}

#[derive(Serialize)]
struct JsonRow<'a> {
    number: usize,
    session_id: &'a str,
    adapter: &'a str,
    started_at: String,
    duration_seconds: Option<f64>,
    total_tokens: u64,
    rung: usize,
    prompt_preview: Option<&'a str>,
}

pub fn render_json(rows: &[Candidate]) -> String {
    let json_rows: Vec<JsonRow> = rows
        .iter()
        .enumerate()
        .map(|(n, c)| JsonRow {
            number: n + 1,
            session_id: &c.session_id,
            adapter: &c.adapter,
            started_at: c.started_at.to_rfc3339(),
            duration_seconds: c.duration_seconds,
            total_tokens: c.total_tokens,
            rung: c.rung,
            prompt_preview: c.prompt_preview.as_deref(),
        })
        .collect();
    serde_json::to_string_pretty(&json_rows).unwrap_or_else(|_| "[]".to_string())
}

// -----------------------------------------------------------------------------
// interactive loop
// -----------------------------------------------------------------------------

/// Raw stdin line reads -- no raw-mode terminal handling, no new crate. `dialoguer` and
/// `inquire` are not workspace dependencies and this doesn't need either: a plain
/// `read_line` covers "type a number", "type text to filter", "m" and "q".
fn interactive_pick(storage: &Storage, ui: &Ui) -> Result<String> {
    let mut pool = candidates(storage, PAGE_SIZE, 0)?;
    let mut filter = String::new();
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    loop {
        let shown: Vec<&Candidate> = if filter.is_empty() {
            pool.iter().collect()
        } else {
            pool.iter().filter(|c| matches_filter(c, &filter)).collect()
        };
        let shown: Vec<Candidate> = shown.into_iter().take(PAGE_SIZE).cloned().collect();

        if shown.is_empty() {
            println!("No sessions match '{filter}'. Type again, or press Enter to clear it.");
        } else {
            let subtitle = if filter.is_empty() {
                String::new()
            } else {
                format!("filter: {filter}")
            };
            print!("{}", render_list(ui, &shown, &subtitle));
        }
        print!("{}", prompt_line(shown.len().max(1)));
        io::stdout().flush().ok();
        println!();

        let Some(Ok(line)) = lines.next() else {
            anyhow::bail!("no input; pass a session id or prefix");
        };
        let input = line.trim();

        if input.is_empty() {
            if let Some(first) = shown.first() {
                return Ok(first.session_id.clone());
            }
            filter.clear();
            continue;
        }
        if input.eq_ignore_ascii_case("q") {
            eprintln!("cancelled");
            std::process::exit(1);
        }
        if input.eq_ignore_ascii_case("m") {
            let more = candidates(storage, PAGE_SIZE, pool.len())?;
            if more.is_empty() {
                println!("No more sessions indexed.");
            } else {
                pool.extend(more);
            }
            continue;
        }
        if let Ok(n) = input.parse::<usize>() {
            if n >= 1 && n <= shown.len() {
                return Ok(shown[n - 1].session_id.clone());
            }
            println!("'{n}' is out of range 1-{}.", shown.len());
            continue;
        }
        filter = input.to_lowercase();
    }
}
