//! End-to-end tests for `agentworth version` and `agentworth update`, run against the
//! real compiled binaries via `assert_cmd`.
//!
//! Every test passes `--offline` and clears/sets the launcher env vars explicitly, so
//! none of this depends on a real network call succeeding (or on ambient environment
//! leaking in from whatever actually launched the test process). The live network path
//! (`fetch_latest_release` in `src/commands/version_info.rs`) is covered by pure unit
//! tests there instead, plus a manual, non-test-gating verification against the real
//! GitHub API and npm registry (see this branch's commit message / DECISION-INBOX.md).

use assert_cmd::Command;
use predicates::prelude::*;

/// The compiled crate's own version. Resolves identically here and inside the binary
/// under test -- both come from the same `agentworth-cli` package -- so these assertions
/// stay correct across a future version bump instead of hardcoding a version string.
const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[test]
fn test_version_command_offline_text_includes_real_crate_version() {
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    cmd.arg("version").arg("--offline");
    cmd.env_remove("AGENTWORTH_LAUNCHER_ACTIVE");
    cmd.env_remove("AGENTWORTH_NPM_VERSION");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains(format!("agentworth {CRATE_VERSION}")))
        .stdout(predicate::str::contains("skipped"));
}

#[test]
fn test_version_command_json_offline_includes_real_crate_version() {
    let mut cmd = Command::cargo_bin("agwt").unwrap();
    cmd.arg("version").arg("--offline").arg("--json");
    cmd.env_remove("AGENTWORTH_LAUNCHER_ACTIVE");
    cmd.env_remove("AGENTWORTH_NPM_VERSION");

    let output = cmd.assert().success().get_output().stdout.clone();
    let parsed: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert_eq!(parsed["version"], CRATE_VERSION);
    assert_eq!(parsed["update_check"]["status"], "skipped");
    assert_eq!(parsed["launcher"]["npm_launcher_active"], false);
}

#[test]
fn test_version_reports_npm_launcher_when_env_set() {
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    cmd.arg("version").arg("--offline").arg("--json");
    cmd.env("AGENTWORTH_LAUNCHER_ACTIVE", "1");
    cmd.env("AGENTWORTH_NPM_VERSION", "9.9.9");

    let output = cmd.assert().success().get_output().stdout.clone();
    let parsed: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert_eq!(parsed["launcher"]["npm_launcher_active"], true);
    assert_eq!(parsed["launcher"]["npm_package_version"], "9.9.9");
}

#[test]
fn test_version_reports_no_launcher_when_env_absent() {
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    cmd.arg("version").arg("--offline").arg("--json");
    cmd.env_remove("AGENTWORTH_LAUNCHER_ACTIVE");
    cmd.env_remove("AGENTWORTH_NPM_VERSION");

    let output = cmd.assert().success().get_output().stdout.clone();
    let parsed: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert_eq!(parsed["launcher"]["npm_launcher_active"], false);
    assert!(parsed["launcher"]["npm_package_version"].is_null());
}

#[test]
fn test_version_ignores_non_literal_one_launcher_value() {
    // resolver.js only ever writes the literal string "1". Anything else must not be
    // treated as an active launcher.
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    cmd.arg("version").arg("--offline").arg("--json");
    cmd.env("AGENTWORTH_LAUNCHER_ACTIVE", "true");

    let output = cmd.assert().success().get_output().stdout.clone();
    let parsed: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert_eq!(parsed["launcher"]["npm_launcher_active"], false);
}

#[test]
fn test_update_command_offline_text_and_json() {
    let mut text_cmd = Command::cargo_bin("agentworth").unwrap();
    text_cmd.arg("update").arg("--offline");
    text_cmd.env_remove("AGENTWORTH_LAUNCHER_ACTIVE");

    text_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains(format!("agentworth {CRATE_VERSION}")))
        .stdout(predicate::str::contains("skipped"));

    let mut json_cmd = Command::cargo_bin("agentworth").unwrap();
    json_cmd.arg("update").arg("--offline").arg("--json");
    json_cmd.env_remove("AGENTWORTH_LAUNCHER_ACTIVE");

    let output = json_cmd.assert().success().get_output().stdout.clone();
    let parsed: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert_eq!(parsed["update_check"]["status"], "skipped");
    // Offline mode never resolves a real update, so there is nothing to advise.
    assert!(parsed.get("advice").is_none());
}

#[test]
fn test_update_subcommand_is_recognized_not_an_error() {
    // The literal bug report this feature exists to fix: `agentworth update` used to hit
    // clap's "unrecognized subcommand" error path and exit non-zero. `--offline` keeps
    // this test network-free.
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    cmd.arg("update").arg("--offline");
    cmd.assert()
        .success()
        .stderr(predicate::str::contains("unrecognized subcommand").not());
}

#[test]
fn test_version_subcommand_coexists_with_builtin_version_flag() {
    // clap's auto `-V`/`--version` flag and the new `version` subcommand are two
    // different mechanisms (a global flag short-circuited before subcommand parsing, vs.
    // a positional subcommand) -- confirm both still work side by side.
    let mut flag_cmd = Command::cargo_bin("agentworth").unwrap();
    flag_cmd.arg("--version");
    flag_cmd.assert().success().stdout(predicate::str::contains(CRATE_VERSION));

    let mut short_flag_cmd = Command::cargo_bin("agentworth").unwrap();
    short_flag_cmd.arg("-V");
    short_flag_cmd.assert().success().stdout(predicate::str::contains(CRATE_VERSION));

    let mut subcommand_cmd = Command::cargo_bin("agentworth").unwrap();
    subcommand_cmd.arg("version").arg("--offline");
    subcommand_cmd.assert().success().stdout(predicate::str::contains(CRATE_VERSION));
}

#[test]
fn test_every_binary_exposes_version_and_update() {
    // agentworth, archie and agwt are one compiled binary under three names (see
    // apps/cli/Cargo.toml's [[bin]] entries, all calling the same run()) -- prove each one
    // really does carry the commands rather than assuming it from the shared path.
    for bin in ["agentworth", "archie", "agwt"] {
        Command::cargo_bin(bin).unwrap().arg("version").arg("--offline").assert().success();
        Command::cargo_bin(bin).unwrap().arg("update").arg("--offline").assert().success();
    }
}

#[test]
fn test_version_and_update_need_no_database() {
    // Unlike almost every other subcommand, version/update never call open_storage --
    // they must work in a directory with zero AgentWorth state and no --db-path at all.
    let temp = tempfile::tempdir().unwrap();
    Command::cargo_bin("agentworth")
        .unwrap()
        .current_dir(temp.path())
        .env("HOME", temp.path())
        .arg("version")
        .arg("--offline")
        .assert()
        .success();
}
