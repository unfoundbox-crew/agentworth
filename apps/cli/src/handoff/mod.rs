//! The handoff: what a session did, assembled from rows rather than written by a model.
//!
//! `docs/specs/handoff.md` is the design. The short version: a hand-written handoff is
//! two-thirds transcription (files changed, commands run, what was promised and not done) and
//! one-third judgment (open decisions, PR state, environment traps). This module produces the
//! transcription and says plainly that it did not produce the judgment.
//!
//! Two rules the rest of this file exists to keep:
//!
//! - **Every claim carries a receipt.** A session id on the document, and a sequence number or
//!   a timestamp on every individual line. A handoff that cannot be traced back to a session
//!   is a paragraph, and the next session has no way to check it.
//! - **A gap is reported, never padded.** `gaps` is the machine-readable "I don't know". When a
//!   session resolves to nothing but a token count, the output is a receipt and an empty body.
//!   A fabricated handoff gets read by an agent that cannot check it, which is the failure mode
//!   this product exists to prevent.
//!
//! It lives in `apps/cli` rather than a crate for the same reason `server/routes.rs` does: it
//! composes storage, outcomes, scoring and redaction, and is consumed by the CLI and the MCP
//! tools, both of which are here.

mod markdown;
mod statements;

use agentworth_core::Scanner;
use agentworth_outcomes::{outcome_rank, LooseEnd, OutcomeDetector};
use agentworth_redaction::Redactor;
use agentworth_schema::{
    AgentWorthTrace, EventPayload, OutcomeEvidence, OutcomeKind, extract_repository_or_workspace,
};
use agentworth_storage::{SessionSummary, Storage};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub use markdown::render_markdown;
pub use statements::{find_decisions, Statement};

/// The copyable prompt `docs/specs/dropped-commitments.md` designs, re-exported so the CLI
/// reaches one definition rather than assembling its own wording.
pub use agentworth_outcomes::loose_ends_prompt as loose_ends_prompt_for;

/// Default line budget for a rendered handoff, per `docs/specs/handoff.md`. 60 lines means
/// truncating the file list, not emitting 200 lines and apologising.
pub const DEFAULT_MAX_LINES: usize = 60;

/// Hard ceiling on the line budget. A caller can ask for more room than the default; it cannot
/// ask for the whole session back, which is what `session_get` is for.
pub const MAX_LINES_CEILING: usize = 120;

/// How many file touches, commands, decisions and loose ends the report carries before the
/// renderer even sees it. The renderer applies the real line budget; this only stops a
/// 29,642-event session from materialising thousands of rows nobody will read.
const SECTION_HARD_CAP: usize = 200;

/// Named gaps -- the machine-readable "I don't know" the spec asks for. Stable strings: a
/// caller is expected to branch on these, so they are part of the contract.
pub mod gap {
    pub const PROMPT_PREVIEW_EMPTY: &str = "prompt_preview_empty";
    pub const NO_FILE_MODIFICATIONS: &str = "no_file_modifications";
    pub const NO_OUTCOME_DETECTED: &str = "no_outcome_detected";
    pub const NO_COMMANDS_RECORDED: &str = "no_commands_recorded";
    pub const NO_DECISIONS_STATED: &str = "no_decisions_stated";
    pub const LOOSE_ENDS_NOT_REQUESTED: &str = "loose_ends_not_requested";
}

/// The two lines at the bottom of every handoff, and they are not optional.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandoffReceipt {
    pub session_id: String,
    pub repo: String,
    pub adapter: String,
    /// Kept so a redacted copy can be built from the report alone, and redacted in that copy.
    pub source_path: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub generated_at: DateTime<Utc>,
    /// Newest `scanned_at` in the index. Says how stale the answer is, which "newest session"
    /// does not -- a scan today over a session from last week moves this and not that.
    pub index_last_updated: Option<DateTime<Utc>>,
    pub redacted: bool,
}

/// One file the session touched, with how often and when it last did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileTouch {
    pub path: String,
    pub edits: usize,
    pub last_at: DateTime<Utc>,
    pub last_sequence: u64,
}

/// One command the session ran, and the strongest thing known about how it ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RanCommand {
    pub command: String,
    /// The real exit code, when the adapter captured one. Often `None`: Claude Code's
    /// transcript records the command a `Bash` tool call asked for but never its exit status,
    /// so for those sessions `failed` below is the only thing known about the ending.
    pub exit_code: Option<i32>,
    /// Whether the harness reported the correlated tool call as an error, when no exit code
    /// was recorded. `None` means genuinely unknown -- which is stated in the output rather
    /// than rounded to "it worked".
    pub failed: Option<bool>,
    pub at: DateTime<Utc>,
    pub sequence: u64,
    /// True when the command is test-, build- or release-shaped, so the renderer can keep the
    /// proof and drop the `ls`.
    pub verification: bool,
}

impl RanCommand {
    /// How this command's ending is written on one line. Never rounds unknown up to success.
    pub fn ending(&self) -> String {
        match (self.exit_code, self.failed) {
            (Some(code), _) => format!("exit {code}"),
            (None, Some(true)) => "reported an error, no exit code recorded".to_string(),
            (None, Some(false)) => "no error reported, no exit code recorded".to_string(),
            (None, None) => "exit not recorded".to_string(),
        }
    }
}

/// The outcome rung this session reached, both as the ladder position and as its own name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutcomeLine {
    pub rung: u8,
    pub kind: String,
    pub summary: String,
}

/// Everything a handoff says, before it is rendered into anything.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandoffReport {
    pub receipt: HandoffReceipt,
    /// The session's first prompt. `None` when `prompt_preview` has not been indexed -- the
    /// single most important line of a handoff, and the field most likely to be empty.
    pub task: Option<String>,
    pub outcome: Option<OutcomeLine>,
    pub total_tokens: u64,
    pub total_events: usize,
    pub duration_seconds: Option<f64>,
    pub compactions: usize,
    pub compaction_tokens_dropped: u64,
    pub files: Vec<FileTouch>,
    pub files_total: usize,
    pub ran: Vec<RanCommand>,
    pub ran_total: usize,
    pub decided: Vec<Statement>,
    pub loose_ends: Vec<LooseEnd>,
    pub gaps: Vec<String>,
}

impl HandoffReport {
    /// True when the session resolved to nothing but a token count. The renderer emits a
    /// receipt and an empty body in that case rather than padding it.
    pub fn body_is_empty(&self) -> bool {
        self.files.is_empty()
            && self.ran.is_empty()
            && self.decided.is_empty()
            && self.loose_ends.is_empty()
            && self.outcome.is_none()
    }

    /// A copy with every free-text field run through `redactor`.
    ///
    /// Pass one [`Redactor::for_trace`]-augmented instance, built from the same trace this
    /// report was assembled from, and use it for nothing else -- that is what makes the
    /// session's own repository/workspace identity get masked consistently across paths,
    /// commands and quoted sentences instead of only on one of them (see
    /// `docs/specs/mcp-server.md`'s `session_get` redaction note, which this follows).
    pub fn redacted(&self, redactor: &Redactor) -> HandoffReport {
        HandoffReport {
            receipt: HandoffReceipt {
                repo: redactor.redact_text(&self.receipt.repo),
                source_path: redactor.redact_text(&self.receipt.source_path),
                redacted: true,
                ..self.receipt.clone()
            },
            task: self.task.as_deref().map(|t| redactor.redact_text(t)),
            outcome: self.outcome.as_ref().map(|o| OutcomeLine {
                rung: o.rung,
                kind: o.kind.clone(),
                summary: redactor.redact_text(&o.summary),
            }),
            files: self
                .files
                .iter()
                .map(|f| FileTouch {
                    path: redactor.redact_text(&f.path),
                    ..f.clone()
                })
                .collect(),
            ran: self
                .ran
                .iter()
                .map(|c| RanCommand {
                    command: redactor.redact_text(&c.command),
                    ..c.clone()
                })
                .collect(),
            decided: self
                .decided
                .iter()
                .map(|s| Statement {
                    text: redactor.redact_text(&s.text),
                    ..s.clone()
                })
                .collect(),
            loose_ends: self
                .loose_ends
                .iter()
                .map(|e| LooseEnd {
                    text: redactor.redact_text(&e.text),
                    ..e.clone()
                })
                .collect(),
            ..self.clone()
        }
    }
}

/// How much of a session to assemble.
#[derive(Debug, Clone, Copy)]
pub struct HandoffOptions {
    pub include_loose_ends: bool,
}

impl Default for HandoffOptions {
    fn default() -> Self {
        Self {
            include_loose_ends: true,
        }
    }
}

/// Loads a session and assembles its handoff, returning the trace alongside so a caller can
/// build the redactor from it (see [`HandoffReport::redacted`]).
///
/// Every derived section comes off the trace that was just read, not off a new index column:
/// the file counts, the exit codes, the quoted sentences. `docs/specs/handoff.md` proposes
/// persisting a shell-command exit-code index; that stays unbuilt on purpose, because the
/// trace is already in memory here and a second copy of the same facts in SQLite is a
/// migration plus a rescan buying nothing this surface needs.
pub fn load_handoff(
    storage: &Storage,
    scanner: &Scanner,
    session_id: &str,
    options: HandoffOptions,
) -> Result<(HandoffReport, AgentWorthTrace)> {
    let summary = storage
        .get_session_by_id(session_id)?
        .with_context(|| format!("session '{session_id}' is not in the index"))?;
    let trace = scanner.load_trace(session_id)?;
    let outcomes = OutcomeDetector::new().detect_outcomes(&trace);
    let index_last_updated = storage.last_scanned_at().unwrap_or(None);

    let report = build_handoff(&summary, &trace, &outcomes, index_last_updated, options);
    Ok((report, trace))
}

/// Assembles a handoff from things already in memory. Split out from [`load_handoff`] so it can
/// be tested against a hand-built trace with no storage, scanner or filesystem involved.
pub fn build_handoff(
    summary: &SessionSummary,
    trace: &AgentWorthTrace,
    outcomes: &[OutcomeEvidence],
    index_last_updated: Option<DateTime<Utc>>,
    options: HandoffOptions,
) -> HandoffReport {
    let mut events: Vec<&agentworth_schema::NormalizedEvent> = trace.events.iter().collect();
    events.sort_by_key(|e| e.sequence);

    let (files, files_total) = collect_files(&events);
    let (ran, ran_total) = collect_commands(&events);
    let mut decided = find_decisions(&events);
    decided.truncate(SECTION_HARD_CAP);

    let loose_ends = if options.include_loose_ends {
        let mut ends = agentworth_outcomes::find_loose_ends(&trace.events);
        ends.truncate(SECTION_HARD_CAP);
        ends
    } else {
        Vec::new()
    };

    let outcome = strongest_outcome_line(outcomes);

    let task = summary
        .prompt_preview
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string);

    let mut gaps = Vec::new();
    if task.is_none() {
        gaps.push(gap::PROMPT_PREVIEW_EMPTY.to_string());
    }
    if files_total == 0 {
        gaps.push(gap::NO_FILE_MODIFICATIONS.to_string());
    }
    if outcome.is_none() {
        gaps.push(gap::NO_OUTCOME_DETECTED.to_string());
    }
    if ran_total == 0 {
        gaps.push(gap::NO_COMMANDS_RECORDED.to_string());
    }
    if decided.is_empty() {
        gaps.push(gap::NO_DECISIONS_STATED.to_string());
    }
    if !options.include_loose_ends {
        gaps.push(gap::LOOSE_ENDS_NOT_REQUESTED.to_string());
    }

    HandoffReport {
        receipt: HandoffReceipt {
            session_id: trace.session_id.clone(),
            repo: extract_repository_or_workspace(&trace.provenance.source_path),
            adapter: trace.adapter.clone(),
            source_path: trace.provenance.source_path.clone(),
            started_at: trace.started_at,
            ended_at: trace.ended_at,
            generated_at: Utc::now(),
            index_last_updated,
            redacted: false,
        },
        task,
        outcome,
        total_tokens: trace.stats.token_usage.total(),
        total_events: trace.stats.total_events.max(trace.events.len()),
        duration_seconds: trace.stats.duration_seconds,
        compactions: trace.stats.compaction_count,
        compaction_tokens_dropped: trace.stats.compaction_tokens_dropped,
        files,
        files_total,
        ran,
        ran_total,
        decided,
        loose_ends,
        gaps,
    }
}

/// Every file the session wrote, edited or deleted, most-recently-touched first.
///
/// Reads are excluded: a handoff answers "what changed", and a session that read two hundred
/// files and edited one should hand over the one.
fn collect_files(events: &[&agentworth_schema::NormalizedEvent]) -> (Vec<FileTouch>, usize) {
    use agentworth_schema::FileActionType;

    let mut by_path: std::collections::HashMap<String, FileTouch> = std::collections::HashMap::new();
    for event in events {
        let EventPayload::FileAction { path, action, .. } = &event.payload else {
            continue;
        };
        if matches!(action, FileActionType::Read) {
            continue;
        }
        by_path
            .entry(path.clone())
            .and_modify(|t| {
                t.edits += 1;
                t.last_at = event.timestamp;
                t.last_sequence = event.sequence;
            })
            .or_insert(FileTouch {
                path: path.clone(),
                edits: 1,
                last_at: event.timestamp,
                last_sequence: event.sequence,
            });
    }

    let mut files: Vec<FileTouch> = by_path.into_values().collect();
    files.sort_by(|a, b| {
        b.last_sequence
            .cmp(&a.last_sequence)
            .then_with(|| a.path.cmp(&b.path))
    });
    let total = files.len();
    files.truncate(SECTION_HARD_CAP);
    (files, total)
}

/// Every command the session ran, verification-shaped ones first, each carrying the strongest
/// thing known about how it ended.
///
/// **Exit codes mostly are not in the events.** `docs/specs/handoff.md` proposes persisting a
/// shell-command exit-code index on the assumption that "the events hold this; the index does
/// not". For Claude Code they do not: `crates/adapters/src/claude.rs` builds every
/// `ShellCommand` with `exit_code: None`, because a `Bash` tool call records the command that
/// was requested and the result comes back separately. So this correlates the two -- tool call
/// id to tool result -- and falls back to the harness's own `is_error` flag, which is a weaker
/// receipt than an exit code and is labelled as such rather than rounded up to success.
///
/// A command with nothing known about its ending is still listed. Naming which commands ran is
/// most of what this section is for; claiming one passed when nothing said so is not.
fn collect_commands(events: &[&agentworth_schema::NormalizedEvent]) -> (Vec<RanCommand>, usize) {
    use agentworth_schema::{ToolCall, ToolResult};

    // call id -> did the harness report the call as an error.
    let mut errored: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    for event in events {
        if let EventPayload::ToolResult(ToolResult { call_id, is_error, .. }) = &event.payload {
            if let Some(id) = call_id {
                errored.insert(id.clone(), *is_error);
            }
        }
    }

    let mut latest: std::collections::HashMap<String, RanCommand> =
        std::collections::HashMap::new();
    let mut pending_call: Option<String> = None;

    for event in events {
        match &event.payload {
            // The adapter emits the shell-command event immediately after the tool call it came
            // from, so the most recent shell-shaped tool call is the one to correlate against.
            EventPayload::ToolCall(ToolCall { id, name, .. }) => {
                let lower = name.to_lowercase();
                pending_call = if lower.contains("bash") || lower.contains("shell") {
                    id.clone()
                } else {
                    None
                };
            }
            EventPayload::ShellCommand(cmd) => {
                let command = cmd.command.trim().to_string();
                if command.is_empty() {
                    continue;
                }
                let failed = cmd.exit_code.map_or_else(
                    || pending_call.as_ref().and_then(|id| errored.get(id)).copied(),
                    |code| Some(code != 0),
                );
                latest.insert(
                    command.clone(),
                    RanCommand {
                        verification: is_verification_command(&command),
                        command,
                        exit_code: cmd.exit_code,
                        failed,
                        at: event.timestamp,
                        sequence: event.sequence,
                    },
                );
                pending_call = None;
            }
            _ => {}
        }
    }

    let mut ran: Vec<RanCommand> = latest.into_values().collect();
    // Verification first, then anything that failed, then most recent -- the three things a
    // next session actually needs off this list, in that order.
    ran.sort_by(|a, b| {
        b.verification
            .cmp(&a.verification)
            .then_with(|| b.failed.unwrap_or(false).cmp(&a.failed.unwrap_or(false)))
            .then_with(|| b.sequence.cmp(&a.sequence))
    });
    let total = ran.len();
    ran.truncate(SECTION_HARD_CAP);
    (ran, total)
}

/// Test-, build- or release-shaped commands: the ones whose exit code is evidence rather than
/// trivia. Deliberately a prefix/substring list and not a parser -- the cost of a false
/// positive here is one extra line in a section that is already ranked, not a wrong claim.
fn is_verification_command(command: &str) -> bool {
    let lower = command.to_lowercase();
    const NEEDLES: &[&str] = &[
        "test", "cargo build", "cargo check", "cargo clippy", "cargo fmt", "npm run", "pnpm run",
        "yarn ", "make ", "pytest", "go build", "go vet", "tsc", "eslint", "vitest", "jest",
        "git commit", "git push", "gh pr", "gh run", "gh workflow", "docker build", "mvn ",
        "gradle", "ruff", "mypy", "nextest",
    ];
    NEEDLES.iter().any(|n| lower.contains(n))
}

/// The highest rung any evidence in this session reached.
fn strongest_outcome_line(outcomes: &[OutcomeEvidence]) -> Option<OutcomeLine> {
    let strongest = outcomes
        .iter()
        .max_by_key(|o| (outcome_rank(o.kind), (o.confidence * 1000.0) as u32))?;
    Some(OutcomeLine {
        rung: outcome_rank(strongest.kind),
        kind: outcome_kind_wire_name(strongest.kind),
        summary: strongest.summary.clone(),
    })
}

fn outcome_kind_wire_name(kind: OutcomeKind) -> String {
    agentworth_outcomes::outcome_kind_name(kind)
}

#[cfg(test)]
mod tests;
