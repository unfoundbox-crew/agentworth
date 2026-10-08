//! An explicit scan root must not ingest the machine's unrelated human-turn history.

use agentworth_schema::fixtures;
use agentworth_storage::Storage;
use assert_cmd::Command;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn scan(home: &Path, db: &Path, root: Option<&Path>, force: bool) -> Value {
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    // Confine the child process's discovery and config to synthetic sources.
    cmd.env("HOME", home)
        .env("USERPROFILE", home)
        .env("AGENTWORTH_CONFIG_PATH", home.join("config.toml"))
        .arg("--db-path")
        .arg(db)
        .arg("scan");
    if let Some(root) = root {
        cmd.arg(root);
    }
    if force {
        cmd.arg("--force");
    }
    let output = cmd
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).unwrap()
}

#[test]
fn scoped_scan_leaves_global_turn_sources_and_version_untouched() {
    let home = tempdir().unwrap();
    let selected = tempdir().unwrap();
    let session_id = "12345678-1234-1234-1234-123456789abc";
    let relative = Path::new(&fixtures::claude_transcript(fixtures::REPO, session_id))
        .strip_prefix(fixtures::HOME)
        .unwrap()
        .to_owned();
    let transcript = selected.path().join(relative);
    fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    fs::write(
        &transcript,
        format!(
            "{}\n{}\n",
            json!({"type":"user", "timestamp":"2026-10-01T10:00:00Z", "content":"fix the fixture"}),
            json!({"type":"assistant", "timestamp":"2026-10-01T10:00:02Z",
                "model":"claude-3-5-sonnet-20241022", "usage":{"input_tokens":100,"output_tokens":20},
                "content":[{"type":"text","text":"Fixture updated."}]})
        ),
    ).unwrap();
    let history = home.path().join(".codex/history.jsonl");
    fs::create_dir_all(history.parent().unwrap()).unwrap();
    let raw = format!(
        "{}\n",
        json!({"session_id":session_id,"ts":1759305600,"text":"unrelated prompt"})
    );
    fs::write(&history, &raw).unwrap();
    let db = home.path().join("index.db");

    for force in [false, true] {
        let summary = scan(home.path(), &db, Some(selected.path()), force);
        assert_eq!(summary["total_indexed_sessions"], 1);
        assert_eq!(summary["human_turns"]["sources_found"], 0);
        let storage = Storage::open_path(&db).unwrap();
        assert_eq!(storage.human_turn_total().unwrap(), 0);
        assert_eq!(storage.human_turn_ingestion_version().unwrap(), 0);
    }

    // A normal machine scan still ingests the turn lane.
    let full = scan(home.path(), &db, None, false);
    assert_eq!(full["human_turns"]["turns_inserted"], 1);
    let storage = Storage::open_path(&db).unwrap();
    let version = storage.human_turn_ingestion_version().unwrap();
    assert!(version > 0);
    assert_eq!(storage.human_turn_total().unwrap(), 1);
    drop(storage);

    // A forced scoped scan must also preserve already-ingested global turn data.
    scan(home.path(), &db, Some(selected.path()), true);
    let storage = Storage::open_path(&db).unwrap();
    assert_eq!(storage.human_turn_total().unwrap(), 1);
    assert_eq!(storage.human_turn_ingestion_version().unwrap(), version);
    assert_eq!(fs::read_to_string(history).unwrap(), raw);
}
