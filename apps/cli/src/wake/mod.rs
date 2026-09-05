//! Wake: what a cold agent was doing, from the index and the checkout it is standing in.
//!
//! `docs/specs/wake.md` is the design. The short version: an agent that starts cold spends 15k
//! to 30k tokens answering four questions before it can work -- what was I doing, what state is
//! the checkout in, what passed and what failed, what is still open. Every one of those is a row
//! the index already holds or a `git` command that costs nothing. This module answers them in
//! one call and at most thirty lines.
//!
//! It is not a shorter handoff. A handoff keeps the inventory so a person can audit it; wake
//! keeps the last proof, the last failure and the loose ends, because that is what the next
//! agent acts on. Both surfaces read the same trace and neither summarises it.
//!
//! The two rules `handoff` holds, held here too: every claim carries a receipt, and a gap is
//! reported rather than padded. Nothing in the output is written by a model.

pub mod git;
mod markdown;

use agentworth_core::Scanner;
use agentworth_outcomes::{LooseEnd, OutcomeDetector};
use agentworth_redaction::Redactor;
use agentworth_schema::{
    extract_repository_or_workspace, AgentWorthTrace, EventPayload, NormalizedEvent,
    OutcomeEvidence, OutcomeKind,
};
use agentworth_storage::{SessionSummary, Storage};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::handoff::{find_decisions, FileTouch, OutcomeLine, RanCommand, Statement};

pub use git::{probe_checkout, Checkout, CheckoutProbe};
pub use markdown::{render_markdown, MAX_LINES};

/// How many prior sessions the "Before that" block names, and how many rows the loose-ends,
/// files and command sections carry. Three is what fits under the line budget with the receipt
/// and the checkout block still intact.
const SECTION_ROWS: usize = 3;

/// Named gaps, the machine-readable "I don't know". Stable strings: a caller branches on these,
/// so they are part of the contract. The four a handoff already names are re-exported from
/// there rather than spelled again, so the two surfaces cannot drift apart.
pub mod gap {
    pub use crate::handoff::gap::{
        NO_COMMANDS_RECORDED, NO_FILE_MODIFICATIONS, NO_OUTCOME_DETECTED, PROMPT_PREVIEW_EMPTY,
    };

    pub const GIT_UNAVAILABLE: &str = "git_unavailable";
    pub const GIT_TIMED_OUT: &str = "git_timed_out";
    pub const NOT_A_GIT_CHECKOUT: &str = "not_a_git_checkout";
    pub const SOURCE_UNREADABLE: &str = "source_unreadable";
    pub const NO_SESSION_FOR_REPO: &str = "no_session_for_repo";
    pub const NO_USER_MESSAGE: &str = "no_user_message";
    pub const NO_LOOSE_ENDS: &str = "no_loose_ends";
    pub const SCAN_BUDGET_EXHAUSTED: &str = "scan_budget_exhausted";

    /// Every gap this module can emit. Exists so a caller can validate a `gaps` list, and so the
    /// tests can hold the whole set to one naming rule.
    pub const ALL: &[&str] = &[
        GIT_UNAVAILABLE,
        GIT_TIMED_OUT,
        NOT_A_GIT_CHECKOUT,
        SOURCE_UNREADABLE,
        NO_SESSION_FOR_REPO,
        PROMPT_PREVIEW_EMPTY,
        NO_USER_MESSAGE,
        NO_OUTCOME_DETECTED,
        NO_COMMANDS_RECORDED,
        NO_FILE_MODIFICATIONS,
        NO_LOOSE_ENDS,
        SCAN_BUDGET_EXHAUSTED,
    ];
}

/// How the checkout probe came out, as one stable string on the report.
pub mod checkout_state {
    pub const FOUND: &str = "found";
    pub const NOT_A_CHECKOUT: &str = "not_a_checkout";
    pub const UNREADABLE: &str = "unreadable";
    pub const GIT_UNAVAILABLE: &str = "git_unavailable";
}

/// The line at the bottom of every wake document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WakeReceipt {
    pub session_id: Option<String>,
    pub repo: String,
    pub adapter: Option<String>,
    pub generated_at: DateTime<Utc>,
    pub index_last_updated: Option<DateTime<Utc>>,
    pub redacted: bool,
}

/// How stale the answer is. Both fields can be unknown, and unknown is said rather than assumed
/// fresh -- a stale index that reads as current is the one failure mode worse than no answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexFreshness {
    pub last_scanned_at: Option<DateTime<Utc>>,
    /// The indexed mtime against the source file's mtime now. `None` when either is unreadable.
    pub source_changed_since_scan: Option<bool>,
}

/// What passed, what failed, and whether the failure was ever retried.
///
/// Over verification-shaped commands only. The handoff's "Ran" section is an inventory; this is
/// the two lines a waking agent acts on, which is why it needs the full per-run list rather than
/// the deduped one -- "the same command ran again later and passed" is invisible once each
/// command string collapses to its last run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proof {
    pub last_passed: Option<RanCommand>,
    pub last_failed: Option<RanCommand>,
    /// Whether the exact command string of `last_failed` ran again later and passed. `None`
    /// when nothing failed.
    pub failed_was_rerun: Option<bool>,
    pub ran_total: usize,
    pub verification_total: usize,
}

/// The directory and branch the last session recorded for itself. Kept apart from the checkout
/// block on purpose: that one is now, this one is what the session saw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RanIn {
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
}

/// The session being woken into.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WakeSession {
    pub session_id: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub total_tokens: u64,
    pub total_events: usize,
    pub compactions: usize,
    pub task: Option<String>,
    pub last_asked: Option<String>,
    pub ran_in: Option<RanIn>,
    pub outcome: Option<OutcomeLine>,
    pub proof: Proof,
    pub files: Vec<FileTouch>,
    pub files_total: usize,
    pub loose_ends: Vec<LooseEnd>,
    pub loose_ends_total: usize,
    pub decided: Option<Statement>,
    pub forgotten_total: usize,
}

/// One earlier session for the same repo, one line's worth, from the index row alone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriorSession {
    pub session_id: String,
    pub started_at: DateTime<Utc>,
    pub outcome_rung: Option<u8>,
    pub outcome_kind: Option<String>,
    pub task: Option<String>,
}

/// The two things to do next, both quoted from the session rather than decided here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Next {
    pub blocker: Option<RanCommand>,
    pub step: Option<LooseEnd>,
}

/// Everything the wake document says, before it is rendered into anything.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WakeReport {
    pub receipt: WakeReceipt,
    /// The path that was probed, which is not necessarily the checkout root.
    pub workspace: String,
    pub checkout: Option<Checkout>,
    /// One of [`checkout_state`]. "No git installed" and "not a repository" are different facts.
    pub checkout_state: String,
    pub index: IndexFreshness,
    pub session: Option<WakeSession>,
    pub before: Vec<PriorSession>,
    /// True when the bounded newest-first scan for this repo ran out of budget before it found
    /// what it was asked for. Older sessions for the repo may exist past it, so "no session"
    /// means "none in the newest `REPO_SCAN_BUDGET`" and has to read that way.
    pub scan_exhausted: bool,
    pub next: Next,
    pub gaps: Vec<String>,
}

/// How much of the trace to expose.
#[derive(Debug, Clone, Copy, Default)]
pub struct WakeOptions {
    /// Off by default, everywhere. Redacted output is the rule; this is the per-call opt-in.
    pub include_raw: bool,
}

impl WakeReport {
    /// Records that the repo scan ran out of budget, and names the gap when it did. A partial
    /// answer that reads as complete is the failure this whole surface exists to avoid.
    pub fn mark_scan_exhausted(&mut self, exhausted: bool) {
        self.scan_exhausted = exhausted;
        if exhausted && !self.gaps.iter().any(|g| g == gap::SCAN_BUDGET_EXHAUSTED) {
            self.gaps.push(gap::SCAN_BUDGET_EXHAUSTED.to_string());
        }
    }

    /// A copy with every free-text field run through `redactor`.
    ///
    /// One [`Redactor::for_trace`] instance built from the same trace, used for nothing else --
    /// the same contract [`crate::handoff::HandoffReport::redacted`] documents, so the session's
    /// own repository identity is masked consistently across paths, commands and quoted
    /// sentences rather than on only one of them.
    pub fn redacted(&self, redactor: &Redactor) -> WakeReport {
        let text = |s: &String| redactor.redact_text(s);
        let opt_text = |s: &Option<String>| s.as_ref().map(&text);
        let command = |c: &RanCommand| RanCommand {
            command: redactor.redact_text(&c.command),
            ..c.clone()
        };
        let loose_end = |e: &LooseEnd| LooseEnd {
            text: redactor.redact_text(&e.text),
            ..e.clone()
        };

        WakeReport {
            receipt: WakeReceipt {
                repo: text(&self.receipt.repo),
                redacted: true,
                ..self.receipt.clone()
            },
            workspace: text(&self.workspace),
            checkout: self.checkout.as_ref().map(|c| Checkout {
                root: text(&c.root),
                branch: opt_text(&c.branch),
                head_subject: opt_text(&c.head_subject),
                upstream: opt_text(&c.upstream),
                ..c.clone()
            }),
            session: self.session.as_ref().map(|s| WakeSession {
                task: opt_text(&s.task),
                last_asked: opt_text(&s.last_asked),
                ran_in: s.ran_in.as_ref().map(|r| RanIn {
                    cwd: opt_text(&r.cwd),
                    git_branch: opt_text(&r.git_branch),
                }),
                outcome: s.outcome.as_ref().map(|o| OutcomeLine {
                    summary: text(&o.summary),
                    ..o.clone()
                }),
                proof: Proof {
                    last_passed: s.proof.last_passed.as_ref().map(&command),
                    last_failed: s.proof.last_failed.as_ref().map(&command),
                    ..s.proof.clone()
                },
                files: s
                    .files
                    .iter()
                    .map(|f| FileTouch {
                        path: text(&f.path),
                        ..f.clone()
                    })
                    .collect(),
                loose_ends: s.loose_ends.iter().map(&loose_end).collect(),
                decided: s.decided.as_ref().map(|d| Statement {
                    text: text(&d.text),
                    ..d.clone()
                }),
                ..s.clone()
            }),
            before: self
                .before
                .iter()
                .map(|p| PriorSession {
                    task: opt_text(&p.task),
                    ..p.clone()
                })
                .collect(),
            next: Next {
                blocker: self.next.blocker.as_ref().map(&command),
                step: self.next.step.as_ref().map(&loose_end),
            },
            ..self.clone()
        }
    }
}

/// Loads the newest primary session for `repo` and assembles the report.
///
/// Never scans. The index is read as it stands and the document says how stale that is; an agent
/// that needs its own current session indexed runs `archie scan`, which is the rule
/// `docs/specs/wake.md` keeps deliberately.
pub fn load_wake(
    storage: &Storage,
    scanner: &Scanner,
    repo: &str,
    workspace: &std::path::Path,
    options: WakeOptions,
) -> Result<WakeReport> {
    let checkout = probe_checkout(workspace);
    let workspace_str = workspace.to_string_lossy().to_string();
    let index_last_updated = storage.last_scanned_at().unwrap_or(None);

    let page = storage.list_sessions_for_repo(repo, SECTION_ROWS, false)?;
    let Some((newest, prior)) = page.sessions.split_first() else {
        let mut report =
            build_wake_without_session(repo, checkout, &workspace_str, index_last_updated);
        report.mark_scan_exhausted(page.scan_exhausted);
        // No trace to build a repository rule from, but the workspace path still carries the
        // user's home directory and their repository name. Redacted is the default here too.
        return Ok(if options.include_raw {
            report
        } else {
            report.redacted(&Redactor::new())
        });
    };

    let trace = scanner.load_trace(&newest.session_id)?;
    let outcomes = OutcomeDetector::new().detect_outcomes(&trace);
    let source_changed = source_changed_since_scan(newest);

    let mut report = build_wake(
        newest,
        &trace,
        &outcomes,
        prior,
        checkout,
        &workspace_str,
        index_last_updated,
        source_changed,
    );

    report.mark_scan_exhausted(page.scan_exhausted);

    if options.include_raw {
        Ok(report)
    } else {
        Ok(report.redacted(&Redactor::new().for_trace(&trace)))
    }
}

/// The indexed mtime against the file's mtime now. `None` when either is unavailable, which is
/// reported as `source_unreadable` rather than as "unchanged".
fn source_changed_since_scan(summary: &SessionSummary) -> Option<bool> {
    let indexed = summary.source_mtime_epoch_secs?;
    let now = std::fs::metadata(&summary.source_path)
        .and_then(|m| m.modified())
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some(now > indexed)
}

/// Assembles the report from things already in memory, so it can be tested with no storage,
/// scanner or filesystem involved.
#[allow(clippy::too_many_arguments)]
pub fn build_wake(
    summary: &SessionSummary,
    trace: &AgentWorthTrace,
    outcomes: &[OutcomeEvidence],
    prior: &[SessionSummary],
    checkout: CheckoutProbe,
    workspace: &str,
    index_last_updated: Option<DateTime<Utc>>,
    source_changed_since_scan: Option<bool>,
) -> WakeReport {
    let mut events: Vec<&NormalizedEvent> = trace.events.iter().collect();
    events.sort_by_key(|e| e.sequence);

    let (mut files, files_total) = crate::handoff::collect_files(&events);
    files.truncate(SECTION_ROWS);

    let proof = build_proof(&events);

    let mut loose_ends = agentworth_outcomes::find_loose_ends(&trace.events);
    loose_ends.sort_by_key(|e| std::cmp::Reverse(e.sequence));
    let loose_ends_total = loose_ends.len();
    loose_ends.truncate(SECTION_ROWS);

    let decided = find_decisions(&events)
        .into_iter()
        .max_by_key(|s| s.sequence);

    let forgotten_total = if trace.stats.compaction_count > 0 {
        let rounds = agentworth_schema::compaction_rounds(trace);
        agentworth_outcomes::diff_compaction_rounds(trace, &rounds, None).forgotten_total()
    } else {
        0
    };

    let task = summary
        .prompt_preview
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string);
    let last_asked = last_user_message(&events);
    let outcome = strongest_outcome_line(outcomes);

    let mut gaps = Vec::new();
    if let Some(name) = checkout_gap(&checkout) {
        gaps.push(name.to_string());
    }
    if source_changed_since_scan.is_none() {
        gaps.push(gap::SOURCE_UNREADABLE.to_string());
    }
    if task.is_none() {
        gaps.push(gap::PROMPT_PREVIEW_EMPTY.to_string());
    }
    if last_asked.is_none() {
        gaps.push(gap::NO_USER_MESSAGE.to_string());
    }
    if outcome.is_none() {
        gaps.push(gap::NO_OUTCOME_DETECTED.to_string());
    }
    if proof.ran_total == 0 {
        gaps.push(gap::NO_COMMANDS_RECORDED.to_string());
    }
    if files_total == 0 {
        gaps.push(gap::NO_FILE_MODIFICATIONS.to_string());
    }
    if loose_ends_total == 0 {
        gaps.push(gap::NO_LOOSE_ENDS.to_string());
    }

    let next = Next {
        blocker: proof
            .last_failed
            .clone()
            .filter(|_| proof.failed_was_rerun != Some(true)),
        step: loose_ends.first().cloned(),
    };

    WakeReport {
        receipt: WakeReceipt {
            session_id: Some(trace.session_id.clone()),
            repo: extract_repository_or_workspace(&trace.provenance.source_path),
            adapter: Some(trace.adapter.clone()),
            generated_at: Utc::now(),
            index_last_updated,
            redacted: false,
        },
        workspace: workspace.to_string(),
        checkout_state: state_name(&checkout).to_string(),
        checkout: found(checkout),
        index: IndexFreshness {
            last_scanned_at: index_last_updated,
            source_changed_since_scan,
        },
        session: Some(WakeSession {
            session_id: trace.session_id.clone(),
            started_at: trace.started_at,
            ended_at: trace.ended_at,
            total_tokens: trace.stats.token_usage.total(),
            total_events: trace.stats.total_events.max(trace.events.len()),
            compactions: trace.stats.compaction_count,
            task,
            last_asked,
            ran_in: ran_in(trace),
            outcome,
            proof,
            files,
            files_total,
            loose_ends,
            loose_ends_total,
            decided,
            forgotten_total,
        }),
        before: prior.iter().map(prior_session).collect(),
        scan_exhausted: false,
        next,
        gaps,
    }
}

/// The report for a repo the index holds no primary session for. The checkout block still
/// stands: it is read from git, and it is the half of the document that does not need an index.
pub fn build_wake_without_session(
    repo: &str,
    checkout: CheckoutProbe,
    workspace: &str,
    index_last_updated: Option<DateTime<Utc>>,
) -> WakeReport {
    let mut gaps = vec![gap::NO_SESSION_FOR_REPO.to_string()];
    if let Some(name) = checkout_gap(&checkout) {
        gaps.push(name.to_string());
    }

    WakeReport {
        receipt: WakeReceipt {
            session_id: None,
            repo: repo.to_string(),
            adapter: None,
            generated_at: Utc::now(),
            index_last_updated,
            redacted: false,
        },
        workspace: workspace.to_string(),
        checkout_state: state_name(&checkout).to_string(),
        checkout: found(checkout),
        index: IndexFreshness {
            last_scanned_at: index_last_updated,
            source_changed_since_scan: None,
        },
        session: None,
        before: Vec::new(),
        scan_exhausted: false,
        next: Next {
            blocker: None,
            step: None,
        },
        gaps,
    }
}

/// The gap a failed checkout probe names, if it failed.
fn checkout_gap(probe: &CheckoutProbe) -> Option<&'static str> {
    match probe {
        CheckoutProbe::Found(_) => None,
        CheckoutProbe::NotACheckout => Some(gap::NOT_A_GIT_CHECKOUT),
        CheckoutProbe::Unreadable => Some(gap::GIT_TIMED_OUT),
        CheckoutProbe::GitUnavailable => Some(gap::GIT_UNAVAILABLE),
    }
}

fn state_name(probe: &CheckoutProbe) -> &'static str {
    match probe {
        CheckoutProbe::Found(_) => checkout_state::FOUND,
        CheckoutProbe::NotACheckout => checkout_state::NOT_A_CHECKOUT,
        CheckoutProbe::Unreadable => checkout_state::UNREADABLE,
        CheckoutProbe::GitUnavailable => checkout_state::GIT_UNAVAILABLE,
    }
}

fn found(probe: CheckoutProbe) -> Option<Checkout> {
    match probe {
        CheckoutProbe::Found(checkout) => Some(checkout),
        _ => None,
    }
}

/// One line each for the sessions before the newest, off the index row alone. No trace is loaded
/// for these: the whole point of the block is that it costs one query.
fn prior_session(summary: &SessionSummary) -> PriorSession {
    let kind = summary.primary_outcome.clone();
    let rung = kind
        .as_deref()
        .and_then(parse_outcome_kind)
        .map(agentworth_outcomes::outcome_rank);
    PriorSession {
        session_id: summary.session_id.clone(),
        started_at: summary.started_at,
        outcome_rung: rung,
        outcome_kind: kind,
        task: summary
            .prompt_preview
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_string),
    }
}

/// `primary_outcome` is stored as the same snake_case string `OutcomeKind` serialises to, so the
/// round trip is the parse. An unrecognised value leaves the rung `None` rather than guessing.
fn parse_outcome_kind(name: &str) -> Option<OutcomeKind> {
    serde_json::from_value(serde_json::Value::String(name.to_string())).ok()
}

/// The last thing the user actually asked for.
///
/// Compaction summaries reach the trace as `Compaction` events, not user messages
/// (`crates/adapters/src/claude.rs` routes `isCompactSummary` records there), and a user record
/// carrying only a tool result becomes a `ToolResult` event with no text left over. The prefix
/// guard stays anyway: it costs one string comparison, and an adapter that has not learned the
/// distinction yet would otherwise hand back a summary as if the user had typed it.
fn last_user_message(events: &[&NormalizedEvent]) -> Option<String> {
    events.iter().rev().find_map(|event| {
        let EventPayload::UserMessage { content } = &event.payload else {
            return None;
        };
        let flat = content.split_whitespace().collect::<Vec<_>>().join(" ");
        if flat.is_empty() || flat.starts_with("This session is being continued") {
            return None;
        }
        Some(flat)
    })
}

/// The `cwd` and branch the adapter recorded from the transcript's own records. Absent for every
/// adapter that does not carry them, and the document then says nothing rather than borrowing
/// the current checkout's branch.
fn ran_in(trace: &AgentWorthTrace) -> Option<RanIn> {
    let workspace = trace.metadata.get("workspace")?;
    let string = |key: &str| {
        workspace
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let ran = RanIn {
        cwd: string("cwd"),
        git_branch: string("git_branch"),
    };
    (ran.cwd.is_some() || ran.git_branch.is_some()).then_some(ran)
}

/// The last test- or build-shaped command that passed, the last that failed, and whether that
/// failure was ever retried successfully.
///
/// Every run is kept, not the last run per command string: `cargo test` failing at 16:02 and
/// passing at 16:31 is two runs, and collapsing them loses exactly the fact this section exists
/// to report. A run whose ending is unknown counts as neither passed nor failed -- `handoff`
/// refuses to round an unrecorded exit up to success and so does this.
/// Test- and build-shaped commands only. `handoff::is_verification_command` also counts
/// commits, pushes and `gh` calls, which is right for an inventory and wrong here: a commit
/// proves the tree was recorded, not that anything passed, and the Outcome line already
/// reports it. Without this, a session whose last act was a commit shows "last passed
/// `git commit`" and hides the test run before it.
fn is_proof_command(c: &RanCommand) -> bool {
    const VCS: &[&str] = &["git commit", "git push", "gh pr", "gh run", "gh workflow"];
    let lower = c.command.to_lowercase();
    c.verification && !VCS.iter().any(|n| lower.contains(n))
}

fn build_proof(events: &[&NormalizedEvent]) -> Proof {
    let runs = crate::handoff::collect_runs(events);
    let verification: Vec<&RanCommand> = runs.iter().filter(|c| is_proof_command(c)).collect();

    let passed = |c: &RanCommand| c.exit_code == Some(0) || c.failed == Some(false);
    let failed =
        |c: &RanCommand| c.exit_code.is_some_and(|code| code != 0) || c.failed == Some(true);

    let last_passed = verification
        .iter()
        .rev()
        .find(|c| passed(c))
        .map(|c| (*c).clone());
    let last_failed = verification
        .iter()
        .rev()
        .find(|c| failed(c))
        .map(|c| (*c).clone());

    let failed_was_rerun = last_failed.as_ref().map(|f| {
        runs.iter()
            .any(|c| c.sequence > f.sequence && c.command == f.command && passed(c))
    });

    Proof {
        last_passed,
        last_failed,
        failed_was_rerun,
        ran_total: runs.len(),
        verification_total: verification.len(),
    }
}

fn strongest_outcome_line(outcomes: &[OutcomeEvidence]) -> Option<OutcomeLine> {
    let strongest = outcomes.iter().max_by_key(|o| {
        (
            agentworth_outcomes::outcome_rank(o.kind),
            (o.confidence * 1000.0) as u32,
        )
    })?;
    Some(OutcomeLine {
        rung: agentworth_outcomes::outcome_rank(strongest.kind),
        kind: agentworth_outcomes::outcome_kind_name(strongest.kind),
        summary: strongest.summary.clone(),
    })
}

#[cfg(test)]
mod tests;
