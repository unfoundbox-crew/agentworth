//! CI coverage for `agentworth doctor --self-test` (apps/cli/src/commands/self_test.rs).
//!
//! This runs the real self-test routine -- the same one Saurabh runs by hand after every
//! release -- against a small synthetic multilingual index built here, so a regression in
//! its own JSON contract, session resolution, or MCP round trip fails CI on any Rust
//! change, not only when someone happens to run `doctor --self-test` on a real machine.
//!
//! No canary corpus from a UTF-8 fixture PR exists on `main` yet (searched: no such PR
//! has merged), so this builds its own small multilingual fixture instead, per the
//! fallback in AGENTS.md's fixture rule.

use assert_cmd::Command;
use std::fs::{self, File};
use std::io::Write;
use tempfile::{tempdir, TempDir};

/// One short session mixing English, Japanese, and Russian content -- enough to exercise
/// the self-test's JSON round trip on non-ASCII text without depending on another PR's
/// fixture. No compaction round: `forgotten` is expected to report `skip` here, and that
/// is itself the behaviour under test.
fn fixture() -> (TempDir, std::path::PathBuf) {
    let temp = tempdir().unwrap();
    let dir = temp.path().join(".claude").join("projects").join("proj");
    fs::create_dir_all(&dir).unwrap();

    let mut f = File::create(dir.join("session_one.jsonl")).unwrap();
    writeln!(
        f,
        r#"{{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"直してください: インデックスのバグ"}}"#
    )
    .unwrap();
    writeln!(
        f,
        r#"{{"type":"assistant","timestamp":"2026-08-29T10:00:02Z","model":"claude-3-5-sonnet-20241022","usage":{{"input_tokens":500,"output_tokens":120,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"content":[{{"type":"tool_use","id":"t1","name":"FileEdit","input":{{"file_path":"src/index.rs","diff":"+ fn index() {{}}"}}}}]}}"#
    )
    .unwrap();
    writeln!(
        f,
        r#"{{"type":"tool_result","timestamp":"2026-08-29T10:00:04Z","tool_use_id":"t1","content":"Файл изменён успешно","is_error":false}}"#
    )
    .unwrap();

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
fn self_test_runs_clean_against_a_small_fixture_index() {
    let (_temp, db) = fixture();

    let assert = Command::cargo_bin("agentworth")
        .unwrap()
        .arg("--db-path")
        .arg(&db)
        .arg("doctor")
        .arg("--self-test")
        .arg("--json")
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("--json report did not parse: {e}\n{stdout}"));

    assert_eq!(report["ok"], serde_json::json!(true), "self-test report: {report:#}");

    let steps = report["steps"].as_array().expect("steps array");
    let names: Vec<&str> = steps.iter().map(|s| s["name"].as_str().unwrap()).collect();
    for expected in [
        "scan",
        "stats",
        "usage --period week",
        "traces --limit 5",
        "inspect <newest non-stub session>",
        "handoff --last",
        "forgotten <newest compacted session>",
        "asks --current",
        "mcp round trip",
    ] {
        assert!(names.contains(&expected), "missing step '{expected}': {names:?}");
    }

    for step in steps {
        let status = step["status"].as_str().unwrap();
        assert_ne!(status, "fail", "step {} failed: {step:#}", step["name"]);
    }

    // This fixture never compacts, so `forgotten` must take the graceful skip path, not
    // silently disappear or come back as a hard failure.
    let forgotten = steps
        .iter()
        .find(|s| s["name"] == "forgotten <newest compacted session>")
        .unwrap();
    assert_eq!(forgotten["status"], "skip", "forgotten step: {forgotten:#}");

    // Never printed transcript content -- only ids and counts. If either non-ASCII
    // fixture string leaked into the report, that would be exactly the bug this checks
    // for.
    assert!(!stdout.contains("インデックス"), "self-test report leaked transcript content:\n{stdout}");
    assert!(!stdout.contains("изменён"), "self-test report leaked transcript content:\n{stdout}");
}

#[test]
fn self_test_human_output_stays_on_the_grid() {
    let (_temp, db) = fixture();

    let assert = Command::cargo_bin("agentworth")
        .unwrap()
        .arg("--db-path")
        .arg(&db)
        .arg("doctor")
        .arg("--self-test")
        .env("COLUMNS", "80")
        .env("NO_COLOR", "1")
        .assert()
        .success();

    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(!out.trim().is_empty(), "doctor --self-test rendered nothing");
    for line in out.lines() {
        let w = console::measure_text_width(line);
        assert!(w <= 78, "a line is {w} wide:\n{line}");
    }
}
