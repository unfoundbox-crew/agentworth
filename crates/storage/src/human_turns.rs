//! Storage operations for the human-turn feature tables and the insights queries over them.
//!
//! Table of contents:
//! - one-batch insert for a source file's turns (dedup via `dedup_sig`, INSERT OR IGNORE;
//!   vocabulary counts ride inside the row)
//! - source fingerprint record / known-source lookups (same conventions as `sources`)
//! - ingestion state read/write (`human_turn_state`: version, wipe)
//! - windowed aggregate queries the insights module consumes (`human-turn window predicate`)

use anyhow::Result;
use rusqlite::{params, Connection};

/// One turn as the orchestrator hands it to storage. Field ordering matters for the batch
/// insert; see [`insert_human_turn_batch`].
#[derive(Debug, Clone)]
pub struct HumanTurnRow {
    pub source: String,
    pub session_id: Option<String>,
    pub turn_index: u64,
    pub timestamp_ms: i64,
    pub epoch_secs: f64,
    pub local_hour: u32,
    pub local_date: String,
    pub word_count: u64,
    pub char_count: u64,
    pub friction_type: String,
    pub dedup_sig: String,
    /// The turn's bounded vocabulary mention counts, already serialized as JSON
    /// `[[term, uses], ...]` — carried inside the row so a merge keeps them (column comment).
    pub vocab_json: String,
}

/// The turn-source identity record: a fingerprint row for one ingested file.
#[derive(Debug, Clone)]
pub struct HumanTurnSourceRow {
    pub source_path: String,
    pub source_label: String,
    pub file_size: i64,
    pub mtime: i64,
    pub fingerprint: String,
    pub turns_indexed: i64,
}

/// One `(path, size, mtime, fingerprint)` triple per already-processed turn file — the
/// unchanged-source skip input, same shape the scanner's `KnownSourceMap` expects.
pub fn known_human_turn_sources(
    conn: &Connection,
) -> Result<std::collections::HashMap<String, (i64, i64, String)>> {
    let mut stmt =
        conn.prepare("SELECT source_path, file_size, mtime, fingerprint FROM human_turn_sources")?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    Ok(rows
        .into_iter()
        .map(|(p, s, m, f)| (p, (s, m, f)))
        .collect())
}

/// Insert one source-file's turns. The dedup signature makes a turn inserted in a previous
/// run — or by a sibling source within this one — an ignored no-op, exactly the Python
/// builder's (round(epoch, 100 ms), first 60 chars) dedup but keyed by a hash so no prompt
/// copy lives in the index.
///
/// Returns `(inserted, duplicates_ignored)`.
pub(crate) fn insert_human_turn_batch(
    conn: &Connection,
    turns: &[HumanTurnRow],
) -> Result<(usize, usize)> {
    if turns.is_empty() {
        return Ok((0, 0));
    }
    let mut inserted = 0;
    let mut ignored = 0;
    // One autocommit transaction per batch: bounded work per write, and a mid-file failure
    // leaves every stored turn valid (per-row ignorance is the dedup contract, not a fault).
    for turn in turns {
        let inserted_here = conn.execute(
            "INSERT OR IGNORE INTO human_turns (
                source, session_id, turn_index, timestamp_ms, epoch_secs,
                local_hour, local_date, word_count, char_count, friction_type, dedup_sig,
                vocab_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                turn.source,
                turn.session_id.as_deref(),
                turn.turn_index,
                turn.timestamp_ms,
                turn.epoch_secs,
                turn.local_hour,
                turn.local_date,
                turn.word_count,
                turn.char_count,
                turn.friction_type,
                turn.dedup_sig,
                turn.vocab_json,
            ],
        )? == 1;
        if inserted_here {
            inserted += 1;
        } else {
            ignored += 1;
        }
    }
    Ok((inserted, ignored))
}

pub(crate) fn record_human_turn_source(
    conn: &Connection,
    src: &HumanTurnSourceRow,
    scanned_at: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO human_turn_sources (
            source_path, source_label, file_size, mtime, fingerprint, turns_indexed, scanned_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            src.source_path,
            src.source_label,
            src.file_size,
            src.mtime,
            src.fingerprint,
            src.turns_indexed,
            scanned_at,
        ],
    )?;
    Ok(())
}

/// Total stored turn rows; zero means the turn lane has never ingested anything.
pub fn human_turn_total(conn: &Connection) -> Result<i64> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM human_turns", [], |r| r.get(0))?;
    Ok(total)
}

/// The recorded ingestion version, 0 when nothing has ever ingested.
pub fn human_turn_ingestion_version(conn: &Connection) -> Result<i64> {
    let version: Option<i64> = conn
        .query_row(
            "SELECT ingestion_version FROM human_turn_state WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    Ok(version.unwrap_or(0))
}

pub(crate) fn set_human_turn_ingestion_version(
    conn: &Connection,
    version: i64,
    now: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO human_turn_state (id, ingestion_version, updated_at)
         VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET ingestion_version = ?1, updated_at = ?2",
        params![version, now],
    )?;
    Ok(())
}

/// Remove every stored turn row, its vocabulary links, and the per-source fingerprint
/// records (NOT the raw histories — those are read-only on someone else's disk).
pub(crate) fn wipe_human_turn_data(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM human_turns", [])?;
    conn.execute("DELETE FROM human_turn_sources", [])?;
    Ok(())
}
