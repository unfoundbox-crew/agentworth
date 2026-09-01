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
}

/// Merge an external SQLite index database located at `source_db_path` into `target_db_path`.
pub fn merge_sqlite_databases(target_db_path: &Path, source_db_path: &Path) -> Result<MergeStats> {
    if !source_db_path.exists() {
        anyhow::bail!(
            "Source database does not exist: {}",
            source_db_path.display()
        );
    }

    // Opening through Storage first guarantees the target has the full schema (sources,
    // sessions, file_modifications, indexes) even if target_db_path is a brand new file --
    // the raw rusqlite Connection below does not run migrations itself.
    drop(agentworth_storage::Storage::open_path(target_db_path)?);

    let mut target_conn = Connection::open(target_db_path)
        .with_context(|| format!("Failed to open target DB: {}", target_db_path.display()))?;

    // Enable WAL mode and foreign keys
    target_conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;

    // SQLite does not allow ATTACH DATABASE while a transaction is active, so attach first
    // and only then open the transaction that does the actual merge work.
    let attach_sql = format!(
        "ATTACH DATABASE '{}' AS other_db;",
        source_db_path.to_string_lossy().replace('\'', "''")
    );
    target_conn.execute_batch(&attach_sql)?;

    let mut stats = MergeStats::default();

    let tx = target_conn.transaction()?;

    // 1. Merge sources table
    let sources_merged: usize = tx.execute(
        "INSERT INTO sources (source_path, adapter, file_size, mtime, fingerprint, scanned_at)
         SELECT source_path, adapter, file_size, mtime, fingerprint, scanned_at
         FROM other_db.sources
         WHERE true
         ON CONFLICT(source_path) DO UPDATE SET
            adapter = excluded.adapter,
            file_size = excluded.file_size,
            mtime = excluded.mtime,
            fingerprint = excluded.fingerprint,
            scanned_at = excluded.scanned_at
         WHERE excluded.mtime > sources.mtime;",
        [],
    )?;
    stats.sources_merged = sources_merged;

    // 2. Iterate and merge sessions, keeping whichever copy captured more of the transcript
    let mut sessions_to_refresh_files: Vec<String> = Vec::new();
    {
        let mut stmt = tx.prepare(
            "SELECT
                session_id, adapter, source_path, fingerprint, started_at, ended_at,
                duration_seconds, total_events, user_messages_count, assistant_messages_count,
                tool_calls_count, input_tokens, output_tokens, cache_read_tokens,
                cache_creation_tokens, total_tokens, models_used, tools_used, metadata,
                scanned_at, primary_outcome, composite_score
             FROM other_db.sessions;",
        )?;

        let mut rows = stmt.query([])?;
        let mut check_stmt =
            tx.prepare("SELECT total_events FROM sessions WHERE session_id = ?;")?;

        let mut insert_stmt = tx.prepare(
            "INSERT INTO sessions (
                session_id, adapter, source_path, fingerprint, started_at, ended_at,
                duration_seconds, total_events, user_messages_count, assistant_messages_count,
                tool_calls_count, input_tokens, output_tokens, cache_read_tokens,
                cache_creation_tokens, total_tokens, models_used, tools_used, metadata,
                scanned_at, primary_outcome, composite_score
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22
            );",
        )?;

        let mut update_stmt = tx.prepare(
            "UPDATE sessions SET
                adapter = ?2, source_path = ?3, fingerprint = ?4, started_at = ?5, ended_at = ?6,
                duration_seconds = ?7, total_events = ?8, user_messages_count = ?9,
                assistant_messages_count = ?10, tool_calls_count = ?11, input_tokens = ?12,
                output_tokens = ?13, cache_read_tokens = ?14, cache_creation_tokens = ?15,
                total_tokens = ?16, models_used = ?17, tools_used = ?18, metadata = ?19,
                scanned_at = ?20, primary_outcome = ?21, composite_score = ?22
             WHERE session_id = ?1;",
        )?;

        while let Some(row) = rows.next()? {
            let session_id: String = row.get(0)?;
            let other_events: i64 = row.get(7)?;

            let mut existing = check_stmt.query([&session_id])?;
            if let Some(existing_row) = existing.next()? {
                let existing_events: i64 = existing_row.get(0)?;
                if other_events > existing_events {
                    update_stmt.execute(rusqlite::params![
                        session_id,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<f64>>(6)?,
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
                        row.get::<_, String>(19)?,
                        row.get::<_, Option<String>>(20)?,
                        row.get::<_, Option<f64>>(21)?,
                    ])?;
                    stats.sessions_updated += 1;
                    sessions_to_refresh_files.push(session_id);
                } else {
                    stats.sessions_skipped += 1;
                }
            } else {
                insert_stmt.execute(rusqlite::params![
                    session_id,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<f64>>(6)?,
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
                    row.get::<_, String>(19)?,
                    row.get::<_, Option<String>>(20)?,
                    row.get::<_, Option<f64>>(21)?,
                ])?;
                stats.sessions_inserted += 1;
                sessions_to_refresh_files.push(session_id);
            }
        }
    }

    // 3. For every session whose copy just changed, replace its file_modifications wholesale
    //    with the source's rows -- mirrors Storage::upsert_session's own "rewrite, don't diff"
    //    approach, so re-running the same merge twice never duplicates rows.
    let mut files_merged = 0usize;
    {
        let mut delete_stmt =
            tx.prepare("DELETE FROM file_modifications WHERE session_id = ?1;")?;
        let mut copy_stmt = tx.prepare(
            "INSERT INTO file_modifications (session_id, file_path, action, occurred_at, model)
             SELECT session_id, file_path, action, occurred_at, model
             FROM other_db.file_modifications
             WHERE session_id = ?1;",
        )?;
        for session_id in &sessions_to_refresh_files {
            delete_stmt.execute(rusqlite::params![session_id])?;
            files_merged += copy_stmt.execute(rusqlite::params![session_id])?;
        }
    }
    stats.files_merged = files_merged;

    tx.commit()?;

    target_conn.execute_batch("DETACH DATABASE other_db;")?;

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
        None => agentworth_storage::default_db_dir()?.join("agentworth.db"),
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

    fn model_invocation_event(seq: u64, now: chrono::DateTime<chrono::Utc>) -> agentworth_schema::NormalizedEvent {
        use agentworth_schema::{EventPayload, NormalizedEvent, TokenUsage};
        NormalizedEvent::new(
            seq,
            now,
            EventPayload::ModelInvocation {
                model: "claude-3-5-sonnet".to_string(),
                token_usage: TokenUsage::new(10, 5, 0, 0),
                cost_usd: None,
                latency_ms: None,
            },
        )
    }

    #[test]
    fn test_merge_sqlite_databases() {
        let db1 = NamedTempFile::new().unwrap();
        let db2 = NamedTempFile::new().unwrap();

        let storage1 = agentworth_storage::Storage::open_path(db1.path()).unwrap();
        let storage2 = agentworth_storage::Storage::open_path(db2.path()).unwrap();

        use agentworth_schema::{AgentWorthTrace, EventPayload, NormalizedEvent, Provenance};
        use chrono::Utc;

        // Populate db1 with Session A (2 events -- list_sessions only surfaces sessions past
        // the total_events > 1 / total_tokens > 0 stub filter, so every trace below needs a
        // real ModelInvocation, not just a bare UserMessage).
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
        trace_a.events.push(model_invocation_event(2, now));
        trace_a.recalculate_stats();
        storage1.upsert_trace(&trace_a).unwrap();

        // Populate db2 with Session A (3 events - more complete) and Session B (new)
        let mut trace_a2 = trace_a.clone();
        trace_a2.events.push(NormalizedEvent::new(
            3,
            now,
            EventPayload::AssistantMessage {
                content: "response a".to_string(),
                thinking: None,
            },
        ));
        trace_a2.recalculate_stats();
        storage2.upsert_trace(&trace_a2).unwrap();

        let prov_b = Provenance::new("/tmp/b.jsonl", "codex", 200, 2000, "fpb");
        let mut trace_b = AgentWorthTrace::new("sess-b", "codex", prov_b, now);
        trace_b.events.push(NormalizedEvent::new(
            1,
            now,
            EventPayload::UserMessage {
                content: "hello b".to_string(),
            },
        ));
        trace_b.events.push(model_invocation_event(2, now));
        trace_b.recalculate_stats();
        storage2.upsert_trace(&trace_b).unwrap();

        // Perform merge
        let stats = merge_sqlite_databases(db1.path(), db2.path()).unwrap();

        assert_eq!(stats.sessions_inserted, 1); // sess-b inserted
        assert_eq!(stats.sessions_updated, 1); // sess-a updated (had 2 events, updated to 3)

        // Verify storage1 now has 2 sessions
        let all = storage1.list_sessions(10).unwrap();
        assert_eq!(all.len(), 2);
    }
}
