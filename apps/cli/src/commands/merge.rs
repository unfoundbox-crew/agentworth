//! Merge subcommand for AgentWorth.
//!
//! Subcommand: `agentworth merge <source-db-path> [--json]`
//! Merges an external SQLite index database into the local database, deduping by `session_id`
//! and preserving the most complete/recent session data.

use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use console::style;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// Statistics reporting the outcome of a database merge operation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeStats {
    pub sessions_inserted: usize,
    pub sessions_updated: usize,
    pub sessions_skipped: usize,
    pub sources_merged: usize,
    pub files_merged: usize,
    pub model_usages_merged: usize,
}

/// Merge an external SQLite index database located at `source_db_path` into `target_db_path`.
pub fn merge_sqlite_databases(target_db_path: &Path, source_db_path: &Path) -> Result<MergeStats> {
    if !source_db_path.exists() {
        anyhow::bail!(
            "Source database does not exist: {}",
            source_db_path.display()
        );
    }

    let mut target_conn = Connection::open(target_db_path)
        .with_context(|| format!("Failed to open target DB: {}", target_db_path.display()))?;

    // Enable WAL mode and foreign keys
    target_conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;

    let mut stats = MergeStats::default();

    let tx = target_conn.transaction()?;

    // Attach source database
    let attach_sql = format!(
        "ATTACH DATABASE '{}' AS other_db;",
        source_db_path.to_string_lossy().replace('\'', "''")
    );
    tx.execute_batch(&attach_sql)?;

    // 1. Merge sources table
    let sources_merged: usize = tx.execute(
        "INSERT OR REPLACE INTO sources (path, adapter_name, file_size_bytes, mtime_epoch_secs, fingerprint, last_scanned_at)
         SELECT path, adapter_name, file_size_bytes, mtime_epoch_secs, fingerprint, last_scanned_at
         FROM other_db.sources
         WHERE true
         ON CONFLICT(path) DO UPDATE SET
            file_size_bytes = excluded.file_size_bytes,
            mtime_epoch_secs = excluded.mtime_epoch_secs,
            fingerprint = excluded.fingerprint,
            last_scanned_at = excluded.last_scanned_at
         WHERE excluded.mtime_epoch_secs > sources.mtime_epoch_secs;",
        [],
    )?;
    stats.sources_merged = sources_merged;

    // 2. Iterate and merge sessions
    {
        let mut stmt = tx.prepare(
            "SELECT 
                session_id, adapter, source_path, started_at, ended_at, duration_seconds,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                total_tokens, total_events, user_messages_count, assistant_messages_count,
                tool_calls_count, file_actions_count, models_used, tools_used,
                primary_outcome, primary_outcome_confidence, error_recovery_count,
                composite_score, trajectory_depth_score, outcome_score, complexity_score,
                rubric_version, rubric_provenance_hash, calculated_at
             FROM other_db.sessions;",
        )?;

        let mut rows = stmt.query([])?;
        let mut check_stmt = tx.prepare(
            "SELECT total_events, total_tokens FROM sessions WHERE session_id = ?;",
        )?;

        let mut insert_stmt = tx.prepare(
            "INSERT INTO sessions (
                session_id, adapter, source_path, started_at, ended_at, duration_seconds,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                total_tokens, total_events, user_messages_count, assistant_messages_count,
                tool_calls_count, file_actions_count, models_used, tools_used,
                primary_outcome, primary_outcome_confidence, error_recovery_count,
                composite_score, trajectory_depth_score, outcome_score, complexity_score,
                rubric_version, rubric_provenance_hash, calculated_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
                ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28
            );",
        )?;

        let mut update_stmt = tx.prepare(
            "UPDATE sessions SET
                adapter = ?2, source_path = ?3, started_at = ?4, ended_at = ?5, duration_seconds = ?6,
                input_tokens = ?7, output_tokens = ?8, cache_read_tokens = ?9, cache_creation_tokens = ?10,
                total_tokens = ?11, total_events = ?12, user_messages_count = ?13, assistant_messages_count = ?14,
                tool_calls_count = ?15, file_actions_count = ?16, models_used = ?17, tools_used = ?18,
                primary_outcome = ?19, primary_outcome_confidence = ?20, error_recovery_count = ?21,
                composite_score = ?22, trajectory_depth_score = ?23, outcome_score = ?24, complexity_score = ?25,
                rubric_version = ?26, rubric_provenance_hash = ?27, calculated_at = ?28
             WHERE session_id = ?1;",
        )?;

        while let Some(row) = rows.next()? {
            let session_id: String = row.get(0)?;
            let other_events: i64 = row.get(11)?;

            let mut existing = check_stmt.query([&session_id])?;
            if let Some(existing_row) = existing.next()? {
                let existing_events: i64 = existing_row.get(0)?;
                if other_events > existing_events {
                    // Update existing session
                    update_stmt.execute(rusqlite::params![
                        session_id,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<f64>>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, i64>(9)?,
                        row.get::<_, i64>(10)?,
                        row.get::<_, i64>(11)?,
                        row.get::<_, i64>(12)?,
                        row.get::<_, i64>(13)?,
                        row.get::<_, i64>(14)?,
                        row.get::<_, i64>(15)?,
                        row.get::<_, String>(16)?,
                        row.get::<_, String>(17)?,
                        row.get::<_, Option<String>>(18)?,
                        row.get::<_, Option<f64>>(19)?,
                        row.get::<_, i64>(20)?,
                        row.get::<_, Option<f64>>(21)?,
                        row.get::<_, Option<f64>>(22)?,
                        row.get::<_, Option<f64>>(23)?,
                        row.get::<_, Option<f64>>(24)?,
                        row.get::<_, Option<String>>(25)?,
                        row.get::<_, Option<String>>(26)?,
                        row.get::<_, Option<String>>(27)?,
                    ])?;
                    stats.sessions_updated += 1;
                } else {
                    stats.sessions_skipped += 1;
                }
            } else {
                // Insert new session
                insert_stmt.execute(rusqlite::params![
                    session_id,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<f64>>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, i64>(11)?,
                    row.get::<_, i64>(12)?,
                    row.get::<_, i64>(13)?,
                    row.get::<_, i64>(14)?,
                    row.get::<_, i64>(15)?,
                    row.get::<_, String>(16)?,
                    row.get::<_, String>(17)?,
                    row.get::<_, Option<String>>(18)?,
                    row.get::<_, Option<f64>>(19)?,
                    row.get::<_, i64>(20)?,
                    row.get::<_, Option<f64>>(21)?,
                    row.get::<_, Option<f64>>(22)?,
                    row.get::<_, Option<f64>>(23)?,
                    row.get::<_, Option<f64>>(24)?,
                    row.get::<_, Option<String>>(25)?,
                    row.get::<_, Option<String>>(26)?,
                    row.get::<_, Option<String>>(27)?,
                ])?;
                stats.sessions_inserted += 1;
            }
        }
    }

    // 3. Merge file modifications
    let files_merged = tx.execute(
        "INSERT OR IGNORE INTO file_modifications (session_id, file_path, action, lines_changed, timestamp)
         SELECT session_id, file_path, action, lines_changed, timestamp
         FROM other_db.file_modifications;",
        [],
    )?;
    stats.files_merged = files_merged;

    // 4. Merge session_model_usage
    let model_usages_merged = tx.execute(
        "INSERT OR IGNORE INTO session_model_usage (session_id, model_name, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens)
         SELECT session_id, model_name, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens
         FROM other_db.session_model_usage;",
        [],
    )?;
    stats.model_usages_merged = model_usages_merged;

    // Detach attached database
    tx.execute_batch("DETACH DATABASE other_db;")?;

    tx.commit()?;

    Ok(stats)
}

/// Execute the `agentworth merge` subcommand.
pub fn run_merge_command(
    source_db_path: PathBuf,
    json: bool,
    target_db_path: Option<PathBuf>,
) -> Result<()> {
    let resolved_target = match target_db_path {
        Some(p) => p,
        None => agentworth_storage::Storage::default_db_path()?,
    };

    let stats = merge_sqlite_databases(&resolved_target, &source_db_path)
        .with_context(|| format!("Failed to merge database from {}", source_db_path.display()))?;

    if json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
        return Ok(());
    }

    println!();
    println!(
        "{}",
        style("┌─ 🗄️  AgentWorth Cross-Machine Index Merge ────────────────┐").bold().cyan()
    );
    println!(
        "│ Target Index: {:<43} │",
        style(resolved_target.file_name().unwrap_or_default().to_string_lossy()).bold()
    );
    println!(
        "│ Source Index: {:<43} │",
        style(source_db_path.file_name().unwrap_or_default().to_string_lossy()).dim()
    );
    println!(
        "{}",
        style("├──────────────────────────────────────────────────────────┤").bold()
    );
    println!(
        "│ Sessions Inserted: {:<37} │",
        style(stats.sessions_inserted).bold().green()
    );
    println!(
        "│ Sessions Updated:  {:<37} │",
        style(stats.sessions_updated).bold().yellow()
    );
    println!(
        "│ Sessions Skipped:  {:<37} │",
        style(stats.sessions_skipped).dim()
    );
    println!(
        "│ Sources Merged:    {:<37} │",
        style(stats.sources_merged).cyan()
    );
    println!(
        "│ Files Merged:      {:<37} │",
        style(stats.files_merged).cyan()
    );
    println!(
        "{}",
        style("└──────────────────────────────────────────────────────────┘").bold()
    );
    println!();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_merge_sqlite_databases() {
        let db1 = NamedTempFile::new().unwrap();
        let db2 = NamedTempFile::new().unwrap();

        let storage1 = agentworth_storage::Storage::open_path(db1.path()).unwrap();
        let storage2 = agentworth_storage::Storage::open_path(db2.path()).unwrap();

        use agentworth_schema::{AgentWorthTrace, EventPayload, NormalizedEvent, Provenance};
        use chrono::Utc;

        // Populate db1 with Session A (1 event)
        let now = Utc::now();
        let prov_a = Provenance::new("/tmp/a.jsonl", "claude_code", 100, 1000, "fpa");
        let mut trace_a = AgentWorthTrace::new("sess-a", "claude_code", prov_a, now);
        trace_a.events.push(NormalizedEvent::new(
            1,
            now,
            EventPayload::UserMessage {
                content: "hello a".to_string(),
            },
        ));
        storage1.insert_trace(&trace_a).unwrap();

        // Populate db2 with Session A (2 events - more complete) and Session B (new)
        let mut trace_a2 = trace_a.clone();
        trace_a2.events.push(NormalizedEvent::new(
            2,
            now,
            EventPayload::AssistantMessage {
                content: "response a".to_string(),
                thinking: None,
            },
        ));
        storage2.insert_trace(&trace_a2).unwrap();

        let prov_b = Provenance::new("/tmp/b.jsonl", "codex", 200, 2000, "fpb");
        let mut trace_b = AgentWorthTrace::new("sess-b", "codex", prov_b, now);
        trace_b.events.push(NormalizedEvent::new(
            1,
            now,
            EventPayload::UserMessage {
                content: "hello b".to_string(),
            },
        ));
        storage2.insert_trace(&trace_b).unwrap();

        // Perform merge
        let stats = merge_sqlite_databases(db1.path(), db2.path()).unwrap();

        assert_eq!(stats.sessions_inserted, 1); // sess-b inserted
        assert_eq!(stats.sessions_updated, 1);  // sess-a updated (had 1 event, updated to 2)

        // Verify storage1 now has 2 sessions
        let all = storage1.list_sessions(Some(10)).unwrap();
        assert_eq!(all.len(), 2);
    }
}
