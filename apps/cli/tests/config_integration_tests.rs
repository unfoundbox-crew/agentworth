//! End-to-end tests for `agentworth config` and for persisted-config defaults feeding
//! into other subcommands (`--json` default, `traces --limit` default).
//!
//! Every test isolates itself from the real `~/.agentworth/config.toml` via the
//! `AGENTWORTH_CONFIG_PATH` env var override (see `apps/cli/src/commands/config.rs`), so
//! these never touch — or race with — a developer's or CI box's actual persisted config.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;

/// Writes one minimal-but-real Claude Code session transcript: a user turn plus two
/// assistant turns that each report token usage, so the indexed session clears storage's
/// `total_events > 1 AND total_tokens > 0` bar for a non-stub session (mirrors the fixture
/// shape in `cli_integration_tests.rs::setup_sample_claude_session`).
fn write_sample_session(root: &std::path::Path, project: &str, session_name: &str) {
    let claude_dir = root.join(".claude").join("projects").join(project);
    fs::create_dir_all(&claude_dir).unwrap();

    let session_file = claude_dir.join(format!("{}.jsonl", session_name));
    let mut file = File::create(&session_file).unwrap();

    writeln!(
        file,
        r#"{{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"Fix the bug in {}"}}"#,
        session_name
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"assistant","timestamp":"2026-08-29T10:00:02Z","model":"claude-3-5-sonnet-20241022","usage":{{"input_tokens":500,"output_tokens":120,"cache_read_input_tokens":50,"cache_creation_input_tokens":10}},"content":[{{"type":"tool_use","id":"t1","name":"FileEdit","input":{{"file_path":"src/db.rs","diff":"+ fn fix() {{}}"}}}}]}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"tool_result","timestamp":"2026-08-29T10:00:04Z","tool_use_id":"t1","content":"File modified successfully","is_error":false}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"assistant","timestamp":"2026-08-29T10:00:05Z","content":[{{"type":"text","text":"Fixed it."}}],"usage":{{"input_tokens":200,"output_tokens":30,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}"#
    )
    .unwrap();
}

#[test]
fn test_config_set_get_list_round_trip_via_binary() {
    let temp = tempdir().unwrap();
    let config_path = temp.path().join("config.toml");

    let mut set_cmd = Command::cargo_bin("agentworth").unwrap();
    set_cmd
        .env("AGENTWORTH_CONFIG_PATH", &config_path)
        .args(["config", "set", "limit", "42"]);
    set_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("Saved limit = 42"));

    let mut get_cmd = Command::cargo_bin("agentworth").unwrap();
    get_cmd
        .env("AGENTWORTH_CONFIG_PATH", &config_path)
        .args(["config", "get", "limit"]);
    get_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("42"));

    let mut list_cmd = Command::cargo_bin("agentworth").unwrap();
    list_cmd
        .env("AGENTWORTH_CONFIG_PATH", &config_path)
        .args(["config", "list", "--json"]);
    list_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("\"limit\": 42"))
        .stdout(predicate::str::contains("\"period\": null"));
}

#[test]
fn test_config_get_unset_key_reports_not_set() {
    let temp = tempdir().unwrap();
    let config_path = temp.path().join("config.toml");

    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    cmd.env("AGENTWORTH_CONFIG_PATH", &config_path)
        .args(["config", "get", "period"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("(not set)"));
}

#[test]
fn test_config_set_rejects_unknown_key_and_bad_value() {
    let temp = tempdir().unwrap();
    let config_path = temp.path().join("config.toml");

    let mut bad_key_cmd = Command::cargo_bin("agentworth").unwrap();
    bad_key_cmd
        .env("AGENTWORTH_CONFIG_PATH", &config_path)
        .args(["config", "set", "bogus", "x"]);
    bad_key_cmd
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown config key"));

    let mut bad_value_cmd = Command::cargo_bin("agentworth").unwrap();
    bad_value_cmd
        .env("AGENTWORTH_CONFIG_PATH", &config_path)
        .args(["config", "set", "period", "fortnight"]);
    bad_value_cmd
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}

/// The core requirement: an explicit `--no-json` flag overrides a persisted `json = true`
/// default, and omitting the flag lets the persisted default take effect. `matrix` is the
/// vehicle because it needs no seeded database (see `run_matrix_command`, which ignores
/// `db_path` entirely) so this test only exercises the config plumbing.
#[test]
fn test_explicit_no_json_flag_overrides_persisted_json_default() {
    let temp = tempdir().unwrap();
    let config_path = temp.path().join("config.toml");

    let mut set_cmd = Command::cargo_bin("agentworth").unwrap();
    set_cmd
        .env("AGENTWORTH_CONFIG_PATH", &config_path)
        .args(["config", "set", "json", "true"]);
    set_cmd.assert().success();

    // No --json and no --no-json: the persisted default (JSON) applies.
    let mut default_cmd = Command::cargo_bin("agentworth").unwrap();
    default_cmd
        .env("AGENTWORTH_CONFIG_PATH", &config_path)
        .arg("matrix");
    default_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("\"total_adapters\""));

    // Explicit --no-json overrides the persisted default back to text output.
    let mut override_cmd = Command::cargo_bin("agentworth").unwrap();
    override_cmd
        .env("AGENTWORTH_CONFIG_PATH", &config_path)
        .args(["--no-json", "matrix"]);
    override_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("agentworth matrix"))
        .stdout(predicate::str::contains("\"total_adapters\"").not());
}

/// Same override property, this time for a valued flag (`--limit`) rather than a bare
/// switch, proven against real indexed sessions rather than a pure unit test: omitting
/// `--limit` picks up the persisted default; passing `--limit` explicitly overrides it.
#[test]
fn test_explicit_limit_flag_overrides_persisted_config_limit() {
    let temp = tempdir().unwrap();
    let config_path = temp.path().join("config.toml");
    let db_path = temp.path().join("test_agentworth.db");

    for i in 0..4 {
        write_sample_session(temp.path(), "test-project", &format!("session_{}", i));
    }

    let mut scan_cmd = Command::cargo_bin("agentworth").unwrap();
    scan_cmd
        .env("AGENTWORTH_CONFIG_PATH", &config_path)
        .arg("--db-path")
        .arg(&db_path)
        .arg("scan")
        .arg(temp.path())
        .arg("--json");
    scan_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("\"scanned_sessions\": 4"));

    let mut set_cmd = Command::cargo_bin("agentworth").unwrap();
    set_cmd
        .env("AGENTWORTH_CONFIG_PATH", &config_path)
        .args(["config", "set", "limit", "2"]);
    set_cmd.assert().success();

    // Omitting --limit: the persisted default (2) applies.
    let mut default_cmd = Command::cargo_bin("agentworth").unwrap();
    default_cmd
        .env("AGENTWORTH_CONFIG_PATH", &config_path)
        .arg("--db-path")
        .arg(&db_path)
        .args(["traces", "--json"]);
    let default_assert = default_cmd.assert().success();
    let default_count = String::from_utf8_lossy(&default_assert.get_output().stdout)
        .matches("\"session_id\"")
        .count();
    assert_eq!(
        default_count, 2,
        "expected persisted config limit=2 to apply when --limit is omitted"
    );

    // Explicit --limit 4 overrides the persisted default of 2.
    let mut explicit_cmd = Command::cargo_bin("agentworth").unwrap();
    explicit_cmd
        .env("AGENTWORTH_CONFIG_PATH", &config_path)
        .arg("--db-path")
        .arg(&db_path)
        .args(["traces", "--limit", "4", "--json"]);
    let explicit_assert = explicit_cmd.assert().success();
    let explicit_count = String::from_utf8_lossy(&explicit_assert.get_output().stdout)
        .matches("\"session_id\"")
        .count();
    assert_eq!(
        explicit_count, 4,
        "expected explicit --limit 4 to override the persisted config default of 2"
    );
}

#[test]
fn test_archie_config_keys_round_trip_via_binary() {
    let temp = tempdir().unwrap();
    let config_path = temp.path().join("config.toml");

    // Typed lowercase, stored canonical: the reply names what was saved, not what was typed.
    Command::cargo_bin("agentworth")
        .unwrap()
        .env("AGENTWORTH_CONFIG_PATH", &config_path)
        .args(["config", "set", "archie.colourway", "c4"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Saved archie.colourway = C4"));

    Command::cargo_bin("agentworth")
        .unwrap()
        .env("AGENTWORTH_CONFIG_PATH", &config_path)
        .args(["config", "set", "archie.accessory", "goggles"])
        .assert()
        .success();

    Command::cargo_bin("agentworth")
        .unwrap()
        .env("AGENTWORTH_CONFIG_PATH", &config_path)
        .args(["config", "get", "archie.accessory"])
        .assert()
        .success()
        .stdout(predicate::str::contains("goggles"));

    Command::cargo_bin("agentworth")
        .unwrap()
        .env("AGENTWORTH_CONFIG_PATH", &config_path)
        .args(["config", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"archie.accessory\": \"goggles\""))
        .stdout(predicate::str::contains("\"archie.colourway\": \"C4\""));
}

/// The committed fixture is a hand-written config.toml in the shape a real one takes.
/// It is here so a serde rename or a field reorder that silently stops parsing an
/// existing user's file fails a test instead of shipping.
#[test]
fn test_reads_a_hand_written_config_file() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/config-with-archie.toml");

    Command::cargo_bin("agentworth")
        .unwrap()
        .env("AGENTWORTH_CONFIG_PATH", &fixture)
        .args(["config", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"limit\": 25"))
        .stdout(predicate::str::contains("\"period\": \"week\""))
        .stdout(predicate::str::contains("\"archie.accessory\": \"goggles\""))
        .stdout(predicate::str::contains("\"archie.colourway\": \"C2\""));
}

#[test]
fn test_archie_config_rejects_a_kit_that_does_not_exist() {
    let temp = tempdir().unwrap();
    let config_path = temp.path().join("config.toml");

    Command::cargo_bin("agentworth")
        .unwrap()
        .env("AGENTWORTH_CONFIG_PATH", &config_path)
        .args(["config", "set", "archie.accessory", "monocle"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}
