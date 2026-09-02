//! Every human-facing screen, rendered to a `String` so it can be snapshotted.
//!
//! Nothing here writes to stdout and nothing here touches a `--json` payload.

use std::fmt::Write;

use super::{
    archie, archie_inline, compact, display_width, duration, lpad, percent, percent_round, rpad,
    thousands, truncate, Lamp, Role, Ui, ARCHIE_BLOCK_MIN_COLUMNS,
};

/// Truncate an assembled row to the content width. Every row goes through this, so no
/// screen can wrap however narrow the window gets.
fn fit(ui: &Ui, line: &str) -> String {
    if display_width(line) <= ui.width() {
        line.to_string()
    } else {
        // Rows are assembled from already-painted fragments only in colour modes, where
        // escapes carry no width; cutting on chars would risk splitting one, so rows that
        // can overflow are assembled unpainted and painted per-cell instead.
        truncate(line, ui.width())
    }
}

/// Trailing spaces are invisible but they widen a line and dirty a diff, so no row keeps
/// the padding that squared its last column.
fn push(out: &mut String, ui: &Ui, line: String) {
    let _ = writeln!(out, "{}", trim_row(&fit(ui, &line)));
}

/// `Ui::section` right-pads its head line to the full inner width, which is exactly the
/// trailing padding `push` exists to strip -- so a section head always goes through it
/// one line at a time rather than being written raw.
fn section(out: &mut String, ui: &Ui, left: &str, right: &str) {
    for line in ui.section(left, right).lines() {
        push(out, ui, line.to_string());
    }
}

/// `trim_end` cannot see padding that sits *inside* a colour span, before the reset — so a
/// coloured row would keep trailing spaces a plain one dropped, and the two renderings
/// would stop agreeing on column positions.
fn trim_row(line: &str) -> String {
    const RESET: &str = "\u{1b}[0m";
    let mut s = line.trim_end_matches(' ').to_string();
    while let Some(rest) = s.strip_suffix(RESET) {
        let trimmed = rest.trim_end_matches(' ');
        if trimmed.len() != rest.len() {
            // Padding lived inside the span; keep the span, drop the padding.
            s = format!("{}{}", trimmed, RESET);
            s = s.trim_end_matches(' ').to_string();
            continue;
        }
        // An empty span — a painted cell that trimmed away to nothing. Drop the whole
        // span so the spaces before it can go too.
        match strip_trailing_sgr(trimmed) {
            Some(before) => s = before.trim_end_matches(' ').to_string(),
            None => break,
        }
    }
    s
}

/// Remove a trailing `ESC [ … m` sequence, if the string ends in one.
#[allow(
    clippy::string_slice,
    reason = "start comes from rfind() on an ASCII escape sequence, always a char boundary"
)]
fn strip_trailing_sgr(s: &str) -> Option<&str> {
    let start = s.rfind("\u{1b}[")?;
    let body = &s[start + 2..];
    if body.ends_with('m') && body[..body.len() - 1].chars().all(|c| c.is_ascii_digit() || c == ';')
    {
        Some(&s[..start])
    } else {
        None
    }
}

/// The severity labels `audit`, `blunder`, `blunder-blame` and `threat-digest` share
/// (`CRITICAL` / `HIGH` / `WARN` or `MEDIUM` / anything else reads as the dimmest tier),
/// mapped to one role each so the four commands agree on what "bad" looks like instead of
/// each picking its own raw colour. `Role` has no bespoke severity tiers of its own, so this
/// borrows four roles in descending urgency: `Error` > `Warn` > `Label` > `Unverified`.
pub fn severity_role(severity: &str) -> Role {
    match severity {
        "CRITICAL" => Role::Error,
        "HIGH" => Role::Warn,
        "WARN" | "MEDIUM" => Role::Label,
        _ => Role::Unverified,
    }
}

/// `[CRITICAL]`, `[HIGH]`, ... -- the bracketed tag every severity-bearing row leads with.
pub fn severity_tag(severity: &str) -> String {
    match severity {
        "CRITICAL" => "[CRITICAL]".to_string(),
        "HIGH" => "[HIGH]".to_string(),
        "WARN" => "[WARN]".to_string(),
        other if !other.is_empty() => format!("[{}]", other),
        _ => "[INFO]".to_string(),
    }
}

/// Word-wrap `text` to `max_width` columns. Splits on whitespace only, so it never cuts a
/// word (and, unlike a byte-offset slice, never cuts a multi-byte char either).
fn wrap(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if display_width(&current) + display_width(word) + 1 > max_width && !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    if lines.is_empty() {
        lines.push(text.to_string());
    }
    lines
}

/// Strip the vendor prefix and the release date, which are the same on every row and so
/// carry nothing. `claude-3-5-sonnet-20241022` reads as `3-5-sonnet`.
pub fn short_model(model: &str) -> String {
    let m = model
        .trim_start_matches("claude-")
        .trim_start_matches("models/")
        .trim_start_matches("gpt-");
    match m.rsplit_once('-') {
        Some((head, tail)) if tail.len() == 8 && tail.chars().all(|c| c.is_ascii_digit()) => {
            head.to_string()
        }
        _ => m.to_string(),
    }
}

/// The ladder, top rung first. Index is the rung number.
pub const RUNG_LABELS: [&str; 6] = [
    "unverified",
    "claimed done",
    "files changed",
    "tests passed",
    "commit landed",
    "CI green",
];

/// Rungs 3 and up are evidence; below is still a claim.
pub const EVIDENCE_FLOOR: usize = 3;

fn rung_role(rung: usize) -> Role {
    if rung >= EVIDENCE_FLOOR {
        Role::Verified
    } else {
        Role::Unverified
    }
}

// -----------------------------------------------------------------------------
// stats
// -----------------------------------------------------------------------------

pub struct StatsView<'a> {
    pub db_path: Option<&'a str>,
    pub total_sessions: usize,
    pub total_events: u64,
    pub first_day: Option<String>,
    pub last_day: Option<String>,
    /// Session count per rung, index 0..=5.
    pub rungs: [usize; 6],
    pub verified: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub adapters: Vec<(String, usize)>,
    pub models: Vec<(String, usize)>,
    pub tools: Vec<(String, usize)>,
    /// False when something else closes the screen. `overview` below sets it so the
    /// window block lands above the one closing line, rather than after it.
    pub show_next: bool,
}

pub fn stats(ui: &Ui, v: &StatsView<'_>) -> String {
    let mut out = String::new();
    let i = ui.inner();

    let right = v.db_path.map(shorten_home).unwrap_or_default();
    out.push_str(&ui.header("archie stats", &right));
    out.push('\n');

    // The one-line summary: what is indexed, over what span, how much of it.
    let sessions = format!("{} sessions", thousands(v.total_sessions as u64));
    let events = format!("{} events", thousands(v.total_events));
    let span = match (&v.first_day, &v.last_day) {
        (Some(a), Some(b)) => format!("{} {} {}", a, ui.arrow(), b),
        _ => String::new(),
    };
    let free = i.saturating_sub(display_width(&sessions) + display_width(&span) + display_width(&events));
    let gap = (free / 2).max(1);
    push(
        &mut out,
        ui,
        format!(
            "  {}{}{}{}{}",
            ui.paint(Role::Emphasis, &sessions),
            " ".repeat(gap),
            ui.paint(Role::Label, &span),
            " ".repeat(free.saturating_sub(gap).max(1)),
            ui.paint(Role::Emphasis, &events),
        ),
    );
    out.push('\n');

    // -- the evidence ladder --------------------------------------------------
    const SESS_W: usize = 9;
    const SHARE_W: usize = 10;
    let lead = i.saturating_sub(SESS_W + SHARE_W);

    push(
        &mut out,
        ui,
        format!(
            "  {}",
            ui.paint(
                Role::Label,
                &format!(
                    "{}{}{}",
                    lpad("EVIDENCE LADDER", lead),
                    rpad("SESSIONS", SESS_W),
                    rpad("SHARE", SHARE_W)
                )
            )
        ),
    );
    push(
        &mut out,
        ui,
        format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))),
    );

    for rung in (0..=5).rev() {
        if rung == EVIDENCE_FLOOR - 1 {
            push(
                &mut out,
                ui,
                format!(
                    "  {}",
                    ui.paint(Role::Chrome, &ui.titled_rule(i, "the evidence line"))
                ),
            );
        }
        let label = format!("{}  {}", rung, RUNG_LABELS[rung]);
        let count = v.rungs[rung];
        let dots = lead.saturating_sub(display_width(&label) + 2);
        push(
            &mut out,
            ui,
            format!(
                "  {} {} {}{}",
                ui.paint(rung_role(rung), &label),
                ui.paint(Role::Chrome, &".".repeat(dots)),
                ui.paint(Role::Value, &rpad(&thousands(count as u64), SESS_W)),
                ui.paint(Role::Label, &rpad(&percent(count, v.total_sessions), SHARE_W)),
            ),
        );
    }

    push(
        &mut out,
        ui,
        format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))),
    );
    // The one violet number on the screen.
    push(
        &mut out,
        ui,
        format!(
            "  {}{}{}",
            ui.paint(Role::Label, &lpad("   VERIFIED    rung 3 and up", lead)),
            ui.paint(Role::Verified, &rpad(&thousands(v.verified as u64), SESS_W)),
            ui.paint(Role::Label, &rpad(&percent(v.verified, v.total_sessions), SHARE_W)),
        ),
    );
    out.push('\n');

    // -- tokens ---------------------------------------------------------------
    let total = v.input_tokens + v.output_tokens + v.cache_read_tokens + v.cache_write_tokens;
    push(
        &mut out,
        ui,
        format!(
            "  {}",
            ui.paint(Role::Label, &{
                let total_label = format!("{} TOTAL", compact(total));
                format!(
                    "{}{}",
                    lpad("TOKENS", i.saturating_sub(display_width(&total_label))),
                    total_label
                )
            })
        ),
    );
    push(
        &mut out,
        ui,
        format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))),
    );

    const TOK_LABEL_W: usize = 12;
    const TOK_VAL_W: usize = 10;
    const TOK_PCT_W: usize = 8;
    let bar_w = i.saturating_sub(TOK_LABEL_W + TOK_VAL_W + TOK_PCT_W).max(4);
    let mut split = [
        ("cache read", v.cache_read_tokens),
        ("cache write", v.cache_write_tokens),
        ("output", v.output_tokens),
        ("input", v.input_tokens),
    ];
    split.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (label, n) in split {
        let frac = if total > 0 { n as f64 / total as f64 } else { 0.0 };
        push(
            &mut out,
            ui,
            format!(
                "  {}{}{}{}",
                ui.paint(Role::Label, &lpad(label, TOK_LABEL_W)),
                ui.paint(Role::Chrome, &ui.bar(frac, bar_w)),
                ui.paint(Role::Value, &rpad(&compact(n), TOK_VAL_W)),
                ui.paint(Role::Label, &rpad(&format!("{:.1}%", frac * 100.0), TOK_PCT_W)),
            ),
        );
    }
    out.push('\n');

    // -- adapters / models / tools -------------------------------------------
    if !v.adapters.is_empty() || !v.models.is_empty() || !v.tools.is_empty() {
        let body = i.saturating_sub(4);
        let c1 = (body * 25 / 72).max(8);
        let c2 = c1;
        let c3 = body.saturating_sub(c1 + c2).max(8);

        push(
            &mut out,
            ui,
            format!(
                "  {}",
                ui.paint(
                    Role::Label,
                    &format!(
                        "{}  {}  {}",
                        lpad("ADAPTERS", c1),
                        lpad("MODELS", c2),
                        lpad("TOOLS", c3)
                    )
                )
            ),
        );
        push(
            &mut out,
            ui,
            format!(
                "  {}  {}  {}",
                ui.paint(Role::Chrome, &ui.rule_of(c1)),
                ui.paint(Role::Chrome, &ui.rule_of(c2)),
                ui.paint(Role::Chrome, &ui.rule_of(c3)),
            ),
        );

        let rows = v.adapters.len().max(v.models.len()).max(v.tools.len()).min(3);
        for r in 0..rows {
            let a = v.adapters.get(r).map(|(n, c)| {
                format!(
                    "{}{}{}",
                    lpad(&truncate(n, c1.saturating_sub(12)), c1.saturating_sub(12)),
                    rpad(&thousands(*c as u64), 7),
                    rpad(&percent_round(*c, v.total_sessions), 5)
                )
            });
            let m = v.models.get(r).map(|(n, c)| {
                format!(
                    "{}{}",
                    lpad(&truncate(n, c2.saturating_sub(7)), c2.saturating_sub(7)),
                    rpad(&thousands(*c as u64), 7)
                )
            });
            let t = v.tools.get(r).map(|(n, c)| {
                format!(
                    "{}{}",
                    lpad(&truncate(n, c3.saturating_sub(9)), c3.saturating_sub(9)),
                    rpad(&thousands(*c as u64), 9)
                )
            });
            push(
                &mut out,
                ui,
                format!(
                    "  {}  {}  {}",
                    ui.paint(Role::Value, &lpad(a.as_deref().unwrap_or(""), c1)),
                    ui.paint(Role::Value, &lpad(m.as_deref().unwrap_or(""), c2)),
                    ui.paint(Role::Value, &lpad(t.as_deref().unwrap_or(""), c3)),
                ),
            );
        }
        out.push('\n');
    }

    if v.show_next {
        out.push_str(&ui.next(
            "archie session list --limit 20",
            "the newest sessions, ladder first",
        ));
    }
    out
}

// -----------------------------------------------------------------------------
// overview
// -----------------------------------------------------------------------------

/// The rolling window, as the overview draws it.
pub struct OverviewWindow {
    pub hours: i64,
    pub span: String,
    pub sessions: usize,
    pub events: usize,
    pub tokens: u64,
    pub burn_rate_tokens_per_hour: f64,
    pub cache_hit_ratio: f64,
    pub estimated_cost_usd: f64,
}

/// What `archie stats` prints, plus the current window.
///
/// The cockpit's first screen and, off a terminal, the whole of what a bare `archie`
/// prints. Both go through here, which is what keeps the cockpit from growing a
/// rendering of its own.
///
/// `show_next` is off inside the cockpit for the same reason `StatsView::show_next` is:
/// the closing line points at `archie tui`, and telling a reader who is already inside
/// the cockpit to open the cockpit is noise.
pub fn overview(
    ui: &Ui,
    stats_view: &StatsView<'_>,
    window: Option<&OverviewWindow>,
    show_next: bool,
) -> String {
    let i = ui.inner();
    let mut out = stats(ui, stats_view);

    match window {
        Some(w) => {
            section(&mut out, ui, &format!("WINDOW  last {}h", w.hours), &w.span);
            for (label, value, role) in [
                ("sessions active", thousands(w.sessions as u64), Role::Value),
                ("events", thousands(w.events as u64), Role::Value),
                ("tokens consumed", compact(w.tokens), Role::Value),
                (
                    "burn velocity",
                    format!("{:.1}M / hour", w.burn_rate_tokens_per_hour / 1_000_000.0),
                    Role::Value,
                ),
                ("cache hit ratio", format!("{:.1}%", w.cache_hit_ratio), Role::Value),
                (
                    "estimated cost",
                    format!("${:.2}", w.estimated_cost_usd),
                    Role::Verified,
                ),
            ] {
                push(&mut out, ui, ui.leaders(&format!("  {}", label), &value, i, role));
            }
            out.push('\n');
        }
        None => {
            section(&mut out, ui, "WINDOW", "");
            push(
                &mut out,
                ui,
                format!("  {}", ui.paint(Role::Label, "Nothing indexed in the last window.")),
            );
            out.push('\n');
        }
    }

    if show_next {
        out.push_str(&ui.next("archie tui", "the same data with a cursor"));
    }
    out
}

// -----------------------------------------------------------------------------
// traces
// -----------------------------------------------------------------------------

pub struct TraceRow {
    pub session_id: String,
    pub adapter: String,
    pub model: String,
    pub score: f64,
    pub rung: usize,
    pub duration_seconds: Option<f64>,
    pub total_tokens: u64,
}

pub fn traces(ui: &Ui, command: &str, indexed: usize, rows: &[TraceRow]) -> String {
    let mut out = String::new();
    let i = ui.inner();

    out.push_str(&ui.header(
        command,
        &format!(
            "{} indexed {} {} shown",
            thousands(indexed as u64),
            ui.dot(),
            rows.len()
        ),
    ));
    out.push('\n');

    // Fixed columns plus six two-space gaps; whatever is left goes to the three names.
    const EV: usize = 8;
    const SCORE: usize = 5;
    const DUR: usize = 7;
    const TOK: usize = 8;
    let fixed = EV + SCORE + DUR + TOK + 12;
    let rem = i.saturating_sub(fixed);
    let sess = (rem / 3 + 3).clamp(4, 14);
    let adapter = rem.saturating_sub(sess) / 2;
    let model = rem.saturating_sub(sess + adapter);

    let head = format!(
        "{}  {}  {}  {}  {}  {}  {}",
        lpad("EVIDENCE", EV),
        lpad("SESSION", sess),
        lpad("ADAPTER", adapter),
        lpad("MODEL", model),
        rpad("SCORE", SCORE),
        rpad("DUR", DUR),
        rpad("TOKENS", TOK),
    );
    push(&mut out, ui, format!("  {}", ui.paint(Role::Label, &head)));
    push(
        &mut out,
        ui,
        format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))),
    );

    for r in rows {
        push(
            &mut out,
            ui,
            format!(
                "  {}  {}  {}  {}  {}  {}  {}",
                ui.paint(rung_role(r.rung), &lpad(&ui.meter(r.rung), EV)),
                ui.paint(Role::Emphasis, &lpad(&truncate(&r.session_id, sess), sess)),
                ui.paint(Role::Value, &lpad(&truncate(&r.adapter, adapter), adapter)),
                ui.paint(Role::Value, &lpad(&truncate(&r.model, model), model)),
                ui.paint(Role::Value, &rpad(&format!("{:.0}", r.score), SCORE)),
                ui.paint(
                    Role::Label,
                    &rpad(&r.duration_seconds.map(duration).unwrap_or_else(|| ui.dash().into()), DUR)
                ),
                ui.paint(Role::Value, &rpad(&compact(r.total_tokens), TOK)),
            ),
        );
    }

    push(
        &mut out,
        ui,
        format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))),
    );

    // The closing line says the finding, not the row count.
    let with_evidence = rows.iter().filter(|r| r.rung >= EVIDENCE_FLOOR).count();
    let best = rows.iter().map(|r| r.rung).max().unwrap_or(0);
    let finding = if with_evidence == 0 {
        format!("None of these {} got past {}.", rows.len(), RUNG_LABELS[best])
    } else {
        format!(
            "{} of these {} left evidence; the rest are still claims.",
            with_evidence,
            rows.len()
        )
    };
    // The best row to look at next: the strongest evidence, else the newest.
    let pick = rows
        .iter()
        .max_by_key(|r| r.rung)
        .or_else(|| rows.first());
    // The SESSION column truncates, but this line must be runnable: `inspect` resolves an
    // exact id, not a prefix, so the closing command carries the whole thing.
    let hint = pick
        .map(|r| format!("archie session show {}", r.session_id))
        .unwrap_or_default();

    if display_width(&finding) + display_width(&hint) + 2 <= i {
        let gap = i - display_width(&finding) - display_width(&hint);
        push(
            &mut out,
            ui,
            format!(
                "  {}{}{}",
                ui.paint(Role::Value, &finding),
                " ".repeat(gap),
                ui.paint(Role::Emphasis, &hint)
            ),
        );
    } else {
        // Two lines rather than a truncated command: a next step that will not run is
        // worse than no next step.
        push(&mut out, ui, format!("  {}", ui.paint(Role::Value, &finding)));
        push(&mut out, ui, format!("  {}", ui.paint(Role::Emphasis, &hint)));
    }
    out
}

// -----------------------------------------------------------------------------
// usage
// -----------------------------------------------------------------------------

pub struct UsageRow {
    pub period: String,
    pub who: String,
    pub sessions: usize,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cost_usd: f64,
    /// False when the adapter keeps no token counts at all. A dash says not measured; a
    /// zero would assert something false.
    pub measured: bool,
}

/// Everything `usage()` needs beyond the rows themselves.
pub struct UsageView<'a> {
    pub command: &'a str,
    /// Column head for the group axis: `ADAPTER`, `MODEL`, or `REPO`.
    pub who_head: &'a str,
    /// Singular noun for one period (`day`/`week`/`month`/`year`); ignored when
    /// `show_period` is false.
    pub period_noun: &'a str,
    pub rows: &'a [UsageRow],
    /// False for `--period all`: no period axis, one row per group across all time, so the
    /// table drops the PERIOD column entirely rather than printing it empty.
    pub show_period: bool,
    /// The honest cost-basis sentence (`CostBasis::label_long`), word-wrapped under the
    /// header so a long subscription-tier name never breaks the grid.
    pub cost_note: &'a str,
    /// Set when `--limit` cut periods (or, under `--period all`, groups) out of the result --
    /// see `UsageReport::truncated`. Rendered as its own wrapped note below the table so the
    /// totals above it are never mistaken for the whole history.
    pub truncation_note: Option<&'a str>,
}

/// Break `text` into lines no wider than `width`, breaking only on spaces. A single word
/// longer than `width` (an unusually long subscription-tier name) is left intact rather than
/// cut mid-word -- `push` truncates it defensively if that's still too wide for the terminal.
fn wrap_note(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate_len = if current.is_empty() {
            word.len()
        } else {
            current.len() + 1 + word.len()
        };
        if candidate_len > width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// The four measured-value cells for one row, or four dashes when the adapter keeps no
/// token counts at all -- a dash says not measured, a zero would assert something false.
fn usage_measured_cells(ui: &Ui, r: &UsageRow) -> (String, String, String, String) {
    if r.measured {
        (
            compact(r.input),
            compact(r.output),
            compact(r.cache_read),
            format!("${:.2}", r.cost_usd),
        )
    } else {
        let d = ui.dash().to_string();
        (d.clone(), d.clone(), d.clone(), d)
    }
}

pub fn usage(ui: &Ui, v: &UsageView) -> String {
    let mut out = String::new();
    let i = ui.inner();
    let sessions: usize = v.rows.iter().map(|r| r.sessions).sum();

    let group_count = {
        let mut g: Vec<&str> = v.rows.iter().map(|r| r.who.as_str()).collect();
        g.sort_unstable();
        g.dedup();
        g.len()
    };
    let periods = {
        let mut p: Vec<&str> = v.rows.iter().map(|r| r.period.as_str()).collect();
        p.dedup();
        p.len()
    };
    let plural = |n: usize, noun: &str| if n == 1 { noun.to_string() } else { format!("{}s", noun) };

    out.push_str(&ui.header(
        v.command,
        &if v.show_period {
            format!(
                "{} {} {} {} sessions",
                periods,
                plural(periods, v.period_noun),
                ui.dot(),
                thousands(sessions as u64)
            )
        } else {
            format!(
                "{} {} {} {} sessions",
                group_count,
                plural(group_count, &v.who_head.to_lowercase()),
                ui.dot(),
                thousands(sessions as u64)
            )
        },
    ));
    out.push('\n');

    for line in wrap_note(v.cost_note, i) {
        push(&mut out, ui, format!("  {}", ui.paint(Role::Label, &line)));
    }
    out.push('\n');

    const P: usize = 10;
    const S: usize = 8;
    const N: usize = 8;
    // Exactly `crate::cost_basis::CostBasis::label_short()`'s width, so the column head and
    // every value beneath it line up without truncation.
    const C: usize = 13;
    let fixed = if v.show_period { P + S + N * 3 + C + 12 } else { S + N * 3 + C + 10 };
    let who = i.saturating_sub(fixed).max(6);
    let cost_head = crate::cost_basis::CostBasis::label_short();

    let header_cells = if v.show_period {
        format!(
            "{}  {}  {}  {}  {}  {}  {}",
            lpad("PERIOD", P),
            lpad(v.who_head, who),
            rpad("SESSIONS", S),
            rpad("INPUT", N),
            rpad("OUTPUT", N),
            rpad("CACHE RD", N),
            rpad(cost_head, C),
        )
    } else {
        format!(
            "{}  {}  {}  {}  {}  {}",
            lpad(v.who_head, who),
            rpad("SESSIONS", S),
            rpad("INPUT", N),
            rpad("OUTPUT", N),
            rpad("CACHE RD", N),
            rpad(cost_head, C),
        )
    };
    push(&mut out, ui, format!("  {}", ui.paint(Role::Label, &header_cells)));

    let mut any_dash = false;
    let mut last_period: Option<&str> = None;
    let mut total_cost = 0.0;
    for r in v.rows {
        if v.show_period {
            // The date column collapses after its first row, so the eye reads periods
            // rather than repetitions.
            let new_period = last_period != Some(r.period.as_str());
            if new_period {
                push(&mut out, ui, format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))));
            }
            let shown_period = if new_period { r.period.as_str() } else { "" };
            last_period = Some(r.period.as_str());
            total_cost += r.cost_usd;
            any_dash |= !r.measured;
            let (input, output, cache, cost) = usage_measured_cells(ui, r);

            push(
                &mut out,
                ui,
                format!(
                    "  {}  {}  {}  {}  {}  {}  {}",
                    ui.paint(Role::Label, &lpad(shown_period, P)),
                    ui.paint(Role::Value, &lpad(&truncate(&r.who, who), who)),
                    ui.paint(Role::Value, &rpad(&thousands(r.sessions as u64), S)),
                    ui.paint(Role::Value, &rpad(&input, N)),
                    ui.paint(Role::Value, &rpad(&output, N)),
                    ui.paint(Role::Value, &rpad(&cache, N)),
                    ui.paint(Role::Value, &rpad(&cost, C)),
                ),
            );
        } else {
            total_cost += r.cost_usd;
            any_dash |= !r.measured;
            let (input, output, cache, cost) = usage_measured_cells(ui, r);

            push(
                &mut out,
                ui,
                format!(
                    "  {}  {}  {}  {}  {}  {}",
                    ui.paint(Role::Value, &lpad(&truncate(&r.who, who), who)),
                    ui.paint(Role::Value, &rpad(&thousands(r.sessions as u64), S)),
                    ui.paint(Role::Value, &rpad(&input, N)),
                    ui.paint(Role::Value, &rpad(&output, N)),
                    ui.paint(Role::Value, &rpad(&cache, N)),
                    ui.paint(Role::Value, &rpad(&cost, C)),
                ),
            );
        }
    }

    push(&mut out, ui, format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))));

    if v.show_period {
        push(
            &mut out,
            ui,
            format!(
                "  {}  {}{}",
                ui.paint(Role::Label, &lpad(&format!("{} {}", periods, plural(periods, v.period_noun)), P)),
                ui.paint(Role::Emphasis, &rpad(&thousands(sessions as u64), who + S + 2)),
                // The one violet number: what the whole table cost.
                ui.paint(
                    Role::Verified,
                    &rpad(&format!("${:.2}", total_cost), i.saturating_sub(P + who + S + 4))
                ),
            ),
        );
    } else {
        push(
            &mut out,
            ui,
            format!(
                "  {}{}",
                ui.paint(Role::Emphasis, &rpad(&format!("{} TOTAL", thousands(sessions as u64)), who + S + 2)),
                ui.paint(
                    Role::Verified,
                    &rpad(&format!("${:.2}", total_cost), i.saturating_sub(who + S + 4))
                ),
            ),
        );
    }

    if any_dash {
        out.push('\n');
        push(
            &mut out,
            ui,
            format!(
                "  {}",
                ui.paint(
                    Role::Label,
                    &format!(
                        "A dash means the adapter keeps no token counts {} not that it burned none.",
                        ui.dash()
                    )
                )
            ),
        );
    }

    if let Some(note) = v.truncation_note {
        out.push('\n');
        for line in wrap_note(note, i) {
            push(&mut out, ui, format!("  {}", ui.paint(Role::Label, &line)));
        }
    }

    out
}

// -----------------------------------------------------------------------------
// blame
// -----------------------------------------------------------------------------

pub struct BlameRow {
    pub when: String,
    pub rung: usize,
    pub session_id: String,
    pub model: String,
    pub tool_calls: usize,
    pub action: String,
}

pub fn blame(ui: &Ui, path: &str, rows: &[BlameRow]) -> String {
    let mut out = String::new();
    let i = ui.inner();

    out.push_str(&ui.header(
        &format!("archie repo blame {}", path),
        &format!(
            "{} session{} touched it",
            rows.len(),
            if rows.len() == 1 { "" } else { "s" }
        ),
    ));
    out.push('\n');

    if rows.is_empty() {
        push(
            &mut out,
            ui,
            format!(
                "  {}",
                ui.paint(Role::Warn, "No indexed session touched a file matching that path.")
            ),
        );
        out.push('\n');
        out.push_str(&ui.next("archie scan", "re-index, if it should be here"));
        return out;
    }

    const WHEN: usize = 14;
    const EV: usize = 8;
    const SESS: usize = 12;
    const CALLS: usize = 6;
    const ACTION: usize = 10;
    let model = i.saturating_sub(WHEN + EV + SESS + CALLS + ACTION + 10).max(6);

    push(
        &mut out,
        ui,
        format!(
            "  {}",
            ui.paint(
                Role::Label,
                &format!(
                    "{}  {}  {}  {}  {}  {}",
                    lpad("WHEN", WHEN),
                    lpad("EVIDENCE", EV),
                    lpad("SESSION", SESS),
                    lpad("MODEL", model),
                    rpad("CALLS", CALLS),
                    lpad("ACTION", ACTION),
                )
            )
        ),
    );
    push(
        &mut out,
        ui,
        format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))),
    );

    for r in rows {
        push(
            &mut out,
            ui,
            format!(
                "  {}  {}  {}  {}  {}  {}",
                ui.paint(Role::Label, &lpad(&truncate(&r.when, WHEN), WHEN)),
                ui.paint(rung_role(r.rung), &lpad(&ui.meter(r.rung), EV)),
                ui.paint(Role::Emphasis, &lpad(&truncate(&r.session_id, SESS), SESS)),
                ui.paint(Role::Value, &lpad(&truncate(&r.model, model), model)),
                ui.paint(Role::Value, &rpad(&thousands(r.tool_calls as u64), CALLS)),
                ui.paint(Role::Value, &lpad(&truncate(&r.action, ACTION), ACTION)),
            ),
        );
    }

    push(
        &mut out,
        ui,
        format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))),
    );

    // The question is which edit is trustworthy, so the closing pair names the last
    // change that had evidence and what came after it.
    match rows.iter().find(|r| r.rung >= EVIDENCE_FLOOR) {
        Some(r) => {
            push(
                &mut out,
                ui,
                format!(
                    "  {}",
                    ui.paint(
                        Role::Value,
                        &format!(
                            "Last change with evidence: {}, {}.",
                            r.when, RUNG_LABELS[r.rung]
                        )
                    )
                ),
            );
            let after = rows.iter().take_while(|x| x.session_id != r.session_id).count();
            if after > 0 {
                push(
                    &mut out,
                    ui,
                    format!(
                        "  {}",
                        ui.paint(
                            Role::Unverified,
                            &format!("Everything after {} is still only a claim.", r.when)
                        )
                    ),
                );
            }
        }
        None => push(
            &mut out,
            ui,
            format!(
                "  {}",
                ui.paint(Role::Unverified, "No change to this file left any evidence.")
            ),
        ),
    }
    out
}

// -----------------------------------------------------------------------------
// doctor
// -----------------------------------------------------------------------------

pub struct DoctorAdapterRow {
    pub name: String,
    pub detected: bool,
    pub roots: usize,
}

pub struct DoctorView {
    pub os: String,
    pub arch: String,
    pub version: String,
    pub db_path: String,
    pub storage_healthy: bool,
    pub db_size_bytes: u64,
    pub total_indexed: usize,
    pub adapters: Vec<DoctorAdapterRow>,
}

pub fn doctor(ui: &Ui, v: &DoctorView) -> String {
    let mut out = String::new();
    let i = ui.inner();

    out.push_str(&ui.header("archie doctor", &format!("v{}", v.version)));
    out.push('\n');

    section(&mut out, ui, "ENVIRONMENT", "");
    push(
        &mut out,
        ui,
        ui.leaders("  os / arch", &format!("{}/{}", v.os, v.arch), ui.width(), Role::Value),
    );
    out.push('\n');

    section(&mut out, ui, "STORAGE", "");
    push(
        &mut out,
        ui,
        ui.leaders("  path", &shorten_home(&v.db_path), ui.width(), Role::Value),
    );
    let (state, state_role) = if v.storage_healthy {
        ("healthy, WAL mode", Role::Verified)
    } else {
        ("not found or uninitialized", Role::Error)
    };
    push(&mut out, ui, ui.leaders("  state", state, ui.width(), state_role));
    push(
        &mut out,
        ui,
        ui.leaders(
            "  size",
            &format!("{:.1} KB", v.db_size_bytes as f64 / 1024.0),
            ui.width(),
            Role::Value,
        ),
    );
    push(
        &mut out,
        ui,
        ui.leaders(
            "  indexed",
            &format!("{} sessions", thousands(v.total_indexed as u64)),
            ui.width(),
            Role::Value,
        ),
    );
    out.push('\n');

    let present = v.adapters.iter().filter(|a| a.detected).count();
    section(
        &mut out,
        ui,
        "ADAPTERS",
        &format!("{} of {} detected", present, v.adapters.len()),
    );

    const NAME: usize = 16;
    // 1 for the cell glyph, 2 gaps of two spaces around the name column.
    let roots_w = i.saturating_sub(1 + 2 + NAME + 2);
    for a in &v.adapters {
        let roots = if a.detected {
            format!("{} root{}", a.roots, if a.roots == 1 { "" } else { "s" })
        } else {
            String::new()
        };
        push(
            &mut out,
            ui,
            format!(
                "  {}  {}  {}",
                ui.paint(
                    if a.detected { Role::Verified } else { Role::Unverified },
                    ui.cell(a.detected)
                ),
                ui.paint(Role::Value, &lpad(&truncate(&a.name, NAME), NAME)),
                ui.paint(Role::Label, &rpad(&roots, roots_w)),
            ),
        );
    }
    out.push('\n');

    out.push_str(&ui.next("archie scan", "pick up anything new since the last index"));
    out
}

// -----------------------------------------------------------------------------
// matrix
// -----------------------------------------------------------------------------

pub struct MatrixRow {
    pub adapter: String,
    /// Live: is this adapter's tool actually found on this machine right now.
    pub detected: bool,
    /// Every registered adapter clears this floor -- it is the trait's own baseline.
    pub parse: bool,
    /// The adapter classifies completion/failure signals into the outcome hierarchy.
    pub outcomes: bool,
    /// The adapter captures both tool calls and shell output, the two event kinds the
    /// shared recovery-loop detector keys off (see `crates/outcomes/src/recovery.rs`).
    pub recoveries: bool,
    /// The adapter tracks per-session context compaction.
    pub compaction: bool,
}

pub fn matrix(ui: &Ui, coverage_pct: &str, rows: &[MatrixRow]) -> String {
    let mut out = String::new();
    let i = ui.inner();

    out.push_str(&ui.header(
        "archie agent list",
        &format!("{} adapters {} {} grounded coverage", rows.len(), ui.dot(), coverage_pct),
    ));
    out.push('\n');

    const COL: usize = 8;
    // The name column absorbs whatever the five fixed cells and their gaps don't need,
    // so the row -- like every other table in this module -- fills exactly `i`.
    let name = i.saturating_sub(COL * 5 + 10).max(10);
    push(
        &mut out,
        ui,
        format!(
            "  {}",
            ui.paint(
                Role::Label,
                &format!(
                    "{}  {}  {}  {}  {}  {}",
                    lpad("ADAPTER", name),
                    lpad("DETECT", COL),
                    lpad("PARSE", COL),
                    lpad("OUTCOMES", COL),
                    lpad("RECOVER", COL),
                    lpad("COMPACT", COL),
                )
            )
        ),
    );
    push(
        &mut out,
        ui,
        format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))),
    );

    let cell = |ui: &Ui, on: bool| -> String {
        ui.paint(if on { Role::Verified } else { Role::Unverified }, ui.cell(on))
    };

    for r in rows {
        push(
            &mut out,
            ui,
            format!(
                "  {}  {}  {}  {}  {}  {}",
                ui.paint(Role::Value, &lpad(&truncate(&r.adapter, name), name)),
                lpad(&cell(ui, r.detected), COL),
                lpad(&cell(ui, r.parse), COL),
                lpad(&cell(ui, r.outcomes), COL),
                lpad(&cell(ui, r.recoveries), COL),
                lpad(&cell(ui, r.compaction), COL),
            ),
        );
    }

    push(
        &mut out,
        ui,
        format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))),
    );
    out.push('\n');
    out.push_str(&ui.next(
        "archie doctor",
        "which of these are actually installed here",
    ));
    out
}

// -----------------------------------------------------------------------------
// scan
// -----------------------------------------------------------------------------

/// The first thing a new user ever sees: Archie, once, before the first scan, saying what
/// is about to happen and where it lands.
///
/// Printed only when the index is empty — a second run has nothing to introduce, and two
/// figures on one screen are two states with one of them lying. `--json` never gets it
/// (he is a state, not a texture, and machine-readable output carries no mascot);
/// `--plain` gets the same figure in the ASCII glyph set, which is what it already is.
pub fn scan_first_run(ui: &Ui, index_path: &str) -> String {
    let mut out = String::new();
    let index_path = shorten_home(index_path);

    if ui.width() < ARCHIE_BLOCK_MIN_COLUMNS {
        let budget = ui.inner().saturating_sub(24);
        push(
            &mut out,
            ui,
            format!(
                " {} {}  {}  {}",
                ui.paint(Role::Value, &archie_inline(Lamp::On)),
                ui.paint(Role::Label, "archie"),
                ui.paint(Role::Label, "first run"),
                ui.paint(Role::Label, &truncate(&index_path, budget)),
            ),
        );
        out.push('\n');
        return out;
    }

    let art = archie(Lamp::On);
    push(&mut out, ui, format!("  {}", ui.paint(Role::Chrome, &art[0])));
    push(
        &mut out,
        ui,
        format!(
            "  {}   {}  {}",
            ui.paint(Role::Value, &art[1]),
            ui.paint(Role::Label, "first run"),
            ui.paint(Role::Label, "nothing indexed here yet"),
        ),
    );
    push(
        &mut out,
        ui,
        format!(
            "  {}     {}",
            ui.paint(Role::Chrome, &art[2]),
            ui.paint(
                Role::Label,
                // 16 columns of figure and indent, 34 of sentence: the path gets what is
                // left, so `~/.agentworth/agentworth.db` lands whole at 80 columns.
                &format!(
                    "reading your agent histories into {}",
                    truncate(&index_path, ui.inner().saturating_sub(48))
                )
            ),
        ),
    );
    out.push('\n');
    out
}

/// One redraw of the progress block. Rendered without a trailing newline on the last
/// line so the caller can cursor back over exactly `scan_progress_lines(ui)` rows.
///
/// The lamp is driven off `frame` — the dig loop's four text frames, alternating held
/// and sweeping. Nothing else in the figure moves.
pub fn scan_progress(ui: &Ui, frame: u64, what: &str, done: usize, total: usize) -> String {
    let lamp = Lamp::dig_frame(frame);
    let frac = if total > 0 {
        done as f64 / total as f64
    } else {
        0.0
    };
    let percent_cell = rpad(&format!("{:.0}%", frac * 100.0), 4);

    let mut out = String::new();

    if ui.width() < ARCHIE_BLOCK_MIN_COLUMNS {
        let bar_w = ui.inner().saturating_sub(30).clamp(6, 20);
        push(
            &mut out,
            ui,
            format!(
                " {} {}  {}  {}  {}",
                ui.paint(Role::Value, &archie_inline(lamp)),
                ui.paint(Role::Label, "archie"),
                ui.paint(Role::Verified, &ui.bar(frac, bar_w)),
                ui.paint(Role::Emphasis, &percent_cell),
                ui.paint(Role::Label, &format!("{} sessions", thousands(done as u64))),
            ),
        );
        return out;
    }

    let art = archie(lamp);
    let bar_w = ui.inner().saturating_sub(40).clamp(8, 40);

    push(&mut out, ui, format!("  {}", ui.paint(Role::Chrome, &art[0])));
    push(
        &mut out,
        ui,
        format!(
            "  {}   {}  {}",
            ui.paint(Role::Value, &art[1]),
            ui.paint(Role::Label, "scanning"),
            ui.paint(Role::Label, &truncate(what, ui.inner().saturating_sub(26)))
        ),
    );
    push(
        &mut out,
        ui,
        format!(
            "  {}     {}  {}  {}",
            ui.paint(Role::Chrome, &art[2]),
            ui.paint(Role::Verified, &ui.bar(frac, bar_w)),
            ui.paint(Role::Emphasis, &percent_cell),
            ui.paint(
                Role::Label,
                &format!("{} / {}", thousands(done as u64), thousands(total as u64))
            ),
        ),
    );
    out
}

/// How many rows `scan_progress` draws at this width, so the caller knows how far to
/// cursor back before the next redraw.
pub fn scan_progress_lines(ui: &Ui) -> usize {
    if ui.width() < ARCHIE_BLOCK_MIN_COLUMNS {
        1
    } else {
        3
    }
}

pub struct ScanView {
    pub discovered: usize,
    pub scanned: usize,
    pub skipped: usize,
    pub backfilled: usize,
    pub reparsed: usize,
    pub errors: usize,
    pub total_indexed: usize,
    pub pruned: usize,
    pub total_tokens: u64,
    pub adapters: Vec<(String, usize)>,
}

pub fn scan_summary(ui: &Ui, v: &ScanView) -> String {
    let mut out = String::new();
    let i = ui.inner();
    // The completion frame holds the light: he found it and stopped sweeping.
    let art = archie(Lamp::On);

    // The completion frame keeps the same three lines so nothing jumps.
    push(&mut out, ui, format!("  {}", ui.paint(Role::Chrome, &art[0])));
    push(
        &mut out,
        ui,
        format!(
            "  {}   {}  {}",
            ui.paint(Role::Value, &art[1]),
            ui.paint(Role::Label, "indexed"),
            ui.paint(
                Role::Emphasis,
                &format!("{} sessions", thousands(v.total_indexed as u64))
            )
        ),
    );
    push(
        &mut out,
        ui,
        format!(
            "  {}     {}",
            ui.paint(Role::Chrome, &art[2]),
            ui.paint(
                Role::Label,
                &format!(
                    "{} new, {} unchanged, {} error{}{}{}{}",
                    thousands(v.scanned as u64),
                    thousands(v.skipped as u64),
                    v.errors,
                    if v.errors == 1 { "" } else { "s" },
                    if v.backfilled > 0 {
                        format!(", {} backfilled", thousands(v.backfilled as u64))
                    } else {
                        String::new()
                    },
                    if v.reparsed > 0 {
                        format!(
                            ", {} reparsed (newer parser)",
                            thousands(v.reparsed as u64)
                        )
                    } else {
                        String::new()
                    },
                    if v.pruned > 0 {
                        format!(", {} stale removed", thousands(v.pruned as u64))
                    } else {
                        String::new()
                    }
                )
            )
        ),
    );
    out.push('\n');

    push(
        &mut out,
        ui,
        format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))),
    );
    push(
        &mut out,
        ui,
        ui.leaders(
            "  sources discovered",
            &thousands(v.discovered as u64),
            ui.width(),
            Role::Value,
        ),
    );
    push(
        &mut out,
        ui,
        ui.leaders(
            "  tokens indexed",
            &compact(v.total_tokens),
            ui.width(),
            Role::Verified,
        ),
    );
    for (name, count) in v.adapters.iter().take(5) {
        push(
            &mut out,
            ui,
            ui.leaders(
                &format!("  {}", name),
                &thousands(*count as u64),
                ui.width(),
                Role::Value,
            ),
        );
    }
    out.push('\n');
    out.push_str(&ui.next("archie stats", "the ladder across everything indexed"));
    out
}

// -----------------------------------------------------------------------------
// error
// -----------------------------------------------------------------------------

/// An error screen is a navigation screen: name the failing noun, then the nearest
/// matches, then the commands that resolve it. No stack trace, no exit-code dump.
pub fn error(
    ui: &Ui,
    command: &str,
    noun: &str,
    nearest_head: &str,
    nearest: &[String],
    next: &[(String, String)],
) -> String {
    let mut out = String::new();

    out.push_str(&ui.header(command, ""));
    out.push('\n');

    // The lamp goes out first, and then nothing else moves. Below the block width the
    // figure collapses to the empty socket rather than crowding the sentence off-screen.
    if ui.width() < ARCHIE_BLOCK_MIN_COLUMNS {
        push(
            &mut out,
            ui,
            format!(
                " {} {}  {}",
                ui.paint(Role::Error, &archie_inline(Lamp::Off)),
                ui.paint(Role::Label, "archie"),
                ui.paint(Role::Error, noun)
            ),
        );
    } else {
        let art = archie(Lamp::Off);
        push(&mut out, ui, format!("  {}", ui.paint(Role::Chrome, &art[0])));
        push(
            &mut out,
            ui,
            format!(
                "  {}   {}",
                ui.paint(Role::Error, &art[1]),
                ui.paint(Role::Error, noun)
            ),
        );
        push(&mut out, ui, format!("  {}", ui.paint(Role::Chrome, &art[2])));
    }

    if !nearest.is_empty() {
        out.push('\n');
        push(
            &mut out,
            ui,
            format!("  {}", ui.paint(Role::Label, nearest_head)),
        );
        // Each suggestion is `id\tcontext`, so the ids form one column.
        let id_w = nearest
            .iter()
            .map(|n| display_width(n.split('\t').next().unwrap_or(n)))
            .max()
            .unwrap_or(0);
        for n in nearest {
            let (id, ctx) = n.split_once('\t').unwrap_or((n.as_str(), ""));
            push(
                &mut out,
                ui,
                format!(
                    "    {}   {}",
                    ui.paint(Role::Emphasis, &lpad(id, id_w)),
                    ui.paint(Role::Label, ctx)
                ),
            );
        }
    }

    if !next.is_empty() {
        out.push('\n');
        let w = next.iter().map(|(c, _)| display_width(c)).max().unwrap_or(0);
        for (cmd, why) in next {
            push(
                &mut out,
                ui,
                format!(
                    "  {}   {}",
                    ui.paint(Role::Emphasis, &lpad(cmd, w)),
                    ui.paint(Role::Label, why)
                ),
            );
        }
    }
    out
}

// -----------------------------------------------------------------------------
// receipt
// -----------------------------------------------------------------------------

pub struct ReceiptView {
    pub session_id: String,
    pub short_session_id: String,
    pub adapter: String,
    pub model: String,
    pub started: String,
    pub duration: String,
    pub turns: usize,
    pub tool_calls: usize,
    pub errors: usize,
    pub recoveries: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub total_tokens: u64,
    pub spend_usd: f64,
    pub rung: usize,
    pub verdict_label: String,
}

/// The one enclosed form in the product, because it is a receipt: what it was, what it
/// did, what it cost, and — last, where a receipt puts the total — what the evidence says.
pub fn receipt(ui: &Ui, v: &ReceiptView) -> String {
    const INNER: usize = 42;
    let indent = ui.width().saturating_sub(INNER + 4).min(8);
    let pad = " ".repeat(indent);

    let (tl, tr, bl, br, vb, hz) = if ui.ascii() {
        ("+", "+", "+", "+", "|", "-")
    } else {
        ("┌", "┐", "└", "┘", "│", "─")
    };

    let mut out = String::new();
    let row = |out: &mut String, content: String| {
        let w = display_width(&content);
        let _ = writeln!(
            out,
            "{}{} {}{} {}",
            pad,
            ui.paint(Role::Chrome, vb),
            content,
            " ".repeat(INNER.saturating_sub(w)),
            ui.paint(Role::Chrome, vb)
        );
    };

    let _ = writeln!(
        out,
        "{}{}",
        pad,
        ui.paint(Role::Chrome, &format!("{}{}{}", tl, hz.repeat(INNER + 2), tr))
    );
    row(&mut out, String::new());
    row(&mut out, ui.paint(Role::Emphasis, "A G E N T W O R T H"));
    row(&mut out, ui.paint(Role::Label, "FLIGHT RECEIPT"));
    row(&mut out, String::new());

    let kv = |k: &str, val: &str, role: Role| {
        format!(
            "{}{}",
            ui.paint(Role::Label, k),
            ui.paint(role, &rpad(val, INNER.saturating_sub(display_width(k))))
        )
    };
    row(&mut out, kv("SESSION", &truncate(&v.short_session_id, 24), Role::Value));
    row(&mut out, kv("ADAPTER", &v.adapter, Role::Value));
    row(&mut out, kv("MODEL", &truncate(&v.model, 24), Role::Value));
    row(&mut out, kv("STARTED", &v.started, Role::Value));
    row(&mut out, kv("DURATION", &v.duration, Role::Value));

    let divider = ui.paint(Role::Chrome, &hz.repeat(INNER));
    row(&mut out, divider.clone());
    row(&mut out, kv("turns", &thousands(v.turns as u64), Role::Value));
    row(&mut out, kv("tool calls", &thousands(v.tool_calls as u64), Role::Value));
    // A zero here is the finding, so it stays in grey rather than being hidden.
    row(
        &mut out,
        kv(
            "errors",
            &thousands(v.errors as u64),
            if v.errors == 0 { Role::Unverified } else { Role::Value },
        ),
    );
    row(
        &mut out,
        kv(
            "recoveries",
            &thousands(v.recoveries as u64),
            if v.recoveries == 0 { Role::Unverified } else { Role::Value },
        ),
    );

    row(&mut out, divider.clone());
    row(&mut out, kv("input", &compact(v.input_tokens), Role::Value));
    row(&mut out, kv("output", &compact(v.output_tokens), Role::Value));
    row(&mut out, kv("cache read", &compact(v.cache_read_tokens), Role::Value));

    row(&mut out, divider.clone());
    row(&mut out, kv("TOTAL", &compact(v.total_tokens), Role::Emphasis));
    row(
        &mut out,
        kv("EST. COST", &format!("${:.2}", v.spend_usd), Role::Emphasis),
    );

    row(&mut out, divider);
    let verdict = format!("{}  rung {}", ui.meter(v.rung), v.rung);
    row(
        &mut out,
        format!(
            "{}{}",
            ui.paint(Role::Label, "EVIDENCE"),
            ui.paint(rung_role(v.rung), &rpad(&verdict, INNER - 8))
        ),
    );
    row(&mut out, ui.paint(Role::Label, &truncate(&v.verdict_label, INNER)));
    row(&mut out, String::new());

    row(&mut out, ui.paint(Role::Value, &barcode(ui, &v.session_id, INNER)));
    row(&mut out, ui.paint(Role::Label, &truncate(&v.session_id, INNER)));
    row(&mut out, String::new());

    // The torn edge closes the receipt instead of a bottom border — a till roll has no
    // bottom. It is ASCII so it survives every font. `bl`/`br` stay unused for that reason.
    let _ = (bl, br);
    let _ = writeln!(
        out,
        "{}{}",
        pad,
        ui.paint(Role::Chrome, &"\\/".repeat((INNER + 4) / 2))
    );
    out
}

// -----------------------------------------------------------------------------
// suspect
// -----------------------------------------------------------------------------

pub struct SuspectSessionRow {
    pub session_id: String,
    pub model: String,
    pub rung: usize,
    /// Reason codes, already ordered.
    pub reasons: Vec<String>,
    pub risk_unknown: bool,
}

pub struct SuspectCommitRow {
    pub short_sha: String,
    pub subject: String,
    pub sessions: Vec<SuspectSessionRow>,
}

pub struct SuspectView<'a> {
    pub repo: &'a str,
    pub range: &'a str,
    pub commits_scanned: usize,
    pub attributed: usize,
    pub unattributed: usize,
    pub unanchored_blame_rows: usize,
    pub sessions_with_unknown_risk: usize,
    pub rows: Vec<SuspectCommitRow>,
    pub prompt: &'a str,
}

/// The screen answers one question — which commits to look at twice — and then hands over a
/// block to paste. Every number that could make the answer look better than it is (commits with
/// no session, evidence that could not be placed, sessions never examined) is printed on every
/// run, including the runs where it is zero: a caveat that only shows up when it bites teaches
/// a reader to trust the quiet runs.
pub fn suspect(ui: &Ui, v: &SuspectView<'_>) -> String {
    let mut out = String::new();
    let i = ui.inner();

    out.push_str(&ui.header(
        "archie repo suspect",
        &format!(
            "{} of {} commit{}",
            v.rows.len(),
            v.commits_scanned,
            if v.commits_scanned == 1 { "" } else { "s" }
        ),
    ));
    out.push('\n');

    push(
        &mut out,
        ui,
        format!(
            "  {}",
            ui.paint(Role::Label, &truncate(&shorten_home(v.repo), i.saturating_sub(2)))
        ),
    );
    push(
        &mut out,
        ui,
        format!("  {}", ui.paint(Role::Label, &truncate(v.range, i.saturating_sub(2)))),
    );
    out.push('\n');

    if v.commits_scanned == 0 {
        push(&mut out, ui, format!("  {}", ui.paint(Role::Warn, "No commits in that range.")));
        return out;
    }

    if v.rows.is_empty() {
        let line = if v.attributed == 0 {
            "No indexed session matched any commit in this range. Unknown, not clean."
        } else {
            "No commit in this range came from a session with a risk signal."
        };
        push(&mut out, ui, format!("  {}", ui.paint(Role::Value, line)));
    } else {
        const SHA: usize = 8;
        const EV: usize = 6;
        const SESS: usize = 10;
        let subject = i.saturating_sub(SHA + EV + SESS + 8).max(10);

        push(
            &mut out,
            ui,
            format!(
                "  {}",
                ui.paint(
                    Role::Label,
                    &format!(
                        "{}  {}  {}  {}",
                        rpad("COMMIT", SHA),
                        lpad("EVIDENCE", EV),
                        rpad("SUBJECT", subject),
                        rpad("SESSION", SESS),
                    )
                )
            ),
        );
        push(&mut out, ui, format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))));

        for row in &v.rows {
            for (n, s) in row.sessions.iter().enumerate() {
                // A commit with two authoring sessions prints its sha once; the second row is
                // clearly a continuation rather than a second commit.
                let (sha, subj) = if n == 0 {
                    (row.short_sha.as_str(), row.subject.as_str())
                } else {
                    ("", "")
                };
                push(
                    &mut out,
                    ui,
                    format!(
                        "  {}  {}  {}  {}",
                        ui.paint(Role::Emphasis, &rpad(&truncate(sha, SHA), SHA)),
                        ui.paint(rung_role(s.rung), &lpad(&ui.meter(s.rung), EV)),
                        ui.paint(Role::Value, &rpad(&truncate(subj, subject), subject)),
                        ui.paint(Role::Value, &rpad(&truncate(&s.session_id, SESS), SESS)),
                    ),
                );
                let why = format!(
                    "{} {} {}{}",
                    s.model,
                    ui.dash(),
                    s.reasons.join(", "),
                    if s.risk_unknown {
                        " (loops and demoted claims not yet scanned)"
                    } else {
                        ""
                    }
                );
                push(
                    &mut out,
                    ui,
                    format!(
                        "  {}{}",
                        " ".repeat(SHA + EV + 4),
                        ui.paint(
                            Role::Unverified,
                            &truncate(&why, i.saturating_sub(SHA + EV + 6))
                        )
                    ),
                );
            }
        }
        push(&mut out, ui, format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))));
    }

    out.push('\n');

    push(
        &mut out,
        ui,
        format!(
            "  {}",
            ui.paint(
                if v.unattributed > 0 { Role::Warn } else { Role::Label },
                &format!(
                    "{} commit{} had no indexed session at all. Unknown, not clean.",
                    v.unattributed,
                    if v.unattributed == 1 { "" } else { "s" }
                )
            )
        ),
    );
    push(
        &mut out,
        ui,
        format!(
            "  {}",
            ui.paint(
                Role::Label,
                &format!(
                    "{} blame row{} could not be placed in any repo, and were dropped.",
                    v.unanchored_blame_rows,
                    if v.unanchored_blame_rows == 1 { "" } else { "s" }
                )
            )
        ),
    );
    if v.sessions_with_unknown_risk > 0 {
        push(
            &mut out,
            ui,
            format!(
                "  {}",
                ui.paint(
                    Role::Label,
                    &format!(
                        "{} session{} never examined for loops or demoted claims. Run a scan.",
                        v.sessions_with_unknown_risk,
                        if v.sessions_with_unknown_risk == 1 { " was" } else { "s were" }
                    )
                )
            ),
        );
    }

    if !v.rows.is_empty() {
        out.push('\n');
        section(&mut out, ui, "COPY THIS TO THE AGENT", "");
        for line in v.prompt.lines() {
            push(
                &mut out,
                ui,
                format!("  {}", ui.paint(Role::Value, &truncate(line, i.saturating_sub(2)))),
            );
        }
    }

    out.push_str(&ui.next("archie repo suspect --hook", "install it as a pre-push note"));
    out
}

/// The session id, rendered as bars. Deterministic, decorative, and inside the allowed
/// block-element set.
fn barcode(ui: &Ui, session_id: &str, width: usize) -> String {
    // Wide bar, narrow bar, gap — weighted toward bars so the block reads as printed ink
    // rather than as a gappy dotted line.
    let glyphs: [&str; 8] = if ui.ascii() {
        ["##", "#", "#", " ", "##", "#", " ", "##"]
    } else {
        ["██", "▌", "▌", " ", "██", "▌", " ", "██"]
    };
    let mut out = String::new();
    for b in session_id.bytes() {
        let next = glyphs[(b % 8) as usize];
        if display_width(&out) + display_width(next) > width {
            break;
        }
        out.push_str(next);
    }
    lpad(out.trim_end(), width)
}

fn shorten_home(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && path.starts_with(&home) => {
            #[allow(
                clippy::string_slice,
                reason = "path.starts_with(&home) just verified home.len() is a char boundary"
            )]
            let rest = &path[home.len()..];
            format!("~{rest}")
        }
        _ => path.to_string(),
    }
}

// -----------------------------------------------------------------------------
// bisect
// -----------------------------------------------------------------------------

pub struct BisectView<'a> {
    pub session_id: &'a str,
    pub adapter: &'a str,
    pub total_events: usize,
    pub turn: Option<usize>,
    pub timestamp: Option<&'a str>,
    pub reason: Option<&'a str>,
    pub summary: &'a str,
    pub context: Option<&'a str>,
}

pub fn bisect(ui: &Ui, v: &BisectView<'_>) -> String {
    let mut out = String::new();
    out.push_str(&ui.header(
        &format!("archie session bisect {}", v.session_id),
        &format!("{} · {} events", v.adapter, thousands(v.total_events as u64)),
    ));
    out.push('\n');

    match v.turn {
        Some(turn) => {
            section(&mut out, ui, "INFLECTION POINT", &format!("event #{}", turn));
            if let Some(reason) = v.reason {
                push(&mut out, ui, ui.leaders("  reason", reason, ui.width(), Role::Warn));
            }
            if let Some(ts) = v.timestamp {
                push(&mut out, ui, ui.leaders("  at", ts, ui.width(), Role::Value));
            }
            out.push('\n');
            push(&mut out, ui, format!("  {}", ui.paint(Role::Value, v.summary)));
            if let Some(ctx) = v.context {
                let room = ui.inner().saturating_sub(2);
                push(&mut out, ui, format!("  {}", ui.paint(Role::Label, &truncate(ctx, room))));
            }
        }
        None => {
            section(&mut out, ui, "TRAJECTORY", "");
            push(
                &mut out,
                ui,
                format!("  {}", ui.paint(Role::Verified, "Clean -- no negative turning point found.")),
            );
        }
    }
    out.push('\n');
    out.push_str(&ui.next(
        &format!("archie session show {}", v.session_id),
        "read the full event timeline",
    ));
    out
}

// -----------------------------------------------------------------------------
// cache-doctor
// -----------------------------------------------------------------------------

pub struct CacheDoctorDropRow<'a> {
    pub turn_index: usize,
    pub previous_ratio: f64,
    pub new_ratio: f64,
    pub drop_pct: f64,
    pub cause: &'a str,
}

pub struct CacheDoctorView<'a> {
    pub session_id: &'a str,
    pub adapter: &'a str,
    pub average_hit_ratio: f64,
    pub drops: Vec<CacheDoctorDropRow<'a>>,
}

pub fn cache_doctor(ui: &Ui, v: &CacheDoctorView<'_>) -> String {
    let mut out = String::new();
    out.push_str(&ui.header(&format!("archie session cache {}", v.session_id), v.adapter));
    out.push('\n');

    section(&mut out, ui, "CACHE HEALTH", "");
    push(
        &mut out,
        ui,
        ui.leaders(
            "  lifetime hit ratio",
            &format!("{:.1}%", v.average_hit_ratio),
            ui.width(),
            Role::Value,
        ),
    );
    out.push('\n');

    if v.drops.is_empty() {
        section(&mut out, ui, "DEGRADATION", "");
        push(
            &mut out,
            ui,
            format!("  {}", ui.paint(Role::Verified, "No significant prompt cache degradation detected.")),
        );
    } else {
        section(&mut out, ui, "DEGRADATION", &format!("{} found", v.drops.len()));
        for drop in &v.drops {
            push(
                &mut out,
                ui,
                format!(
                    "  turn #{}  {}",
                    drop.turn_index,
                    ui.paint(
                        Role::Warn,
                        &format!("{:.1}% -> {:.1}% ({:.1}% drop)", drop.previous_ratio, drop.new_ratio, drop.drop_pct)
                    )
                ),
            );
            push(&mut out, ui, format!("    {}", ui.paint(Role::Label, drop.cause)));
        }
    }
    out.push('\n');
    out.push_str(&ui.next(
        &format!("archie session show {}", v.session_id),
        "read the full event timeline",
    ));
    out
}

// -----------------------------------------------------------------------------
// pr-blame
// -----------------------------------------------------------------------------

pub struct PrBlameRow<'a> {
    pub file_path: &'a str,
    pub ai_touched: bool,
    pub session_id: Option<&'a str>,
    pub adapter: Option<&'a str>,
    pub models: &'a [String],
    pub outcome: Option<&'a str>,
    pub confidence: Option<f64>,
}

pub struct PrBlameView<'a> {
    pub files_analyzed: usize,
    pub ai_authored_files: usize,
    pub rows: Vec<PrBlameRow<'a>>,
}

pub fn pr_blame(ui: &Ui, v: &PrBlameView<'_>) -> String {
    let mut out = String::new();
    out.push_str(&ui.header("archie repo pr-blame", ""));
    out.push('\n');

    section(&mut out, ui, "OVERLAY", "");
    push(
        &mut out,
        ui,
        ui.leaders("  changed files", &v.files_analyzed.to_string(), ui.width(), Role::Value),
    );
    push(
        &mut out,
        ui,
        ui.leaders(
            "  AI-authored",
            &format!("{} ({})", v.ai_authored_files, percent_round(v.ai_authored_files, v.files_analyzed.max(1))),
            ui.width(),
            Role::Verified,
        ),
    );
    out.push('\n');

    section(&mut out, ui, "FILES", "");
    for row in &v.rows {
        push(&mut out, ui, format!("  {}", ui.paint(Role::Emphasis, row.file_path)));
        if row.ai_touched {
            push(
                &mut out,
                ui,
                format!(
                    "    {}",
                    ui.paint(
                        Role::Value,
                        &format!(
                            "{} ({})",
                            row.session_id.unwrap_or("-"),
                            row.adapter.unwrap_or("-")
                        )
                    )
                ),
            );
            if !row.models.is_empty() {
                push(&mut out, ui, format!("    models: {}", ui.paint(Role::Label, &row.models.join(", "))));
            }
            if let Some(outcome) = row.outcome {
                push(
                    &mut out,
                    ui,
                    format!(
                        "    outcome: {}",
                        ui.paint(
                            Role::Verified,
                            &format!("{} ({:.0}% conf)", outcome, row.confidence.unwrap_or(0.5) * 100.0)
                        )
                    ),
                );
            }
        } else {
            push(&mut out, ui, format!("    {}", ui.paint(Role::Unverified, "human / unindexed")));
        }
    }
    out.push('\n');
    out.push_str(&ui.next("archie repo blame <path>", "the full session ladder for one file"));
    out
}

// -----------------------------------------------------------------------------
// blunder-blame
// -----------------------------------------------------------------------------

pub struct BlunderBlameTrailRow<'a> {
    pub severity: &'a str,
    pub title: &'a str,
    pub session_id: &'a str,
    pub model: &'a str,
    pub blamed_files: Vec<(String, String)>,
}

pub fn blunder_blame_trails(ui: &Ui, rows: &[BlunderBlameTrailRow<'_>]) -> String {
    let mut out = String::new();
    out.push_str(&ui.header("archie repo blunder-blame", &format!("{} traced", rows.len())));
    out.push('\n');

    for (i, row) in rows.iter().enumerate() {
        section(&mut out, ui, &format!("BLUNDER #{:02}", i + 1), &severity_tag(row.severity));
        push(
            &mut out,
            ui,
            format!("  {}", ui.paint(severity_role(row.severity), row.title)),
        );
        push(
            &mut out,
            ui,
            ui.leaders("  session", row.session_id, ui.width(), Role::Value),
        );
        push(&mut out, ui, ui.leaders("  model", &short_model(row.model), ui.width(), Role::Value));
        if row.blamed_files.is_empty() {
            push(&mut out, ui, format!("  {}", ui.paint(Role::Unverified, "no blamed files indexed")));
        } else {
            push(
                &mut out,
                ui,
                ui.leaders("  blamed files", &row.blamed_files.len().to_string(), ui.width(), Role::Label),
            );
            for (path, action) in &row.blamed_files {
                push(
                    &mut out,
                    ui,
                    format!("    {} [{}]", ui.paint(Role::Value, &truncate(path, ui.inner().saturating_sub(14))), action),
                );
            }
        }
        out.push('\n');
    }
    out.push_str(&ui.next("archie session blunder", "the full ranked exhibit list"));
    out
}

pub struct BlunderBlameFileMatchRow<'a> {
    pub session_id: &'a str,
    pub model: &'a str,
    pub blunder: Option<(&'a str, &'a str)>,
}

pub fn blunder_blame_file_report(ui: &Ui, file_path: &str, rows: &[BlunderBlameFileMatchRow<'_>]) -> String {
    let mut out = String::new();
    out.push_str(&ui.header(&format!("archie repo blunder-blame --file {}", file_path), ""));
    out.push('\n');

    if rows.is_empty() {
        section(&mut out, ui, "SESSIONS", "");
        push(&mut out, ui, format!("  {}", ui.paint(Role::Unverified, "No indexed sessions touched this file.")));
    } else {
        section(&mut out, ui, "SESSIONS", &format!("{}", rows.len()));
        for row in rows {
            push(&mut out, ui, ui.leaders("  session", row.session_id, ui.width(), Role::Value));
            push(&mut out, ui, ui.leaders("    model", &short_model(row.model), ui.width(), Role::Value));
            match row.blunder {
                Some((severity, title)) => push(
                    &mut out,
                    ui,
                    format!("    {} {}", ui.paint(severity_role(severity), &severity_tag(severity)), title),
                ),
                None => push(&mut out, ui, format!("    {}", ui.paint(Role::Verified, "no recorded blunder"))),
            }
        }
    }
    out.push('\n');
    out.push_str(&ui.next("archie session blunder", "the full ranked exhibit list"));
    out
}

// -----------------------------------------------------------------------------
// merge
// -----------------------------------------------------------------------------

pub struct MergeView<'a> {
    pub target_name: &'a str,
    pub source_name: &'a str,
    pub sessions_inserted: usize,
    pub sessions_updated: usize,
    pub sessions_skipped: usize,
    pub sources_merged: usize,
    pub files_merged: usize,
    pub child_rows_merged: usize,
}

pub fn merge(ui: &Ui, v: &MergeView<'_>) -> String {
    let mut out = String::new();
    out.push_str(&ui.header(&format!("archie merge {}", v.source_name), v.target_name));
    out.push('\n');

    section(&mut out, ui, "SESSIONS", "");
    push(&mut out, ui, ui.leaders("  inserted", &thousands(v.sessions_inserted as u64), ui.width(), Role::Verified));
    push(&mut out, ui, ui.leaders("  updated", &thousands(v.sessions_updated as u64), ui.width(), Role::Value));
    push(&mut out, ui, ui.leaders("  skipped (already current)", &thousands(v.sessions_skipped as u64), ui.width(), Role::Unverified));
    out.push('\n');

    section(&mut out, ui, "CHILD ROWS", "");
    push(&mut out, ui, ui.leaders("  sources", &thousands(v.sources_merged as u64), ui.width(), Role::Value));
    push(&mut out, ui, ui.leaders("  file modifications", &thousands(v.files_merged as u64), ui.width(), Role::Value));
    push(&mut out, ui, ui.leaders("  other", &thousands(v.child_rows_merged as u64), ui.width(), Role::Value));
    out.push('\n');

    out.push_str(&ui.next("archie stats", "see the merged index's totals"));
    out
}

// -----------------------------------------------------------------------------
// watch
// -----------------------------------------------------------------------------

pub fn watch_banner(ui: &Ui) -> String {
    let mut out = String::new();
    out.push_str(&ui.header("archie session watch", "polling for doom loops"));
    out.push('\n');
    out
}

pub fn watch_clean(ui: &Ui, at: &str) -> String {
    let mut out = String::new();
    push(
        &mut out,
        ui,
        format!("  {}", ui.paint(Role::Verified, &format!("[{}] all monitored sessions normal", at))),
    );
    out
}

pub struct WatchAlertRow<'a> {
    pub session_id: &'a str,
    pub kind: &'a str,
    pub target: &'a str,
    pub repeat_count: usize,
    pub outcome: &'a str,
    pub outcome_role: Role,
}

pub fn watch_alerts(ui: &Ui, rows: &[WatchAlertRow<'_>]) -> String {
    let mut out = String::new();
    section(&mut out, ui, "LOOP ALERT", &format!("{} found", rows.len()));
    for row in rows {
        push(&mut out, ui, ui.leaders("  session", row.session_id, ui.width(), Role::Value));
        push(&mut out, ui, ui.leaders("  type", row.kind, ui.width(), Role::Warn));
        push(&mut out, ui, ui.leaders("  target", row.target, ui.width(), Role::Value));
        push(
            &mut out,
            ui,
            ui.leaders("  repeats", &format!("{} iterations", row.repeat_count), ui.width(), Role::Warn),
        );
        push(&mut out, ui, ui.leaders("  outcome", row.outcome, ui.width(), row.outcome_role));
        out.push('\n');
    }
    out
}

// -----------------------------------------------------------------------------
// blind-spots
// -----------------------------------------------------------------------------

pub struct BlindSpotRow<'a> {
    pub session_id: &'a str,
    pub adapter: &'a str,
    pub outcome: &'a str,
    pub spend_usd: f64,
}

pub struct BlindSpotsView<'a> {
    pub total_blind_spots: usize,
    pub total_unverified_tokens: u64,
    pub total_unverified_spend_usd: f64,
    pub rows: Vec<BlindSpotRow<'a>>,
}

pub fn blind_spots(ui: &Ui, v: &BlindSpotsView<'_>) -> String {
    let mut out = String::new();
    out.push_str(&ui.header("archie session list --unproven", ""));
    out.push('\n');

    section(&mut out, ui, "UNVERIFIED", "");
    push(
        &mut out,
        ui,
        ui.leaders("  sessions", &thousands(v.total_blind_spots as u64), ui.width(), Role::Warn),
    );
    push(&mut out, ui, ui.leaders("  tokens burned", &compact(v.total_unverified_tokens), ui.width(), Role::Value));
    push(
        &mut out,
        ui,
        ui.leaders("  spend", &format!("${:.2}", v.total_unverified_spend_usd), ui.width(), Role::Value),
    );
    out.push('\n');

    if v.rows.is_empty() {
        section(&mut out, ui, "SESSIONS", "");
        push(
            &mut out,
            ui,
            format!("  {}", ui.paint(Role::Verified, "Every indexed session is verified by CI/tests.")),
        );
    } else {
        section(&mut out, ui, "SESSIONS", &format!("top {}", v.rows.len()));
        for row in &v.rows {
            push(
                &mut out,
                ui,
                format!(
                    "  {}  {}  {}  ${:.2}",
                    ui.paint(Role::Emphasis, row.session_id),
                    ui.paint(Role::Label, row.adapter),
                    ui.paint(Role::Warn, row.outcome),
                    row.spend_usd
                ),
            );
        }
    }
    out.push('\n');
    out.push_str(&ui.next("archie session show <session-id>", "read the full event timeline"));
    out
}

// -----------------------------------------------------------------------------
// audit
// -----------------------------------------------------------------------------

pub struct AuditFindingRow<'a> {
    pub severity: &'a str,
    pub title: &'a str,
    pub rule_id: &'a str,
    pub project: &'a str,
    pub session_id: &'a str,
    pub adapter: &'a str,
    pub timestamp: &'a str,
    pub turn_index: usize,
    pub description: &'a str,
    pub offending_snippet: &'a str,
}

pub struct AuditView<'a> {
    pub total_sessions_audited: usize,
    pub critical_count: usize,
    pub high_count: usize,
    pub warn_count: usize,
    pub findings: Vec<AuditFindingRow<'a>>,
}

pub fn audit(ui: &Ui, v: &AuditView<'_>) -> String {
    let mut out = String::new();
    out.push_str(&ui.header(
        "archie session audit",
        &format!("{} sessions", thousands(v.total_sessions_audited as u64)),
    ));
    out.push('\n');

    section(&mut out, ui, "THREAT SUMMARY", "");
    push(&mut out, ui, ui.leaders("  critical", &v.critical_count.to_string(), ui.width(), Role::Error));
    push(&mut out, ui, ui.leaders("  high", &v.high_count.to_string(), ui.width(), Role::Warn));
    push(&mut out, ui, ui.leaders("  warn", &v.warn_count.to_string(), ui.width(), Role::Label));
    out.push('\n');

    if v.findings.is_empty() {
        section(&mut out, ui, "FINDINGS", "");
        push(
            &mut out,
            ui,
            format!("  {}", ui.paint(Role::Verified, "No dangerous tool calls or leaked secrets found.")),
        );
        out.push('\n');
        out.push_str(&ui.next("archie scan", "pick up anything new since the last index"));
        return out;
    }

    section(&mut out, ui, "FINDINGS", &format!("{}", v.findings.len()));
    let wrap_width = ui.inner().saturating_sub(2);
    for (i, f) in v.findings.iter().enumerate() {
        push(
            &mut out,
            ui,
            format!(
                "  #{:02}  {}  {}",
                i + 1,
                ui.paint(severity_role(f.severity), &severity_tag(f.severity)),
                ui.paint(Role::Emphasis, f.title)
            ),
        );
        push(&mut out, ui, format!("    rule: {}   project: {}", f.rule_id, f.project));
        push(&mut out, ui, format!("    session: {}   adapter: {}", f.session_id, f.adapter));
        push(&mut out, ui, format!("    at: {}   turn #{}", f.timestamp, f.turn_index));
        for line in wrap(f.description, wrap_width) {
            push(&mut out, ui, format!("    {}", ui.paint(Role::Label, &line)));
        }
        for line in wrap(f.offending_snippet, wrap_width) {
            push(&mut out, ui, format!("    {}", ui.paint(Role::Error, &line)));
        }
        out.push('\n');
    }
    out.push_str(&ui.next(
        "archie session export <session-id> --redact",
        "the full session with secrets stripped",
    ));
    out
}

// -----------------------------------------------------------------------------
// blunder
// -----------------------------------------------------------------------------

pub struct BlunderExhibitRow<'a> {
    pub severity: &'a str,
    pub title: &'a str,
    pub rule_id: &'a str,
    pub project: &'a str,
    pub model: &'a str,
    pub adapter: &'a str,
    pub tokens: u64,
    pub spend_usd: f64,
    pub turns: usize,
    pub apology_count: usize,
    pub apology_quote: &'a str,
    pub code_snippet: &'a str,
    pub session_hash: &'a str,
}

pub fn blunder(ui: &Ui, rows: &[BlunderExhibitRow<'_>]) -> String {
    let mut out = String::new();
    out.push_str(&ui.header("archie session blunder", &format!("{} exhibit(s)", rows.len())));
    out.push('\n');

    let wrap_width = ui.inner().saturating_sub(2);
    for (i, ex) in rows.iter().enumerate() {
        section(&mut out, ui, &format!("EXHIBIT #{:02}", i + 1), &severity_tag(ex.severity));
        push(&mut out, ui, format!("  {}", ui.paint(severity_role(ex.severity), ex.title)));
        push(&mut out, ui, format!("    rule: {}   project: {}", ex.rule_id, ex.project));
        push(&mut out, ui, format!("    model: {}   adapter: {}", short_model(ex.model), ex.adapter));
        push(
            &mut out,
            ui,
            format!(
                "    tokens: {}   spend: ${:.2}   turns: {}   apologies: {}",
                compact(ex.tokens),
                ex.spend_usd,
                ex.turns,
                ex.apology_count
            ),
        );
        if !ex.apology_quote.is_empty() {
            push(&mut out, ui, "    remorse quote:".to_string());
            for line in wrap(ex.apology_quote, wrap_width) {
                push(&mut out, ui, format!("      {}", ui.paint(Role::Label, &line)));
            }
        }
        if !ex.code_snippet.is_empty() {
            push(&mut out, ui, "    fatal snippet:".to_string());
            for line in wrap(ex.code_snippet, wrap_width) {
                push(&mut out, ui, format!("      {}", ui.paint(Role::Error, &line)));
            }
        }
        push(&mut out, ui, ui.leaders("    receipt hash", ex.session_hash, ui.width(), Role::Unverified));
        out.push('\n');
    }
    out.push_str(&ui.next("archie session blunder --submit", "publish the top exhibit"));
    out
}

pub fn blunder_submitted(ui: &Ui, status: &str, url: &str, id: &str) -> String {
    let mut out = String::new();
    section(&mut out, ui, "DISPATCHED", "");
    push(&mut out, ui, ui.leaders("  status", status, ui.width(), Role::Verified));
    push(&mut out, ui, ui.leaders("  url", url, ui.width(), Role::Value));
    push(&mut out, ui, ui.leaders("  id", id, ui.width(), Role::Label));
    out
}

pub fn blunder_submit_failed(ui: &Ui, err: &str, receipt_url: &str) -> String {
    let mut out = String::new();
    section(&mut out, ui, "DISPATCH FAILED", "");
    push(&mut out, ui, format!("  {}", ui.paint(Role::Warn, &format!("could not reach submission endpoint: {}", err))));
    push(&mut out, ui, ui.leaders("  receipt", receipt_url, ui.width(), Role::Label));
    out
}

// -----------------------------------------------------------------------------
// threat-digest
// -----------------------------------------------------------------------------

pub struct ThreatDigestSessionRow<'a> {
    pub severity: &'a str,
    pub session_id: &'a str,
    pub risk_score: u64,
    pub adapter: &'a str,
    pub findings: usize,
    pub categories: String,
}

pub struct ThreatDigestView<'a> {
    pub sessions_scanned: usize,
    pub sessions_with_exposure: usize,
    pub sessions_clean: usize,
    pub sessions_unreadable: usize,
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub categories: &'a [(String, usize)],
    pub top_sessions: Vec<ThreatDigestSessionRow<'a>>,
}

pub fn threat_digest(ui: &Ui, v: &ThreatDigestView<'_>) -> String {
    let mut out = String::new();
    out.push_str(&ui.header(
        "archie session risk",
        &format!("{} sessions scanned", thousands(v.sessions_scanned as u64)),
    ));
    out.push('\n');

    section(&mut out, ui, "EXPOSURE", "");
    push(&mut out, ui, ui.leaders("  exposed", &thousands(v.sessions_with_exposure as u64), ui.width(), Role::Warn));
    push(&mut out, ui, ui.leaders("  clean", &thousands(v.sessions_clean as u64), ui.width(), Role::Verified));
    if v.sessions_unreadable > 0 {
        push(
            &mut out,
            ui,
            ui.leaders(
                "  unreadable (source moved)",
                &thousands(v.sessions_unreadable as u64),
                ui.width(),
                Role::Unverified,
            ),
        );
    }
    out.push('\n');

    section(&mut out, ui, "BY PEAK SEVERITY", "");
    push(&mut out, ui, ui.leaders("  critical", &v.critical.to_string(), ui.width(), Role::Error));
    push(&mut out, ui, ui.leaders("  high", &v.high.to_string(), ui.width(), Role::Warn));
    push(&mut out, ui, ui.leaders("  medium", &v.medium.to_string(), ui.width(), Role::Label));
    push(&mut out, ui, ui.leaders("  low", &v.low.to_string(), ui.width(), Role::Unverified));
    out.push('\n');

    if !v.categories.is_empty() {
        section(&mut out, ui, "CATEGORIES", "");
        for (category, count) in v.categories {
            push(&mut out, ui, ui.leaders(&format!("  {}", category), &count.to_string(), ui.width(), Role::Value));
        }
        out.push('\n');
    }

    if v.top_sessions.is_empty() {
        section(&mut out, ui, "TOP SESSIONS", "");
        push(
            &mut out,
            ui,
            format!("  {}", ui.paint(Role::Verified, "No exposure at or above the requested severity threshold.")),
        );
    } else {
        section(&mut out, ui, "TOP SESSIONS", "rotate these first");
        for row in &v.top_sessions {
            push(
                &mut out,
                ui,
                format!(
                    "  {}  {}  risk {}",
                    ui.paint(severity_role(row.severity), &severity_tag(row.severity)),
                    ui.paint(Role::Emphasis, row.session_id),
                    row.risk_score
                ),
            );
            push(
                &mut out,
                ui,
                format!("    adapter: {}   findings: {}", row.adapter, row.findings),
            );
            if !row.categories.is_empty() {
                push(&mut out, ui, format!("    {}", ui.paint(Role::Label, &row.categories)));
            }
        }
    }
    out.push('\n');
    out.push_str(&ui.next(
        "archie session export <session-id> --redact",
        "the full session with secrets stripped",
    ));
    out
}

// -----------------------------------------------------------------------------
// handoff
// -----------------------------------------------------------------------------

/// One body section of a handoff: a heading, its true row count, and the rows that fit.
pub struct HandoffSection<'a> {
    pub title: &'a str,
    pub total: usize,
    /// `(gutter, claim)` — the gutter carries the receipt (`seq 8441`, `14:07`) so every line
    /// on screen can be traced back to the transcript without the reader hunting for it.
    pub rows: Vec<(String, String)>,
    pub dropped: usize,
}

pub struct HandoffView<'a> {
    pub command: &'a str,
    pub repo: &'a str,
    pub task: Option<&'a str>,
    /// `(rung, kind)`, absent when no outcome evidence was found.
    pub outcome: Option<(usize, String)>,
    pub cost: &'a str,
    pub sections: &'a [HandoffSection<'a>],
    pub skipped: &'a [(String, usize)],
    pub receipt: [String; 2],
    pub next: Option<(String, String)>,
}

/// The handoff, on the grid. Same facts as the markdown the MCP tools return, in the shape a
/// terminal reads: gutter, then the claim. Where the markdown spends a line budget, this
/// spends the column budget — a long path is truncated, never wrapped.
pub fn handoff(ui: &Ui, v: &HandoffView<'_>) -> String {
    let mut out = String::new();
    let i = ui.inner();

    out.push_str(&ui.header(v.command, v.repo));
    out.push('\n');

    const LABEL: usize = 10;
    let field = i.saturating_sub(LABEL + 2).max(8);
    push(
        &mut out,
        ui,
        format!(
            "  {}  {}",
            ui.paint(Role::Label, &rpad("Task", LABEL)),
            match v.task {
                Some(t) => ui.paint(Role::Emphasis, &truncate(t, field)),
                None => ui.paint(Role::Unverified, "first prompt not indexed yet"),
            }
        ),
    );
    push(
        &mut out,
        ui,
        format!(
            "  {}  {}",
            ui.paint(Role::Label, &rpad("Outcome", LABEL)),
            match &v.outcome {
                Some((rung, kind)) => format!(
                    "{}  {}",
                    ui.paint(rung_role(*rung), &ui.meter(*rung)),
                    ui.paint(rung_role(*rung), &format!("rung {}, {}", rung, kind))
                ),
                None => ui.paint(Role::Unverified, "no outcome evidence in this session"),
            }
        ),
    );
    push(
        &mut out,
        ui,
        format!(
            "  {}  {}",
            ui.paint(Role::Label, &rpad("Cost", LABEL)),
            ui.paint(Role::Value, &truncate(v.cost, field))
        ),
    );

    if v.sections.is_empty() && v.skipped.is_empty() {
        out.push('\n');
        push(
            &mut out,
            ui,
            format!(
                "  {}",
                ui.paint(
                    Role::Unverified,
                    "Nothing to hand over but the receipt: no file changes, no commands, no \
                     outcome evidence."
                )
            ),
        );
    }

    // The gutter is sized once, across every section, so every claim starts at one column.
    let gutter = v
        .sections
        .iter()
        .flat_map(|s| s.rows.iter())
        .map(|(g, _)| display_width(g))
        .max()
        .unwrap_or(0)
        .min(12);

    for section in v.sections {
        out.push('\n');
        out.push_str(&ui.section(&section.title.to_uppercase(), &section.total.to_string()));
        for (gutter_text, row) in &section.rows {
            push(
                &mut out,
                ui,
                format!(
                    "  {}  {}",
                    ui.paint(Role::Chrome, &rpad(&truncate(gutter_text, gutter), gutter)),
                    ui.paint(Role::Value, &truncate(row, i.saturating_sub(gutter + 2)))
                ),
            );
        }
        if section.dropped > 0 {
            push(
                &mut out,
                ui,
                format!(
                    "  {}",
                    ui.paint(
                        Role::Unverified,
                        &format!("{} more, not shown", section.dropped)
                    )
                ),
            );
        }
    }

    if !v.skipped.is_empty() {
        let named = v
            .skipped
            .iter()
            .map(|(title, total)| format!("{} ({})", title, total))
            .collect::<Vec<_>>()
            .join(", ");
        push(
            &mut out,
            ui,
            format!(
                "  {}",
                ui.paint(Role::Warn, &format!("Dropped whole, for room: {}", named))
            ),
        );
    }

    // The section the machine cannot fill. Saying so is the point: a generated handoff that
    // quietly omits the open decisions gets read as complete.
    out.push('\n');
    out.push_str(&ui.section("NOT IN THIS HANDOFF", ""));
    push(
        &mut out,
        ui,
        format!(
            "  {}",
            ui.paint(
                Role::Label,
                "Open decisions, PR and CI state, and environment traps are not in the index."
            )
        ),
    );
    push(
        &mut out,
        ui,
        format!(
            "  {}",
            ui.paint(
                Role::Label,
                "The inventory above is the machine's; the judgment is yours."
            )
        ),
    );

    out.push('\n');
    push(&mut out, ui, format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))));
    for line in &v.receipt {
        push(&mut out, ui, format!("  {}", ui.paint(Role::Chrome, line)));
    }

    if let Some((command, why)) = &v.next {
        out.push('\n');
        out.push_str(&ui.next(command, why));
    }

    out
}

// -----------------------------------------------------------------------------
// doctor --self-test
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfTestStatus {
    Pass,
    Slow,
    Fail,
    Skip,
}

impl SelfTestStatus {
    fn label(self) -> &'static str {
        match self {
            SelfTestStatus::Pass => "PASS",
            SelfTestStatus::Slow => "SLOW",
            SelfTestStatus::Fail => "FAIL",
            SelfTestStatus::Skip => "SKIP",
        }
    }

    fn role(self) -> Role {
        match self {
            SelfTestStatus::Pass => Role::Verified,
            SelfTestStatus::Slow => Role::Warn,
            SelfTestStatus::Fail => Role::Error,
            SelfTestStatus::Skip => Role::Unverified,
        }
    }
}

pub struct SelfTestStepView {
    pub name: String,
    pub status: SelfTestStatus,
    pub elapsed_ms: u128,
    pub receipt: String,
}

/// `340ms`, `1.2s` -- millisecond precision below one second, since most steps here
/// finish well under it and `duration()`'s whole-second granularity would print `0s`
/// for nearly every row.
fn self_test_elapsed(ms: u128) -> String {
    if ms >= 1000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{ms}ms")
    }
}

pub fn self_test(
    ui: &Ui,
    version: &str,
    steps: &[SelfTestStepView],
    total_ms: u128,
    ok: bool,
) -> String {
    let mut out = String::new();

    out.push_str(&ui.header("archie doctor --self-test", &format!("v{}", version)));
    out.push('\n');

    section(&mut out, ui, "WORKFLOW", &format!("{} steps", steps.len()));

    const STATUS: usize = 6;
    const TIME: usize = 8;
    let name_w = ui.inner().saturating_sub(STATUS + TIME + 6).max(10);

    for s in steps {
        push(
            &mut out,
            ui,
            format!(
                "  {}  {}  {}",
                ui.paint(Role::Value, &lpad(&truncate(&s.name, name_w), name_w)),
                ui.paint(s.status.role(), &lpad(s.status.label(), STATUS)),
                ui.paint(Role::Label, &rpad(&self_test_elapsed(s.elapsed_ms), TIME)),
            ),
        );
        if !s.receipt.is_empty() {
            push(
                &mut out,
                ui,
                format!(
                    "    {}",
                    ui.paint(Role::Label, &truncate(&s.receipt, ui.inner().saturating_sub(4)))
                ),
            );
        }
    }
    out.push('\n');

    let (state, role) = if ok {
        ("all steps passed", Role::Verified)
    } else {
        ("one or more steps failed", Role::Error)
    };
    push(
        &mut out,
        ui,
        format!(
            "  {}  {}",
            ui.paint(role, state),
            ui.paint(Role::Label, &format!("({} total)", self_test_elapsed(total_ms))),
        ),
    );
    out.push('\n');

    out.push_str(&ui.next("archie doctor", "the environment and storage snapshot alone"));
    out
}

// -----------------------------------------------------------------------------
// repo list
// -----------------------------------------------------------------------------

pub struct RepoListRow<'a> {
    pub repo: &'a str,
    pub sessions: usize,
}

pub struct RepoListView<'a> {
    pub total_repos: usize,
    pub total_sessions: usize,
    pub rows: &'a [RepoListRow<'a>],
}

pub fn repo_list(ui: &Ui, v: &RepoListView<'_>) -> String {
    let mut out = String::new();
    let i = ui.inner();

    out.push_str(&ui.header(
        "archie repo list",
        &format!(
            "{} repos {} {} sessions",
            v.total_repos,
            ui.dot(),
            thousands(v.total_sessions as u64)
        ),
    ));
    out.push('\n');

    if v.rows.is_empty() {
        push(
            &mut out,
            ui,
            format!("  {}", ui.paint(Role::Unverified, "Nothing indexed yet.")),
        );
        out.push('\n');
        out.push_str(&ui.next("archie scan", "index what is already on this machine"));
        return out;
    }

    const COUNT: usize = 9;
    const SHARE: usize = 7;
    let name = i.saturating_sub(COUNT + SHARE + 6).max(10);
    push(
        &mut out,
        ui,
        format!(
            "  {}",
            ui.paint(
                Role::Label,
                &format!(
                    "{}  {}  {}",
                    rpad("REPOSITORY", name),
                    lpad("SESSIONS", COUNT),
                    lpad("SHARE", SHARE)
                )
            )
        ),
    );
    push(&mut out, ui, format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))));

    for r in v.rows {
        push(
            &mut out,
            ui,
            format!(
                "  {}  {}  {}",
                ui.paint(Role::Value, &rpad(&truncate(r.repo, name), name)),
                lpad(&thousands(r.sessions as u64), COUNT),
                ui.paint(Role::Label, &lpad(&percent(r.sessions, v.total_sessions), SHARE)),
            ),
        );
    }
    push(&mut out, ui, format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))));
    out.push('\n');
    out.push_str(&ui.next(
        "archie repo blame <file>",
        "which session wrote a given file",
    ));
    out
}

// -----------------------------------------------------------------------------
// agent show
// -----------------------------------------------------------------------------

pub struct AgentShowView<'a> {
    pub adapter: &'a str,
    pub source_root: &'a str,
    pub detected: bool,
    /// Capability name and whether this adapter extracts it, in the order the matrix uses.
    pub capabilities: &'a [(&'a str, bool)],
    pub indexed_sessions: usize,
    pub indexed_tokens: u64,
    pub models: &'a [String],
}

pub fn agent_show(ui: &Ui, v: &AgentShowView<'_>) -> String {
    let mut out = String::new();
    let supported = v.capabilities.iter().filter(|(_, on)| *on).count();

    out.push_str(&ui.header(
        &format!("archie agent show {}", v.adapter),
        &format!(
            "{}/{} extracted {} {}",
            supported,
            v.capabilities.len(),
            ui.dot(),
            if v.detected { "present here" } else { "not found here" }
        ),
    ));
    out.push('\n');

    section(&mut out, ui, "SOURCE", "");
    push(
        &mut out,
        ui,
        ui.leaders("  default root", v.source_root, ui.width(), Role::Value),
    );
    push(
        &mut out,
        ui,
        ui.leaders(
            "  detected now",
            if v.detected { "yes" } else { "no" },
            ui.width(),
            if v.detected { Role::Verified } else { Role::Unverified },
        ),
    );
    out.push('\n');

    section(&mut out, ui, "EXTRACTS", "");
    for (name, on) in v.capabilities {
        push(
            &mut out,
            ui,
            format!(
                "  {} {}",
                ui.paint(
                    if *on { Role::Verified } else { Role::Unverified },
                    ui.cell(*on)
                ),
                ui.paint(if *on { Role::Value } else { Role::Unverified }, name)
            ),
        );
    }
    out.push('\n');

    section(&mut out, ui, "IN THIS INDEX", "");
    push(
        &mut out,
        ui,
        ui.leaders(
            "  sessions",
            &thousands(v.indexed_sessions as u64),
            ui.width(),
            Role::Value,
        ),
    );
    push(
        &mut out,
        ui,
        ui.leaders("  tokens", &compact(v.indexed_tokens), ui.width(), Role::Value),
    );
    if v.models.is_empty() {
        push(
            &mut out,
            ui,
            ui.leaders("  models", "none recorded", ui.width(), Role::Unverified),
        );
    } else {
        for model in v.models {
            push(
                &mut out,
                ui,
                format!("  {} {}", ui.dot(), ui.paint(Role::Label, &short_model(model))),
            );
        }
    }
    out.push('\n');
    out.push_str(&ui.next(
        &format!("archie session list --adapter {}", v.adapter),
        "what this adapter actually recorded",
    ));
    out
}

// -----------------------------------------------------------------------------
// window list
// -----------------------------------------------------------------------------

pub struct WindowListRow {
    pub label: String,
    pub sessions: usize,
    pub total_tokens: u64,
    pub estimated_cost_usd: f64,
    pub burn_rate_tokens_per_hour: f64,
}

pub struct WindowListView<'a> {
    pub hours: i64,
    pub anchor: &'a str,
    pub rows: &'a [WindowListRow],
}

pub fn window_list(ui: &Ui, v: &WindowListView<'_>) -> String {
    let mut out = String::new();
    let i = ui.inner();

    out.push_str(&ui.header(
        "archie window list",
        &format!("{}h windows {} to {}", v.hours, ui.dot(), v.anchor),
    ));
    out.push('\n');

    if v.rows.is_empty() {
        push(
            &mut out,
            ui,
            format!("  {}", ui.paint(Role::Unverified, "No sessions in range.")),
        );
        out.push('\n');
        out.push_str(&ui.next("archie scan", "index what is already on this machine"));
        return out;
    }

    const N: usize = 8;
    const TOK: usize = 9;
    const COST: usize = 9;
    const RATE: usize = 11;
    let label = i.saturating_sub(N + TOK + COST + RATE + 8).max(12);
    push(
        &mut out,
        ui,
        format!(
            "  {}",
            ui.paint(
                Role::Label,
                &format!(
                    "{}  {}  {}  {}  {}",
                    rpad("WINDOW", label),
                    lpad("SESSIONS", N),
                    lpad("TOKENS", TOK),
                    lpad("COST", COST),
                    lpad("TOKENS/H", RATE)
                )
            )
        ),
    );
    push(&mut out, ui, format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))));

    for r in v.rows {
        push(
            &mut out,
            ui,
            format!(
                "  {}  {}  {}  {}  {}",
                ui.paint(Role::Value, &rpad(&truncate(&r.label, label), label)),
                lpad(&thousands(r.sessions as u64), N),
                lpad(&compact(r.total_tokens), TOK),
                lpad(&format!("${:.2}", r.estimated_cost_usd), COST),
                ui.paint(
                    Role::Label,
                    &lpad(&compact(r.burn_rate_tokens_per_hour as u64), RATE)
                ),
            ),
        );
    }
    push(&mut out, ui, format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))));
    out.push('\n');
    out.push_str(&ui.next("archie window show", "the current window in full"));
    out
}

// -----------------------------------------------------------------------------
// stats outcomes
// -----------------------------------------------------------------------------

pub struct StatsOutcomesRow<'a> {
    pub key: &'a str,
    pub n: usize,
    pub verified: usize,
    pub rate: Option<f64>,
    pub reason: Option<&'a str>,
}

pub struct StatsOutcomesView<'a> {
    pub group_by: &'a str,
    pub min_n: usize,
    pub baseline_n: usize,
    pub baseline_rate: f64,
    pub suppressed_groups: usize,
    pub rows: &'a [StatsOutcomesRow<'a>],
    pub counted_at: &'a str,
}

pub fn stats_outcomes(ui: &Ui, v: &StatsOutcomesView<'_>) -> String {
    let mut out = String::new();
    let i = ui.inner();

    out.push_str(&ui.header(
        "archie stats outcomes",
        &format!("by {} {} min n {}", v.group_by, ui.dot(), v.min_n),
    ));
    out.push('\n');

    section(&mut out, ui, "YOUR RATE", "");
    push(
        &mut out,
        ui,
        ui.leaders(
            "  claimed sessions",
            &thousands(v.baseline_n as u64),
            ui.width(),
            Role::Value,
        ),
    );
    push(
        &mut out,
        ui,
        ui.leaders(
            "  left evidence",
            &format!("{:.1}%", v.baseline_rate * 100.0),
            ui.width(),
            Role::Verified,
        ),
    );
    out.push('\n');

    if v.rows.is_empty() {
        section(&mut out, ui, "GROUPS", "");
        push(
            &mut out,
            ui,
            format!(
                "  {}",
                ui.paint(
                    Role::Unverified,
                    "No group clears the sample floor; nothing here is worth a number yet."
                )
            ),
        );
    } else {
        section(&mut out, ui, "GROUPS", &format!("{} shown", v.rows.len()));
        const N: usize = 7;
        const VER: usize = 9;
        const RATE: usize = 8;
        let key = i.saturating_sub(N + VER + RATE + 6).max(12);
        push(
            &mut out,
            ui,
            format!(
                "  {}",
                ui.paint(
                    Role::Label,
                    &format!(
                        "{}  {}  {}  {}",
                        rpad("GROUP", key),
                        lpad("N", N),
                        lpad("VERIFIED", VER),
                        lpad("RATE", RATE)
                    )
                )
            ),
        );
        push(&mut out, ui, format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))));
        for r in v.rows {
            // A group with sessions but no detected outcome is not a low rate -- it is a
            // parsing gap, and printing 0% for it would be a measurement we never made.
            let (rate_text, rate_role) = match r.rate {
                Some(rate) => (
                    format!("{:.0}%", rate * 100.0),
                    if rate * 100.0 >= 50.0 { Role::Verified } else { Role::Warn },
                ),
                None => (
                    r.reason.unwrap_or("not measured").to_string(),
                    Role::Unverified,
                ),
            };
            push(
                &mut out,
                ui,
                format!(
                    "  {}  {}  {}  {}",
                    ui.paint(Role::Value, &rpad(&truncate(r.key, key), key)),
                    lpad(&thousands(r.n as u64), N),
                    lpad(&thousands(r.verified as u64), VER),
                    ui.paint(rate_role, &lpad(&rate_text, RATE)),
                ),
            );
        }
        push(&mut out, ui, format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))));
    }

    if v.suppressed_groups > 0 {
        push(
            &mut out,
            ui,
            format!(
                "  {}",
                ui.paint(
                    Role::Unverified,
                    &format!(
                        "{} group(s) under n={} suppressed, not counted as zero.",
                        v.suppressed_groups, v.min_n
                    )
                )
            ),
        );
    }
    push(
        &mut out,
        ui,
        format!("  {}", ui.paint(Role::Label, &format!("counted at {}", v.counted_at))),
    );
    out.push('\n');
    out.push_str(&ui.next(
        "archie session list --unproven",
        "the sessions with nothing behind the claim",
    ));
    out
}
