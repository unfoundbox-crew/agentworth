//! Rendering a [`WakeReport`] as markdown, inside a hard thirty-line budget.
//!
//! The budget is the product. A waking agent reads this before it does anything else, and a
//! document that runs long is one the agent pays for on every session. So the fixed lines are
//! spent first, a section with nothing in it is omitted rather than rendered empty, and the
//! loose-ends list -- the only variable-length part -- is trimmed to whatever room is left.
//!
//! Nothing here pads. A fact the index does not hold prints the phrasing
//! `docs/specs/wake.md` gives it and nothing else.

use super::{checkout_state, Checkout, Next, PriorSession, Proof, WakeReport, WakeSession};
use crate::handoff::{FileTouch, RanCommand};

/// The line ceiling, and it is a ceiling rather than a target: `render_markdown` never returns
/// more than this many lines, for any report, at any size of session.
pub const MAX_LINES: usize = 30;

/// How many earlier sessions the "Before that" block names. Two, because the block is context
/// and the budget belongs to the session being woken into.
const MAX_PRIOR: usize = 2;

/// Renders the wake document. Never longer than [`MAX_LINES`] lines.
///
/// The head, the checkout block, the "Next" block and the receipt are always present. Everything
/// between them is omitted when the session has nothing to put there; the loose-ends rows are
/// cut last and first, so a session with fifty of them still fits.
pub fn render_markdown(report: &WakeReport) -> String {
    let mut pre: Vec<String> = Vec::new();
    let mut ends: Vec<String> = Vec::new();
    let mut post: Vec<String> = Vec::new();

    pre.push(format!(
        "# Wake · {} · {}",
        report.receipt.repo,
        stamp_space(report.receipt.generated_at)
    ));
    pre.push(checkout_line(report));
    pre.push(index_line(report));

    match &report.session {
        Some(session) => {
            pre.push(String::new());
            pre.push(session_heading(session));
            pre.push(format!(
                "**Task** {}",
                match &session.task {
                    Some(task) => one_line(task, 120),
                    None => "_first prompt not indexed yet_".to_string(),
                }
            ));
            if let Some(asked) = &session.last_asked {
                pre.push(format!("**Last asked** {}", one_line(asked, 120)));
            }
            if let Some(ran_in) = &session.ran_in {
                let mut line = String::from("**Ran in**");
                if let Some(cwd) = &ran_in.cwd {
                    line.push_str(&format!(" {}", shorten_path(cwd)));
                }
                if let Some(branch) = &ran_in.git_branch {
                    line.push_str(&format!(" on {branch}"));
                }
                pre.push(line);
            }
            pre.push(match &session.outcome {
                Some(outcome) => format!("**Outcome** rung {}, {}", outcome.rung, outcome.kind),
                None => "**Outcome** _no outcome evidence in this session_".to_string(),
            });
            pre.push(format!("**Proof** {}", proof_line(&session.proof)));
            pre.push(format!("**Changed** {}", changed_line(session)));

            if !session.loose_ends.is_empty() {
                pre.push(format!("**Loose ends** ({})", session.loose_ends_total));
                ends = session
                    .loose_ends
                    .iter()
                    .map(|e| format!("- \"{}\"  [seq {}]", one_line(&e.text, 120), e.sequence))
                    .collect();
            }

            if let Some(decided) = &session.decided {
                post.push(format!(
                    "**Said it decided** \"{}\"  [seq {}]",
                    one_line(&decided.text, 120),
                    decided.sequence
                ));
            }
            if session.forgotten_total > 0 {
                post.push(format!(
                    "**Forgotten** {} decision{} dropped by compaction — `session_forgotten` has them",
                    session.forgotten_total,
                    if session.forgotten_total == 1 { "" } else { "s" }
                ));
            }
        }
        None => {
            pre.push(String::new());
            pre.push(
                "_No session for this repo in the index. `archie scan` indexes what is on disk._"
                    .to_string(),
            );
            if report.scan_exhausted {
                pre.push(format!(
                    "_The newest {} sessions held none for this repo; older ones may exist._",
                    crate::ui::thousands(agentworth_storage::REPO_SCAN_BUDGET as u64)
                ));
            }
        }
    }

    if !report.before.is_empty() {
        post.push(String::new());
        post.push("## Before that".to_string());
        post.extend(report.before.iter().take(MAX_PRIOR).map(prior_line));
    }

    if report.session.is_some() {
        post.push(String::new());
        post.push("## Next".to_string());
        post.push(next_blocker_line(&report.next));
        post.push(next_step_line(&report.next));
        post.push(
            "Not here: PR and CI state, open decisions. `gh pr list` for the first.".to_string(),
        );
    }

    post.push(String::new());
    post.push("---".to_string());
    post.push(receipt_line(report));

    // The loose-ends rows are the only part that can grow, so they are the part that pays for
    // the budget. If not even one row fits, the heading goes too rather than standing over
    // nothing.
    let room = MAX_LINES.saturating_sub(pre.len() + post.len());
    if room == 0 && !ends.is_empty() {
        pre.pop();
    }
    ends.truncate(room);

    let mut lines: Vec<String> = pre.into_iter().chain(ends).chain(post).collect();
    // The arithmetic above already fits every section a report can hold. This is the backstop:
    // the ceiling is a promise to the caller, and a promise enforced by arithmetic alone is one
    // a later section can break without anyone noticing.
    lines.truncate(MAX_LINES);
    lines.join("\n")
}

fn checkout_line(report: &WakeReport) -> String {
    match (&report.checkout, report.checkout_state.as_str()) {
        (Some(checkout), _) => found_checkout_line(checkout),
        (None, checkout_state::GIT_UNAVAILABLE) => "Checkout: git unavailable".to_string(),
        (None, checkout_state::UNREADABLE) => "Checkout: git did not answer in time".to_string(),
        _ => "Checkout: not a git checkout".to_string(),
    }
}

fn found_checkout_line(checkout: &Checkout) -> String {
    let mut parts = vec![format!("Checkout {}", shorten_path(&checkout.root))];
    parts.push(match &checkout.branch {
        Some(branch) => format!("branch {branch}"),
        None => "detached HEAD".to_string(),
    });
    if let Some(head) = &checkout.head_short {
        parts.push(match &checkout.head_subject {
            Some(subject) => format!("HEAD {head} \"{}\"", one_line(subject, 72)),
            None => format!("HEAD {head}"),
        });
    }
    if let Some(dirty) = checkout.dirty_files {
        parts.push(match dirty {
            0 => "clean".to_string(),
            1 => "1 file dirty".to_string(),
            n => format!("{n} files dirty"),
        });
    }
    if let Some(upstream) = &checkout.upstream {
        let mut drift = Vec::new();
        if let Some(ahead) = checkout.ahead.filter(|n| *n > 0) {
            drift.push(format!("{ahead} ahead"));
        }
        if let Some(behind) = checkout.behind.filter(|n| *n > 0) {
            drift.push(format!("{behind} behind"));
        }
        parts.push(if drift.is_empty() {
            format!("even with {upstream}")
        } else {
            format!("{} of {upstream}", drift.join(", "))
        });
    }
    parts.join(" · ")
}

fn index_line(report: &WakeReport) -> String {
    let scanned = match report.index.last_scanned_at {
        Some(at) => format!("Index scanned {}", stamp_space(at)),
        None => "Index never scanned".to_string(),
    };
    match report.index.source_changed_since_scan {
        Some(true) => format!("{scanned} · source changed since scan"),
        Some(false) => format!("{scanned} · source unchanged since scan"),
        None => format!("{scanned} · source not readable"),
    }
}

fn session_heading(session: &WakeSession) -> String {
    let when = match session.ended_at {
        Some(end) => format!(
            "{}–{}",
            session.started_at.format("%Y-%m-%d %H:%M"),
            end.format("%H:%M")
        ),
        None => session.started_at.format("%Y-%m-%d %H:%M").to_string(),
    };
    let mut line = format!(
        "## Last session {} · {} · {} tokens · {} events",
        short_id(&session.session_id),
        when,
        crate::ui::compact(session.total_tokens),
        crate::ui::thousands(session.total_events as u64),
    );
    if session.compactions > 0 {
        line.push_str(&format!(
            " · {} compaction{}",
            session.compactions,
            if session.compactions == 1 { "" } else { "s" }
        ));
    }
    line
}

fn proof_line(proof: &Proof) -> String {
    let mut parts = Vec::new();
    if let Some(passed) = &proof.last_passed {
        parts.push(format!("last passed {}", command_at(passed)));
    }
    if let Some(failed) = &proof.last_failed {
        parts.push(format!(
            "last failed {}, {}",
            command_at(failed),
            if proof.failed_was_rerun == Some(true) {
                "re-run and passed"
            } else {
                "not re-run"
            }
        ));
    }
    if parts.is_empty() {
        return "none recorded".to_string();
    }
    parts.join(" · ")
}

fn command_at(command: &RanCommand) -> String {
    format!(
        "`{}` {}",
        one_line(&command.command, 120),
        clock(command.at)
    )
}

fn changed_line(session: &WakeSession) -> String {
    if session.files_total == 0 {
        return "none recorded".to_string();
    }
    let mut parts = vec![format!(
        "{} file{}",
        session.files_total,
        if session.files_total == 1 { "" } else { "s" }
    )];
    parts.extend(
        session
            .files
            .iter()
            .map(|f| format!("{} ({})", basename(f), f.edits)),
    );
    parts.join(" · ")
}

fn prior_line(prior: &PriorSession) -> String {
    let rung = match (prior.outcome_rung, &prior.outcome_kind) {
        (Some(rung), _) => format!("rung {rung}"),
        (None, Some(kind)) => kind.clone(),
        (None, None) => "no outcome".to_string(),
    };
    format!(
        "- {} · {} · \"{}\"",
        prior.started_at.format("%m-%d %H:%M"),
        rung,
        match &prior.task {
            Some(task) => one_line(task, 90),
            None => "_first prompt not indexed yet_".to_string(),
        }
    )
}

fn next_blocker_line(next: &Next) -> String {
    match &next.blocker {
        Some(blocker) => format!(
            "Blocker `{}` failed at {} and was not run again.",
            one_line(&blocker.command, 120),
            clock(blocker.at)
        ),
        None => "Blocker none recorded.".to_string(),
    }
}

fn next_step_line(next: &Next) -> String {
    match &next.step {
        Some(step) => format!(
            "Next \"{}\"  [seq {}]",
            one_line(&step.text, 120),
            step.sequence
        ),
        None => "Next none recorded.".to_string(),
    }
}

fn receipt_line(report: &WakeReport) -> String {
    let mut parts = Vec::new();
    if let Some(id) = &report.receipt.session_id {
        parts.push(format!("session {id}"));
    }
    if let Some(adapter) = &report.receipt.adapter {
        parts.push(adapter.clone());
    }
    parts.push(format!("generated {}", stamp(report.receipt.generated_at)));
    if report.receipt.redacted {
        parts.push("redacted".to_string());
    }
    parts.join(" · ")
}

/// The last path segment, which is what identifies a file in a list the reader is skimming. The
/// full path is on the report.
fn basename(file: &FileTouch) -> &str {
    file.path.rsplit('/').next().unwrap_or(&file.path)
}

/// Replaces the home directory with `~`. Only ever shortens, so the line stays checkable.
fn shorten_path(path: &str) -> String {
    let Some(home) = home_dir() else {
        return path.to_string();
    };
    match path.strip_prefix(&home) {
        Some(rest) => format!("~{rest}"),
        None => path.to_string(),
    }
}

fn home_dir() -> Option<String> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .filter(|h| !h.is_empty())
}

fn short_id(session_id: &str) -> &str {
    agentworth_schema::text::truncate_chars(session_id, 8)
}

fn stamp(t: chrono::DateTime<chrono::Utc>) -> String {
    t.format("%Y-%m-%dT%H:%MZ").to_string()
}

fn stamp_space(t: chrono::DateTime<chrono::Utc>) -> String {
    t.format("%Y-%m-%d %H:%MZ").to_string()
}

fn clock(t: chrono::DateTime<chrono::Utc>) -> String {
    t.format("%H:%M").to_string()
}

fn one_line(text: &str, max_chars: usize) -> String {
    crate::handoff::markdown::one_line(text, max_chars)
}
