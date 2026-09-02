//! Independent, ground-truth verification of outcome evidence.
//!
//! `outcome.rs` classifies outcomes from what a transcript *says* happened: an assistant
//! claiming "tests pass", a tool call requesting `gh pr create`, a shell command reporting
//! exit code 0. This module re-checks those claims against things a transcript can't fake:
//! other real, structured events in the same trace, and — when the session's working
//! repository can be found on this machine — real git history and real file mtimes.
//!
//! Every check here is read-only. This module never mutates the repository it inspects; it
//! only shells out to read-only `git` plumbing (`status`, `log`, `cat-file -e`) and reads file
//! metadata. `std::process::Command::arg`/`args` pass argv entries directly (no shell), so even
//! an adversarial path or command string from a transcript can't be interpreted as a second
//! command.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use agentworth_schema::{
    EventPayload, FileActionType, NormalizedEvent, OutcomeEvidence, OutcomeKind, Provenance,
    ShellCommand, ToolCall,
};
use chrono::{DateTime, TimeZone, Utc};

use crate::outcome::{is_ci_or_deploy_command, is_commit_command, is_test_or_build_command};

/// How much clock skew / filesystem timestamp granularity to tolerate when comparing a file's
/// real mtime (or a commit's real timestamp) against the time an event claims it happened.
const VERIFY_SLACK_SECS: i64 = 5;

/// How far around a claimed commit's timestamp to search `git log` when no commit hash could
/// be parsed out of the claimed command's own output text.
const COMMIT_WINDOW_MINUTES: i64 = 10;

/// Safety bound on how many parent directories to walk looking for `.git`.
const MAX_WALK_UP: usize = 32;

/// One independent verification action taken against a piece of outcome evidence: what it
/// looked like before, what it looks like after, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct VerificationNote {
    /// The sequence number of the event that produced the verified evidence.
    pub event_sequence: u64,
    pub original_kind: OutcomeKind,
    pub final_kind: OutcomeKind,
    pub original_confidence: f32,
    pub final_confidence: f32,
    pub reason: String,
}

/// Best-effort resolution of the real repository this trace's session worked in.
///
/// Tries, in order: the most common non-empty `ShellCommand.cwd` seen in the trace (walking up
/// to the nearest `.git`), then — for Claude Code specifically — reversing the project-path
/// mangling Claude Code uses for its transcript directory names. Returns `None` rather than a
/// guess when nothing resolves to a real, existing, git-managed directory.
pub(crate) fn resolve_repo_root(
    events: &[NormalizedEvent],
    provenance: Option<&Provenance>,
) -> Option<PathBuf> {
    if let Some(root) = resolve_from_cwds(events) {
        return Some(root);
    }
    let prov = provenance?;
    if prov.adapter_name != "claude_code" {
        return None;
    }
    resolve_from_claude_code_transcript_path(&prov.source_path)
}

fn resolve_from_cwds(events: &[NormalizedEvent]) -> Option<PathBuf> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for event in events {
        if let EventPayload::ShellCommand(cmd) = &event.payload {
            if let Some(cwd) = cmd.cwd.as_deref() {
                if !cwd.is_empty() {
                    *counts.entry(cwd).or_insert(0) += 1;
                }
            }
        }
    }
    let mut candidates: Vec<(&str, usize)> = counts.into_iter().collect();
    candidates.sort_by_key(|c| std::cmp::Reverse(c.1));
    candidates
        .into_iter()
        .find_map(|(cwd, _)| walk_up_to_git_root(Path::new(cwd)))
}

/// Claude Code stores each session's transcript at
/// `~/.claude/projects/<mangled-cwd>/<session-uuid>.jsonl`, where `<mangled-cwd>` is the
/// project's original absolute path with every `/` and `.` replaced by `-` (confirmed against
/// real `~/.claude/projects` directory names on disk, e.g. `/Users/x/code/agentworth` becomes
/// `-Users-x-code-agentworth`). That mapping is lossy — a literal `-` in a real path is
/// indistinguishable from an encoded `/` or `.` — so the reconstructed path is a guess, never
/// trusted until it's confirmed to exist and sit inside a real git repo.
fn resolve_from_claude_code_transcript_path(source_path: &str) -> Option<PathBuf> {
    let parent = Path::new(source_path).parent()?;
    let mangled = parent.file_name()?.to_str()?;
    if !mangled.starts_with('-') {
        return None;
    }
    let candidate = PathBuf::from(mangled.replace('-', "/"));
    walk_up_to_git_root(&candidate)
}

fn walk_up_to_git_root(start: &Path) -> Option<PathBuf> {
    if !start.exists() {
        return None;
    }
    let mut cur = start.to_path_buf();
    for _ in 0..MAX_WALK_UP {
        if cur.join(".git").exists() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
    None
}

/// Run the full independent-verification pass over already-classified outcomes, mutating each
/// `OutcomeEvidence` in place (confidence, and — for an unconfirmed bare tool-call intent — its
/// kind) and returning a note for every adjustment made.
pub(crate) fn verify_outcomes(
    events: &[NormalizedEvent],
    outcomes: &mut [(usize, OutcomeEvidence)],
    repo_root: Option<&Path>,
) -> Vec<VerificationNote> {
    let real_successes = collect_real_command_successes(events);

    let mut notes = Vec::new();
    for (idx, evidence) in outcomes.iter_mut() {
        let event = &events[*idx];
        let before = (evidence.kind, evidence.confidence);

        match (&event.payload, evidence.kind) {
            // A bare self-report: "I ran the tests, they pass" with nothing structured behind
            // it yet. Cross-check against every real, structured signal in the same trace.
            (EventPayload::AssistantMessage { .. }, OutcomeKind::DoneClaimed) => {
                verify_done_claimed(evidence, &real_successes);
            }

            // A tool call is a *request*, not a result — `arguments` describe intent, not what
            // happened. `classify_command_string` (outcome.rs) scores these from the command
            // string alone, with no exit code available, so a claim sourced from a bare
            // ToolCall has no structural evidence it ever ran, let alone succeeded.
            //
            // `DoneClaimed` is in scope here too: the classifier now refuses to hand a
            // test/CI rung to a command with no captured exit code and emits a "result
            // unknown" `DoneClaimed` instead, and a *requested* command deserves less trust
            // than an observed one whose exit code merely went unrecorded.
            (EventPayload::ToolCall(tool), kind)
                if is_execution_claim(kind) || kind == OutcomeKind::DoneClaimed =>
            {
                verify_bare_tool_call(evidence, tool, &real_successes);
            }

            // A claimed file change: does the file really exist (or, for a delete, really not)
            // with a real mtime or git history to back it up?
            (EventPayload::FileAction { path, action, .. }, OutcomeKind::ArtifactChanged) => {
                if let Some(root) = repo_root {
                    verify_artifact_changed(evidence, root, path, *action, event.timestamp);
                }
            }

            // A claimed commit: does a commit matching the claim really exist in the repo?
            (EventPayload::ShellCommand(cmd), OutcomeKind::CommitObserved) => {
                if let Some(root) = repo_root {
                    verify_commit_observed(evidence, root, cmd, event.timestamp);
                }
            }

            _ => {}
        }

        if (evidence.kind, evidence.confidence) != before {
            notes.push(VerificationNote {
                event_sequence: event.sequence,
                original_kind: before.0,
                final_kind: evidence.kind,
                original_confidence: before.1,
                final_confidence: evidence.confidence,
                reason: evidence.summary.clone(),
            });
        }
    }
    notes
}

fn is_execution_claim(kind: OutcomeKind) -> bool {
    matches!(
        kind,
        OutcomeKind::TestOrBuildPassed
            | OutcomeKind::CommitObserved
            | OutcomeKind::CiOrDeploymentVerified
    )
}

/// Every command in this trace that has a *real, structurally captured* exit code of 0 —
/// i.e. actually observed to succeed, not merely requested or described in free text.
fn collect_real_command_successes(events: &[NormalizedEvent]) -> Vec<(OutcomeKind, DateTime<Utc>)> {
    let mut out = Vec::new();
    for event in events {
        if let EventPayload::ShellCommand(cmd) = &event.payload {
            if cmd.exit_code != Some(0) {
                continue;
            }
            let trimmed = cmd.command.trim().to_lowercase();
            if is_ci_or_deploy_command(&trimmed) {
                out.push((OutcomeKind::CiOrDeploymentVerified, event.timestamp));
            } else if is_commit_command(&trimmed) {
                out.push((OutcomeKind::CommitObserved, event.timestamp));
            } else if is_test_or_build_command(&trimmed) {
                out.push((OutcomeKind::TestOrBuildPassed, event.timestamp));
            }
        }
    }
    out
}

fn verify_done_claimed(evidence: &mut OutcomeEvidence, real_successes: &[(OutcomeKind, DateTime<Utc>)]) {
    if real_successes.is_empty() {
        bump_contradicted(
            evidence,
            0.10,
            "no shell command anywhere in this trace has a real recorded exit code backing up the claim",
        );
    } else {
        bump_confirmed(
            evidence,
            0.55,
            "a real command elsewhere in this trace really exited 0",
        );
    }
}

fn verify_bare_tool_call(
    evidence: &mut OutcomeEvidence,
    tool: &ToolCall,
    real_successes: &[(OutcomeKind, DateTime<Utc>)],
) {
    let claimed_kind = evidence.kind;
    let corroborated = real_successes.iter().any(|(k, _)| *k == claimed_kind);
    if corroborated {
        bump_confirmed(
            evidence,
            0.90,
            "a matching command elsewhere in this trace really exited 0",
        );
    } else {
        let reason = format!(
            "tool call '{}' was only ever requested; no tool result or shell command anywhere \
             in this trace shows it actually ran, let alone succeeded",
            tool.name
        );
        evidence.kind = OutcomeKind::DoneClaimed;
        evidence.confidence = evidence.confidence.min(0.20);
        evidence.summary = format!("{} [downgraded: {}]", evidence.summary, reason);
    }
}

fn verify_artifact_changed(
    evidence: &mut OutcomeEvidence,
    repo_root: &Path,
    claimed_path: &str,
    action: FileActionType,
    claimed_time: DateTime<Utc>,
) {
    let Some(abs_path) = resolve_claimed_path(repo_root, claimed_path) else {
        return;
    };
    let exists = abs_path.exists();

    if action == FileActionType::Delete {
        if exists {
            bump_contradicted(evidence, 0.20, "the file claimed deleted is still present on disk");
        } else {
            bump_confirmed(evidence, 0.85, "the file is really absent, matching the claimed deletion");
        }
        return;
    }

    if mtime_at_or_after(&abs_path, claimed_time) {
        bump_confirmed(evidence, 0.85, "the file's real mtime matches the claimed edit");
        return;
    }

    if let Some(commit_time) = git_last_commit_time_for_path(repo_root, claimed_path) {
        if commit_time + chrono::Duration::seconds(VERIFY_SLACK_SECS) >= claimed_time {
            bump_confirmed(
                evidence,
                0.85,
                "a real git commit touching this path lands at or after the claimed edit",
            );
            return;
        }
    }

    if git_status_is_dirty(repo_root, claimed_path) {
        bump_confirmed(evidence, 0.75, "git reports this path as currently modified");
        return;
    }

    if exists {
        bump_contradicted(
            evidence,
            0.30,
            "the file exists, but neither its mtime nor git history back up the claimed edit time",
        );
    } else {
        bump_contradicted(evidence, 0.15, "the claimed file does not exist in the working tree");
    }
}

fn verify_commit_observed(
    evidence: &mut OutcomeEvidence,
    repo_root: &Path,
    cmd: &ShellCommand,
    claimed_time: DateTime<Utc>,
) {
    let output = cmd.output.as_deref().unwrap_or("");
    if let Some(hash) = extract_short_hash(output) {
        if git_commit_exists(repo_root, &hash) {
            bump_confirmed(
                evidence,
                0.97,
                &format!("commit {} really exists in this repository", hash),
            );
        } else {
            bump_contradicted(
                evidence,
                0.15,
                &format!("no commit {} exists in this repository", hash),
            );
        }
        return;
    }

    if git_any_commit_near(repo_root, claimed_time) {
        bump_confirmed(
            evidence,
            0.80,
            "a real commit landed in this repository around the claimed time",
        );
    } else {
        bump_contradicted(
            evidence,
            0.20,
            "no commit landed in this repository around the claimed time",
        );
    }
}

fn resolve_claimed_path(repo_root: &Path, claimed_path: &str) -> Option<PathBuf> {
    let cleaned = claimed_path.trim();
    if cleaned.is_empty() {
        return None;
    }
    let p = Path::new(cleaned);
    Some(if p.is_absolute() {
        p.to_path_buf()
    } else {
        repo_root.join(p)
    })
}

fn mtime_at_or_after(path: &Path, claimed_time: DateTime<Utc>) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    let mtime: DateTime<Utc> = modified.into();
    mtime + chrono::Duration::seconds(VERIFY_SLACK_SECS) >= claimed_time
}

fn bump_confirmed(evidence: &mut OutcomeEvidence, floor: f32, note: &str) {
    evidence.confidence = evidence.confidence.max(floor).min(1.0);
    evidence.summary = format!("{} [confirmed: {}]", evidence.summary, note);
}

fn bump_contradicted(evidence: &mut OutcomeEvidence, ceiling: f32, note: &str) {
    evidence.confidence = evidence.confidence.min(ceiling).max(0.0);
    evidence.summary = format!("{} [unconfirmed: {}]", evidence.summary, note);
}

static COMMIT_HASH_REGEX: OnceLock<regex::Regex> = OnceLock::new();

/// Pulls a short/long commit hash out of a `git commit`-shaped output line, e.g.
/// `[main 9f3e1a2] feat: implement feature` -> `9f3e1a2`.
fn extract_short_hash(output: &str) -> Option<String> {
    let re = COMMIT_HASH_REGEX
        .get_or_init(|| regex::Regex::new(r"\[[^\s\]]+\s+([0-9a-fA-F]{7,40})\]").expect("valid regex"));
    re.captures(output)?.get(1).map(|m| m.as_str().to_string())
}

/// Shells out to `git`, read-only, using `-C` so the repo path is passed as a single argv
/// entry (never interpolated into a shell string) — safe even if the path or claimed command
/// text came from an adversarial transcript. Never called with a subcommand that mutates.
fn run_git(repo_root: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        None
    }
}

fn git_last_commit_time_for_path(repo_root: &Path, claimed_path: &str) -> Option<DateTime<Utc>> {
    let out = run_git(repo_root, &["log", "-1", "--format=%ct", "--", claimed_path])?;
    let secs: i64 = out.trim().parse().ok()?;
    Utc.timestamp_opt(secs, 0).single()
}

fn git_status_is_dirty(repo_root: &Path, claimed_path: &str) -> bool {
    run_git(repo_root, &["status", "--porcelain", "--", claimed_path])
        .map(|out| !out.trim().is_empty())
        .unwrap_or(false)
}

fn git_commit_exists(repo_root: &Path, hash: &str) -> bool {
    let rev = format!("{hash}^{{commit}}");
    run_git(repo_root, &["cat-file", "-e", &rev]).is_some()
}

fn git_any_commit_near(repo_root: &Path, claimed_time: DateTime<Utc>) -> bool {
    let since = (claimed_time - chrono::Duration::minutes(COMMIT_WINDOW_MINUTES)).to_rfc3339();
    let until = (claimed_time + chrono::Duration::minutes(COMMIT_WINDOW_MINUTES)).to_rfc3339();
    run_git(
        repo_root,
        &["log", "--since", &since, "--until", &until, "--oneline", "-1"],
    )
    .map(|out| !out.trim().is_empty())
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::OutcomeEvidence;
    use chrono::Utc;

    fn init_git_repo(dir: &Path) {
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .status()
                .expect("git must be installed to run this test");
            assert!(status.success(), "git {:?} failed", args);
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
    }

    /// Writes, adds, and commits `rel_path` inside `dir`; returns the full commit hash.
    fn commit_file(dir: &Path, rel_path: &str, contents: &str) -> String {
        let full = dir.join(rel_path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&full, contents).unwrap();
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {:?} failed", args);
        };
        run(&["add", rel_path]);
        run(&["commit", "-q", "-m", "test commit"]);
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    fn evidence(kind: OutcomeKind, confidence: f32) -> OutcomeEvidence {
        OutcomeEvidence {
            kind,
            summary: "original claim".to_string(),
            confidence,
        }
    }

    /// A directory under `/tmp` with a name guaranteed free of `.` and `-`, unlike
    /// `tempfile`'s own default naming (which can include a leading dot). The claude-code
    /// path-unmangling test round-trips a path through a lossy `/`-and-`.`-to-`-` transform, so
    /// it needs a path it can prove has none of those characters to collide with; hardcoding
    /// `/tmp` (present on every machine this runs on: this Mac, lenovo, CI) sidesteps whatever
    /// `$TMPDIR`/`tempfile` defaults happen to be instead of hoping they're clean.
    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new(label: &str) -> Self {
            let dir = PathBuf::from("/tmp").join(format!(
                "agentworth_outcomes_verify_test_{}_{}",
                label,
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir); // clear out any stale dir from a crashed prior run
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn tool_call_only_claim_without_corroboration_is_downgraded() {
        let mut outcomes = vec![(0usize, evidence(OutcomeKind::CiOrDeploymentVerified, 0.90))];
        let events = vec![NormalizedEvent::new(
            1,
            Utc::now(),
            EventPayload::ToolCall(ToolCall {
                id: Some("c1".to_string()),
                name: "Bash".to_string(),
                arguments: serde_json::json!({"command": "gh pr create --title x"}),
            }),
        )];

        let notes = verify_outcomes(&events, &mut outcomes, None);

        assert_eq!(outcomes[0].1.kind, OutcomeKind::DoneClaimed);
        assert!(outcomes[0].1.confidence <= 0.20);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].original_kind, OutcomeKind::CiOrDeploymentVerified);
        assert_eq!(notes[0].final_kind, OutcomeKind::DoneClaimed);
        assert_eq!(notes[0].event_sequence, 1);
    }

    #[test]
    fn tool_call_claim_confirmed_by_a_real_matching_success_elsewhere() {
        let mut outcomes = vec![(0usize, evidence(OutcomeKind::CommitObserved, 0.80))];
        let events = vec![
            NormalizedEvent::new(
                1,
                Utc::now(),
                EventPayload::ToolCall(ToolCall {
                    id: Some("c1".to_string()),
                    name: "Bash".to_string(),
                    arguments: serde_json::json!({"command": "git commit -m x"}),
                }),
            ),
            NormalizedEvent::new(
                2,
                Utc::now(),
                EventPayload::ShellCommand(ShellCommand {
                    command: "git commit -m x".to_string(),
                    cwd: None,
                    exit_code: Some(0),
                    output: Some("[main abc1234] x".to_string()),
                }),
            ),
        ];

        let notes = verify_outcomes(&events, &mut outcomes, None);

        assert_eq!(outcomes[0].1.kind, OutcomeKind::CommitObserved);
        assert!(outcomes[0].1.confidence >= 0.90);
        assert!(outcomes[0].1.summary.contains("confirmed"));
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn done_claimed_without_any_real_command_is_contradicted() {
        let mut outcomes = vec![(0usize, evidence(OutcomeKind::DoneClaimed, 0.35))];
        let events = vec![NormalizedEvent::new(
            1,
            Utc::now(),
            EventPayload::AssistantMessage {
                content: "I have completed the task!".to_string(),
                thinking: None,
            },
        )];

        verify_outcomes(&events, &mut outcomes, None);

        assert_eq!(outcomes[0].1.kind, OutcomeKind::DoneClaimed);
        assert!(outcomes[0].1.confidence <= 0.10);
        assert!(outcomes[0].1.summary.contains("unconfirmed"));
    }

    #[test]
    fn done_claimed_with_a_real_backing_command_is_confirmed() {
        let mut outcomes = vec![(1usize, evidence(OutcomeKind::DoneClaimed, 0.35))];
        let events = vec![
            NormalizedEvent::new(
                1,
                Utc::now(),
                EventPayload::ShellCommand(ShellCommand {
                    command: "cargo test".to_string(),
                    cwd: None,
                    exit_code: Some(0),
                    output: Some("test result: ok. 3 passed; 0 failed".to_string()),
                }),
            ),
            NormalizedEvent::new(
                2,
                Utc::now(),
                EventPayload::AssistantMessage {
                    content: "I have completed the task!".to_string(),
                    thinking: None,
                },
            ),
        ];

        verify_outcomes(&events, &mut outcomes, None);

        assert!((outcomes[0].1.confidence - 0.55).abs() < f32::EPSILON);
        assert!(outcomes[0].1.summary.contains("confirmed"));
    }

    #[test]
    fn artifact_change_confirmed_by_real_file_mtime_without_a_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("src.txt"), "hello").unwrap();

        let mut outcomes = vec![(0usize, evidence(OutcomeKind::ArtifactChanged, 0.60))];
        let events = vec![NormalizedEvent::new(
            1,
            Utc::now() - chrono::Duration::seconds(1),
            EventPayload::FileAction {
                path: "src.txt".to_string(),
                action: FileActionType::Write,
                diff: None,
                lines_changed: Some(1),
            },
        )];

        verify_outcomes(&events, &mut outcomes, Some(dir.path()));

        assert!(outcomes[0].1.confidence >= 0.85);
        assert!(outcomes[0].1.summary.contains("confirmed"));
    }

    #[test]
    fn artifact_change_contradicted_when_file_never_existed() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());

        let mut outcomes = vec![(0usize, evidence(OutcomeKind::ArtifactChanged, 0.60))];
        let events = vec![NormalizedEvent::new(
            1,
            Utc::now(),
            EventPayload::FileAction {
                path: "never_existed.txt".to_string(),
                action: FileActionType::Write,
                diff: None,
                lines_changed: Some(1),
            },
        )];

        verify_outcomes(&events, &mut outcomes, Some(dir.path()));

        assert!(outcomes[0].1.confidence <= 0.15);
        assert!(outcomes[0].1.summary.contains("unconfirmed"));
    }

    #[test]
    fn artifact_delete_confirmed_when_file_really_absent() {
        let dir = tempfile::tempdir().unwrap();

        let mut outcomes = vec![(0usize, evidence(OutcomeKind::ArtifactChanged, 0.60))];
        let events = vec![NormalizedEvent::new(
            1,
            Utc::now(),
            EventPayload::FileAction {
                path: "gone.txt".to_string(),
                action: FileActionType::Delete,
                diff: None,
                lines_changed: None,
            },
        )];

        verify_outcomes(&events, &mut outcomes, Some(dir.path()));

        assert!(outcomes[0].1.confidence >= 0.85);
        assert!(outcomes[0].1.summary.contains("confirmed"));
    }

    #[test]
    fn artifact_delete_contradicted_when_file_still_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("still_here.txt"), "hi").unwrap();

        let mut outcomes = vec![(0usize, evidence(OutcomeKind::ArtifactChanged, 0.60))];
        let events = vec![NormalizedEvent::new(
            1,
            Utc::now(),
            EventPayload::FileAction {
                path: "still_here.txt".to_string(),
                action: FileActionType::Delete,
                diff: None,
                lines_changed: None,
            },
        )];

        verify_outcomes(&events, &mut outcomes, Some(dir.path()));

        assert!(outcomes[0].1.confidence <= 0.20);
        assert!(outcomes[0].1.summary.contains("unconfirmed"));
    }

    #[test]
    fn commit_observed_confirmed_when_hash_really_exists() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        let hash = commit_file(dir.path(), "a.txt", "hi");
        let short = &hash[..7];

        let mut outcomes = vec![(0usize, evidence(OutcomeKind::CommitObserved, 0.90))];
        let events = vec![NormalizedEvent::new(
            1,
            Utc::now(),
            EventPayload::ShellCommand(ShellCommand {
                command: "git commit -m x".to_string(),
                cwd: None,
                exit_code: Some(0),
                output: Some(format!("[main {}] x\n 1 file changed", short)),
            }),
        )];

        verify_outcomes(&events, &mut outcomes, Some(dir.path()));

        assert!(outcomes[0].1.confidence >= 0.95);
        assert!(outcomes[0].1.summary.contains("confirmed"));
    }

    #[test]
    fn commit_observed_contradicted_when_hash_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        commit_file(dir.path(), "a.txt", "hi");

        let mut outcomes = vec![(0usize, evidence(OutcomeKind::CommitObserved, 0.90))];
        let events = vec![NormalizedEvent::new(
            1,
            Utc::now(),
            EventPayload::ShellCommand(ShellCommand {
                command: "git commit -m x".to_string(),
                cwd: None,
                exit_code: Some(0),
                output: Some("[main 0000000] x\n 1 file changed".to_string()),
            }),
        )];

        verify_outcomes(&events, &mut outcomes, Some(dir.path()));

        assert!(outcomes[0].1.confidence <= 0.15);
        assert!(outcomes[0].1.summary.contains("unconfirmed"));
    }

    #[test]
    fn commit_observed_confirmed_via_time_window_when_output_has_no_hash() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        commit_file(dir.path(), "a.txt", "hi"); // a real commit landing "now"

        let mut outcomes = vec![(0usize, evidence(OutcomeKind::CommitObserved, 0.90))];
        let events = vec![NormalizedEvent::new(
            1,
            Utc::now(), // within the +/-10 minute window of the commit made just above
            EventPayload::ShellCommand(ShellCommand {
                command: "git commit -m x".to_string(),
                cwd: None,
                exit_code: Some(0),
                output: Some("Committed successfully, no hash printed here".to_string()),
            }),
        )];

        verify_outcomes(&events, &mut outcomes, Some(dir.path()));

        assert!(outcomes[0].1.confidence >= 0.75);
        assert!(outcomes[0].1.summary.contains("confirmed"));
    }

    #[test]
    fn commit_observed_contradicted_via_time_window_when_claim_is_far_from_any_commit() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        commit_file(dir.path(), "a.txt", "hi"); // a real commit landing "now"

        let mut outcomes = vec![(0usize, evidence(OutcomeKind::CommitObserved, 0.90))];
        let events = vec![NormalizedEvent::new(
            1,
            Utc::now() - chrono::Duration::hours(2), // well outside the +/-10 minute window
            EventPayload::ShellCommand(ShellCommand {
                command: "git commit -m x".to_string(),
                cwd: None,
                exit_code: Some(0),
                output: Some("Committed successfully, no hash printed here".to_string()),
            }),
        )];

        verify_outcomes(&events, &mut outcomes, Some(dir.path()));

        assert!(outcomes[0].1.confidence <= 0.20);
        assert!(outcomes[0].1.summary.contains("unconfirmed"));
    }

    #[test]
    fn repo_root_resolves_from_most_common_shell_cwd() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        let cwd = dir.path().to_string_lossy().to_string();

        let events = vec![
            NormalizedEvent::new(
                1,
                Utc::now(),
                EventPayload::ShellCommand(ShellCommand {
                    command: "ls".to_string(),
                    cwd: Some(cwd.clone()),
                    exit_code: Some(0),
                    output: None,
                }),
            ),
            NormalizedEvent::new(
                2,
                Utc::now(),
                EventPayload::ShellCommand(ShellCommand {
                    command: "pwd".to_string(),
                    cwd: Some(cwd),
                    exit_code: Some(0),
                    output: None,
                }),
            ),
        ];

        let root = resolve_repo_root(&events, None);
        assert_eq!(root, Some(dir.path().to_path_buf()));
    }

    #[test]
    fn repo_root_is_none_when_nothing_resolves() {
        let events = vec![NormalizedEvent::new(
            1,
            Utc::now(),
            EventPayload::ShellCommand(ShellCommand {
                command: "ls".to_string(),
                cwd: Some("/definitely/not/a/real/path/xyz123".to_string()),
                exit_code: Some(0),
                output: None,
            }),
        )];
        assert_eq!(resolve_repo_root(&events, None), None);
    }

    #[test]
    fn repo_root_resolves_from_claude_code_mangled_transcript_path() {
        let dir = ScratchDir::new("claude_fallback_match");
        init_git_repo(dir.path());

        let real_path = dir.path().canonicalize().unwrap();
        let mangled: String = real_path
            .to_string_lossy()
            .chars()
            .map(|c| if c == '/' || c == '.' { '-' } else { c })
            .collect();
        let fake_source_path = format!("/wherever/claude/projects/{}/session.jsonl", mangled);
        let prov = Provenance::new(fake_source_path, "claude_code", 10, 0, "fp");

        let root = resolve_repo_root(&[], Some(&prov));
        assert_eq!(root, Some(real_path));
    }

    #[test]
    fn claude_code_fallback_is_not_used_for_other_adapters() {
        let dir = ScratchDir::new("claude_fallback_gated");
        init_git_repo(dir.path());
        let real_path = dir.path().canonicalize().unwrap();
        let mangled: String = real_path
            .to_string_lossy()
            .chars()
            .map(|c| if c == '/' || c == '.' { '-' } else { c })
            .collect();
        let fake_source_path = format!("/wherever/projects/{}/session.jsonl", mangled);
        let prov = Provenance::new(fake_source_path, "codex", 10, 0, "fp");

        assert_eq!(resolve_repo_root(&[], Some(&prov)), None);
    }

    #[test]
    fn adapter_embedded_outcome_evidence_is_never_touched() {
        // Events whose payload IS already an OutcomeEvidence (adapter-embedded) don't match any
        // verification arm and must pass through completely unchanged.
        let mut outcomes = vec![(0usize, evidence(OutcomeKind::CiOrDeploymentVerified, 0.42))];
        let events = vec![NormalizedEvent::new(
            1,
            Utc::now(),
            EventPayload::OutcomeEvidence(evidence(OutcomeKind::CiOrDeploymentVerified, 0.42)),
        )];

        let notes = verify_outcomes(&events, &mut outcomes, None);

        assert!(notes.is_empty());
        assert_eq!(outcomes[0].1.kind, OutcomeKind::CiOrDeploymentVerified);
        assert_eq!(outcomes[0].1.confidence, 0.42);
        assert_eq!(outcomes[0].1.summary, "original claim");
    }
}
