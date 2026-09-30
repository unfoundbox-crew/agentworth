//! Codex human-turn history.jsonl parse — fixtures under a rooted `codex-home/`.

use agentworth_adapters::human_turns::{HumanTurnIngestor, CODEX};
use chrono::FixedOffset;
use std::fs;
use tempfile::TempDir;

fn offset() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600).unwrap()
}

#[test]
fn enumerates_codex_history_jsonl_when_present() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let home = root.join("codex-home");
    fs::create_dir_all(&home).unwrap();
    fs::write(
        home.join("history.jsonl"),
        r#"{"session_id":"019c617f-ce70-7971-92de-93d8ff78ecbc","ts":1770294600,"text":"hello world from codex"}
"#,
    )
    .unwrap();
    let ingestor = HumanTurnIngestor::rooted(root, offset());
    let sources = ingestor.enumerate().unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].source, CODEX);
    assert!(sources[0].path.ends_with("history.jsonl"));
}

#[test]
fn absent_codex_history_is_a_quiet_miss() {
    let dir = TempDir::new().unwrap();
    let ingestor = HumanTurnIngestor::rooted(dir.path(), offset());
    assert!(!ingestor.detect().unwrap());
    assert!(ingestor.enumerate().unwrap().is_empty());
}

#[test]
fn parses_session_id_ts_seconds_and_text() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let home = root.join("codex-home");
    fs::create_dir_all(&home).unwrap();
    let path = home.join("history.jsonl");
    fs::write(
        &path,
        r#"{"session_id":"019c617f-ce70-7971-92de-93d8ff78ecbc","ts":1770294600,"text":"stop looping again please"}
"#,
    )
    .unwrap();
    let ingestor = HumanTurnIngestor::rooted(root, offset());
    let src = &ingestor.enumerate().unwrap()[0];
    let turn = ingestor
        .parse_line(
            src,
            r#"{"session_id":"019c617f-ce70-7971-92de-93d8ff78ecbc","ts":1770294600,"text":"stop looping again please"}"#,
        )
        .expect("parse");
    assert_eq!(turn.source, CODEX);
    assert_eq!(
        turn.session_id.as_deref(),
        Some("019c617f-ce70-7971-92de-93d8ff78ecbc")
    );
    assert_eq!(turn.timestamp_ms, 1770294600 * 1000);
    assert_eq!(turn.friction_type, "loop_interruption");
}

#[test]
fn ingestion_version_is_three_with_codex_source() {
    assert_eq!(agentworth_adapters::human_turns::INGESTION_VERSION, 3);
}
