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
    /// Rows copied across every OTHER per-session child table (`session_compaction`,
    /// `session_risk`, `session_model_usage` -- see `SESSION_CHILD_TABLES`). Kept separate
    /// from `files_merged` rather than folded into it so a caller can still see
    /// `file_modifications` broken out on its own, matching the field's established meaning.
    pub child_rows_merged: usize,
}

/// One per-session child table that must be carried across on every merge, alongside the
/// `sessions` row it belongs to.
///
/// This is the single place a new such table must be registered. Forgetting to add a table
/// here is exactly the bug this const exists to prevent (#84's finding 2: `merge` silently
/// dropped `session_model_usage`, `session_risk`, and `session_compaction`, and nothing caught
/// it) -- `test_session_child_tables_cover_every_session_id_foreign_key` in this module fails
/// against `sqlite_master` if a table with a `session_id` column referencing `sessions` is
/// added without a matching entry here.
struct ChildTable {
    name: &'static str,
    /// Every column except an autoincrement surrogate key (`file_modifications.id`), in
    /// declaration order -- both databases share the exact same `initialize_schema`, so this
    /// order is guaranteed identical on both sides of the copy.
    columns: &'static [&'static str],
}

const SESSION_CHILD_TABLES: &[ChildTable] = &[
    ChildTable {
        name: "file_modifications",
        columns: &["session_id", "file_path", "action", "occurred_at", "model"],
    },
    ChildTable {
        name: "session_compaction",
        columns: &[
            "session_id",
            "round",
            "start_seq",
            "end_seq",
            "summary_seq",
            "tokens_before",
            "summary_tokens",
        ],
    },
    ChildTable {
        name: "session_risk",
        columns: &[
            "session_id",
            "demoted_claims",
            "loop_alerts",
            "unresolved_loops",
            "recoveries",
            "demoted_evidence",
            "loop_evidence",
            "computed_at",
        ],
    },
    ChildTable {
        name: "session_model_usage",
        columns: &[
            "session_id",
            "model",
            "input_tokens",
            "output_tokens",
            "cache_read_tokens",
            "cache_creation_tokens",
        ],
    },
];

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
    // the raw rusqlite Connection below does not run migrations itself. The source gets the
    // same treatment: it may be an older index (pre-dating `parser_version`/
    // `backfilled_version`), and the cross-database SELECT below names those columns
    // explicitly, so it must exist on both sides before ATTACH.
    drop(agentworth_storage::Storage::open_path(target_db_path)?);
    drop(agentworth_storage::Storage::open_path(source_db_path)?);

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
                scanned_at, primary_outcome, composite_score, parser_version, backfilled_version
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
                scanned_at, primary_outcome, composite_score, parser_version, backfilled_version
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24
            );",
        )?;

        let mut update_stmt = tx.prepare(
            "UPDATE sessions SET
                adapter = ?2, source_path = ?3, fingerprint = ?4, started_at = ?5, ended_at = ?6,
                duration_seconds = ?7, total_events = ?8, user_messages_count = ?9,
                assistant_messages_count = ?10, tool_calls_count = ?11, input_tokens = ?12,
                output_tokens = ?13, cache_read_tokens = ?14, cache_creation_tokens = ?15,
                total_tokens = ?16, models_used = ?17, tools_used = ?18, metadata = ?19,
                scanned_at = ?20, primary_outcome = ?21, composite_score = ?22,
                parser_version = ?23, backfilled_version = ?24
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
                        row.get::<_, i64>(22)?,
                        row.get::<_, Option<i64>>(23)?,
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
                    row.get::<_, i64>(22)?,
                    row.get::<_, Option<i64>>(23)?,
                ])?;
                stats.sessions_inserted += 1;
                sessions_to_refresh_files.push(session_id);
            }
        }
    }

    // 3. For every session whose copy just changed, replace every per-session child table
    //    wholesale with the source's rows -- mirrors Storage::upsert_session's own "rewrite,
    //    don't diff" approach, so re-running the same merge twice never duplicates rows.
    //    Loops over `SESSION_CHILD_TABLES` so a table added there is automatically carried;
    //    previously only `file_modifications` was hardcoded here and `session_model_usage`,
    //    `session_risk`, and `session_compaction` were silently dropped on every merge (#84).
    let mut files_merged = 0usize;
    let mut child_rows_merged = 0usize;
    for table in SESSION_CHILD_TABLES {
        let column_list = table.columns.join(", ");
        let delete_sql = format!("DELETE FROM {} WHERE session_id = ?1;", table.name);
        let copy_sql = format!(
            "INSERT INTO {table_name} ({column_list}) \
             SELECT {column_list} FROM other_db.{table_name} WHERE session_id = ?1;",
            table_name = table.name,
            column_list = column_list
        );
        let mut delete_stmt = tx.prepare(&delete_sql)?;
        let mut copy_stmt = tx.prepare(&copy_sql)?;
        for session_id in &sessions_to_refresh_files {
            delete_stmt.execute(rusqlite::params![session_id])?;
            let copied = copy_stmt.execute(rusqlite::params![session_id])?;
            if table.name == "file_modifications" {
                files_merged += copied;
            } else {
                child_rows_merged += copied;
            }
        }
    }
    stats.files_merged = files_merged;
    stats.child_rows_merged = child_rows_merged;

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
        "│ Child Rows Merged: {:<37} │",
        style(stats.child_rows_merged).cyan()
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

    /// Finding #2: the merge SQL used to copy `primary_outcome`/`composite_score` verbatim
    /// but never selected `parser_version` or `backfilled_version`, so every merged row landed
    /// at the schema default (`parser_version = 0`) -- correct for a source that also exists
    /// locally (it just gets reparsed once, same as any legacy row), but permanently wrong for
    /// a row whose source lives only on another machine: `needs_backfill` would flag it as
    /// stale forever with no local file to ever reparse it from. Both the insert path (a
    /// session that exists only in the source db) and the update path (a session that exists
    /// in both, where the source's copy is more complete) must carry the source's own
    /// `parser_version` and `backfilled_version` across.
    #[test]
    fn test_merge_carries_parser_version_and_backfill_marker() {
        let db1 = NamedTempFile::new().unwrap();
        let db2 = NamedTempFile::new().unwrap();

        let storage1 = agentworth_storage::Storage::open_path(db1.path()).unwrap();
        let storage2 = agentworth_storage::Storage::open_path(db2.path()).unwrap();

        use agentworth_schema::{AgentWorthTrace, EventPayload, NormalizedEvent, Provenance};
        use chrono::Utc;

        let now = Utc::now();

        // Insert path: a session that exists only in the source db, parsed and scored there
        // at parser version 3 -- the shape a row from another machine's index takes.
        let prov_new = Provenance::new("/machine-b/session.jsonl", "claude_code", 100, 1000, "fp_new");
        let mut trace_new = AgentWorthTrace::new("sess-new", "claude_code", prov_new, now);
        trace_new.events.push(NormalizedEvent::new(
            1,
            now,
            EventPayload::UserMessage { content: "hello from machine b".to_string() },
        ));
        trace_new.events.push(model_invocation_event(2, now));
        trace_new.recalculate_stats();
        storage2
            .upsert_session(&trace_new, Some("done_claimed"), Some(0.75), 3)
            .unwrap();

        // Update path: a session in both dbs, where the source's copy is more complete (3
        // events vs. 2) and was scored there at parser version 5 -- higher than the local
        // copy's version 2, mirroring a source that has since been reparsed under newer code.
        let prov_shared =
            Provenance::new("/machine-b/shared.jsonl", "claude_code", 100, 1000, "fp_shared");
        let mut trace_shared = AgentWorthTrace::new("sess-shared", "claude_code", prov_shared, now);
        trace_shared.events.push(NormalizedEvent::new(
            1,
            now,
            EventPayload::UserMessage { content: "hi".to_string() },
        ));
        trace_shared.events.push(model_invocation_event(2, now));
        trace_shared.recalculate_stats();
        storage1
            .upsert_session(&trace_shared, Some("done_claimed"), Some(0.4), 2)
            .unwrap();

        let mut trace_shared_remote = trace_shared.clone();
        trace_shared_remote.events.push(NormalizedEvent::new(
            3,
            now,
            EventPayload::AssistantMessage { content: "more".to_string(), thinking: None },
        ));
        trace_shared_remote.recalculate_stats();
        storage2
            .upsert_session(&trace_shared_remote, Some("done_claimed"), Some(0.9), 5)
            .unwrap();

        merge_sqlite_databases(db1.path(), db2.path()).unwrap();

        let target = Connection::open(db1.path()).unwrap();
        let (pv_new, bv_new): (i64, Option<i64>) = target
            .query_row(
                "SELECT parser_version, backfilled_version FROM sessions WHERE session_id = 'sess-new'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(pv_new, 3, "an inserted merged row must carry the source's parser_version");
        assert_eq!(bv_new, Some(3), "and its backfill marker, so it isn't flagged as stale forever");

        let (pv_shared, bv_shared): (i64, Option<i64>) = target
            .query_row(
                "SELECT parser_version, backfilled_version FROM sessions WHERE session_id = 'sess-shared'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            pv_shared, 5,
            "an updated merged row (the source's more-complete copy) must carry its parser_version too"
        );
        assert_eq!(bv_shared, Some(5));
    }

    /// #84 finding 2: merge copied `sources`, `sessions`, and `file_modifications`, but
    /// silently dropped `session_model_usage`, `session_risk`, and `session_compaction` for
    /// every merged session. This drives one fixture session through all four per-session
    /// child tables and asserts the merged target ends up with the same row count as the
    /// source in each -- the exact regression check the finding asked for.
    #[test]
    fn test_merge_carries_every_session_child_table() {
        let db1 = NamedTempFile::new().unwrap();
        let db2 = NamedTempFile::new().unwrap();

        // Target starts as a schema-only, empty index.
        drop(agentworth_storage::Storage::open_path(db1.path()).unwrap());

        {
            let storage2 = agentworth_storage::Storage::open_path(db2.path()).unwrap();

            use agentworth_schema::{
                AgentWorthTrace, EventPayload, FileActionType, NormalizedEvent, Provenance,
            };
            use chrono::Utc;

            let now = Utc::now();
            let prov = Provenance::new("/tmp/child.jsonl", "claude_code", 100, 1000, "fp-child");
            let mut trace = AgentWorthTrace::new("sess-child", "claude_code", prov, now);
            trace.events.push(NormalizedEvent::new(
                1,
                now,
                EventPayload::UserMessage {
                    content: "start".to_string(),
                },
            ));
            // ModelInvocation -> populates session_model_usage via upsert_trace.
            trace.events.push(model_invocation_event(2, now));
            // FileAction -> populates file_modifications via upsert_trace.
            trace.events.push(NormalizedEvent::new(
                3,
                now,
                EventPayload::FileAction {
                    action: FileActionType::Edit,
                    path: "src/lib.rs".to_string(),
                    diff: None,
                    lines_changed: Some(4),
                },
            ));
            trace.recalculate_stats();
            storage2.upsert_trace(&trace).unwrap();

            // session_risk has its own write path (core's risk detector +
            // Storage::upsert_session_risk), not something upsert_trace populates by itself.
            storage2
                .upsert_session_risk(&agentworth_storage::SessionRisk {
                    session_id: "sess-child".to_string(),
                    demoted_claims: 1,
                    loop_alerts: 2,
                    unresolved_loops: 0,
                    recoveries: 1,
                    demoted_evidence: Vec::new(),
                    loop_evidence: Vec::new(),
                    computed_at: Some(now),
                })
                .unwrap();
        }

        // session_compaction likewise has no convenient event-driven path here; write it
        // directly -- it is still one of the four tables merge must carry.
        {
            let conn = Connection::open(db2.path()).unwrap();
            conn.execute(
                "INSERT INTO session_compaction \
                 (session_id, round, start_seq, end_seq, summary_seq, tokens_before, summary_tokens) \
                 VALUES ('sess-child', 1, 1, 10, 5, 1000, 200)",
                [],
            )
            .unwrap();
        }

        let stats = merge_sqlite_databases(db1.path(), db2.path()).unwrap();
        assert_eq!(stats.sessions_inserted, 1);

        let source_conn = Connection::open(db2.path()).unwrap();
        let target_conn = Connection::open(db1.path()).unwrap();

        let row_count = |conn: &Connection, table: &str| -> i64 {
            conn.query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE session_id = 'sess-child'"),
                [],
                |row| row.get(0),
            )
            .unwrap()
        };

        for table in SESSION_CHILD_TABLES {
            let source_count = row_count(&source_conn, table.name);
            let target_count = row_count(&target_conn, table.name);
            assert!(
                source_count > 0,
                "fixture did not populate '{}' -- test is not exercising this table",
                table.name
            );
            assert_eq!(
                target_count, source_count,
                "merged index has {target_count} row(s) in '{}' for sess-child, source has {source_count}",
                table.name
            );
        }
    }

    /// The registry itself must stay complete: any table with a foreign key into
    /// `sessions(session_id)` has to be listed in `SESSION_CHILD_TABLES`, or a future table
    /// added the way `session_risk`/`session_compaction`/`session_model_usage` were will repeat
    /// #84's bug -- silently dropped by every merge, with nothing failing to say so.
    #[test]
    fn test_session_child_tables_cover_every_session_id_foreign_key() {
        let db = NamedTempFile::new().unwrap();
        drop(agentworth_storage::Storage::open_path(db.path()).unwrap());

        let conn = Connection::open(db.path()).unwrap();
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
            .unwrap();
        let table_names: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(!table_names.is_empty());

        let registered: std::collections::HashSet<&str> =
            SESSION_CHILD_TABLES.iter().map(|t| t.name).collect();

        for table_name in &table_names {
            if table_name == "sessions" || table_name == "sources" {
                continue;
            }
            let mut fk_stmt = conn
                .prepare(&format!("PRAGMA foreign_key_list({table_name})"))
                .unwrap();
            // Column 2 of `PRAGMA foreign_key_list` output is the referenced table name.
            let references_sessions = fk_stmt
                .query_map([], |row| row.get::<_, String>(2))
                .unwrap()
                .filter_map(Result::ok)
                .any(|referenced| referenced == "sessions");

            if references_sessions {
                assert!(
                    registered.contains(table_name.as_str()),
                    "table '{table_name}' has a foreign key into sessions(session_id) but is \
                     not listed in SESSION_CHILD_TABLES -- merge_sqlite_databases will silently \
                     drop its rows for every merged session. Add it to SESSION_CHILD_TABLES.",
                );
            }
        }

        // And the reverse: every registered entry must name a real table, so a stale or
        // renamed entry can't pass this test vacuously.
        for table in SESSION_CHILD_TABLES {
            assert!(
                table_names.iter().any(|n| n == table.name),
                "SESSION_CHILD_TABLES lists '{}' but no such table exists in the schema",
                table.name
            );
        }
    }
}
