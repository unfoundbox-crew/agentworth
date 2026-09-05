//! What `git` says about the directory the tool is standing in, read-only.
//!
//! `docs/specs/wake.md` explains why this exists at all: a handoff describes a past session and
//! must not carry the current branch, but wake is about now, and a waking agent's first three
//! commands are `pwd`, `git branch` and `git status`. Running them here costs one process each
//! and saves three round trips.
//!
//! Three rules hold every probe honest:
//!
//! - **Read-only.** Every command below only reads. `GIT_OPTIONAL_LOCKS=0` is set on the child
//!   so even `status` cannot write the index it would normally refresh.
//! - **Bounded.** Each call gets two seconds and is killed after that. A repository on a stalled
//!   network mount must not hang the tool that was supposed to save time.
//! - **A field that could not be read stays `None`.** Nothing here guesses a branch or a HEAD.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long the whole probe may take. One deadline for all eight calls rather than two seconds
/// each: the point is that a waking agent is never left waiting, and eight two-second calls is
/// sixteen seconds of waiting.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// What the checkout looks like right now. Every field is one `git` read; every one of them can
/// be absent on its own without invalidating the rest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checkout {
    pub root: String,
    /// True when `--git-dir` and `--git-common-dir` differ, which is what a linked worktree is.
    pub is_worktree: bool,
    /// `None` on a detached HEAD, which is a fact about the checkout rather than a failure.
    pub branch: Option<String>,
    pub head_short: Option<String>,
    pub head_subject: Option<String>,
    pub dirty_files: Option<usize>,
    pub ahead: Option<usize>,
    pub behind: Option<usize>,
    pub upstream: Option<String>,
}

/// The four answers a probe can give. "No git on this machine", "not a repository" and "git did
/// not answer" are three different facts, and the document says which one it got rather than
/// rounding all three down to the same missing line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "checkout")]
pub enum CheckoutProbe {
    Found(Checkout),
    NotACheckout,
    /// `git` was there and did not answer inside the deadline -- a repository on a stalled
    /// network mount, or one large enough that the probe is not worth the wait.
    Unreadable,
    GitUnavailable,
}

/// Reads the checkout at `path` with the `git` on `PATH`.
pub fn probe_checkout(path: &Path) -> CheckoutProbe {
    probe_checkout_with_git(path, "git")
}

/// Same probe against a named `git` binary, so a test can point at one that does not exist.
pub fn probe_checkout_with_git(path: &Path, git: &str) -> CheckoutProbe {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let run = |args: &[&str]| run_git(git, path, args, deadline);

    let root = match run(&["rev-parse", "--show-toplevel"]) {
        Run::Unavailable => return CheckoutProbe::GitUnavailable,
        Run::TimedOut => return CheckoutProbe::Unreadable,
        // An empty root is no root. Everything after this point degrades to `None` instead,
        // because a checkout with an unreadable branch is still a checkout.
        other => match other.value() {
            Some(root) => root,
            None => return CheckoutProbe::NotACheckout,
        },
    };

    let git_dir = run(&["rev-parse", "--git-dir"]).value();
    let common_dir = run(&["rev-parse", "--git-common-dir"]).value();
    let is_worktree = match (&git_dir, &common_dir) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    };

    let upstream = run(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]).value();
    let (ahead, behind) = match upstream {
        Some(_) => divergence(run(&["rev-list", "--left-right", "--count", "@{u}...HEAD"]).value()),
        None => (None, None),
    };

    CheckoutProbe::Found(Checkout {
        root,
        is_worktree,
        branch: run(&["symbolic-ref", "--short", "-q", "HEAD"]).value(),
        head_short: run(&["rev-parse", "--short", "HEAD"]).value(),
        head_subject: run(&["log", "-1", "--format=%s"]).value(),
        dirty_files: run(&["status", "--porcelain=v1", "--untracked-files=normal"])
            .value_allowing_empty()
            .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count()),
        ahead,
        behind,
        upstream,
    })
}

/// `rev-list --left-right --count @{u}...HEAD` prints behind and ahead, in that order.
fn divergence(counts: Option<String>) -> (Option<usize>, Option<usize>) {
    let Some(line) = counts else {
        return (None, None);
    };
    let mut parts = line.split_whitespace();
    let behind = parts.next().and_then(|n| n.parse().ok());
    let ahead = parts.next().and_then(|n| n.parse().ok());
    (ahead, behind)
}

enum Run {
    Ok(String),
    /// `git` ran and said no: not a repository, no upstream, a detached HEAD.
    Failed,
    /// The deadline passed with the call still running, or before it started.
    TimedOut,
    /// `git` could not be started at all.
    Unavailable,
}

impl Run {
    /// The trimmed stdout of a successful call. An empty answer counts as nothing known, which
    /// is right for a branch name or a HEAD but not for `status` -- see below.
    fn value(self) -> Option<String> {
        match self {
            Run::Ok(s) if !s.is_empty() => Some(s),
            _ => None,
        }
    }

    /// The trimmed stdout of a successful call, where empty is a real answer: a clean working
    /// tree is zero dirty files, not an unknown count.
    fn value_allowing_empty(self) -> Option<String> {
        match self {
            Run::Ok(s) => Some(s),
            _ => None,
        }
    }
}

/// Runs one `git` command against the probe's shared deadline, killing it if it overruns.
///
/// stdout is drained on its own thread rather than after the wait: a `status` on a large
/// repository fills the pipe, and a child blocked writing into a full pipe never exits, so
/// polling `try_wait` alone would time out every large repository.
fn run_git(git: &str, path: &Path, args: &[&str], deadline: Instant) -> Run {
    if Instant::now() >= deadline {
        return Run::TimedOut;
    }

    let mut child = match Command::new(git)
        .arg("-C")
        .arg(path)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return Run::Unavailable,
    };

    let (tx, rx) = mpsc::channel();
    if let Some(mut out) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut buf = String::new();
            use std::io::Read;
            let _ = out.read_to_string(&mut buf);
            let _ = tx.send(buf);
        });
    }

    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                timed_out = true;
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            // `try_wait` itself failed, so this process is not going to learn how the child
            // ended. Kill and reap it rather than leaving it behind.
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };

    match status {
        Some(status) if status.success() => {
            // The child has already exited, so the reader thread is at most one buffer behind.
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(out) => Run::Ok(out.trim().to_string()),
                Err(_) => Run::Failed,
            }
        }
        Some(_) => Run::Failed,
        None if timed_out => Run::TimedOut,
        None => Run::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The test host may have no `git`. That is a skip, not a failure -- the probe's own
    /// `GitUnavailable` path is tested separately and does not need a real binary.
    fn git_present() -> bool {
        Command::new("git")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn reads_branch_head_and_dirt_from_a_real_repository() {
        if !git_present() {
            eprintln!("skipped: no git on this host");
            return;
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        git(root, &["init", "--initial-branch=main"]);
        std::fs::write(root.join("a.txt"), "one\n").expect("write");
        git(root, &["add", "a.txt"]);
        git(root, &["commit", "-m", "first commit"]);
        std::fs::write(root.join("a.txt"), "two\n").expect("write");

        let CheckoutProbe::Found(checkout) = probe_checkout(root) else {
            panic!("a real repository must probe as Found");
        };
        assert_eq!(checkout.branch.as_deref(), Some("main"));
        assert_eq!(checkout.head_subject.as_deref(), Some("first commit"));
        assert!(checkout.head_short.is_some_and(|h| !h.is_empty()));
        assert_eq!(checkout.dirty_files, Some(1));
        assert!(!checkout.is_worktree);
        // No remote, so no upstream and nothing to be ahead of. Absent, never zero.
        assert_eq!(checkout.upstream, None);
        assert_eq!(checkout.ahead, None);
        assert_eq!(checkout.behind, None);
    }

    #[test]
    fn a_clean_tree_is_zero_dirty_files_not_an_unknown_count() {
        if !git_present() {
            eprintln!("skipped: no git on this host");
            return;
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        git(root, &["init", "--initial-branch=main"]);
        std::fs::write(root.join("a.txt"), "one\n").expect("write");
        git(root, &["add", "a.txt"]);
        git(root, &["commit", "-m", "first commit"]);

        let CheckoutProbe::Found(checkout) = probe_checkout(root) else {
            panic!("a real repository must probe as Found");
        };
        assert_eq!(checkout.dirty_files, Some(0));
    }

    #[test]
    fn a_plain_directory_is_not_a_checkout() {
        if !git_present() {
            eprintln!("skipped: no git on this host");
            return;
        }
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(probe_checkout(dir.path()), CheckoutProbe::NotACheckout);
    }

    /// The deadline covers the whole probe, so a call that starts after it has passed never
    /// spawns anything -- and says it timed out rather than that the answer was no.
    #[test]
    fn a_call_that_starts_past_the_deadline_times_out_without_spawning() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(matches!(
            run_git("git", dir.path(), &["--version"], Instant::now()),
            Run::TimedOut
        ));
    }

    #[test]
    fn no_git_binary_is_reported_as_unavailable_not_as_a_missing_repository() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            probe_checkout_with_git(dir.path(), "git-that-does-not-exist-9f3e1a2"),
            CheckoutProbe::GitUnavailable
        );
    }
}
