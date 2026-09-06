//! Reafference: what the checkout actually looks like when the agent stops.
//!
//! At `Stop` the loop asks git what changed and compares it with the union of this session's
//! predicted writes. A changed path this session predicted is `own`. A changed path nobody here
//! predicted is exafference -- the world moved, and `crate::support::drift` is what names who
//! moved it.
//!
//! The probe is read-only and bounded, for the same two reasons `apps/cli/src/wake/git.rs` gives:
//! `GIT_OPTIONAL_LOCKS=0` so `status` cannot write the index it would normally refresh, and one
//! deadline across both calls so a repository on a stalled mount never holds up a `Stop`.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// One line of `git status --porcelain=v1`: the two-letter XY code and the path it applies to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedChange {
    pub path: PathBuf,
    pub status: String,
}

/// What the checkout looked like at this moment.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Observation {
    pub head: Option<String>,
    pub changes: Vec<ObservedChange>,
}

/// Changed paths split by whether this session said it was going to write them.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Classified {
    pub own: Vec<PathBuf>,
    pub world: Vec<PathBuf>,
}

/// Reads the working tree at `cwd`. Errors when git is missing, times out, or `cwd` is not a
/// checkout -- three facts a caller may want to report differently from an empty observation.
pub fn observe_checkout(cwd: &Path) -> Result<Observation> {
    observe_checkout_with_git(cwd, "git")
}

/// The same probe against a named `git` binary, so a test can point at one that does not exist.
pub fn observe_checkout_with_git(cwd: &Path, git: &str) -> Result<Observation> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let status = run_git(
        git,
        cwd,
        &["status", "--porcelain=v1", "--untracked-files=normal"],
        deadline,
    )?;
    let head = run_git(git, cwd, &["rev-parse", "--short", "HEAD"], deadline)
        .ok()
        .map(|out| out.trim().to_string())
        .filter(|head| !head.is_empty());
    Ok(Observation {
        head,
        changes: parse_porcelain(&status),
    })
}

/// Splits the observation's paths into what this session predicted and what it did not. A
/// prediction matches a path when it is that path or a directory containing it.
pub fn classify(observation: &Observation, cwd: &Path, predicted: &[PathBuf]) -> Classified {
    let predicted: Vec<PathBuf> = predicted.iter().map(|path| absolutise(cwd, path)).collect();
    let mut classified = Classified::default();
    for change in &observation.changes {
        let path = absolutise(cwd, &change.path);
        if predicted
            .iter()
            .any(|candidate| path == *candidate || path.starts_with(candidate))
        {
            classified.own.push(path);
        } else {
            classified.world.push(path);
        }
    }
    classified
}

/// `XY <path>`, with a rename printed as `XY <old> -> <new>`. The new path is the one that
/// exists, so it is the one recorded.
fn parse_porcelain(output: &str) -> Vec<ObservedChange> {
    output
        .lines()
        .filter(|line| line.len() > 3)
        .filter_map(|line| {
            let status = line.get(0..2)?.to_string();
            let rest = line.get(3..)?.trim();
            let path = rest.rsplit(" -> ").next().unwrap_or(rest);
            let path = path.trim_matches('"');
            if path.is_empty() {
                return None;
            }
            Some(ObservedChange {
                path: PathBuf::from(path),
                status,
            })
        })
        .collect()
}

/// Makes a path absolute against `cwd` and removes `.` and `..` textually. Textually because a
/// deleted file cannot be canonicalised, and a deleted file is exactly the case that matters.
fn absolutise(cwd: &Path, path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let mut out = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// One bounded, read-only `git` call. Same shape as `apps/cli/src/wake/git.rs`: stdout is drained
/// on its own thread, because a child blocked writing into a full pipe never exits and every
/// large repository would otherwise time out.
fn run_git(git: &str, cwd: &Path, args: &[&str], deadline: Instant) -> Result<String> {
    if Instant::now() >= deadline {
        return Err(anyhow!("git {args:?} did not start before the deadline"));
    }
    let mut child = Command::new(git)
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| anyhow!("git could not be started: {err}"))?;

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
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };

    match status {
        Some(status) if status.success() => rx
            .recv_timeout(Duration::from_secs(1))
            .map_err(|_| anyhow!("git {args:?} produced no output")),
        Some(_) => Err(anyhow!(
            "git {args:?} failed; {} is not a readable checkout",
            cwd.display()
        )),
        None if timed_out => Err(anyhow!("git {args:?} passed the deadline")),
        None => Err(anyhow!("git {args:?} could not be waited on")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_dirty_checkout_reports_its_head_and_its_changes() {
        if !git_present() {
            eprintln!("skipped: no git on this host");
            return;
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        git(root, &["init", "--initial-branch=main"]);
        std::fs::write(root.join("a.txt"), "one\n").expect("write");
        git(root, &["add", "a.txt"]);
        git(root, &["commit", "-m", "first"]);
        std::fs::write(root.join("a.txt"), "two\n").expect("write");
        std::fs::write(root.join("b.txt"), "new\n").expect("write");

        let observation = observe_checkout(root).expect("observes");
        assert!(observation.head.is_some_and(|head| !head.is_empty()));
        let mut paths: Vec<&Path> = observation
            .changes
            .iter()
            .map(|c| c.path.as_path())
            .collect();
        paths.sort_unstable();
        assert_eq!(paths, [Path::new("a.txt"), Path::new("b.txt")]);
        let untracked = observation
            .changes
            .iter()
            .find(|c| c.path == Path::new("b.txt"));
        assert_eq!(untracked.expect("b.txt").status, "??");
    }

    #[test]
    fn a_predicted_path_is_own_and_everything_else_is_the_world() {
        if !git_present() {
            eprintln!("skipped: no git on this host");
            return;
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        git(root, &["init", "--initial-branch=main"]);
        std::fs::create_dir(root.join("src")).expect("mkdir");
        std::fs::write(root.join("src/lib.rs"), "//\n").expect("write");
        std::fs::write(root.join("other.txt"), "x\n").expect("write");
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "first"]);
        // Untracked directories collapse to one entry under `--untracked-files=normal`, so both
        // files are committed first and then edited: the change set has to be two real paths.
        std::fs::write(root.join("src/lib.rs"), "// two\n").expect("write");
        std::fs::write(root.join("other.txt"), "y\n").expect("write");

        let observation = observe_checkout(root).expect("observes");
        let classified = classify(&observation, root, &[PathBuf::from("src")]);
        assert_eq!(classified.own, [root.join("src/lib.rs")]);
        assert_eq!(classified.world, [root.join("other.txt")]);

        let exact = classify(&observation, root, &[root.join("other.txt")]);
        assert_eq!(exact.own, [root.join("other.txt")]);
        assert_eq!(exact.world, [root.join("src/lib.rs")]);
    }

    #[test]
    fn a_relative_prediction_and_an_absolute_change_are_the_same_path() {
        let observation = Observation {
            head: None,
            changes: vec![ObservedChange {
                path: PathBuf::from("./src/../src/lib.rs"),
                status: " M".to_string(),
            }],
        };
        let classified = classify(
            &observation,
            Path::new("/repo"),
            &[PathBuf::from("/repo/src/lib.rs")],
        );
        assert_eq!(classified.own, [PathBuf::from("/repo/src/lib.rs")]);
        assert!(classified.world.is_empty());
    }

    #[test]
    fn a_rename_records_the_path_that_now_exists() {
        let changes = parse_porcelain("R  old/name.rs -> new/name.rs\n M kept.rs\n");
        assert_eq!(changes[0].path, PathBuf::from("new/name.rs"));
        assert_eq!(changes[0].status, "R ");
        assert_eq!(changes[1].path, PathBuf::from("kept.rs"));
        assert_eq!(changes[1].status, " M");
    }

    #[test]
    fn no_git_binary_is_an_error_not_an_empty_observation() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(observe_checkout_with_git(dir.path(), "git-that-does-not-exist-9f3e1a2").is_err());
    }

    #[test]
    fn a_plain_directory_is_not_a_checkout() {
        if !git_present() {
            eprintln!("skipped: no git on this host");
            return;
        }
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(observe_checkout(dir.path()).is_err());
    }
}
