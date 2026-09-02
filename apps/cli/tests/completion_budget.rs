//! The 100 ms Tab budget, and the two ways a completer is allowed to come back empty.
//!
//! A shell completer runs inside a keypress. The rules it has to keep are cheap to state
//! and easy to break by accident: one bounded read, and never a stall. This measures the
//! first and asserts the second.

use std::path::Path;
use std::time::{Duration, Instant};

use assert_cmd::Command;
use std::fs::{self, File};
use std::io::Write;
use tempfile::{tempdir, TempDir};

/// The spec's number. Generous against a fixture index -- the point of the assertion is to
/// catch a completer that starts scanning, not to measure this machine.
const TAB_BUDGET: Duration = Duration::from_millis(100);

fn fixture() -> (TempDir, std::path::PathBuf) {
    let temp = tempdir().unwrap();
    let dir = temp.path().join(".claude").join("projects").join("proj");
    fs::create_dir_all(&dir).unwrap();

    for n in 0..12 {
        let mut f = File::create(dir.join(format!("session_{n:02}.jsonl"))).unwrap();
        writeln!(f, r#"{{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"fix the index"}}"#).unwrap();
        writeln!(f, r#"{{"type":"assistant","timestamp":"2026-08-29T10:00:02Z","model":"claude-3-5-sonnet-20241022","usage":{{"input_tokens":900,"output_tokens":120,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"content":[{{"type":"tool_use","id":"t1","name":"FileEdit","input":{{"file_path":"src/index.rs","diff":"+ fn index() {{}}"}}}}]}}"#).unwrap();
        writeln!(f, r#"{{"type":"tool_result","timestamp":"2026-08-29T10:00:04Z","tool_use_id":"t1","content":"File modified successfully","is_error":false}}"#).unwrap();
    }

    let db = temp.path().join("index.db");
    Command::cargo_bin("agentworth")
        .unwrap()
        .arg("--db-path")
        .arg(&db)
        .arg("scan")
        .arg(temp.path())
        .arg("--json")
        .assert()
        .success();

    (temp, db)
}

#[test]
fn every_completer_answers_within_the_tab_budget() {
    let (_temp, db) = fixture();

    // Warm the page cache the way a second Tab would find it; the first read of a fresh
    // SQLite file is not what the budget is about.
    let _ = agentworth_cli::completions::session_candidates_for(&db);

    let cases: [(&str, fn(&Path) -> usize); 3] = [
        ("session", |p| {
            agentworth_cli::completions::session_candidates_for(p).len()
        }),
        ("repo", |p| {
            agentworth_cli::completions::repo_candidates_for(p).len()
        }),
        ("model", |p| {
            agentworth_cli::completions::model_candidates_for(p).len()
        }),
    ];

    for (name, f) in cases {
        let start = Instant::now();
        let count = f(&db);
        let elapsed = start.elapsed();
        assert!(
            count > 0,
            "the {name} completer found nothing against a seeded index"
        );
        assert!(
            elapsed < TAB_BUDGET,
            "the {name} completer took {elapsed:?}, over the {TAB_BUDGET:?} Tab budget"
        );
    }
}

#[test]
fn a_missing_index_completes_to_nothing_rather_than_failing() {
    let temp = tempdir().unwrap();
    let missing = temp.path().join("there-is-no-index-here.db");

    let start = Instant::now();
    assert!(agentworth_cli::completions::session_candidates_for(&missing).is_empty());
    assert!(agentworth_cli::completions::repo_candidates_for(&missing).is_empty());
    assert!(agentworth_cli::completions::model_candidates_for(&missing).is_empty());
    assert!(
        start.elapsed() < TAB_BUDGET,
        "a missing index should be answered immediately, not waited on"
    );
}

#[test]
fn an_empty_index_completes_to_nothing() {
    let temp = tempdir().unwrap();
    let db = temp.path().join("empty.db");
    // `scan` with a path holding nothing creates the schema and no sessions.
    Command::cargo_bin("agentworth")
        .unwrap()
        .arg("--db-path")
        .arg(&db)
        .arg("scan")
        .arg(temp.path())
        .arg("--json")
        .assert()
        .success();

    assert!(agentworth_cli::completions::session_candidates_for(&db).is_empty());
}

/// The adapter list is static, so it answers with no index at all.
#[test]
fn the_adapter_completer_needs_no_index() {
    assert!(!agentworth_cli::completions::adapter_candidates().is_empty());
}
