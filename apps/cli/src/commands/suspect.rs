//! Suspect commits: which commits on this branch came out of a session that never proved
//! anything.
//!
//! See `docs/specs/suspect-commits.md`. The short version: five products already read the diff,
//! which is the part a human can read too. This reads how the code was written — and stops
//! there. The output is a list of commits, session ids, and reasons. Never a patch, never a
//! diff, never a PR. `docs/specs/market-autofix.md` surveyed twenty observe-and-fix tools and
//! reached the conclusion this obeys: a trajectory can say the session was going badly, but it
//! cannot say what the code does wrong, so it triages and hands the work to something that can.
//!
//! Two things this must never get wrong, because both would make it lie quietly:
//!
//! 1. **Anchoring.** A blame row counts only when it is shown to lie inside *this* checkout.
//!    The naive suffix join flags 33% of commits and 9 of 10 sampled flags are false. See
//!    `Storage::blame_for_repo`.
//! 2. **Unknown is not clean.** A commit no indexed session touched is unattributed, and says
//!    so in its own field. A session with no `session_risk` row was never examined for loops or
//!    demoted claims, and that count is reported too.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use agentworth_outcomes::outcome_rank;
use agentworth_schema::OutcomeKind;
use agentworth_storage::{AnchoredBlameRow, SessionRisk, Storage};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, TimeZone, Utc};
use serde::{Deserialize, Serialize};

/// Attribution window: how long before a commit a session's file touch still counts as having
/// authored it. The spec's own open question — a commit can land days after the session that
/// wrote it, and a rebase moves the timestamp — so this is a default, not a measured value.
pub const DEFAULT_WINDOW_HOURS: i64 = 24;

/// Hard ceiling on how many commits one call will walk, so an unbounded `git log` on a large
/// repository cannot turn into an unbounded query.
pub const MAX_COMMITS_CEILING: usize = 1000;

/// Default when the caller names no range and no upstream resolves.
pub const DEFAULT_MAX_COMMITS: usize = 200;

/// The rung a session has to reach before "it never proved anything" stops being true.
const PROVEN_RUNG: u8 = 3; // OutcomeKind::TestOrBuildPassed

/// How far past a commit's recorded time a file touch still counts as having preceded it.
///
/// Git stores a committer time in whole seconds, truncated. A blame row carries sub-second
/// precision. So an edit made at 12:00:00.700 and committed at 12:00:00.900 is recorded as
/// happening *after* a commit stamped 12:00:00 — the ordering is real, the comparison is not.
/// One second is exactly the truncation, no more.
const COMMIT_TIME_SLACK: Duration = Duration::seconds(1);

/// One reason a session is worth a second look, and the receipt behind it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuspectReason {
    /// `no_test_run`, `no_outcome_detected`, `demoted_claim`, or `loop`.
    pub code: String,
    pub detail: String,
    /// The event in the session that produced this signal, when it has one. `no_test_run` is
    /// the absence of an event, so it has none — its receipt is the outcome rung.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_sequence: Option<u64>,
}

/// One indexed session that touched a file this commit changed, inside the attribution window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuspectSession {
    pub session_id: String,
    pub adapter: String,
    pub model: Option<String>,
    pub outcome: Option<String>,
    pub reasons: Vec<SuspectReason>,
    /// The anchored path that tied this session to this commit — the thing to check first if
    /// the attribution looks wrong.
    pub evidence_path: String,
    pub occurred_at: DateTime<Utc>,
    /// True when no `session_risk` row exists, so the demoted-claim and loop signals could not
    /// be evaluated for this session at all. Not the same as finding none.
    pub risk_unknown: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuspectCommit {
    pub sha: String,
    pub short_sha: String,
    pub subject: String,
    pub committed_at: DateTime<Utc>,
    pub sessions: Vec<SuspectSession>,
}

/// What this answer can be checked against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuspectReceipt {
    pub counted_at: DateTime<Utc>,
    /// Most recent session in the whole index, so a caller can notice a stale index without a
    /// second call.
    pub index_last_session_at: Option<DateTime<Utc>>,
    pub db_path: String,
    /// The rule that decided which blame rows counted, stated so the answer can be re-derived.
    pub anchoring_rule: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuspectReport {
    pub repo: String,
    pub repo_identity: String,
    /// How the commit range was chosen, in words — `origin/main..HEAD`, `--since=…`, or the
    /// fallback. The range is half the answer, so it is never left implicit.
    pub range: String,
    pub window_hours: i64,
    pub commits_scanned: usize,
    /// Commits at least one anchored session touched.
    pub attributed: usize,
    /// Commits no indexed session touched: written before the index existed, on another
    /// machine, or by hand. Unknown, never clean.
    pub unattributed: usize,
    /// Blame rows that could not be placed in any repository (relative paths from sessions
    /// whose own repository does not match this one). Dropped, and said out loud.
    pub unanchored_blame_rows: usize,
    /// Distinct attributed sessions with no `session_risk` row — never examined for loops or
    /// demoted claims. Re-run `agentworth scan` to fill these in.
    pub sessions_with_unknown_risk: usize,
    pub suspect: Vec<SuspectCommit>,
    /// The copyable block. This is the deliverable: a prompt, not a patch.
    pub prompt: String,
    pub receipt: SuspectReceipt,
}

/// What to look at, and over what range.
#[derive(Debug, Clone)]
pub struct SuspectQuery {
    pub repo: PathBuf,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub window_hours: i64,
    pub max_commits: usize,
}

impl Default for SuspectQuery {
    fn default() -> Self {
        Self {
            repo: PathBuf::from("."),
            branch: None,
            base: None,
            since: None,
            window_hours: DEFAULT_WINDOW_HOURS,
            max_commits: DEFAULT_MAX_COMMITS,
        }
    }
}

/// One commit as `git log` reported it.
#[derive(Debug, Clone)]
struct GitCommit {
    sha: String,
    committed_at: DateTime<Utc>,
    subject: String,
    files: Vec<String>,
}

/// Reject a ref that git would read as a flag, or that cannot be a ref at all. Arguments are
/// passed as argv entries (never through a shell), so this is not about injection — it is about
/// a `--force`-looking ref silently becoming an option.
fn validate_ref(name: &str, what: &str) -> Result<()> {
    if name.is_empty() {
        bail!("{what} must not be empty");
    }
    if name.starts_with('-') {
        bail!("{what} '{name}' starts with '-'; git would read it as an option");
    }
    if name.chars().any(|c| c.is_whitespace() || c.is_control()) {
        bail!("{what} '{name}' contains whitespace or control characters; not a valid git ref");
    }
    Ok(())
}

fn git(repo: &Path, args: &[&str]) -> Result<std::process::Output> {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .with_context(|| format!("failed to run `git {}` in {:?}", args.join(" "), repo))
}

fn git_stdout(repo: &Path, args: &[&str]) -> Result<String> {
    let out = git(repo, args)?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn ref_exists(repo: &Path, name: &str) -> bool {
    git(repo, &["rev-parse", "--verify", "--quiet", name])
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// The repository root for `path`, which must be inside a git checkout. Everything downstream
/// anchors against this exact string, so it has to be git's own answer rather than whatever the
/// caller typed.
pub fn resolve_repo_root(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        bail!("repo path {:?} does not exist", path);
    }
    let root = git_stdout(path, &["rev-parse", "--show-toplevel"])
        .with_context(|| format!("{:?} is not inside a git repository", path))?;
    Ok(PathBuf::from(root))
}

/// Every checkout of this repository on disk, besides the main one.
///
/// Not a nicety. A worktree normally sits inside the main checkout
/// (`<repo>/.claude/worktrees/<name>`), so an edit made in one records an absolute path that
/// anchors to `.claude/worktrees/<name>/crates/a.rs` — a path no commit will ever name.
/// Measured on this repository's own index, 58 of 308 in-repo blame rows were written from a
/// worktree, and every one of them attributed to nothing. Returns an empty list rather than
/// failing: a missing worktree list makes the answer smaller, not wrong.
fn linked_worktrees(root: &Path) -> Vec<String> {
    let Ok(out) = git_stdout(root, &["worktree", "list", "--porcelain"]) else {
        return Vec::new();
    };
    out.lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .map(str::to_string)
        .filter(|p| p != &root.to_string_lossy())
        .collect()
}

/// Choose the commit range, and say in words which rule chose it.
fn resolve_range(root: &Path, q: &SuspectQuery) -> Result<(Vec<String>, String)> {
    let head = match &q.branch {
        Some(b) => {
            validate_ref(b, "branch")?;
            b.clone()
        }
        None => "HEAD".to_string(),
    };

    if let Some(base) = &q.base {
        validate_ref(base, "base")?;
        if !ref_exists(root, base) {
            bail!("base ref '{base}' does not resolve in this repository");
        }
        let range = format!("{base}..{head}");
        return Ok((vec![range.clone()], range));
    }

    if let Some(since) = q.since {
        let arg = format!("--since={}", since.to_rfc3339());
        return Ok((
            vec![arg.clone(), head.clone()],
            format!("{head} {arg}"),
        ));
    }

    // The branch's own upstream is the honest default: it is the set of commits about to be
    // pushed, which is exactly what a pre-push hook is asking about.
    if let Ok(upstream) = git_stdout(root, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]) {
        if !upstream.is_empty() {
            let range = format!("{upstream}..{head}");
            return Ok((vec![range.clone()], range));
        }
    }
    for fallback in ["origin/main", "origin/master"] {
        if ref_exists(root, fallback) {
            let range = format!("{fallback}..{head}");
            return Ok((vec![range.clone()], range));
        }
    }

    Ok((
        vec![format!("-n{}", q.max_commits), head.clone()],
        format!("{head}, last {} commits (no upstream and no origin/main)", q.max_commits),
    ))
}

/// Read commits and their changed paths in one `git log`.
///
/// `%x01` starts each record and `%x1f` separates the fields inside its first line, so a subject
/// containing newlines, tabs, or pipes cannot be mistaken for structure.
fn read_commits(root: &Path, range: &[String], max_commits: usize) -> Result<Vec<GitCommit>> {
    let cap = max_commits.clamp(1, MAX_COMMITS_CEILING);
    let n_arg = format!("-n{cap}");
    let mut args: Vec<&str> = vec![
        "log",
        "--no-color",
        "--name-only",
        "--format=%x01%H%x1f%ct%x1f%s",
    ];
    let range_strs: Vec<&str> = range.iter().map(String::as_str).collect();
    // A caller-supplied `-n` already in the range wins; otherwise cap here.
    if !range.iter().any(|a| a.starts_with("-n")) {
        args.push(&n_arg);
    }
    args.extend(range_strs);
    args.push("--");

    let out = git(root, &args)?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        bail!("git log failed: {}", stderr.trim());
    }
    let text = String::from_utf8_lossy(&out.stdout);

    let mut commits = Vec::new();
    for record in text.split('\u{1}').skip(1) {
        let mut lines = record.lines();
        let Some(header) = lines.next() else { continue };
        let mut fields = header.split('\u{1f}');
        let (Some(sha), Some(ts), Some(subject)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let committed_at = ts
            .trim()
            .parse::<i64>()
            .ok()
            .and_then(|secs| Utc.timestamp_opt(secs, 0).single());
        let Some(committed_at) = committed_at else { continue };

        let files: Vec<String> = lines
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();

        commits.push(GitCommit {
            sha: sha.trim().to_string(),
            committed_at,
            subject: subject.trim().to_string(),
            files,
        });
    }
    Ok(commits)
}

/// The reasons one session is worth a second look. Empty means nothing was found — which, when
/// `risk_unknown` is set, is not the same as nothing being there.
fn reasons_for(outcome: Option<&str>, risk: Option<&SessionRisk>) -> Vec<SuspectReason> {
    let mut reasons = Vec::new();

    match outcome {
        None => reasons.push(SuspectReason {
            // Deliberately its own code rather than folded into `no_test_run`. Several adapters
            // extract no outcomes at all (`docs/capability-matrix.md`), so a null outcome can
            // mean "this session proved nothing" or "this adapter cannot tell us" -- weaker
            // evidence, and a reader must be able to see which one they have. Same distinction
            // `outcome_rate` draws with its `no_outcome_detection` reason.
            code: "no_outcome_detected".to_string(),
            detail: "no outcome of any kind was detected in this session; the adapter may not \
                     extract them, so this is weaker than a measured low rung"
                .to_string(),
            event_sequence: None,
        }),
        Some(name) => {
            let rank = parse_outcome_rank(name);
            if rank < PROVEN_RUNG {
                reasons.push(SuspectReason {
                    code: "no_test_run".to_string(),
                    detail: format!(
                        "session reached '{name}' (rung {rank}); nothing in it exited 0 on a test \
                         or build"
                    ),
                    event_sequence: None,
                });
            }
        }
    }

    if let Some(risk) = risk {
        if risk.demoted_claims > 0 {
            let first = risk.demoted_evidence.first();
            reasons.push(SuspectReason {
                code: "demoted_claim".to_string(),
                detail: match first {
                    Some(d) => format!(
                        "{} claim(s) contradicted by verification; first: '{}' downgraded to '{}'",
                        risk.demoted_claims, d.original_kind, d.final_kind
                    ),
                    None => format!("{} claim(s) contradicted by verification", risk.demoted_claims),
                },
                event_sequence: first.map(|d| d.event_sequence),
            });
        }
        if risk.loop_alerts > 0 {
            let first = risk.loop_evidence.first();
            reasons.push(SuspectReason {
                code: "loop".to_string(),
                detail: match first {
                    Some(l) => format!(
                        "{} loop alert(s), {} not self-corrected; first: {} on '{}' x{}",
                        risk.loop_alerts, risk.unresolved_loops, l.kind, l.target, l.repeat_count
                    ),
                    None => format!("{} loop alert(s)", risk.loop_alerts),
                },
                event_sequence: None,
            });
        }
    }

    reasons
}

/// Rank a stored `primary_outcome` string. Unknown strings rank 0 — below every real rung, so
/// an encoding the detector no longer writes reads as unproven rather than as proven.
fn parse_outcome_rank(name: &str) -> u8 {
    serde_json::from_value::<OutcomeKind>(serde_json::Value::String(name.to_string()))
        .map(outcome_rank)
        .unwrap_or(0)
}

/// Run the whole query: resolve the range, read the commits, join them to anchored sessions,
/// and attach each session's risk signals.
pub fn compute_suspect_commits(storage: &Storage, q: &SuspectQuery) -> Result<SuspectReport> {
    let root = resolve_repo_root(&q.repo)?;
    let root_str = root.to_string_lossy().to_string();
    let (range_args, range_desc) = resolve_range(&root, q)?;
    let commits = read_commits(&root, &range_args, q.max_commits)?;

    let window = Duration::hours(q.window_hours.max(0));
    // An empty range — nothing to push — must not turn into an unbounded blame query. With no
    // commits there is no `since` to bound it, so it would read the whole table to learn
    // nothing.
    let earliest = match commits.iter().map(|c| c.committed_at).min() {
        Some(t) => t - window,
        None => Utc::now(),
    };
    let worktrees = linked_worktrees(&root);
    let blame = storage.blame_for_repo(&root_str, &worktrees, Some(earliest))?;

    // One lookup per changed path, rather than a scan of every blame row per commit.
    let mut by_path: BTreeMap<&str, Vec<&AnchoredBlameRow>> = BTreeMap::new();
    for row in &blame.rows {
        by_path
            .entry(row.repo_relative_path.as_str())
            .or_default()
            .push(row);
    }

    // Candidate sessions first, so risk rows are fetched in one query rather than per commit.
    let mut matched: Vec<(usize, BTreeMap<String, &AnchoredBlameRow>)> = Vec::new();
    for (idx, commit) in commits.iter().enumerate() {
        let mut sessions: BTreeMap<String, &AnchoredBlameRow> = BTreeMap::new();
        for file in &commit.files {
            let Some(rows) = by_path.get(file.as_str()) else { continue };
            for row in rows {
                if row.modified_at > commit.committed_at + COMMIT_TIME_SLACK
                    || row.modified_at < commit.committed_at - window
                {
                    continue;
                }
                sessions
                    .entry(row.session_id.clone())
                    .and_modify(|existing| {
                        if row.modified_at > existing.modified_at {
                            *existing = row;
                        }
                    })
                    .or_insert(row);
            }
        }
        if !sessions.is_empty() {
            matched.push((idx, sessions));
        }
    }

    let session_ids: Vec<String> = matched
        .iter()
        .flat_map(|(_, s)| s.keys().cloned())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let risks = storage.get_session_risks(&session_ids)?;
    let sessions_with_unknown_risk = session_ids
        .iter()
        .filter(|id| !risks.contains_key(*id))
        .count();

    let mut suspect = Vec::new();
    for (idx, sessions) in &matched {
        let commit = &commits[*idx];
        let flagged: Vec<SuspectSession> = sessions
            .values()
            .filter_map(|row| {
                let risk = risks.get(&row.session_id);
                let reasons = reasons_for(row.primary_outcome.as_deref(), risk);
                if reasons.is_empty() {
                    return None;
                }
                Some(SuspectSession {
                    session_id: row.session_id.clone(),
                    adapter: row.adapter.clone(),
                    model: row.model.clone().or_else(|| row.models_used.first().cloned()),
                    outcome: row.primary_outcome.clone(),
                    reasons,
                    evidence_path: row.file_path.clone(),
                    occurred_at: row.modified_at,
                    risk_unknown: risk.is_none(),
                })
            })
            .collect();

        if !flagged.is_empty() {
            suspect.push(SuspectCommit {
                sha: commit.sha.clone(),
                short_sha: commit.sha.chars().take(8).collect(),
                subject: commit.subject.clone(),
                committed_at: commit.committed_at,
                sessions: flagged,
            });
        }
    }
    suspect.sort_by_key(|c| std::cmp::Reverse(c.committed_at));

    let attributed = matched.len();
    let prompt = build_prompt(&range_desc, &suspect, commits.len(), attributed);

    Ok(SuspectReport {
        repo: root_str,
        repo_identity: blame.repo_identity.clone(),
        range: range_desc,
        window_hours: q.window_hours,
        commits_scanned: commits.len(),
        attributed,
        unattributed: commits.len().saturating_sub(attributed),
        unanchored_blame_rows: blame.unanchored_rows,
        sessions_with_unknown_risk,
        suspect,
        prompt,
        receipt: SuspectReceipt {
            counted_at: Utc::now(),
            index_last_session_at: storage
                .get_aggregate_stats(true)
                .ok()
                .and_then(|s| s.last_session_at),
            db_path: storage
                .db_path()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "in-memory".to_string()),
            anchoring_rule: "a blame row counts only if, resolved against the session's own \
                             repository, it lies inside this repository; relative paths that \
                             cannot be placed are excluded and counted as unanchored"
                .to_string(),
        },
    })
}

/// The copyable block: what to review, and why, in the words a reviewer would use.
pub fn build_prompt(
    range: &str,
    suspect: &[SuspectCommit],
    commits_scanned: usize,
    attributed: usize,
) -> String {
    if suspect.is_empty() {
        if attributed == 0 {
            return format!(
                "No indexed session matched any of the {commits_scanned} commit(s) on {range}. \
                 That is not a clean bill of health — it means these commits have no session \
                 history on this machine to check."
            );
        }
        return format!(
            "None of the {attributed} attributed commit(s) on {range} came from a session with a \
             risk signal."
        );
    }

    let mut out = format!(
        "{} of {} commit(s) on {} came from a session that never proved anything.\n\n",
        suspect.len(),
        commits_scanned,
        range
    );
    for c in suspect {
        for s in &c.sessions {
            let codes: Vec<&str> = s.reasons.iter().map(|r| r.code.as_str()).collect();
            out.push_str(&format!(
                "  · {}  {}  [session {} · {} · {}]\n",
                c.short_sha,
                truncate_subject(&c.subject, 40),
                short_id(&s.session_id),
                s.model.as_deref().unwrap_or(&s.adapter),
                codes.join(", ")
            ));
        }
    }
    out.push_str(
        "\nReview these before pushing. Session ids are queryable with session_get; the \
         signals say the session was going badly, not what the code does wrong — read the diff \
         for that.\n",
    );
    out
}

fn short_id(session_id: &str) -> String {
    session_id.chars().take(8).collect()
}

fn truncate_subject(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{head}…")
}

/// The pre-push hook, printed for the user to install by hand.
///
/// It never blocks the push. A gate on a heuristic with a measured single-digit hit rate gets
/// turned off within a day, and then it protects nothing — so this prints and exits 0, always,
/// including when `agentworth` is not installed.
pub const PRE_PUSH_HOOK: &str = r#"#!/bin/sh
# agentworth suspect — a note on the way out, never a gate.
# Install: save as .git/hooks/pre-push and `chmod +x` it.
#
# Lists commits about to be pushed whose authoring session never proved anything.
# It exits 0 no matter what it finds, and no matter whether agentworth is installed.

command -v agentworth >/dev/null 2>&1 || exit 0
agentworth suspect --repo . --quiet || true
exit 0
"#;

/// Everything `agentworth suspect` accepts from the command line, so the runner takes one
/// argument instead of nine positional ones a caller can transpose.
#[derive(Debug, Default, Clone)]
pub struct SuspectArgs {
    pub repo: Option<PathBuf>,
    /// A date or a git ref — resolved by `parse_since`.
    pub since: Option<String>,
    pub branch: Option<String>,
    pub base: Option<String>,
    pub window_hours: Option<i64>,
    pub json: bool,
    pub hook: bool,
    pub quiet: bool,
}

/// Execute `agentworth suspect`.
pub fn run_suspect_command(
    args: SuspectArgs,
    db_path: Option<PathBuf>,
    ui: &crate::ui::Ui,
) -> Result<()> {
    if args.hook {
        print!("{PRE_PUSH_HOOK}");
        return Ok(());
    }

    // `--since` takes a date or a ref, because a person reaching for it means "since this
    // point" and does not want to know which of the two the tool wanted.
    let (since_time, base_ref) = match args.since {
        None => (None, args.base),
        Some(value) => match parse_since(&value) {
            Some(t) => (Some(t), args.base),
            None => {
                if args.base.is_some() {
                    bail!("--since '{value}' is not a date, and --base is already set");
                }
                (None, Some(value))
            }
        },
    };

    let storage = match db_path {
        Some(p) => Storage::open_path(&p)?,
        None => Storage::open_default()?,
    };
    let query = SuspectQuery {
        repo: args.repo.unwrap_or_else(|| PathBuf::from(".")),
        branch: args.branch,
        base: base_ref,
        since: since_time,
        window_hours: args.window_hours.unwrap_or(DEFAULT_WINDOW_HOURS),
        max_commits: DEFAULT_MAX_COMMITS,
    };
    let report = crate::ui::with_status(ui, "sweeping commits", || {
        compute_suspect_commits(&storage, &query)
    })?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    // `--quiet` is what the hook runs: say nothing on a clean push rather than printing a
    // screen the reader learns to scroll past.
    if args.quiet {
        if !report.suspect.is_empty() {
            println!();
            print!("{}", report.prompt);
        }
        return Ok(());
    }

    let rows: Vec<crate::ui::views::SuspectCommitRow> = report
        .suspect
        .iter()
        .map(|c| crate::ui::views::SuspectCommitRow {
            short_sha: c.short_sha.clone(),
            subject: c.subject.clone(),
            sessions: c
                .sessions
                .iter()
                .map(|s| crate::ui::views::SuspectSessionRow {
                    session_id: s.session_id.clone(),
                    model: s
                        .model
                        .as_deref()
                        .map(crate::ui::views::short_model)
                        .unwrap_or_else(|| s.adapter.clone()),
                    rung: s
                        .outcome
                        .as_deref()
                        .map(|o| parse_outcome_rank(o) as usize)
                        .unwrap_or(0),
                    reasons: s.reasons.iter().map(|r| r.code.clone()).collect(),
                    risk_unknown: s.risk_unknown,
                })
                .collect(),
        })
        .collect();

    print!(
        "{}",
        crate::ui::views::suspect(
            ui,
            &crate::ui::views::SuspectView {
                repo: &report.repo,
                range: &report.range,
                commits_scanned: report.commits_scanned,
                attributed: report.attributed,
                unattributed: report.unattributed,
                unanchored_blame_rows: report.unanchored_blame_rows,
                sessions_with_unknown_risk: report.sessions_with_unknown_risk,
                rows,
                prompt: &report.prompt,
            }
        )
    );

    Ok(())
}

/// `Some` when the string is a date this tool understands: RFC 3339, or a bare `YYYY-MM-DD`
/// read as midnight UTC. `None` means "treat it as a git ref".
fn parse_since(value: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return Some(dt.with_timezone(&Utc));
    }
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|naive| Utc.from_utc_datetime(&naive))
}

#[cfg(test)]
mod tests;
