//! The loop, end to end through the real binary: hook in, spool, scan, and the three verbs
//! that read what came back (docs/specs/loop.md).
//!
//! No server runs here on purpose. The spool is the path that has to work with nothing
//! listening, offline, and it is the path `archie scan` ingests -- so a green run of this file
//! is a working loop on a machine where `archie serve` was never started.

use assert_cmd::Command;
use serde_json::Value;
use std::path::Path;
use tempfile::{tempdir, TempDir};

const SESSION_A: &str = "8f2a1c40-3d5e-4b7a-9c1e-2f6d8a0b3e51";
const SESSION_B: &str = "1c7e4b92-6a0d-4f38-b5c1-9e2a7d40f6b3";

/// One `archie` invocation with `HOME` pointed at the sandbox, so the spool, the socket path
/// and the config directory are all inside the tempdir and nothing touches the real machine.
fn archie(home: &Path, db: &Path) -> Command {
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    cmd.env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("HERDR_PANE_ID")
        .arg("--db-path")
        .arg(db);
    cmd
}

fn hook(home: &Path, db: &Path, payload: Value) {
    archie(home, db)
        .arg("hook")
        .write_stdin(payload.to_string())
        .assert()
        .success()
        .stdout("");
}

fn scan(home: &Path, db: &Path, root: &Path) {
    archie(home, db)
        .arg("scan")
        .arg(root)
        .arg("--json")
        .assert()
        .success();
}

fn json_out(cmd: &mut Command) -> Value {
    let output = cmd.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&output).expect("the command printed JSON")
}

fn spool_dir(home: &Path) -> std::path::PathBuf {
    home.join(".agentworth").join("spool")
}

fn sandbox() -> (TempDir, std::path::PathBuf) {
    let home = tempdir().unwrap();
    let db = home.path().join("index.db");
    (home, db)
}

#[test]
fn a_hook_with_nowhere_to_deliver_spools_and_says_nothing() {
    let (home, db) = sandbox();
    hook(
        home.path(),
        &db,
        serde_json::json!({
            "session_id": SESSION_A,
            "hook_event_name": "SessionStart",
            "cwd": home.path(),
        }),
    );

    let file = spool_dir(home.path()).join(format!("{SESSION_A}.jsonl"));
    let spooled = std::fs::read_to_string(&file).expect("the spool file exists");
    assert_eq!(
        spooled.lines().count(),
        1,
        "one event, one line: {spooled:?}"
    );
    let event: Value = serde_json::from_str(spooled.lines().next().unwrap()).unwrap();
    assert_eq!(event["session_id"], SESSION_A);
    assert_eq!(event["hook_event_name"], "SessionStart");
}

#[test]
fn the_spool_a_scan_ingests_answers_status_anchors_and_drift() {
    let (home, db) = sandbox();
    let work = home.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let written = work.join("written.rs");
    let read = work.join("read.md");
    std::fs::write(&written, "fn main() {}\n").unwrap();
    std::fs::write(&read, "# ground truth\n").unwrap();

    // Session A: writes one file, reads another, then stops.
    hook(
        home.path(),
        &db,
        serde_json::json!({
            "session_id": SESSION_A, "hook_event_name": "SessionStart", "cwd": work,
        }),
    );
    hook(
        home.path(),
        &db,
        serde_json::json!({
            "session_id": SESSION_A,
            "hook_event_name": "PreToolUse",
            "cwd": work,
            "tool_name": "Edit",
            "tool_use_id": "toolu_write_1",
            "tool_input": {"file_path": written, "old_string": "a", "new_string": "b"},
        }),
    );
    hook(
        home.path(),
        &db,
        serde_json::json!({
            "session_id": SESSION_A,
            "hook_event_name": "PostToolUse",
            "cwd": work,
            "tool_name": "Edit",
            "tool_use_id": "toolu_write_1",
            "tool_input": {"file_path": written},
            "tool_response": "ok",
        }),
    );
    hook(
        home.path(),
        &db,
        serde_json::json!({
            "session_id": SESSION_A,
            "hook_event_name": "PreToolUse",
            "cwd": work,
            "tool_name": "Read",
            "tool_use_id": "toolu_read_1",
            "tool_input": {"file_path": read},
        }),
    );
    hook(
        home.path(),
        &db,
        serde_json::json!({
            "session_id": SESSION_A,
            "hook_event_name": "PostToolUse",
            "cwd": work,
            "tool_name": "Read",
            "tool_use_id": "toolu_read_1",
            "tool_input": {"file_path": read},
            "tool_response": "     1\t# ground truth\n",
        }),
    );
    hook(
        home.path(),
        &db,
        serde_json::json!({
            "session_id": SESSION_A, "hook_event_name": "Stop", "cwd": work,
        }),
    );

    scan(home.path(), &db, home.path());
    assert!(
        std::fs::read_dir(spool_dir(home.path()))
            .map(|dir| dir.filter_map(Result::ok).count())
            .unwrap_or(0)
            == 0,
        "an ingested spool file is deleted"
    );

    let status = json_out(archie(home.path(), &db).arg("agent").arg("status").arg("--json"));
    let session = &status["sessions"][0];
    assert_eq!(session["session_id"], SESSION_A);
    assert_eq!(session["state"], "idle", "Stop leaves the session idle");

    let sha = sha256_of(&written);
    let anchors = json_out(
        archie(home.path(), &db)
            .arg("session")
            .arg("anchors")
            .arg(SESSION_A)
            .arg("--json"),
    );
    let values: Vec<String> = anchors["kinds"]["sha256"]
        .as_array()
        .expect("a sha256 group")
        .iter()
        .map(|a| a["value"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(
        values.contains(&sha),
        "the edited file's hash is an anchor: {values:?} wanted {sha}"
    );

    // A second session edits the file the first one read, through the same spool.
    hook(
        home.path(),
        &db,
        serde_json::json!({
            "session_id": SESSION_B, "hook_event_name": "SessionStart", "cwd": work,
        }),
    );
    hook(
        home.path(),
        &db,
        serde_json::json!({
            "session_id": SESSION_B,
            "hook_event_name": "PreToolUse",
            "cwd": work,
            "tool_name": "Edit",
            "tool_use_id": "toolu_write_2",
            "tool_input": {"file_path": read, "old_string": "ground", "new_string": "moved"},
        }),
    );
    std::fs::write(&read, "# moved truth\n").unwrap();
    hook(
        home.path(),
        &db,
        serde_json::json!({
            "session_id": SESSION_B,
            "hook_event_name": "PostToolUse",
            "cwd": work,
            "tool_name": "Edit",
            "tool_use_id": "toolu_write_2",
            "tool_input": {"file_path": read},
            "tool_response": "ok",
        }),
    );
    scan(home.path(), &db, home.path());

    let drift = json_out(
        archie(home.path(), &db)
            .arg("session")
            .arg("drift")
            .arg(SESSION_A)
            .arg("--json"),
    );
    assert_eq!(drift["session_id"], SESSION_A);
    let drifted = drift["drift"].as_array().expect("a drift list");
    assert_eq!(drifted.len(), 1, "one path moved: {drifted:?}");
    assert_eq!(drifted[0]["path"].as_str().unwrap(), read.to_str().unwrap());
    assert_eq!(
        drifted[0]["writer"]["session_id"], SESSION_B,
        "the second session is named as the writer"
    );
}

/// The duplicate-delivery half of the `hook_recovery` gate, through the real binary:
/// the same Stop (same event id, as a hook retry sends it) spooled twice applies once.
#[test]
fn the_same_event_id_delivered_twice_applies_once() {
    let (home, db) = sandbox();
    let work = home.path().join("work");
    std::fs::create_dir_all(&work).unwrap();

    hook(
        home.path(),
        &db,
        serde_json::json!({
            "session_id": SESSION_A, "hook_event_name": "SessionStart",
            "cwd": work, "event_id": "evt-dup-start",
        }),
    );
    hook(
        home.path(),
        &db,
        serde_json::json!({
            "session_id": SESSION_A, "hook_event_name": "UserPromptSubmit",
            "cwd": work, "event_id": "evt-dup-prompt",
        }),
    );
    let stop = serde_json::json!({
        "session_id": SESSION_A, "hook_event_name": "Stop",
        "cwd": work, "event_id": "evt-dup-stop",
    });
    hook(home.path(), &db, stop.clone());
    // The retry: the hook fired again before the first delivery was acknowledged.
    hook(home.path(), &db, stop);

    scan(home.path(), &db, home.path());

    let status = json_out(archie(home.path(), &db).arg("agent").arg("status").arg("--json"));
    let session = &status["sessions"][0];
    assert_eq!(session["session_id"], SESSION_A);
    assert_eq!(session["state"], "idle");
    assert_eq!(
        session["last_seq"], 3,
        "Start + Prompt + Stop is three effects; the retried Stop is none: {session:?}"
    );
}

/// The kill-mid-lane half of the `hook_recovery` gate: no SessionEnd, no handoff, only the
/// spool the write-ahead left behind. A scan drains it and the session reads back idle at
/// the exact sequence -- crashed-recovered, never a stale or doubled handoff.
#[test]
fn a_kill_mid_lane_spool_recovers_without_duplicates_or_loss() {
    let (home, db) = sandbox();
    let work = home.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let written = work.join("written.rs");
    std::fs::write(&written, "fn main() {}\n").unwrap();

    let lane = [
        ("SessionStart", "evt-kill-1"),
        ("UserPromptSubmit", "evt-kill-2"),
        ("PreToolUse", "evt-kill-3"),
        ("PostToolUse", "evt-kill-4"),
        ("Stop", "evt-kill-5"),
        // The retry already in flight when the process died: same id, must not double-apply.
        ("Stop", "evt-kill-5"),
    ];
    for (name, id) in lane {
        let mut payload = serde_json::json!({
            "session_id": SESSION_A, "hook_event_name": name,
            "cwd": work, "event_id": id,
        });
        if name == "PreToolUse" || name == "PostToolUse" {
            payload["tool_name"] = serde_json::json!("Edit");
            payload["tool_use_id"] = serde_json::json!("toolu_kill_1");
            payload["tool_input"] = serde_json::json!({"file_path": written});
        }
        if name == "PostToolUse" {
            payload["tool_response"] = serde_json::json!("ok");
        }
        hook(home.path(), &db, payload);
    }
    // No SessionEnd: the process died here. The spool is all that is left.

    scan(home.path(), &db, home.path());
    assert!(
        std::fs::read_dir(spool_dir(home.path()))
            .map(|dir| dir.filter_map(Result::ok).count())
            .unwrap_or(0)
            == 0,
        "an ingested spool file is deleted"
    );

    let status = json_out(archie(home.path(), &db).arg("agent").arg("status").arg("--json"));
    let session = &status["sessions"][0];
    assert_eq!(session["session_id"], SESSION_A);
    assert_eq!(session["state"], "idle", "the Stop closed the loop");
    assert_eq!(
        session["last_seq"], 5,
        "five unique events are five effects: {session:?}"
    );
}

#[test]
fn the_printed_snippet_is_json_and_registers_twelve_events() {
    let (home, db) = sandbox();
    let output = archie(home.path(), &db)
        .arg("hook")
        .arg("print")
        .arg("claude")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    let (comment, snippet) = text.split_once('\n').expect("a comment line then the JSON");
    assert!(
        comment.contains("settings.json"),
        "the first line says where to paste it: {comment}"
    );
    let value: Value = serde_json::from_str(snippet).expect("the snippet is JSON");
    let hooks = value["hooks"].as_object().expect("a hooks object");
    assert_eq!(hooks.len(), 12, "every documented event: {:?}", hooks.keys());
    for (name, entries) in hooks {
        let command = &entries[0]["hooks"][0];
        assert_eq!(command["command"], "archie hook", "{name}");
        assert_eq!(command["async"], true, "{name}");
    }
}

fn sha256_of(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).unwrap();
    hex::encode(Sha256::digest(&bytes))
}
