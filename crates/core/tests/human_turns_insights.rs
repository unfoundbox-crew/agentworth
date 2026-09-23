//! The insights turn lane, end-to-end over synthetic fixtures: ingest → index → insights
//! payload. Real code path only — the adapters' per-line normalizer (crates/adapters), the
//! core orchestrator, the storage tables, `compute_insights` — against a temp synthetic home
//! built with the `fixtures` module's names, no real paths or third-party names anywhere.

use agentworth_adapters::human_turns::HumanTurnIngestor;
use agentworth_core::turns::{ingest_human_turns, TurnIngestSummary};
use agentworth_schema::fixtures;
use agentworth_storage::insights::parse_window;
use agentworth_storage::Storage;
use chrono::FixedOffset;
use std::path::Path;
use tempfile::TempDir;

/// The pinned local offset the goldens are computed against (+05:00): a 10:30Z turn lands
/// 15:30 the same local day; 20:00Z on Feb 5 lands 01:00 on Feb 6 — one day-shift cell.
const OFFSET_SECS: i32 = 5 * 3600;

fn write_fixtures(root: &Path) {
    fixtures::write_human_turns_fixture(root);
}

fn run_ingest(root: &Path, storage: &Storage) -> TurnIngestSummary {
    let ingestor = HumanTurnIngestor::rooted(root, FixedOffset::east_opt(OFFSET_SECS).unwrap());
    ingest_human_turns(&ingestor, storage, false).unwrap()
}

/// A taxonomy-version delta rewrites every turn's derived features (never serves stale
/// empties): version 0 means "old pipeline wrote the rows", so the pass wipes and re-ingests
/// everything, and the golden counts hold again.
#[test]
fn version_bump_reingests_the_whole_lane() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write_fixtures(root);
    let storage = Storage::open_path(&root.join("insights-fixture.db")).unwrap();
    run_ingest(root, &storage);
    storage
        .set_human_turn_ingestion_version(0, "stale")
        .unwrap();

    let rerun = run_ingest(root, &storage);
    assert_eq!(rerun.turns_inserted, 4);
    // The transcript's copy of turn 2 is again the same-pass sibling duplicate.
    assert_eq!(rerun.turns_deduplicated, 1);
    assert_eq!(
        storage.human_turn_ingestion_version().unwrap(),
        agentworth_adapters::human_turns::INGESTION_VERSION
    );
}

/// A multiple-source copy path: the same turn recorded once by a sibling source *in the same
/// pass* is ignored by the dedup signature, never double-counted.
#[test]
fn same_pass_siblings_dedup_through_the_shared_signature() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write_fixtures(root);
    let storage = Storage::open_path(&root.join("insights-fixture.db")).unwrap();
    let summary = run_ingest(root, &storage);
    assert_eq!(
        summary.total_turns(),
        5,
        "4 inserted + 1 cross-source duplicate"
    );
    assert_eq!(storage.human_turn_total().unwrap(), 4);
}

// ---- Golden numbers over the fixture set --------------------------------------------
//
// Stored turns:
//   1. claude history 10:30Z Feb 5 → local 15:30 Feb 5 (Thu, dow 4), loop_interruption,
//      vocabulary rust 1
//   2. context_amnesia turn, same local date, hour 1 (Feb 6 local), vocabulary doppler 1,
//      mcp 1, receipts 1 — deduped across history + transcript
//   3. antigravity history 18:00Z Feb 6 → local 23:00 Feb 6 (Fri), hallucination_pushback
//   4. brain 20:00Z Feb 6 → local 01:00 Feb 7 (Sat), friction none, vocabulary cargo 1,
//      receipts 1, truth 1
//
// Degraded counts: the task-notification record, the malformed line, the tool-result record,
// the assistant record, and the antigravity wrong-shape record — five degrades total.

#[test]
fn ingest_is_incremental_and_golden_counts_match_the_synthetic_set() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write_fixtures(root);
    let storage = Storage::open_path(&root.join("insights-fixture.db")).unwrap();

    let first = run_ingest(root, &storage);
    assert_eq!(
        first.sources_found, 4,
        "claude history + project transcript + antigravity history + brain transcript"
    );
    assert_eq!(
        first.turns_inserted, 4,
        "claude turn-1, claude turn-2 (dup ignored), agy history turn, brain turn"
    );
    assert_eq!(first.turns_deduplicated, 1);
    assert_eq!(first.turns_degraded, 5);
    assert_eq!(first.errors, 0);
    assert_eq!(storage.human_turn_total().unwrap(), 4);

    let second = run_ingest(root, &storage);
    assert_eq!(second.sources_skipped_unchanged, 4);
    assert_eq!(second.turns_inserted, 0);
    assert_eq!(storage.human_turn_total().unwrap(), 4);
}

/// The full insights payload over the ingested fixture set: heatmap cells, friction by
/// trigger, vocabulary mentions, and the friction-rate delta across a window boundary.
#[test]
fn insights_payload_carries_day_hour_friction_and_vocabulary_from_stored_turns() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write_fixtures(root);
    let storage = Storage::open_path(&root.join("insights-fixture.db")).unwrap();
    run_ingest(root, &storage);

    let insights = storage.get_insights().unwrap();
    let dims: Vec<&str> = insights
        .deferred
        .iter()
        .map(|d| d.dimension.as_str())
        .collect();
    assert!(
        dims.contains(&"intent_categories"),
        "intent categories stay deferred with a stated reason"
    );
    assert!(
        !dims.contains(&"friction_triggers"),
        "not measured once turns exist"
    );
    assert!(!dims.contains(&"time_of_day_histogram"));
    assert!(!dims.contains(&"vocabulary_mentions"));

    // Heatmap: local cells (dow, hour, turns), 0=Sunday.
    let cells: Vec<(i64, i64, i64)> = insights
        .day_hour
        .iter()
        .map(|c| (c.dow, c.hour, c.turns))
        .collect();
    assert!(cells.contains(&(4, 15, 1)), "Thu 15:00 local: {:?}", cells);
    assert!(cells.contains(&(5, 1, 1)));
    assert!(cells.contains(&(5, 23, 1)));
    assert!(cells.contains(&(6, 1, 1)));
    assert_eq!(insights.day_hour.len(), 4);

    // Friction by trigger.
    let fric: Vec<(&str, i64)> = insights
        .friction
        .iter()
        .map(|r| (r.trigger.as_str(), r.turns))
        .collect();
    assert!(fric.contains(&("loop_interruption", 1)), "{:?}", fric);
    assert!(fric.contains(&("context_amnesia", 1)));
    assert!(fric.contains(&("hallucination_pushback", 1)));
    assert_eq!(fric.len(), 3);

    // Vocabulary mentions.
    let vocab: Vec<(&str, i64)> = insights
        .vocabulary
        .iter()
        .map(|r| (r.term.as_str(), r.mentions))
        .collect();
    assert!(vocab.contains(&("receipts", 2)), "{:?}", vocab);
    assert!(vocab.contains(&("doppler", 1)));
    assert!(vocab.contains(&("mcp", 1)));
    assert!(vocab.contains(&("rust", 1)));
    assert!(vocab.contains(&("cargo", 1)));
    assert!(vocab.contains(&("truth", 1)));
    assert_eq!(vocab.len(), 6);

    // Friction rate, all-time: 3 of 4 turns.
    assert_eq!(
        insights.deltas.friction_rate.current,
        serde_json::json!(75.0)
    );
}

/// The window (`?since=&until=`) bounds the turn blocks with the same half-open rule the
/// session metrics use, and the friction-rate delta reads the previous window.
#[test]
fn window_filters_day_hour_friction_vocabulary_and_delta() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write_fixtures(root);
    let storage = Storage::open_path(&root.join("insights-fixture.db")).unwrap();
    run_ingest(root, &storage);

    let w = parse_window(
        Some("2026-02-06T00:00:00Z".into()),
        Some("2026-02-07T00:00:00Z".into()),
    )
    .unwrap()
    .unwrap();
    let insights = storage.get_insights_windowed(&w).unwrap();

    assert_eq!(insights.day_hour.len(), 2);
    assert!(insights
        .day_hour
        .iter()
        .any(|c| c.dow == 5 && c.hour == 23 && c.turns == 1));
    assert!(insights
        .day_hour
        .iter()
        .any(|c| c.dow == 6 && c.hour == 1 && c.turns == 1));

    let fric: Vec<&str> = insights
        .friction
        .iter()
        .map(|r| r.trigger.as_str())
        .collect();
    assert_eq!(fric, vec!["hallucination_pushback"]);

    let vocab: Vec<(&str, i64)> = insights
        .vocabulary
        .iter()
        .map(|r| (r.term.as_str(), r.mentions))
        .collect();
    assert!(vocab.contains(&("receipts", 1)), "{:?}", vocab);
    assert!(vocab.contains(&("cargo", 1)));
    assert!(vocab.contains(&("truth", 1)));

    // Current window: 1 of 2 turns friction → 50%; previous equal-length window [Feb 5,
    // Feb 6): both turns friction → 100%.
    assert_eq!(
        insights.deltas.friction_rate.current,
        serde_json::json!(50.0)
    );
    assert_eq!(
        insights.deltas.friction_rate.previous,
        serde_json::json!(100.0)
    );
    assert_eq!(
        insights.deltas.friction_rate.delta,
        serde_json::json!(-50.0)
    );
    assert_eq!(
        insights.deltas.friction_rate.delta_pct,
        serde_json::json!(-50.0)
    );
}

/// An empty index (no turn rows) must keep the three dimensions honestly deferred, with the
/// `friction_rate` delta carrying its reason, not a zero.
#[test]
fn empty_index_defers_the_turn_blocks_with_a_stated_reason() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write_fixtures(root);
    let storage = Storage::open_path(&root.join("insights-fixture.db")).unwrap();

    let insights = storage.get_insights().unwrap();
    let dims: Vec<(&str, bool)> = insights
        .deferred
        .iter()
        .map(|d| (d.dimension.as_str(), !d.reason.is_empty()))
        .collect();
    assert!(dims
        .iter()
        .any(|(d, has_reason)| *d == "friction_triggers" && *has_reason));
    assert!(dims
        .iter()
        .any(|(d, has_reason)| *d == "time_of_day_histogram" && *has_reason));
    assert!(dims
        .iter()
        .any(|(d, has_reason)| *d == "vocabulary_mentions" && *has_reason));
    assert!(insights.day_hour.is_empty());
    assert!(insights.friction.is_empty());
    assert!(insights.vocabulary.is_empty());
    assert!(insights.deltas.friction_rate.reason.is_some());
}
