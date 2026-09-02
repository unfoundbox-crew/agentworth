//! Every human-facing screen, rendered to a `String` so it can be snapshotted.
//!
//! Nothing here writes to stdout and nothing here touches a `--json` payload.

use std::fmt::Write;

use super::{
    archie, compact, display_width, duration, eyes, lpad, percent, percent_round, rpad, thousands,
    truncate, EyeKind, Role, Ui,
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
}

pub fn stats(ui: &Ui, v: &StatsView<'_>) -> String {
    let mut out = String::new();
    let i = ui.inner();

    let right = v.db_path.map(shorten_home).unwrap_or_default();
    out.push_str(&ui.header("agentworth stats", &right));
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

    out.push_str(&ui.next(
        "agentworth traces --limit 20",
        "the newest sessions, ladder first",
    ));
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
        .map(|r| format!("agentworth inspect {}", r.session_id))
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

pub fn usage(
    ui: &Ui,
    command: &str,
    who_head: &str,
    period_noun: &str,
    rows: &[UsageRow],
) -> String {
    let mut out = String::new();
    let i = ui.inner();

    let periods = {
        let mut p: Vec<&str> = rows.iter().map(|r| r.period.as_str()).collect();
        p.dedup();
        p.len()
    };
    let noun = |n: usize| {
        if n == 1 {
            period_noun.to_string()
        } else {
            format!("{}s", period_noun)
        }
    };
    let sessions: usize = rows.iter().map(|r| r.sessions).sum();
    out.push_str(&ui.header(
        command,
        &format!(
            "{} {} {} {} sessions",
            periods,
            noun(periods),
            ui.dot(),
            thousands(sessions as u64)
        ),
    ));
    out.push('\n');

    const P: usize = 10;
    const S: usize = 8;
    const N: usize = 8;
    const C: usize = 10;
    let who = i.saturating_sub(P + S + N * 3 + C + 12).max(6);

    push(
        &mut out,
        ui,
        format!(
            "  {}",
            ui.paint(
                Role::Label,
                &format!(
                    "{}  {}  {}  {}  {}  {}  {}",
                    lpad("PERIOD", P),
                    lpad(who_head, who),
                    rpad("SESSIONS", S),
                    rpad("INPUT", N),
                    rpad("OUTPUT", N),
                    rpad("CACHE RD", N),
                    rpad("COST", C),
                )
            )
        ),
    );

    let mut any_dash = false;
    let mut last_period: Option<&str> = None;
    let mut total_cost = 0.0;
    for r in rows {
        // The date column collapses after its first row, so the eye reads periods rather
        // than repetitions.
        let new_period = last_period != Some(r.period.as_str());
        if new_period {
            push(
                &mut out,
                ui,
                format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))),
            );
        }
        let shown_period = if new_period { r.period.as_str() } else { "" };
        last_period = Some(r.period.as_str());
        total_cost += r.cost_usd;

        let (input, output, cache, cost) = if r.measured {
            (
                compact(r.input),
                compact(r.output),
                compact(r.cache_read),
                format!("${:.2}", r.cost_usd),
            )
        } else {
            any_dash = true;
            let d = ui.dash().to_string();
            (d.clone(), d.clone(), d.clone(), d)
        };

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
    }

    push(
        &mut out,
        ui,
        format!("  {}", ui.paint(Role::Chrome, &ui.rule_of(i))),
    );
    push(
        &mut out,
        ui,
        format!(
            "  {}  {}{}",
            ui.paint(Role::Label, &lpad(&format!("{} {}", periods, noun(periods)), P)),
            ui.paint(
                Role::Emphasis,
                &rpad(&thousands(sessions as u64), who + S + 2)
            ),
            // The one violet number: what the whole table cost.
            ui.paint(
                Role::Verified,
                &rpad(
                    &format!("${:.2}", total_cost),
                    i.saturating_sub(P + who + S + 4)
                )
            ),
        ),
    );

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
        &format!("agentworth blame {}", path),
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
        out.push_str(&ui.next("agentworth scan", "re-index, if it should be here"));
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

    out.push_str(&ui.header("agentworth doctor", &format!("v{}", v.version)));
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

    out.push_str(&ui.next("agentworth scan", "pick up anything new since the last index"));
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
        "agentworth matrix",
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
        "agentworth doctor",
        "which of these are actually installed here",
    ));
    out
}

// -----------------------------------------------------------------------------
// scan
// -----------------------------------------------------------------------------

/// One redraw of the three-line progress block. Rendered without a trailing newline on
/// the last line so the caller can cursor back over exactly three rows.
pub fn scan_progress(ui: &Ui, frame: u64, what: &str, done: usize, total: usize) -> String {
    // The meter advances every frame; the face changes every second frame. Two rhythms,
    // so the block feels alive without either reading as a spinner.
    let face = if frame.is_multiple_of(2) {
        eyes(ui, EyeKind::Digging)
    } else {
        eyes(ui, EyeKind::Done)
    };
    let art = archie(ui, &face);
    let frac = if total > 0 {
        done as f64 / total as f64
    } else {
        0.0
    };
    let bar_w = ui.inner().saturating_sub(38).clamp(8, 40);

    let mut out = String::new();
    push(&mut out, ui, format!("  {}", ui.paint(Role::Chrome, &art[0])));
    push(
        &mut out,
        ui,
        format!(
            "  {}   {}  {}",
            ui.paint(Role::Value, &art[1]),
            ui.paint(Role::Label, "scanning"),
            ui.paint(Role::Label, &truncate(what, ui.inner().saturating_sub(24)))
        ),
    );
    push(
        &mut out,
        ui,
        format!(
            "  {}     {}  {}  {}",
            ui.paint(Role::Chrome, &art[2]),
            ui.paint(Role::Verified, &ui.bar(frac, bar_w)),
            ui.paint(Role::Emphasis, &rpad(&format!("{:.0}%", frac * 100.0), 4)),
            ui.paint(
                Role::Label,
                &format!("{} / {}", thousands(done as u64), thousands(total as u64))
            ),
        ),
    );
    out
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
    let art = archie(ui, &eyes(ui, EyeKind::Done));

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
    out.push_str(&ui.next("agentworth stats", "the ladder across everything indexed"));
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
    let art = archie(ui, &eyes(ui, EyeKind::Failed));

    out.push_str(&ui.header(command, ""));
    out.push('\n');

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
        "agentworth suspect",
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

    out.push_str(&ui.next("agentworth suspect --hook", "install it as a pre-push note"));
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
            format!("~{}", &path[home.len()..])
        }
        _ => path.to_string(),
    }
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
