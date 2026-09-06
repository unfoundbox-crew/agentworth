//! Rendering a [`HandoffReport`] as markdown, inside a real line budget.
//!
//! "Under 60 lines" is a constraint on the output, not an aspiration: a handoff that runs to
//! 200 lines and apologises for it does not get read, and the next session is back to reading
//! the transcript. So the budget is allocated before anything is written -- the sections that
//! decay fastest are served first, every truncated section says how many rows it dropped, and
//! a section that could not be afforded at all is named rather than silently disappearing.

use super::{HandoffReport, MAX_LINES_CEILING};

/// Lines the document always spends: five of head, then the blank/heading/prose of "Not in
/// this handoff" and the blank/rule/two lines of the receipt. Neither is ever truncated, so
/// this is also the floor -- asking for fewer lines than this cannot buy a shorter document,
/// only a document with no body.
const FIXED_LINES: usize = 12;

/// Blank line + heading, paid once per section that has anything in it.
const SECTION_CHROME: usize = 2;

/// Renders the handoff. `max_lines` is clamped into `FIXED_LINES + 1..=MAX_LINES_CEILING` and
/// the result never exceeds it. The floor is one line above the fixed cost so that a document
/// with no room for any section still has room to say which sections it dropped.
pub fn render_markdown(report: &HandoffReport, max_lines: usize) -> String {
    let budget = max_lines.clamp(FIXED_LINES + 1, MAX_LINES_CEILING);
    let mut out: Vec<String> = Vec::new();

    out.push(format!(
        "# Session {} · {} · {}",
        short_id(&report.receipt.session_id),
        report.receipt.repo,
        when_range(report),
    ));
    out.push(String::new());
    out.push(format!(
        "**Task** {}",
        match &report.task {
            Some(t) => one_line(t, 160),
            None => "_first prompt not indexed yet_".to_string(),
        }
    ));
    out.push(match &report.outcome {
        Some(o) => format!("**Outcome** rung {}, {}", o.rung, o.kind),
        None => "**Outcome** _no outcome evidence in this session_".to_string(),
    });
    out.push(format!("**Cost** {}", cost_line(report)));

    if report.body_is_empty() {
        // Nothing but a token count. Say that, and stop -- padding here would be inventing.
        out.push(String::new());
        out.push(
            "_This session left no file changes, no commands, and no outcome evidence. There \
             is nothing to hand over but the receipt._"
                .to_string(),
        );
    } else {
        let (sections, skipped) = allocate(report, budget);
        for section in sections {
            out.push(String::new());
            out.push(format!("## {} ({})", section.heading, section.total));
            out.extend(section.lines);
            if section.dropped > 0 {
                out.push(format!("- … {} more, not shown", section.dropped));
            }
        }
        if !skipped.is_empty() {
            let named = skipped
                .iter()
                .map(|(heading, total)| format!("{heading} ({total})"))
                .collect::<Vec<_>>()
                .join(", ");
            out.push(format!(
                "_Dropped whole, for room: {named}. Raise the line budget to see them._"
            ));
        }
    }

    out.push(String::new());
    out.push("## Not in this handoff".to_string());
    out.push(
        "Open decisions, PR and CI state, and environment traps are not in the index. The \
         machine owns the inventory above; the judgment is yours to add."
            .to_string(),
    );
    out.push(String::new());
    out.push("---".to_string());
    out.push(format!(
        "session {} · {} · generated {}{}",
        report.receipt.session_id,
        report.receipt.adapter,
        stamp(report.receipt.generated_at),
        if report.receipt.redacted {
            " · redacted"
        } else {
            ""
        }
    ));
    out.push(match report.receipt.index_last_updated {
        Some(t) => format!("index last updated {}", stamp(t)),
        None => "index last updated unknown".to_string(),
    });

    out.join("\n")
}

struct RenderedSection {
    heading: &'static str,
    total: usize,
    lines: Vec<String>,
    dropped: usize,
}

/// One body section before the budget touches it: heading, true row count, rendered rows.
type Candidate = (&'static str, usize, Vec<String>);

/// Splits the line budget across the body sections.
///
/// Order is priority order, and it is the argument the whole document makes: what compaction
/// already deleted from the model's own view is unrecoverable from anywhere else and goes
/// first (and is absent entirely for a session that never compacted), what was promised and
/// never done decays fastest, what ran is the only proof anything worked, what changed is
/// recoverable from git, and quoted decisions are a convenience. Each section gets one row
/// before any section gets a second.
///
/// Returns the sections that fit and the ones that did not, because a section vanishing
/// unannounced would make the document look complete when it isn't -- the exact failure this
/// tool exists to prevent, and the easiest one to inflict on ourselves.
fn allocate(
    report: &HandoffReport,
    budget: usize,
) -> (Vec<RenderedSection>, Vec<(&'static str, usize)>) {
    let mut candidates: Vec<Candidate> = Vec::new();

    if !report.forgotten.is_empty() {
        candidates.push((
            "Decided, then compacted away",
            report.forgotten_total,
            report
                .forgotten
                .iter()
                .map(|s| {
                    format!(
                        "- \"{}\"  [round {}, seq {}]",
                        one_line(&s.text, 150),
                        s.round,
                        s.sequence
                    )
                })
                .collect(),
        ));
    }
    if !report.loose_ends.is_empty() {
        candidates.push((
            "Said it would, no evidence it did",
            report.loose_ends.len(),
            report
                .loose_ends
                .iter()
                .map(|e| format!("- \"{}\"  [seq {}]", one_line(&e.text, 150), e.sequence))
                .collect(),
        ));
    }
    if !report.ran.is_empty() {
        candidates.push((
            "Ran",
            report.ran_total,
            report
                .ran
                .iter()
                .map(|c| {
                    format!(
                        "- `{}` — {}, {}",
                        one_line(&c.command, 110),
                        c.ending(),
                        clock(c.at)
                    )
                })
                .collect(),
        ));
    }
    if !report.files.is_empty() {
        candidates.push((
            "Files touched",
            report.files_total,
            report
                .files
                .iter()
                .map(|f| {
                    format!(
                        "- {} — {} edit{}, last {}",
                        f.path,
                        f.edits,
                        if f.edits == 1 { "" } else { "s" },
                        clock(f.last_at),
                    )
                })
                .collect(),
        ));
    }
    if !report.decided.is_empty() {
        candidates.push((
            "Said it decided",
            report.decided.len(),
            report
                .decided
                .iter()
                .map(|s| format!("- \"{}\"  [seq {}]", one_line(&s.text, 150), s.sequence))
                .collect(),
        ));
    }

    if candidates.is_empty() {
        return (Vec::new(), Vec::new());
    }

    // Naming the dropped sections costs its own line, and only when there are any. Plan once
    // without it; if anything was dropped, re-plan with the line reserved so the notice cannot
    // push the document one line over its own budget.
    let caps = plan(&candidates, budget, FIXED_LINES);
    let caps = if caps.contains(&0) {
        plan(&candidates, budget, FIXED_LINES + 1)
    } else {
        caps
    };

    let mut sections = Vec::new();
    let mut skipped = Vec::new();
    for (i, (heading, total, mut lines)) in candidates.into_iter().enumerate() {
        if caps[i] == 0 {
            skipped.push((heading, total));
            continue;
        }
        lines.truncate(caps[i]);
        sections.push(RenderedSection {
            heading,
            total,
            dropped: total.saturating_sub(caps[i]),
            lines,
        });
    }

    (sections, skipped)
}

/// How many rows each candidate gets. `base` is what the non-section parts of the document
/// already cost.
fn plan(candidates: &[Candidate], budget: usize, base: usize) -> Vec<usize> {
    // What a section costs at `k` rows: blank line and heading, the rows, and the "… N more"
    // line whenever anything is left over. Every step below is checked against this first, so
    // the total cannot overrun.
    let cost = |k: usize, total: usize| SECTION_CHROME + k + usize::from(total > k);

    let mut caps = vec![0usize; candidates.len()];
    let mut used = base;

    // One row each, in priority order, while there is room. A section that cannot afford even
    // its first row is dropped whole rather than rendered as a bare heading.
    for (i, (_, total, _)) in candidates.iter().enumerate() {
        let c = cost(1, *total);
        if used + c <= budget {
            caps[i] = 1;
            used += c;
        }
    }

    // Then round-robin the rest. Growing onto the last row removes the truncation notice, so
    // that particular step is free -- hence a per-step delta rather than a flat one line each.
    let mut progress = true;
    while progress {
        progress = false;
        for (i, (_, total, lines)) in candidates.iter().enumerate() {
            if caps[i] == 0 || caps[i] >= lines.len() {
                continue;
            }
            let delta = cost(caps[i] + 1, *total) - cost(caps[i], *total);
            if used + delta <= budget {
                caps[i] += 1;
                used += delta;
                progress = true;
            }
        }
    }

    caps
}

/// First eight characters of a UUID-shaped session id, which is what a human recognises it by.
/// The full id is on the receipt line, so nothing is lost.
fn short_id(session_id: &str) -> &str {
    agentworth_schema::text::truncate_chars(session_id, 8)
}

fn when_range(report: &HandoffReport) -> String {
    let start = report.receipt.started_at.format("%Y-%m-%d %H:%M");
    match report.receipt.ended_at {
        Some(end) => format!("{start}–{}", end.format("%H:%M")),
        None => start.to_string(),
    }
}

fn cost_line(report: &HandoffReport) -> String {
    let mut parts = vec![
        format!("{} tokens", compact(report.total_tokens)),
        format!("{} events", thousands(report.total_events as u64)),
    ];
    if let Some(seconds) = report.duration_seconds {
        parts.push(duration(seconds));
    }
    if report.compactions > 0 {
        parts.push(format!(
            "{} compaction{} ({} dropped)",
            report.compactions,
            if report.compactions == 1 { "" } else { "s" },
            compact(report.compaction_tokens_dropped),
        ));
    }
    parts.join(" · ")
}

fn stamp(t: chrono::DateTime<chrono::Utc>) -> String {
    t.format("%Y-%m-%dT%H:%MZ").to_string()
}

fn clock(t: chrono::DateTime<chrono::Utc>) -> String {
    t.format("%H:%M").to_string()
}

/// Collapses whitespace and caps length, so one quoted sentence can never become three lines
/// and blow a budget that was allocated in lines.
pub(crate) fn one_line(text: &str, max_chars: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max_chars {
        return flat;
    }
    format!(
        "{}…",
        agentworth_schema::text::truncate_chars(&flat, max_chars.saturating_sub(1))
    )
}

fn thousands(n: u64) -> String {
    crate::ui::thousands(n)
}

fn compact(n: u64) -> String {
    crate::ui::compact(n)
}

fn duration(seconds: f64) -> String {
    crate::ui::duration(seconds)
}
