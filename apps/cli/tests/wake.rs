//! `session_wake` / `archie session wake`, end to end through the real binary.
//!
//! Fixture per AGENTS.md's rule: a real-shaped Claude Code transcript
//! (`tests/fixtures/wake/`) is copied into a fresh `.claude/projects/` tree, scanned through
//! the real `scan` command, then read back through `session wake`. The fixture also has a
//! subagent transcript that starts later than the parent -- the point of the test is that the
//! parent, not the subagent, is what wake reports.

use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::{tempdir, TempDir};

const PROJECT_DIR: &str = "-Users-x-code-unfoundbox-agentworth";
const PARENT_ID: &str = "7f3c9a2e-1b4d-4c8e-9a0f-2d6e8b1c3a5f";

/// Copies a directory tree, `subagents/` included.
fn copy_dir_all(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let dest_path = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir_all(&entry.path(), &dest_path);
        } else {
            fs::copy(entry.path(), &dest_path).unwrap();
        }
    }
}

/// Copies the real-shaped fixture into `<tempdir>/.claude/projects/<project>/`, scans it, and
/// returns the index tempdir plus the db path.
fn seeded_index() -> (TempDir, std::path::PathBuf) {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = manifest_dir
        .join("tests/fixtures/wake")
        .join(PROJECT_DIR);

    let index_dir = tempdir().unwrap();
    let dest = index_dir
        .path()
        .join(".claude")
        .join("projects")
        .join(PROJECT_DIR);
    copy_dir_all(&src, &dest);

    let db = index_dir.path().join("index.db");
    Command::cargo_bin("agentworth")
        .unwrap()
        .arg("--db-path")
        .arg(&db)
        .arg("scan")
        .arg(index_dir.path())
        .arg("--json")
        .assert()
        .success();

    (index_dir, db)
}

/// A second, unrelated checkout: one commit, one dirty file. `git` is skipped (with a message,
/// not a failure) if it is not on PATH -- the wake checkout block is best-effort by design.
fn git_checkout() -> Option<TempDir> {
    if std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping checkout assertions: git not found on PATH");
        return None;
    }

    let dir = tempdir().unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .env("GIT_AUTHOR_NAME", "wake-test")
            .env("GIT_AUTHOR_EMAIL", "wake-test@example.com")
            .env("GIT_COMMITTER_NAME", "wake-test")
            .env("GIT_COMMITTER_EMAIL", "wake-test@example.com")
            .status()
            .expect("run git")
    };

    assert!(run(&["init", "-q"]).success());
    fs::write(dir.path().join("README.md"), "wake fixture checkout\n").unwrap();
    assert!(run(&["add", "README.md"]).success());
    assert!(run(&["commit", "-q", "-m", "init"]).success());
    fs::write(dir.path().join("dirty.txt"), "uncommitted\n").unwrap();

    Some(dir)
}

fn run_json(db: &Path, workspace: &Path, extra: &[&str]) -> Value {
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    cmd.arg("--db-path")
        .arg(db)
        .arg("session")
        .arg("wake")
        .arg("--workspace")
        .arg(workspace)
        .arg("--repo")
        .arg("unfoundbox/agentworth")
        .arg("--json");
    for a in extra {
        cmd.arg(a);
    }
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "`session wake --json` failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("session wake --json output")
}

fn run_markdown(db: &Path, workspace: &Path) -> String {
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    cmd.arg("--db-path")
        .arg(db)
        .arg("session")
        .arg("wake")
        .arg("--workspace")
        .arg(workspace)
        .arg("--repo")
        .arg("unfoundbox/agentworth");
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "`session wake` failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn session_wake_reports_the_parent_not_the_subagent() {
    let (_index_dir, db) = seeded_index();
    let git_dir = git_checkout();
    let fallback_dir;
    let workspace: &Path = match &git_dir {
        Some(d) => d.path(),
        None => {
            fallback_dir = tempdir().unwrap();
            fallback_dir.path()
        }
    };

    let report = run_json(&db, workspace, &[]);
    let session = &report["session"];

    assert_eq!(session["session_id"], PARENT_ID, "the parent, not the subagent");
    let task = session["task"].as_str().expect("task");
    assert!(
        task.starts_with("Add a session_wake MCP tool"),
        "task: {task}"
    );

    let last_asked = session["last_asked"].as_str().expect("last_asked");
    assert_eq!(last_asked, "Now rebuild the docs site and push, then open the PR.");

    let ran_in = &session["ran_in"];
    assert!(
        ran_in["cwd"]
            .as_str()
            .expect("ran_in.cwd")
            .ends_with("worktrees/session-wake"),
        "ran_in.cwd: {ran_in}"
    );
    assert_eq!(ran_in["git_branch"], "claude/session-wake");

    // The commit at 09:01 is newer than the test run at 05:38 but it is not proof; the proof
    // line is test- and build-shaped only, so the newest pass is the storage test run.
    let proof = &session["proof"];
    let last_passed = proof["last_passed"]["command"].as_str().expect("last_passed.command");
    assert_eq!(last_passed, "cargo test -p agentworth-storage");
    let last_failed = proof["last_failed"]["command"].as_str().expect("last_failed.command");
    assert!(last_failed.contains("npm run build"), "last_failed: {last_failed}");
    assert_eq!(proof["failed_was_rerun"], false);

    assert_eq!(session["compactions"], 1);

    let next = &report["next"];
    let blocker = next["blocker"]["command"].as_str().expect("next.blocker.command");
    assert!(blocker.contains("npm run build"), "blocker: {blocker}");

    if git_dir.is_some() {
        assert!(
            report["checkout"]["branch"].is_string(),
            "the checkout block names the temp repo's own branch: {report}"
        );
    }
}

#[test]
fn session_wake_markdown_fits_the_budget() {
    let (_index_dir, db) = seeded_index();
    let workspace = tempdir().unwrap();

    let markdown = run_markdown(&db, workspace.path());
    let line_count = markdown.lines().count();
    assert!(line_count <= 30, "wake markdown is at most 30 lines, got {line_count}:\n{markdown}");
    assert!(markdown.contains("## Next"), "{markdown}");
}
