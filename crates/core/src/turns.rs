//! Human-turn ingestion orchestration (insights data lane): detect the turn sources through
//! `agentworth_adapters::human_turns`, skip unchanged files with the same fingerprint
//! conventions sessions use, stream each file line by line through the per-line normalizer
//! (multi-GB sources, one turn's memory at a time plus a fixed batch buffer), and land the
//! derived features in the `human_turns` table via storage.
//!
//! Boundaries: this module never writes to the raw histories; a malformed record degrades to
//! a counted skip; a failing file errors for that file only — its error keeps the summary
//! honest while the rest of the lane proceeds.

use std::fs::File;
use std::io::{BufRead, BufReader};

use agentworth_adapters::human_turns::{HumanTurnIngestor, TurnFileSource};
use agentworth_storage::human_turns::HumanTurnSourceRow;
use agentworth_storage::Storage;
use anyhow::Result;
use chrono::Utc;
use tracing::warn;

/// Ingestion version gate value: what `STORAGE` considers fresh. Mirrors the taxonomy
/// version — a bump here rewrites every turn's derived features, which is why the orchestrator
/// wipes and re-ingests on a delta.
const TAXONOMY_VERSION: i64 = agentworth_adapters::human_turns::INGESTION_VERSION;

/// What one ingestion pass did, joined into `ScanSummary`.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct TurnIngestSummary {
    /// Turn-bearing files found this pass.
    pub sources_found: usize,
    /// Files whose (size, mtime, fingerprint) still match what's on record — skipped without
    /// parsing, the session lane's unchanged-source skip.
    pub sources_skipped_unchanged: usize,
    /// Turn rows actually inserted.
    pub turns_inserted: usize,
    /// Re-encounters ignored by the dedup signature (a past run, or a sibling source in this
    /// pass, already stored the same turn).
    pub turns_deduplicated: usize,
    /// Malformed records the normalizer degraded to skips. Never fails the file.
    pub turns_degraded: usize,
    pub errors: usize,
}

impl TurnIngestSummary {
    pub fn total_turns(&self) -> usize {
        self.turns_inserted + self.turns_deduplicated
    }
}

/// Ingest the turn lane against `ingestor`'s homes. `force` re-parses every source and lets
/// the dedup signature decide what actually changes — no silent stale-serve.
pub fn ingest_human_turns(
    ingestor: &HumanTurnIngestor,
    storage: &Storage,
    force: bool,
) -> Result<TurnIngestSummary> {
    let mut summary = TurnIngestSummary::default();
    if !ingestor.detect()? {
        // No turn sources installed: an all-time truth, recorded so version gates stay
        // monotone rather than a permanent "always re-wipes" state.
        storage.set_human_turn_ingestion_version(TAXONOMY_VERSION, &now_iso())?;
        return Ok(summary);
    }

    // Version gate: a taxonomy bump rewrites every turn's derived features for files that
    // did not change, so file fingerprints alone cannot decide staleness — wipe once and
    // re-ingest.
    let recorded_version = storage.human_turn_ingestion_version()?;
    if recorded_version != TAXONOMY_VERSION {
        storage.wipe_human_turn_data()?;
    }

    let known = storage.known_human_turn_sources()?;
    for src in ingestor.enumerate()? {
        summary.sources_found += 1;
        let path_key = src.path.to_string_lossy().to_string();
        let unchanged = known
            .get(&path_key)
            .filter(|(size, mtime, fp)| {
                *size == src.file_size_bytes as i64
                    && *mtime == src.mtime_epoch_secs
                    && *fp == src.fingerprint
            })
            .map(|_| ());
        if unchanged.is_some() && !force {
            summary.sources_skipped_unchanged += 1;
            continue;
        }
        match ingest_source(ingestor, &src, &path_key, storage, force, &mut summary) {
            Ok(()) => {}
            Err(e) => {
                warn!("Turn source {:?} failed: {}", src.path, e);
                summary.errors += 1;
            }
        }
    }
    storage.set_human_turn_ingestion_version(TAXONOMY_VERSION, &now_iso())?;
    Ok(summary)
}

fn ingest_source(
    ingestor: &HumanTurnIngestor,
    src: &TurnFileSource,
    path_key: &str,
    storage: &Storage,
    _force: bool,
    summary: &mut TurnIngestSummary,
) -> Result<()> {
    let file = File::open(&src.path)?;
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut batch: Vec<agentworth_storage::human_turns::HumanTurnRow> = Vec::with_capacity(256);
    let mut inserted_in_file = 0usize;
    let mut index: u64 = 0;
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed.is_empty() {
            continue;
        }
        match ingestor.parse_line(src, trimmed) {
            Some(mut turn) => {
                turn.turn_index = index;
                index += 1;
                batch.push(to_row(&turn));
                if batch.len() >= 256 {
                    flush_turns(storage, &mut batch, summary, &mut inserted_in_file);
                }
            }
            None => summary.turns_degraded += 1,
        }
    }
    flush_turns(storage, &mut batch, summary, &mut inserted_in_file);
    storage.record_human_turn_source(
        &HumanTurnSourceRow {
            source_path: path_key.to_string(),
            source_label: src.source.to_string(),
            file_size: src.file_size_bytes as i64,
            mtime: src.mtime_epoch_secs,
            fingerprint: src.fingerprint.clone(),
            turns_indexed: inserted_in_file as i64,
        },
        &now_iso(),
    )?;
    Ok(())
}

fn flush_turns(
    storage: &Storage,
    batch: &mut Vec<agentworth_storage::human_turns::HumanTurnRow>,
    summary: &mut TurnIngestSummary,
    inserted_in_file: &mut usize,
) {
    if batch.is_empty() {
        return;
    }
    match storage.insert_human_turn_batch(batch) {
        Ok((inserted, ignored)) => {
            summary.turns_inserted += inserted;
            summary.turns_deduplicated += ignored;
            *inserted_in_file += inserted;
        }
        Err(e) => {
            warn!("Turn batch insert failed: {}", e);
            summary.errors += 1;
        }
    }
    batch.clear();
}

fn to_row(
    turn: &agentworth_adapters::human_turns::HumanTurn,
) -> agentworth_storage::human_turns::HumanTurnRow {
    agentworth_storage::human_turns::HumanTurnRow {
        source: turn.source.to_string(),
        session_id: turn.session_id.clone(),
        turn_index: turn.turn_index,
        timestamp_ms: turn.timestamp_ms,
        epoch_secs: turn.epoch_secs,
        local_hour: turn.local_hour,
        local_date: turn.local_date.clone(),
        word_count: turn.word_count,
        char_count: turn.char_count,
        friction_type: turn.friction_type.to_string(),
        dedup_sig: turn.dedup_sig.clone(),
        vocab_json: serde_json::to_string(&turn.vocab).expect("vocab entries serialize"),
    }
}

fn now_iso() -> String {
    Utc::now().to_rfc3339()
}
