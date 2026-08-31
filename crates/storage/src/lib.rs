pub mod chunker;
pub mod embedder;
pub mod vector;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use agentworth_adapter_sdk::SessionSource;
use agentworth_schema::{AgentWorthTrace, TokenUsage};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use directories::BaseDirs;
use rusqlite::types::ToSql;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

pub use chunker::TrajectoryChunker;
pub use embedder::LocalEmbedder;
pub use vector::{SqliteVectorStore, VectorStore};

/// High-level aggregate statistics across all scanned sessions in the SQLite index.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AggregateStats {
    pub total_sessions: usize,
    pub total_events: usize,
    pub token_usage: TokenUsage,
    pub sessions_by_adapter: BTreeMap<String, usize>,
    pub models_usage_count: BTreeMap<String, usize>,
    pub tools_usage_count: BTreeMap<String, usize>,
    pub verified_outcomes_count: usize,
    pub first_session_at: Option<DateTime<Utc>>,
    pub last_session_at: Option<DateTime<Utc>>,
}

/// Ordering options when querying session traces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionOrderBy {
    #[default]
    StartedAtDesc,
    StartedAtAsc,
    TokensDesc,
    TokensAsc,
    EventsDesc,
    EventsAsc,
    DurationDesc,
    ScoreDesc,
    ScoreAsc,
}

/// Filter criteria for querying session traces.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionFilter {
    pub adapter: Option<String>,
    pub model: Option<String>,
    pub search: Option<String>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub min_tokens: Option<u64>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub order_by: Option<SessionOrderBy>,
    pub include_stubs: Option<bool>,
    pub outcome: Option<String>,
}

/// Lightweight summary of a stored session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub adapter: String,
    pub source_path: String,
    pub started_at: DateTime<Utc>,
    pub duration_seconds: Option<f64>,
    pub total_tokens: u64,
    pub total_events: usize,
    pub tool_calls_count: usize,
    pub models_used: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composite_score: Option<f64>,
}

/// Usage aggregation rollup for a time period (Day, Week, Month).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsagePeriodSummary {
    pub period: String,
    pub adapter: String,
    pub session_count: usize,
    pub total_events: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_tokens: u64,
    pub total_duration_seconds: f64,
    pub estimated_cost_usd: f64,
    pub cache_hit_ratio: f64,
}

/// Rolling pacing window summary (e.g. 5-hour window).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PacingSummary {
    pub window_hours: i64,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub session_count: usize,
    pub total_events: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_tokens: u64,
    pub burn_rate_tokens_per_hour: f64,
    pub estimated_cost_usd: f64,
    pub cache_hit_ratio: f64,
    pub active_adapters: Vec<String>,
    pub active_models: Vec<String>,
}

/// AI Code Blame record matching a file modification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlameMatch {
    pub session_id: String,
    pub adapter: String,
    pub source_path: String,
    pub started_at: DateTime<Utc>,
    pub models_used: Vec<String>,
    pub total_tokens: u64,
    pub tool_calls_count: usize,
}

/// SQLite-backed storage index.
pub struct Storage {
    conn: Arc<Mutex<Connection>>,
    db_path: Option<PathBuf>,
}

impl Storage {
    /// Open or create the default local SQLite index at `~/.agentworth/agentworth.db`.
    pub fn open_default() -> Result<Self> {
        let db_dir = default_db_dir()?;
        std::fs::create_dir_all(&db_dir)
            .with_context(|| format!("Failed to create storage directory at {:?}", db_dir))?;
        let db_path = db_dir.join("agentworth.db");
        Self::open_path(&db_path)
    }

    /// Open or create SQLite index at a specific file path.
    pub fn open_path(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open SQLite database at {:?}", path))?;
        let storage = Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: Some(path.to_path_buf()),
        };
        storage.initialize_schema()?;
        Ok(storage)
    }

    /// Open an ephemeral in-memory SQLite index (ideal for unit testing).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let storage = Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: None,
        };
        storage.initialize_schema()?;
        Ok(storage)
    }

    /// Path to the active database file, if not in-memory.
    pub fn db_path(&self) -> Option<&Path> {
        self.db_path.as_deref()
    }

    /// Obtain a vector store instance sharing the underlying database connection.
    pub fn vector_store(&self) -> Result<SqliteVectorStore> {
        SqliteVectorStore::from_shared_connection(Arc::clone(&self.conn))
    }

    /// Run migrations / schema initialization.
    pub fn initialize_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;

            CREATE TABLE IF NOT EXISTS sources (
                source_path TEXT PRIMARY KEY,
                adapter TEXT NOT NULL,
                file_size INTEGER NOT NULL,
                mtime INTEGER NOT NULL,
                fingerprint TEXT NOT NULL,
                scanned_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sessions (
                session_id TEXT PRIMARY KEY,
                adapter TEXT NOT NULL,
                source_path TEXT NOT NULL,
                fingerprint TEXT NOT NULL,
                started_at TEXT NOT NULL,
                ended_at TEXT,
                duration_seconds REAL,
                total_events INTEGER NOT NULL DEFAULT 0,
                user_messages_count INTEGER NOT NULL DEFAULT 0,
                assistant_messages_count INTEGER NOT NULL DEFAULT 0,
                tool_calls_count INTEGER NOT NULL DEFAULT 0,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                models_used TEXT NOT NULL,
                tools_used TEXT NOT NULL,
                metadata TEXT,
                scanned_at TEXT NOT NULL,
                primary_outcome TEXT,
                composite_score REAL,
                FOREIGN KEY(source_path) REFERENCES sources(source_path) ON DELETE CASCADE
            );
            "#,
        )?;

        // Fallback schema migrations for existing databases before creating indexes
        let mut stmt = conn.prepare("PRAGMA table_info(sessions)")?;
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(Result::ok)
            .collect();

        if !columns.is_empty() {
            if !columns.contains(&"primary_outcome".to_string()) {
                let _ = conn.execute("ALTER TABLE sessions ADD COLUMN primary_outcome TEXT", []);
            }
            if !columns.contains(&"composite_score".to_string()) {
                let _ = conn.execute("ALTER TABLE sessions ADD COLUMN composite_score REAL", []);
            }
        }

        conn.execute_batch(
            r#"
            CREATE INDEX IF NOT EXISTS idx_sessions_adapter ON sessions(adapter);
            CREATE INDEX IF NOT EXISTS idx_sessions_started ON sessions(started_at);
            CREATE INDEX IF NOT EXISTS idx_sessions_tokens ON sessions(total_tokens);
            CREATE INDEX IF NOT EXISTS idx_sessions_events ON sessions(total_events);
            CREATE INDEX IF NOT EXISTS idx_sessions_duration ON sessions(duration_seconds);
            CREATE INDEX IF NOT EXISTS idx_sessions_started_at ON sessions(started_at);
            CREATE INDEX IF NOT EXISTS idx_sessions_primary_outcome ON sessions(primary_outcome);
            CREATE INDEX IF NOT EXISTS idx_sessions_composite_score ON sessions(composite_score);
            CREATE INDEX IF NOT EXISTS idx_sources_fingerprint ON sources(fingerprint);

            CREATE VIEW IF NOT EXISTS v_daily_usage AS
            SELECT
                DATE(started_at) AS period,
                adapter,
                COUNT(*) AS session_count,
                COALESCE(SUM(total_events), 0) AS total_events,
                COALESCE(SUM(input_tokens), 0) AS input_tokens,
                COALESCE(SUM(output_tokens), 0) AS output_tokens,
                COALESCE(SUM(cache_read_tokens), 0) AS cache_read_tokens,
                COALESCE(SUM(cache_creation_tokens), 0) AS cache_creation_tokens,
                COALESCE(SUM(total_tokens), 0) AS total_tokens,
                COALESCE(SUM(duration_seconds), 0.0) AS total_duration_seconds
            FROM sessions
            WHERE started_at > '2020-01-01'
            GROUP BY DATE(started_at), adapter
            ORDER BY period DESC, total_tokens DESC;

            CREATE VIEW IF NOT EXISTS v_weekly_usage AS
            SELECT
                strftime('%Y-W%W', started_at) AS period,
                adapter,
                COUNT(*) AS session_count,
                COALESCE(SUM(total_events), 0) AS total_events,
                COALESCE(SUM(input_tokens), 0) AS input_tokens,
                COALESCE(SUM(output_tokens), 0) AS output_tokens,
                COALESCE(SUM(cache_read_tokens), 0) AS cache_read_tokens,
                COALESCE(SUM(cache_creation_tokens), 0) AS cache_creation_tokens,
                COALESCE(SUM(total_tokens), 0) AS total_tokens,
                COALESCE(SUM(duration_seconds), 0.0) AS total_duration_seconds
            FROM sessions
            WHERE started_at > '2020-01-01'
            GROUP BY strftime('%Y-W%W', started_at), adapter
            ORDER BY period DESC, total_tokens DESC;

            CREATE VIEW IF NOT EXISTS v_monthly_usage AS
            SELECT
                strftime('%Y-%m', started_at) AS period,
                adapter,
                COUNT(*) AS session_count,
                COALESCE(SUM(total_events), 0) AS total_events,
                COALESCE(SUM(input_tokens), 0) AS input_tokens,
                COALESCE(SUM(output_tokens), 0) AS output_tokens,
                COALESCE(SUM(cache_read_tokens), 0) AS cache_read_tokens,
                COALESCE(SUM(cache_creation_tokens), 0) AS cache_creation_tokens,
                COALESCE(SUM(total_tokens), 0) AS total_tokens,
                COALESCE(SUM(duration_seconds), 0.0) AS total_duration_seconds
            FROM sessions
            WHERE started_at > '2020-01-01'
            GROUP BY strftime('%Y-%m', started_at), adapter
            ORDER BY period DESC, total_tokens DESC;
            "#,
        )?;

        Ok(())
    }

    /// Checks if a file source has already been indexed with the exact same fingerprint, size, and mtime.
    pub fn should_scan_source(&self, source: &SessionSource) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT file_size, mtime, fingerprint FROM sources WHERE source_path = ?1")?;

        let mut rows = stmt.query(params![source.path.to_string_lossy().to_string()])?;
        if let Some(row) = rows.next()? {
            let file_size: i64 = row.get(0)?;
            let mtime: i64 = row.get(1)?;
            let fingerprint: String = row.get(2)?;

            if file_size == source.file_size_bytes as i64
                && mtime == source.mtime_epoch_secs
                && fingerprint == source.fingerprint
            {
                return Ok(false); // Unchanged -> skip
            }
        }

        Ok(true) // New or modified -> needs scan
    }

    /// Upsert an indexed session into the database atomically with verdict and score.
    pub fn upsert_session(
        &self,
        trace: &AgentWorthTrace,
        primary_outcome: Option<&str>,
        composite_score: Option<f64>,
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        let scanned_at = Utc::now().to_rfc3339();
        let started_at_str = trace.started_at.to_rfc3339();
        let ended_at_str = trace.ended_at.map(|t| t.to_rfc3339());
        let models_json = serde_json::to_string(&trace.stats.models_used)?;
        let tools_json = serde_json::to_string(&trace.stats.tools_used)?;
        let metadata_json = serde_json::to_string(&trace.metadata)?;

        // 1. Upsert source
        tx.execute(
            r#"
            INSERT INTO sources (source_path, adapter, file_size, mtime, fingerprint, scanned_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(source_path) DO UPDATE SET
                file_size = excluded.file_size,
                mtime = excluded.mtime,
                fingerprint = excluded.fingerprint,
                scanned_at = excluded.scanned_at;
            "#,
            params![
                trace.provenance.source_path,
                trace.adapter,
                trace.provenance.file_size_bytes as i64,
                trace.provenance.mtime_epoch_secs,
                trace.provenance.content_fingerprint,
                scanned_at,
            ],
        )?;

        // 2. Upsert session
        tx.execute(
            r#"
            INSERT INTO sessions (
                session_id, adapter, source_path, fingerprint, started_at, ended_at,
                duration_seconds, total_events, user_messages_count, assistant_messages_count,
                tool_calls_count, input_tokens, output_tokens, cache_read_tokens,
                cache_creation_tokens, total_tokens, models_used, tools_used, metadata, scanned_at,
                primary_outcome, composite_score
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)
            ON CONFLICT(session_id) DO UPDATE SET
                adapter = excluded.adapter,
                source_path = excluded.source_path,
                fingerprint = excluded.fingerprint,
                started_at = excluded.started_at,
                ended_at = excluded.ended_at,
                duration_seconds = excluded.duration_seconds,
                total_events = excluded.total_events,
                user_messages_count = excluded.user_messages_count,
                assistant_messages_count = excluded.assistant_messages_count,
                tool_calls_count = excluded.tool_calls_count,
                input_tokens = excluded.input_tokens,
                output_tokens = excluded.output_tokens,
                cache_read_tokens = excluded.cache_read_tokens,
                cache_creation_tokens = excluded.cache_creation_tokens,
                total_tokens = excluded.total_tokens,
                models_used = excluded.models_used,
                tools_used = excluded.tools_used,
                metadata = excluded.metadata,
                scanned_at = excluded.scanned_at,
                primary_outcome = excluded.primary_outcome,
                composite_score = excluded.composite_score;
            "#,
            params![
                trace.session_id,
                trace.adapter,
                trace.provenance.source_path,
                trace.provenance.content_fingerprint,
                started_at_str,
                ended_at_str,
                trace.stats.duration_seconds,
                trace.stats.total_events as i64,
                trace.stats.user_messages_count as i64,
                trace.stats.assistant_messages_count as i64,
                trace.stats.tool_calls_count as i64,
                trace.stats.token_usage.input_tokens as i64,
                trace.stats.token_usage.output_tokens as i64,
                trace.stats.token_usage.cache_read_tokens as i64,
                trace.stats.token_usage.cache_creation_tokens as i64,
                trace.stats.token_usage.total() as i64,
                models_json,
                tools_json,
                metadata_json,
                scanned_at,
                primary_outcome,
                composite_score,
            ],
        )?;

        tx.commit()?;
        Ok(())
    }

    /// Upsert an indexed trace into the database atomically.
    pub fn upsert_trace(&self, trace: &AgentWorthTrace) -> Result<()> {
        self.upsert_session(trace, None, None)
    }

    /// Retrieve summary statistics across the whole indexed database.
    pub fn get_aggregate_stats(&self) -> Result<AggregateStats> {
        let conn = self.conn.lock().unwrap();

        let mut stats = AggregateStats::default();

        let mut stmt = conn.prepare(
            r#"
            SELECT
                COUNT(*),
                COALESCE(SUM(total_events), 0),
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(cache_read_tokens), 0),
                COALESCE(SUM(cache_creation_tokens), 0),
                MIN(CASE WHEN started_at > '2020-01-01' THEN started_at ELSE NULL END),
                MAX(started_at),
                COALESCE(SUM(CASE WHEN primary_outcome IN ('CiOrDeploymentVerified', 'CommitObserved', 'TestOrBuildPassed') THEN 1 ELSE 0 END), 0)
            FROM sessions
            "#,
        )?;

        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            stats.total_sessions = row.get(0)?;
            stats.total_events = row.get(1)?;
            let input: i64 = row.get(2)?;
            let output: i64 = row.get(3)?;
            let cache_read: i64 = row.get(4)?;
            let cache_creation: i64 = row.get(5)?;

            let min_started: Option<String> = row.get(6)?;
            let max_started: Option<String> = row.get(7)?;
            let verified_count: i64 = row.get(8)?;

            stats.verified_outcomes_count = verified_count as usize;

            stats.token_usage = TokenUsage::new(
                input as u64,
                output as u64,
                cache_read as u64,
                cache_creation as u64,
            );

            stats.first_session_at = min_started.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            });
            stats.last_session_at = max_started.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            });
        }

        // Sessions count by adapter
        let mut adapter_stmt = conn.prepare(
            "SELECT adapter, COUNT(*) FROM sessions GROUP BY adapter ORDER BY COUNT(*) DESC",
        )?;
        let mut adapter_rows = adapter_stmt.query([])?;
        while let Some(row) = adapter_rows.next()? {
            let adapter: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            stats.sessions_by_adapter.insert(adapter, count as usize);
        }

        // Aggregate models and tools
        let mut items_stmt = conn.prepare("SELECT models_used, tools_used FROM sessions")?;
        let mut items_rows = items_stmt.query([])?;
        while let Some(row) = items_rows.next()? {
            let models_str: String = row.get(0)?;
            let tools_str: String = row.get(1)?;

            if let Ok(models) = serde_json::from_str::<Vec<String>>(&models_str) {
                for m in models {
                    *stats.models_usage_count.entry(m).or_insert(0) += 1;
                }
            }
            if let Ok(tools) = serde_json::from_str::<BTreeMap<String, usize>>(&tools_str) {
                for (t, count) in tools {
                    *stats.tools_usage_count.entry(t).or_insert(0) += count;
                }
            }
        }

        Ok(stats)
    }

    /// Group indexed sessions by their primary outcome with session counts, token volume, and estimated cost.
    pub fn get_outcome_distribution(&self) -> Result<Vec<(String, usize, u64, f64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT
                COALESCE(primary_outcome, 'Unresolved') AS outcome,
                COUNT(*) AS session_count,
                COALESCE(SUM(total_tokens), 0) AS total_tokens,
                COALESCE(SUM(input_tokens), 0) AS input_tokens,
                COALESCE(SUM(output_tokens), 0) AS output_tokens,
                COALESCE(SUM(cache_read_tokens), 0) AS cache_read_tokens,
                COALESCE(SUM(cache_creation_tokens), 0) AS cache_creation_tokens
            FROM sessions
            GROUP BY COALESCE(primary_outcome, 'Unresolved')
            ORDER BY session_count DESC, total_tokens DESC
            "#,
        )?;

        let mut rows = stmt.query([])?;
        let mut results = Vec::new();

        while let Some(row) = rows.next()? {
            let outcome: String = row.get(0)?;
            let session_count: i64 = row.get(1)?;
            let total_tokens: i64 = row.get(2)?;
            let input: i64 = row.get(3)?;
            let output: i64 = row.get(4)?;
            let cache_read: i64 = row.get(5)?;
            let cache_creation: i64 = row.get(6)?;

            let total_cost = estimate_tokens_cost_usd(
                input as u64,
                output as u64,
                cache_read as u64,
                cache_creation as u64,
            );

            results.push((
                outcome,
                session_count as usize,
                total_tokens as u64,
                total_cost,
            ));
        }

        Ok(results)
    }

    /// Retrieve a single session summary by its unique session ID.
    pub fn get_session_by_id(&self, session_id: &str) -> Result<Option<SessionSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT session_id, adapter, source_path, started_at, duration_seconds,
                   total_tokens, total_events, tool_calls_count, models_used,
                   primary_outcome, composite_score
            FROM sessions
            WHERE session_id = ?1
            "#,
        )?;

        let mut rows = stmt.query(params![session_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_session_summary(row)?))
        } else {
            Ok(None)
        }
    }

    /// List recent sessions with a limit.
    pub fn list_sessions(&self, limit: usize) -> Result<Vec<SessionSummary>> {
        self.list_sessions_filtered(&SessionFilter {
            limit: Some(limit),
            ..Default::default()
        })
    }

    /// Query sessions with rich filtering, ordering, and pagination.
    pub fn list_sessions_filtered(&self, filter: &SessionFilter) -> Result<Vec<SessionSummary>> {
        let conn = self.conn.lock().unwrap();

        let mut sql = String::from(
            r#"
            SELECT session_id, adapter, source_path, started_at, duration_seconds,
                   total_tokens, total_events, tool_calls_count, models_used,
                   primary_outcome, composite_score
            FROM sessions
            WHERE 1=1
            "#,
        );

        let mut param_values: Vec<Box<dyn ToSql>> = Vec::new();

        if !filter.include_stubs.unwrap_or(false) {
            sql.push_str(" AND (total_events > 1 AND total_tokens > 0)");
        }

        if let Some(ref adapter) = filter.adapter {
            sql.push_str(" AND adapter = ?");
            param_values.push(Box::new(adapter.clone()));
        }

        if let Some(ref model) = filter.model {
            sql.push_str(" AND models_used LIKE ?");
            param_values.push(Box::new(format!("%{}%", model)));
        }

        if let Some(ref outcome) = filter.outcome {
            sql.push_str(" AND primary_outcome = ?");
            param_values.push(Box::new(outcome.clone()));
        }

        if let Some(ref search) = filter.search {
            if !search.trim().is_empty() {
                let pattern = format!("%{}%", search.trim());
                sql.push_str(" AND (session_id LIKE ? OR source_path LIKE ? OR models_used LIKE ? OR adapter LIKE ?)");
                param_values.push(Box::new(pattern.clone()));
                param_values.push(Box::new(pattern.clone()));
                param_values.push(Box::new(pattern.clone()));
                param_values.push(Box::new(pattern.clone()));
            }
        }

        if let Some(start_date) = filter.start_date {
            sql.push_str(" AND started_at >= ?");
            param_values.push(Box::new(start_date.to_rfc3339()));
        }

        if let Some(end_date) = filter.end_date {
            sql.push_str(" AND started_at <= ?");
            param_values.push(Box::new(end_date.to_rfc3339()));
        }

        if let Some(min_tokens) = filter.min_tokens {
            sql.push_str(" AND total_tokens >= ?");
            param_values.push(Box::new(min_tokens as i64));
        }

        let order_by = filter.order_by.unwrap_or(SessionOrderBy::StartedAtDesc);
        match order_by {
            SessionOrderBy::StartedAtDesc => sql.push_str(" ORDER BY started_at DESC"),
            SessionOrderBy::StartedAtAsc => sql.push_str(" ORDER BY started_at ASC"),
            SessionOrderBy::TokensDesc => sql.push_str(" ORDER BY total_tokens DESC"),
            SessionOrderBy::TokensAsc => sql.push_str(" ORDER BY total_tokens ASC"),
            SessionOrderBy::EventsDesc => sql.push_str(" ORDER BY total_events DESC"),
            SessionOrderBy::EventsAsc => sql.push_str(" ORDER BY total_events ASC"),
            SessionOrderBy::DurationDesc => sql.push_str(" ORDER BY duration_seconds DESC"),
            SessionOrderBy::ScoreDesc => sql.push_str(" ORDER BY composite_score DESC"),
            SessionOrderBy::ScoreAsc => sql.push_str(" ORDER BY composite_score ASC"),
        }

        let limit = filter.limit.unwrap_or(50);
        sql.push_str(" LIMIT ?");
        param_values.push(Box::new(limit as i64));

        if let Some(offset) = filter.offset {
            sql.push_str(" OFFSET ?");
            param_values.push(Box::new(offset as i64));
        }

        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn ToSql> = param_values.iter().map(|b| b.as_ref()).collect();
        let mut rows = stmt.query(params_refs.as_slice())?;
        let mut results = Vec::new();

        while let Some(row) = rows.next()? {
            results.push(row_to_session_summary(row)?);
        }

        Ok(results)
    }

    /// Extract and rank top repository / workspace paths from indexed sessions.
    pub fn get_top_repositories(&self) -> Result<Vec<(String, usize)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT source_path FROM sessions")?;
        let mut rows = stmt.query([])?;

        let mut repo_counts: BTreeMap<String, usize> = BTreeMap::new();

        while let Some(row) = rows.next()? {
            let source_path: String = row.get(0)?;
            let repo = extract_repository_or_workspace(&source_path);
            *repo_counts.entry(repo).or_insert(0) += 1;
        }

        let mut ranked: Vec<(String, usize)> = repo_counts.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(ranked)
    }

    /// Retrieve daily usage summaries grouped by day and adapter.
    pub fn get_daily_usage(&self, limit: Option<usize>) -> Result<Vec<UsagePeriodSummary>> {
        self.query_usage_view("v_daily_usage", limit.unwrap_or(30))
    }

    /// Retrieve weekly usage summaries grouped by week and adapter.
    pub fn get_weekly_usage(&self, limit: Option<usize>) -> Result<Vec<UsagePeriodSummary>> {
        self.query_usage_view("v_weekly_usage", limit.unwrap_or(20))
    }

    /// Retrieve monthly usage summaries grouped by month and adapter.
    pub fn get_monthly_usage(&self, limit: Option<usize>) -> Result<Vec<UsagePeriodSummary>> {
        self.query_usage_view("v_monthly_usage", limit.unwrap_or(12))
    }

    fn query_usage_view(&self, view_name: &str, limit: usize) -> Result<Vec<UsagePeriodSummary>> {
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT period, adapter, session_count, total_events, input_tokens, output_tokens, \
             cache_read_tokens, cache_creation_tokens, total_tokens, total_duration_seconds \
             FROM {} LIMIT ?1",
            view_name
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params![limit as i64])?;
        let mut results = Vec::new();

        while let Some(row) = rows.next()? {
            let period: String = row.get(0)?;
            let adapter: String = row.get(1)?;
            let session_count: i64 = row.get(2)?;
            let total_events: i64 = row.get(3)?;
            let input: i64 = row.get(4)?;
            let output: i64 = row.get(5)?;
            let cache_read: i64 = row.get(6)?;
            let cache_creation: i64 = row.get(7)?;
            let total: i64 = row.get(8)?;
            let duration: f64 = row.get(9)?;

            let estimated_cost_usd = estimate_tokens_cost_usd(
                input as u64,
                output as u64,
                cache_read as u64,
                cache_creation as u64,
            );
            let cache_hit_ratio =
                calculate_cache_hit_ratio(input as u64, cache_read as u64, cache_creation as u64);

            results.push(UsagePeriodSummary {
                period,
                adapter,
                session_count: session_count as usize,
                total_events: total_events as usize,
                input_tokens: input as u64,
                output_tokens: output as u64,
                cache_read_tokens: cache_read as u64,
                cache_creation_tokens: cache_creation as u64,
                total_tokens: total as u64,
                total_duration_seconds: duration,
                estimated_cost_usd,
                cache_hit_ratio,
            });
        }

        Ok(results)
    }

    /// Calculate rolling pacing summary for the last N hours.
    pub fn get_pacing_window(&self, hours: i64) -> Result<PacingSummary> {
        let conn = self.conn.lock().unwrap();

        // Get max date in DB as anchor if current real-time clock has no recent sessions
        let mut max_stmt = conn.prepare("SELECT MAX(started_at) FROM sessions")?;
        let max_str: Option<String> = max_stmt.query_row([], |r| r.get(0)).ok();
        let anchor_time = max_str
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc)))
            .unwrap_or_else(Utc::now);

        let window_start = anchor_time - chrono::Duration::hours(hours);
        let start_str = window_start.to_rfc3339();

        let mut stmt = conn.prepare(
            r#"
            SELECT
                COUNT(*),
                COALESCE(SUM(total_events), 0),
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(cache_read_tokens), 0),
                COALESCE(SUM(cache_creation_tokens), 0),
                COALESCE(SUM(total_tokens), 0)
            FROM sessions
            WHERE started_at >= ?1
            "#,
        )?;

        let mut rows = stmt.query(params![start_str])?;
        let (session_count, total_events, input, output, cache_read, cache_creation, total) =
            if let Some(row) = rows.next()? {
                (
                    row.get::<_, i64>(0)? as usize,
                    row.get::<_, i64>(1)? as usize,
                    row.get::<_, i64>(2)? as u64,
                    row.get::<_, i64>(3)? as u64,
                    row.get::<_, i64>(4)? as u64,
                    row.get::<_, i64>(5)? as u64,
                    row.get::<_, i64>(6)? as u64,
                )
            } else {
                (0, 0, 0, 0, 0, 0, 0)
            };

        // Query active adapters in this window
        let mut ad_stmt = conn.prepare(
            "SELECT DISTINCT adapter FROM sessions WHERE started_at >= ?1 ORDER BY adapter",
        )?;
        let mut ad_rows = ad_stmt.query(params![start_str])?;
        let mut active_adapters = Vec::new();
        while let Some(r) = ad_rows.next()? {
            active_adapters.push(r.get(0)?);
        }

        let mut mod_stmt =
            conn.prepare("SELECT models_used FROM sessions WHERE started_at >= ?1")?;
        let mut mod_rows = mod_stmt.query(params![start_str])?;
        let mut model_set = std::collections::BTreeSet::new();
        while let Some(r) = mod_rows.next()? {
            let m_str: String = r.get(0)?;
            if let Ok(models) = serde_json::from_str::<Vec<String>>(&m_str) {
                for m in models {
                    model_set.insert(m);
                }
            }
        }

        let burn_rate_tokens_per_hour = if hours > 0 {
            total as f64 / hours as f64
        } else {
            0.0
        };

        let estimated_cost_usd =
            estimate_tokens_cost_usd(input, output, cache_read, cache_creation);
        let cache_hit_ratio = calculate_cache_hit_ratio(input, cache_read, cache_creation);

        Ok(PacingSummary {
            window_hours: hours,
            started_at: window_start,
            ended_at: anchor_time,
            session_count,
            total_events,
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_creation_tokens: cache_creation,
            total_tokens: total,
            burn_rate_tokens_per_hour,
            estimated_cost_usd,
            cache_hit_ratio,
            active_adapters,
            active_models: model_set.into_iter().collect(),
        })
    }

    /// Search sessions that modified or touched a specific target file path for AI Code Blame.
    pub fn find_sessions_for_blame(&self, file_path_pattern: &str) -> Result<Vec<BlameMatch>> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("%{}%", file_path_pattern);

        let mut stmt = conn.prepare(
            r#"
            SELECT session_id, adapter, source_path, started_at, models_used, total_tokens, tool_calls_count
            FROM sessions
            WHERE source_path LIKE ?1 OR metadata LIKE ?1 OR tools_used LIKE ?1
            ORDER BY started_at DESC
            LIMIT 25
            "#,
        )?;

        let mut rows = stmt.query(params![pattern])?;
        let mut results = Vec::new();

        while let Some(row) = rows.next()? {
            let session_id: String = row.get(0)?;
            let adapter: String = row.get(1)?;
            let source_path: String = row.get(2)?;
            let started_str: String = row.get(3)?;
            let models_str: String = row.get(4)?;
            let total_tokens: i64 = row.get(5)?;
            let tool_calls_count: i64 = row.get(6)?;

            let started_at = DateTime::parse_from_rfc3339(&started_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            let models_used = serde_json::from_str::<Vec<String>>(&models_str).unwrap_or_default();

            results.push(BlameMatch {
                session_id,
                adapter,
                source_path,
                started_at,
                models_used,
                total_tokens: total_tokens as u64,
                tool_calls_count: tool_calls_count as usize,
            });
        }

        Ok(results)
    }
}

/// Estimate developer token cost in USD based on standard blended pricing.
pub fn estimate_tokens_cost_usd(
    input: u64,
    output: u64,
    cache_read: u64,
    cache_creation: u64,
) -> f64 {
    (input as f64 * 3.0 / 1_000_000.0)
        + (output as f64 * 15.0 / 1_000_000.0)
        + (cache_read as f64 * 0.30 / 1_000_000.0)
        + (cache_creation as f64 * 3.75 / 1_000_000.0)
}

/// Calculate prompt cache hit percentage.
pub fn calculate_cache_hit_ratio(input: u64, cache_read: u64, cache_creation: u64) -> f64 {
    let total_input = input + cache_read + cache_creation;
    if total_input == 0 {
        0.0
    } else {
        (cache_read as f64 / total_input as f64) * 100.0
    }
}

fn row_to_session_summary(row: &rusqlite::Row) -> Result<SessionSummary> {
    let session_id: String = row.get(0)?;
    let adapter: String = row.get(1)?;
    let source_path: String = row.get(2)?;
    let started_str: String = row.get(3)?;
    let duration_seconds: Option<f64> = row.get(4)?;
    let total_tokens: i64 = row.get(5)?;
    let total_events: i64 = row.get(6)?;
    let tool_calls_count: i64 = row.get(7)?;
    let models_str: String = row.get(8)?;
    let primary_outcome: Option<String> = row.get(9).ok();
    let composite_score: Option<f64> = row.get(10).ok();

    let started_at = DateTime::parse_from_rfc3339(&started_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    let models_used = serde_json::from_str::<Vec<String>>(&models_str).unwrap_or_default();

    Ok(SessionSummary {
        session_id,
        adapter,
        source_path,
        started_at,
        duration_seconds,
        total_tokens: total_tokens as u64,
        total_events: total_events as usize,
        tool_calls_count: tool_calls_count as usize,
        models_used,
        primary_outcome,
        composite_score,
    })
}

/// Extract repository or workspace name/path from a session source path.
pub fn extract_repository_or_workspace(source_path: &str) -> String {
    let path = Path::new(source_path);
    let path_str = source_path.replace('\\', "/");

    // Skip plugin/package internal cache artifacts
    if path_str.contains("/plugins/cache/")
        || path_str.contains("/node_modules/")
        || path_str.contains("/.bun/")
    {
        return "plugins/cache".to_string();
    }

    // 1. Check if path has a Claude Code project slug format:
    // e.g. ~/.claude/projects/-Users-saurabh-code-unfoundbox-agentworth/uuid.jsonl
    // e.g. ~/.claude/projects/-Users-saurabh-code-motionvector-pluto--claude-worktrees-repo-branches-inventory-108a70/...
    if let Some(idx) = path_str.find("/projects/-") {
        let after = &path_str[idx + "/projects/-".len()..];
        let full_slug = after.split('/').next().unwrap_or(after);
        // Prune worktree / sub-branch suffix starting at '--'
        let base_slug = full_slug.split("--").next().unwrap_or(full_slug);
        
        // Decode -Users-saurabh-code-foo-bar -> /Users/saurabh/code/foo/bar
        let decoded = format!("/{}", base_slug.replace('-', "/"));
        let parts: Vec<&str> = decoded.split('/').filter(|s| !s.is_empty()).collect();
        if parts.len() >= 2 {
            return format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1]);
        } else if let Some(last) = parts.last() {
            return last.to_string();
        }
    }

    // 2. Check if path directly contains a code/ or projects/ path
    if let Some(idx) = path_str.find("/code/") {
        let after = &path_str[idx + "/code/".len()..];
        let parts: Vec<&str> = after.split('/').filter(|s| !s.is_empty()).collect();
        if parts.len() >= 2 {
            return format!("{}/{}", parts[0], parts[1]);
        } else if let Some(first) = parts.first() {
            return first.to_string();
        }
    }

    // 3. Check for hidden directory boundary (.claude, .cursor, .agentworth, .git, .gemini)
    let components: Vec<&str> = path_str.split('/').filter(|s| !s.is_empty()).collect();
    for (i, comp) in components.iter().enumerate() {
        if comp.starts_with('.') && i > 0 {
            let parent = components[i - 1];
            if i >= 2 {
                return format!("{}/{}", components[i - 2], parent);
            }
            return parent.to_string();
        }
    }

    // 4. Fallback: parent folder name or relative repo path
    if let Some(parent) = path.parent() {
        let parent_str = parent.to_string_lossy();
        let comps: Vec<&str> = parent_str.split('/').filter(|s| !s.is_empty()).collect();
        if comps.len() >= 2 {
            return format!("{}/{}", comps[comps.len() - 2], comps[comps.len() - 1]);
        } else if let Some(last) = comps.last() {
            return last.to_string();
        }
    }

    "unknown".to_string()
}

pub fn default_db_dir() -> Result<PathBuf> {
    if let Some(base_dirs) = BaseDirs::new() {
        Ok(base_dirs.home_dir().join(".agentworth"))
    } else {
        Ok(PathBuf::from(".agentworth"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::Provenance;
    use chrono::Duration;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_storage_in_memory_crud_and_incremental() {
        let storage = Storage::open_in_memory().expect("open storage");

        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(b"sample file").unwrap();

        let source = SessionSource::from_path(temp.path(), "claude_code").unwrap();

        // 1. Initially should scan
        assert!(storage.should_scan_source(&source).unwrap());

        // 2. Insert trace
        let prov = Provenance::new(
            source.path.to_string_lossy().to_string(),
            "claude_code",
            source.file_size_bytes,
            source.mtime_epoch_secs,
            &source.fingerprint,
        );
        let start_time = Utc::now();
        let mut trace = AgentWorthTrace::new("sess_100", "claude_code", prov, start_time);
        trace.stats.token_usage = TokenUsage::new(500, 100, 50, 10);
        trace.stats.total_events = 5;
        trace.stats.models_used = vec!["claude-3-5-sonnet".to_string()];
        trace.stats.tools_used.insert("Bash".to_string(), 2);

        storage.upsert_trace(&trace).expect("upsert trace");

        // 3. Rescan check -> now should NOT scan (already indexed with same fingerprint)
        assert!(!storage.should_scan_source(&source).unwrap());

        // 4. Check aggregate stats
        let stats = storage.get_aggregate_stats().expect("get stats");
        assert_eq!(stats.total_sessions, 1);
        assert_eq!(stats.total_events, 5);
        assert_eq!(stats.token_usage.total(), 660);
        assert_eq!(stats.sessions_by_adapter.get("claude_code"), Some(&1));
        assert_eq!(stats.models_usage_count.get("claude-3-5-sonnet"), Some(&1));
        assert_eq!(stats.tools_usage_count.get("Bash"), Some(&2));
        assert!(stats.first_session_at.is_some());
        assert!(stats.last_session_at.is_some());

        // 5. Get session by ID
        let sess = storage.get_session_by_id("sess_100").expect("get by id");
        assert!(sess.is_some());
        let s = sess.unwrap();
        assert_eq!(s.session_id, "sess_100");
        assert_eq!(s.adapter, "claude_code");
        assert_eq!(s.total_tokens, 660);
        assert_eq!(s.models_used, vec!["claude-3-5-sonnet"]);

        // 6. Get non-existent session
        let not_found = storage.get_session_by_id("sess_none").expect("not found");
        assert!(not_found.is_none());
    }

    #[test]
    fn test_list_sessions_filtered_and_sorting() {
        let storage = Storage::open_in_memory().expect("open storage");
        let base_time = Utc::now() - Duration::hours(5);

        // Insert 3 sessions
        for i in 1..=3 {
            let prov = Provenance::new(
                format!("/Users/dev/code/org/repo{}/.claude/s{}.jsonl", i, i),
                if i == 3 { "codex" } else { "claude_code" },
                100,
                100,
                format!("fp{}", i),
            );
            let mut trace = AgentWorthTrace::new(
                format!("sess_{}", i),
                if i == 3 { "codex" } else { "claude_code" },
                prov,
                base_time + Duration::hours(i as i64),
            );
            trace.stats.token_usage = TokenUsage::new(i as u64 * 1000, 0, 0, 0);
            trace.stats.total_events = i * 10;
            trace.stats.duration_seconds = Some(i as f64 * 60.0);
            trace.stats.models_used = if i == 1 {
                vec!["claude-3-5-sonnet".to_string()]
            } else if i == 2 {
                vec!["claude-3-opus".to_string()]
            } else {
                vec!["gpt-4o".to_string()]
            };
            storage.upsert_trace(&trace).expect("upsert");
        }

        // Test 1: Filter by adapter
        let claude_sessions = storage
            .list_sessions_filtered(&SessionFilter {
                adapter: Some("claude_code".to_string()),
                ..Default::default()
            })
            .expect("filter adapter");
        assert_eq!(claude_sessions.len(), 2);

        // Test 2: Filter by model
        let opus_sessions = storage
            .list_sessions_filtered(&SessionFilter {
                model: Some("opus".to_string()),
                ..Default::default()
            })
            .expect("filter model");
        assert_eq!(opus_sessions.len(), 1);
        assert_eq!(opus_sessions[0].session_id, "sess_2");

        // Test 3: Filter by min tokens
        let heavy_sessions = storage
            .list_sessions_filtered(&SessionFilter {
                min_tokens: Some(2500),
                ..Default::default()
            })
            .expect("filter min tokens");
        assert_eq!(heavy_sessions.len(), 1);
        assert_eq!(heavy_sessions[0].session_id, "sess_3");

        // Test 4: Ordering by tokens descending
        let ordered_tokens = storage
            .list_sessions_filtered(&SessionFilter {
                order_by: Some(SessionOrderBy::TokensDesc),
                ..Default::default()
            })
            .expect("order tokens desc");
        assert_eq!(ordered_tokens[0].session_id, "sess_3");
        assert_eq!(ordered_tokens[2].session_id, "sess_1");

        // Test 5: Pagination (limit & offset)
        let page1 = storage
            .list_sessions_filtered(&SessionFilter {
                limit: Some(2),
                offset: Some(0),
                order_by: Some(SessionOrderBy::StartedAtDesc),
                ..Default::default()
            })
            .expect("page1");
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].session_id, "sess_3");
        assert_eq!(page1[1].session_id, "sess_2");

        let page2 = storage
            .list_sessions_filtered(&SessionFilter {
                limit: Some(2),
                offset: Some(2),
                order_by: Some(SessionOrderBy::StartedAtDesc),
                ..Default::default()
            })
            .expect("page2");
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].session_id, "sess_1");

        // Test 6: Search keyword matching session ID or path
        let searched = storage
            .list_sessions_filtered(&SessionFilter {
                search: Some("sess_2".to_string()),
                ..Default::default()
            })
            .expect("search");
        assert_eq!(searched.len(), 1);
        assert_eq!(searched[0].session_id, "sess_2");
    }

    #[test]
    fn test_top_repositories_extraction() {
        let storage = Storage::open_in_memory().expect("open storage");

        let paths = [
            "/Users/saurabh/.claude/projects/-Users-saurabh-code-unfoundbox-agentworth/uuid1.jsonl",
            "/Users/saurabh/.claude/projects/-Users-saurabh-code-unfoundbox-agentworth/uuid2.jsonl",
            "/Users/saurabh/.claude/projects/-Users-saurabh-code-motionvector-fleet/uuid3.jsonl",
            "/Users/saurabh/code/standalone-repo/.claude/session.jsonl",
        ];

        for (i, p) in paths.iter().enumerate() {
            let prov = Provenance::new(*p, "claude_code", 100, 100, format!("fp{}", i));
            let trace =
                AgentWorthTrace::new(format!("sess_{}", i), "claude_code", prov, Utc::now());
            storage.upsert_trace(&trace).expect("upsert");
        }

        let repos = storage.get_top_repositories().expect("top repos");
        assert!(!repos.is_empty());
        assert_eq!(repos[0].0, "unfoundbox/agentworth");
        assert_eq!(repos[0].1, 2);
    }

    #[test]
    fn test_usage_views_daily_weekly_monthly() {
        let storage = Storage::open_in_memory().expect("open storage");
        let base_date = DateTime::parse_from_rfc3339("2026-08-30T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        // Insert sessions across 2 days
        for day_offset in 0..2 {
            let prov = Provenance::new(
                format!("/path/to/day{}/session.jsonl", day_offset),
                "claude_code",
                100,
                100,
                format!("fp_day{}", day_offset),
            );
            let mut trace = AgentWorthTrace::new(
                format!("sess_day_{}", day_offset),
                "claude_code",
                prov,
                base_date - Duration::days(day_offset),
            );
            trace.stats.token_usage = TokenUsage::new(1000, 200, 5000, 500);
            trace.stats.total_events = 15;
            trace.stats.duration_seconds = Some(120.0);
            storage.upsert_trace(&trace).expect("upsert trace");
        }

        let daily = storage.get_daily_usage(Some(10)).expect("daily usage");
        assert_eq!(daily.len(), 2);
        assert_eq!(daily[0].session_count, 1);
        assert_eq!(daily[0].input_tokens, 1000);
        assert_eq!(daily[0].cache_read_tokens, 5000);
        assert!(daily[0].estimated_cost_usd > 0.0);
        assert!(daily[0].cache_hit_ratio > 0.0);

        let weekly = storage.get_weekly_usage(Some(10)).expect("weekly usage");
        assert!(!weekly.is_empty());

        let monthly = storage.get_monthly_usage(Some(10)).expect("monthly usage");
        assert!(!monthly.is_empty());
    }

    #[test]
    fn test_pacing_window_calculation() {
        let storage = Storage::open_in_memory().expect("open storage");
        let now = DateTime::parse_from_rfc3339("2026-08-30T18:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        // Insert 1 session 1 hour ago, and 1 session 10 hours ago
        let prov1 = Provenance::new("/path/s1.jsonl", "claude_code", 100, 100, "fp1");
        let mut trace1 =
            AgentWorthTrace::new("sess_recent", "claude_code", prov1, now - Duration::hours(1));
        trace1.stats.token_usage = TokenUsage::new(5000, 1000, 20000, 0);
        trace1.stats.models_used = vec!["claude-3-5-sonnet".to_string()];
        storage.upsert_trace(&trace1).expect("upsert 1");

        let prov2 = Provenance::new("/path/s2.jsonl", "codex", 100, 100, "fp2");
        let mut trace2 =
            AgentWorthTrace::new("sess_old", "codex", prov2, now - Duration::hours(10));
        trace2.stats.token_usage = TokenUsage::new(10000, 2000, 0, 0);
        trace2.stats.models_used = vec!["gpt-4o".to_string()];
        storage.upsert_trace(&trace2).expect("upsert 2");

        // 5-hour pacing window should only include sess_recent
        let pacing = storage.get_pacing_window(5).expect("pacing");
        assert_eq!(pacing.session_count, 1);
        assert_eq!(pacing.total_tokens, 26000);
        assert_eq!(pacing.active_adapters, vec!["claude_code"]);
        assert_eq!(pacing.active_models, vec!["claude-3-5-sonnet"]);
        assert!(pacing.burn_rate_tokens_per_hour > 0.0);
    }

    #[test]
    fn test_find_sessions_for_blame() {
        let storage = Storage::open_in_memory().expect("open storage");

        let prov = Provenance::new(
            "/Users/dev/code/motionvector/src/engine.rs.jsonl",
            "claude_code",
            100,
            100,
            "fp_blame",
        );
        let mut trace = AgentWorthTrace::new("sess_blame_1", "claude_code", prov, Utc::now());
        trace.stats.models_used = vec!["claude-3-5-sonnet".to_string()];
        trace.stats.tools_used.insert("Edit".to_string(), 3);
        storage.upsert_trace(&trace).expect("upsert");

        let matches = storage
            .find_sessions_for_blame("engine.rs")
            .expect("blame matches");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].session_id, "sess_blame_1");
        assert_eq!(matches[0].adapter, "claude_code");
    }

    #[test]
    fn test_primary_outcome_and_composite_score_persistence() {
        let storage = Storage::open_in_memory().expect("open storage");
        let prov = Provenance::new("/path/to/log.jsonl", "claude_code", 100, 100, "fp_test");
        let mut trace = AgentWorthTrace::new("sess_verdict_1", "claude_code", prov, Utc::now());
        trace.stats.total_events = 10;
        trace.stats.token_usage = TokenUsage::new(1000, 200, 0, 0);

        storage
            .upsert_session(&trace, Some("CommitObserved"), Some(0.88))
            .expect("upsert session");

        let loaded = storage
            .get_session_by_id("sess_verdict_1")
            .expect("get session")
            .expect("found");
        assert_eq!(loaded.primary_outcome.as_deref(), Some("CommitObserved"));
        assert_eq!(loaded.composite_score, Some(0.88));

        let list = storage
            .list_sessions_filtered(&SessionFilter {
                outcome: Some("CommitObserved".to_string()),
                ..Default::default()
            })
            .expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].primary_outcome.as_deref(), Some("CommitObserved"));
        assert_eq!(list[0].composite_score, Some(0.88));
    }

    #[test]
    fn test_verified_outcomes_count_and_distribution() {
        let storage = Storage::open_in_memory().expect("open storage");

        let specs = [
            ("s1", Some("CiOrDeploymentVerified"), 0.98, 1000u64),
            ("s2", Some("CommitObserved"), 0.85, 2000u64),
            ("s3", Some("TestOrBuildPassed"), 0.75, 3000u64),
            ("s4", Some("ArtifactChanged"), 0.50, 4000u64),
            ("s5", Some("DoneClaimed"), 0.20, 5000u64),
            ("s6", None, 0.0, 6000u64),
        ];

        for (id, outcome, score, tokens) in specs {
            let prov = Provenance::new(format!("/path/{}.jsonl", id), "claude_code", 100, 100, format!("fp_{}", id));
            let mut trace = AgentWorthTrace::new(id, "claude_code", prov, Utc::now());
            trace.stats.total_events = 5;
            trace.stats.token_usage = TokenUsage::new(tokens, 100, 0, 0);
            storage
                .upsert_session(&trace, outcome, Some(score))
                .expect("upsert");
        }

        let stats = storage.get_aggregate_stats().expect("aggregate stats");
        assert_eq!(stats.total_sessions, 6);
        // Verified count should only include: CiOrDeploymentVerified, CommitObserved, TestOrBuildPassed
        assert_eq!(stats.verified_outcomes_count, 3);

        let dist = storage.get_outcome_distribution().expect("distribution");
        assert_eq!(dist.len(), 6);
        let dist_map: BTreeMap<String, usize> = dist.into_iter().map(|(outcome, count, _, _)| (outcome, count)).collect();
        assert_eq!(dist_map.get("CiOrDeploymentVerified"), Some(&1));
        assert_eq!(dist_map.get("CommitObserved"), Some(&1));
        assert_eq!(dist_map.get("TestOrBuildPassed"), Some(&1));
        assert_eq!(dist_map.get("ArtifactChanged"), Some(&1));
        assert_eq!(dist_map.get("DoneClaimed"), Some(&1));
        assert_eq!(dist_map.get("Unresolved"), Some(&1));
    }

    #[test]
    fn test_stubs_filtering_by_default() {
        let storage = Storage::open_in_memory().expect("open storage");

        // 1. Stub: total_events = 1
        let prov1 = Provenance::new("/path/stub1.jsonl", "claude_code", 100, 100, "fp_stub1");
        let mut stub1 = AgentWorthTrace::new("stub_1", "claude_code", prov1, Utc::now());
        stub1.stats.total_events = 1;
        stub1.stats.token_usage = TokenUsage::new(100, 0, 0, 0);
        storage.upsert_trace(&stub1).expect("upsert stub 1");

        // 2. Stub: 0 tokens
        let prov2 = Provenance::new("/path/stub2.jsonl", "claude_code", 100, 100, "fp_stub2");
        let mut stub2 = AgentWorthTrace::new("stub_2", "claude_code", prov2, Utc::now());
        stub2.stats.total_events = 5;
        stub2.stats.token_usage = TokenUsage::new(0, 0, 0, 0);
        storage.upsert_trace(&stub2).expect("upsert stub 2");

        // 3. Real session
        let prov3 = Provenance::new("/path/real.jsonl", "claude_code", 100, 100, "fp_real");
        let mut real = AgentWorthTrace::new("real_1", "claude_code", prov3, Utc::now());
        real.stats.total_events = 10;
        real.stats.token_usage = TokenUsage::new(500, 100, 0, 0);
        storage.upsert_trace(&real).expect("upsert real");

        // Default: stubs filtered out
        let default_list = storage.list_sessions(50).expect("list default");
        assert_eq!(default_list.len(), 1);
        assert_eq!(default_list[0].session_id, "real_1");

        // Explicit include_stubs: true
        let all_list = storage
            .list_sessions_filtered(&SessionFilter {
                include_stubs: Some(true),
                ..Default::default()
            })
            .expect("list all");
        assert_eq!(all_list.len(), 3);
    }
}
