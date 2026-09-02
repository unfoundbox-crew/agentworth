mod anchoring;
pub mod chunker;
pub mod embedder;
pub mod pricing;
pub mod vector;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use agentworth_adapter_sdk::SessionSource;
use agentworth_outcomes::outcome_rank;
use agentworth_schema::{
    compaction_rounds, AgentWorthTrace, CompactionRound, EventPayload, FileActionType, OutcomeKind,
    TokenUsage,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use directories::BaseDirs;
use rusqlite::types::ToSql;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

pub use chunker::TrajectoryChunker;
pub use embedder::LocalEmbedder;
pub use pricing::{estimate_model_tokens_cost_usd, get_model_rates, ModelRates, MODEL_PRICING_TABLE};
pub use vector::{SqliteVectorStore, VectorStore};

/// High-level aggregate statistics across all scanned sessions in the SQLite index.
///
/// Every field describes the same population of sessions -- whichever one the caller chose via
/// `get_aggregate_stats`'s `include_stubs` argument. Mixing a stub-included field from one call
/// with a stub-excluded field from another (e.g. comparing `total_sessions` here against a
/// `list_sessions_filtered` count) silently produces nonsense ratios; see
/// docs/DECISION-INBOX.md's stats/stub-count-mismatch entry.
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

/// SQL predicate identifying a "stub" session for *aggregate reporting*: near-empty, no real
/// activity. Single source of truth for `list_sessions_filtered`, `get_aggregate_stats`, the
/// usage views/rollups, and the pacing window, so none of them can drift on what counts as a
/// stub the way `total_sessions` and the traces list drifted before this const existed
/// (docs/DECISION-INBOX.md, stats/stub-count-mismatch entry).
///
/// Deliberately NOT used to decide whether the scanner stores a row at all (see
/// `NEAR_EMPTY_EVENTS_SQL_PREDICATE` for that) -- excluding a session from a count and
/// deleting it from the index outright are very different stakes. A real, multi-event session
/// can legitimately carry zero recorded tokens (an adapter that doesn't capture usage, or a
/// model invocation whose response never reported one) without being any less real: it still
/// needs to be findable by `agentworth audit`/`autopsy`/`blind-spots`, which query with
/// `include_stubs: true` specifically to see this population. Only `total_tokens` (not
/// `total_events`) is allowed to make that call.
const NON_STUB_SQL_PREDICATE: &str = "total_events > 1 AND total_tokens > 0";

/// Why an unchanged source has to be reparsed anyway. See `Storage::needs_backfill`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackfillReason {
    /// The row is missing a value the current code derives from the source content
    /// (`prompt_preview`, `composite_score`) -- it was indexed before that value existed.
    MissingDerivedField,
    /// The adapter's parse output has changed since this row was written, so what is indexed
    /// is what an older, wronger parser produced from bytes that never changed.
    StaleParserVersion,
}

/// SQL predicate identifying a session as too near-empty to keep indexed at all, independent
/// of token telemetry -- 0 or 1 normalized events total. This is the scanner's own, narrower
/// bar for whether to store/prune a row (see `Storage::stub_sessions` and
/// `Scanner::run_scan`), separate from `NON_STUB_SQL_PREDICATE`'s broader aggregate-reporting
/// definition. The real-world shape this targets: a non-session file (config, cache, telemetry
/// dump) or a truly-abandoned session with at most one normalized event -- not a genuine
/// conversation that simply lacks captured usage numbers.
const NEAR_EMPTY_EVENTS_SQL_PREDICATE: &str = "total_events <= 1";

/// Rust-side mirror of `NEAR_EMPTY_EVENTS_SQL_PREDICATE`, for the scanner to apply the same
/// "too thin to index" test to an in-memory trace before it ever reaches a row in `sessions`.
/// Keep this in lockstep with the SQL string above.
pub fn is_near_empty_session(total_events: usize) -> bool {
    total_events <= 1
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
    /// Maximum rows to return. `None` means unlimited -- callers that want the old
    /// implicit default-50 pagination behavior must pass `Some(50)` explicitly.
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
    // Unlike the other Option fields below, these two are never skipped when absent: the
    // `/api/traces` list endpoint (apps/cli/src/server/routes.rs::get_traces_handler) returns
    // `SessionSummary` directly, and a client scanning that list for outcome/score needs the
    // key present (as `null`) to distinguish "not yet scored" from "field doesn't exist" --
    // omitting it silently on every un-scored session made the field look dropped entirely.
    #[serde(default)]
    pub primary_outcome: Option<String>,
    #[serde(default)]
    pub composite_score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_mtime_epoch_secs: Option<i64>,
    /// Number of times this session's context was compacted. 0 (the common case, not
    /// `None`) means never compacted -- matches `tool_calls_count`'s own convention rather
    /// than `primary_outcome`'s, since this is always knowable once a session is parsed.
    #[serde(default)]
    pub compaction_count: usize,
    /// Total tokens dropped across every compaction round in this session. See
    /// `agentworth_schema::TraceStats::compaction_tokens_dropped` for how it's computed.
    #[serde(default)]
    pub compaction_tokens_dropped: u64,
}

/// One bucket of `Storage::get_compaction_outcome_correlation`'s result: a session count for
/// one (compacted-or-not, outcome) combination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactionOutcomeBucket {
    pub compacted: bool,
    pub outcome: String,
    pub session_count: usize,
}

/// Grouping dimension for `Storage::get_outcome_rate`. See `docs/specs/verified-outcome-rate.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeRateGroupBy {
    Model,
    Adapter,
    Repo,
}

/// The since/until bounds actually applied to an `outcome_rate` query. `until` always names a
/// concrete instant -- the caller's own value, or the moment the query ran when left open --
/// so a printed window never reads as "whenever this happened to run."
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutcomeRateWindow {
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
}

/// The caller's own verified-outcome rate over the same window, for comparison against
/// individual rows. There is no cross-user baseline, now or later -- see
/// docs/specs/verified-outcome-rate.md.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutcomeRateBaseline {
    pub n: usize,
    pub verified: usize,
    pub rate: f64,
}

/// One group's row in `Storage::get_outcome_rate`'s result.
///
/// `n` and `verified` count "claimed" sessions only (non-null `primary_outcome`), within the
/// non-stub population unless `include_stubs` was set. `rate` and `delta_vs_baseline` are
/// `None` in exactly one case: `n == 0`, and then `reason` is `Some("no_outcome_detection")`.
/// A group with `n` between 1 and `min_n - 1` is not returned as a row at all -- it is folded
/// into `OutcomeRateResult::suppressed_groups` instead. These are deliberately different
/// signals: "zero outcomes detected for this group" is not the same claim as "too little data
/// to be sure," and collapsing them would make an adapter's total non-detection (`codex`,
/// `gemini`, ...) read as a small sample instead of a parsing gap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutcomeRateRow {
    pub key: String,
    pub n: usize,
    pub verified: usize,
    pub rate: Option<f64>,
    pub delta_vs_baseline: Option<f64>,
    /// Claimed-session count by rung ("1".."5", `agentworth_outcomes::outcome_rank`'s scale),
    /// summing to `n`.
    pub rungs: BTreeMap<String, usize>,
    pub reason: Option<String>,
    /// Session IDs behind this row's `n`, capped at `OUTCOME_RATE_SESSION_IDS_CAP` -- see
    /// `session_ids_truncated`. What makes the row checkable instead of a bare assertion.
    pub session_ids: Vec<String>,
    pub session_ids_truncated: bool,
}

/// What a `get_outcome_rate` answer can be checked against: which index, computed when, using
/// which stub/verification predicate, and how stale that index was at the moment of counting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutcomeRateReceipt {
    pub counted_at: DateTime<Utc>,
    /// The most recent `started_at` across the *whole* index, independent of this query's own
    /// window or stub filter -- lets a caller notice a stale index without a separate call.
    /// `None` only for a genuinely empty index.
    pub index_last_session_at: Option<DateTime<Utc>>,
    pub db_path: String,
    /// The literal predicate `get_outcome_rate` applies when `include_stubs` is false (see
    /// `NON_STUB_SQL_PREDICATE`), named here so the answer can be checked against a fresh SQL
    /// query rather than trusted blind.
    pub non_stub_predicate: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutcomeRateResult {
    pub group_by: OutcomeRateGroupBy,
    pub min_n: usize,
    pub window: OutcomeRateWindow,
    pub baseline: OutcomeRateBaseline,
    pub rows: Vec<OutcomeRateRow>,
    pub suppressed_groups: usize,
    pub receipt: OutcomeRateReceipt,
}

/// Cap on `OutcomeRateRow::session_ids` -- see `OutcomeRateRow::session_ids_truncated`.
const OUTCOME_RATE_SESSION_IDS_CAP: usize = 50;

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

/// Per-model token usage rollup for a time period (Day, Week, Month), mirroring
/// `UsagePeriodSummary` but grouped by model instead of adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelUsagePeriodSummary {
    pub period: String,
    pub model: String,
    pub session_count: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_tokens: u64,
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
    /// Path recorded on the specific matching file-modification event (not necessarily identical
    /// to the search pattern, since matching is substring-based).
    pub file_path: String,
    /// "read" | "write" | "edit" | "delete", per `FileActionType`.
    pub action: String,
    /// Timestamp of this session's most recent modification to `file_path`.
    pub modified_at: DateTime<Utc>,
    /// Model active at the time of that modification, if the trace recorded one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// One recorded file modification that provably happened inside a given repository.
///
/// The difference from `BlameMatch` is the whole point of this type: a `BlameMatch` is whatever
/// substring-matched a pattern, and an `AnchoredBlameRow` has been shown to lie inside one
/// specific checkout (see `crates/storage/src/anchoring.rs` for the rule and the measured
/// reason it exists).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnchoredBlameRow {
    /// `file_path` expressed relative to the repository root — the form `git log --name-only`
    /// also produces, so the two can be compared directly.
    pub repo_relative_path: String,
    /// The path exactly as the adapter recorded it: absolute for most adapters, repository-
    /// relative for `opencode`.
    pub file_path: String,
    pub session_id: String,
    pub adapter: String,
    pub source_path: String,
    pub action: String,
    pub modified_at: DateTime<Utc>,
    /// Model active at the time of the modification, if the trace recorded one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub models_used: Vec<String>,
    pub primary_outcome: Option<String>,
}

/// Everything `Storage::blame_for_repo` found, plus what it had to throw away.
///
/// `unanchored_rows` is not a footnote. It is the count of recorded modifications that could
/// not be placed in any repository — relative paths from sessions whose own repository does not
/// match this one. Reporting a result without it would silently pass off "we dropped evidence"
/// as "there was none."
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnchoredBlame {
    /// The repository root the rows were anchored to, normalized (no trailing slash).
    pub repo_root: String,
    /// The `org/repo` identity that relative paths were matched against.
    pub repo_identity: String,
    pub rows: Vec<AnchoredBlameRow>,
    pub unanchored_rows: usize,
}

/// Label for a `FileActionType`, matching its `#[serde(rename_all = "snake_case")]` form.
fn file_action_label(action: FileActionType) -> &'static str {
    match action {
        FileActionType::Read => "read",
        FileActionType::Write => "write",
        FileActionType::Edit => "edit",
        FileActionType::Delete => "delete",
    }
}

/// SQLite-backed storage index.
///
/// `conn` recovers from lock poisoning (see the `unwrap_or_else` on every `.lock()`
/// below) instead of propagating it. This connection is shared across every request
/// handler in `agentworth serve`'s multi-threaded Axum server, so a panic inside one
/// handler must not wedge the lock and take down every other endpoint until restart.
/// A `rusqlite::Connection` has no in-process invariant a panic mid-query could leave
/// broken -- SQLite itself already committed or rolled back at the engine level -- so
/// reusing it after a poison is safe.
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
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA busy_timeout = 5000;

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
                prompt_preview TEXT,
                compaction_count INTEGER NOT NULL DEFAULT 0,
                compaction_tokens_dropped INTEGER NOT NULL DEFAULT 0,
                parser_version INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY(source_path) REFERENCES sources(source_path) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS file_modifications (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                file_path TEXT NOT NULL,
                action TEXT NOT NULL,
                occurred_at TEXT NOT NULL,
                model TEXT,
                FOREIGN KEY(session_id) REFERENCES sessions(session_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS session_compaction (
                session_id TEXT NOT NULL,
                round INTEGER NOT NULL,
                start_seq INTEGER NOT NULL,
                end_seq INTEGER NOT NULL,
                summary_seq INTEGER,
                tokens_before INTEGER,
                summary_tokens INTEGER,
                PRIMARY KEY(session_id, round),
                FOREIGN KEY(session_id) REFERENCES sessions(session_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS session_model_usage (
                session_id TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY(session_id, model),
                FOREIGN KEY(session_id) REFERENCES sessions(session_id) ON DELETE CASCADE
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
            if !columns.contains(&"prompt_preview".to_string()) {
                let _ = conn.execute("ALTER TABLE sessions ADD COLUMN prompt_preview TEXT", []);
            }
            if !columns.contains(&"compaction_count".to_string()) {
                let _ = conn.execute(
                    "ALTER TABLE sessions ADD COLUMN compaction_count INTEGER NOT NULL DEFAULT 0",
                    [],
                );
            }
            if !columns.contains(&"compaction_tokens_dropped".to_string()) {
                let _ = conn.execute(
                    "ALTER TABLE sessions ADD COLUMN compaction_tokens_dropped INTEGER NOT NULL DEFAULT 0",
                    [],
                );
            }
            // 0, not 1: a row that predates this column was written by an unknown parser, so
            // it must sort below every adapter's current version and get reparsed once.
            if !columns.contains(&"parser_version".to_string()) {
                let _ = conn.execute(
                    "ALTER TABLE sessions ADD COLUMN parser_version INTEGER NOT NULL DEFAULT 0",
                    [],
                );
            }
        }

        // Data migration: `primary_outcome` used to be written in hand-rolled PascalCase
        // (e.g. "CommitObserved") by a since-fixed bug in `outcome_kind_name` (see
        // crates/outcomes/src/outcome.rs) that diverged from `OutcomeKind`'s own serde
        // `#[serde(rename_all = "snake_case")]` encoding. Every row already on disk from
        // before that fix still carries the old casing, so a read-side fix alone would leave
        // the database itself permanently inconsistent with anything that queries it directly.
        // This UPDATE corrects those rows in place. It is safe to run on every open: the WHERE
        // clause only ever matches the five old PascalCase literals, so it is a no-op on a
        // fresh database (no rows) and a no-op on an already-migrated one (no rows still carry
        // the old casing) — safe to run twice, safe on any schema state above.
        conn.execute(
            r#"
            UPDATE sessions
            SET primary_outcome = CASE primary_outcome
                WHEN 'DoneClaimed' THEN 'done_claimed'
                WHEN 'ArtifactChanged' THEN 'artifact_changed'
                WHEN 'TestOrBuildPassed' THEN 'test_or_build_passed'
                WHEN 'CommitObserved' THEN 'commit_observed'
                WHEN 'CiOrDeploymentVerified' THEN 'ci_or_deployment_verified'
                ELSE primary_outcome
            END
            WHERE primary_outcome IN (
                'DoneClaimed', 'ArtifactChanged', 'TestOrBuildPassed',
                'CommitObserved', 'CiOrDeploymentVerified'
            )
            "#,
            [],
        )?;

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
            CREATE INDEX IF NOT EXISTS idx_sessions_compaction_count ON sessions(compaction_count);
            CREATE INDEX IF NOT EXISTS idx_sources_fingerprint ON sources(fingerprint);
            CREATE INDEX IF NOT EXISTS idx_file_modifications_path ON file_modifications(file_path);
            CREATE INDEX IF NOT EXISTS idx_file_modifications_session ON file_modifications(session_id);
            CREATE INDEX IF NOT EXISTS idx_session_model_usage_model ON session_model_usage(model);

            "#,
        )?;

        // The three usage views are dropped and recreated on every open rather than
        // `CREATE VIEW IF NOT EXISTS` so a schema-definition change (like the stub
        // predicate added here) actually takes effect on a database that already has them
        // -- SQLite has no `CREATE OR REPLACE VIEW`, and `IF NOT EXISTS` would leave an
        // existing view's SQL text frozen at whatever it was when first created. Same
        // stub predicate as `list_sessions_filtered`/`get_aggregate_stats`
        // (`NON_STUB_SQL_PREDICATE`): a session that fails it contributes zero tokens
        // anyway (`total_tokens > 0` is part of the predicate), so only `session_count`
        // actually changes here, not the token sums.
        conn.execute_batch(&format!(
            r#"
            DROP VIEW IF EXISTS v_daily_usage;
            CREATE VIEW v_daily_usage AS
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
            WHERE started_at > '2020-01-01' AND {NON_STUB_SQL_PREDICATE}
            GROUP BY DATE(started_at), adapter
            ORDER BY period DESC, total_tokens DESC;

            DROP VIEW IF EXISTS v_weekly_usage;
            CREATE VIEW v_weekly_usage AS
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
            WHERE started_at > '2020-01-01' AND {NON_STUB_SQL_PREDICATE}
            GROUP BY strftime('%Y-W%W', started_at), adapter
            ORDER BY period DESC, total_tokens DESC;

            DROP VIEW IF EXISTS v_monthly_usage;
            CREATE VIEW v_monthly_usage AS
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
            WHERE started_at > '2020-01-01' AND {NON_STUB_SQL_PREDICATE}
            GROUP BY strftime('%Y-%m', started_at), adapter
            ORDER BY period DESC, total_tokens DESC;
            "#,
        ))?;

        Ok(())
    }

    /// Checks if a file source has already been indexed with the exact same fingerprint, size, and mtime.
    pub fn should_scan_source(&self, source: &SessionSource) -> Result<bool> {
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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

    /// Returns true if the indexed row(s) for `source_path` are missing a field that's
    /// derived from the source content but wasn't computed by the code that produced this
    /// row -- the shape left behind when a session was indexed before a feature like
    /// `prompt_preview` (v0.1.10) existed. `should_scan_source` alone can't catch this: an
    /// unchanged source is unchanged forever, so a row stuck in this state would otherwise
    /// never get rescanned.
    ///
    /// Two conditions, both unambiguous:
    ///
    /// - `prompt_preview` is null or empty. It's nullable and every real user-initiated session
    ///   has a non-empty first user message, so "missing" means "the extractor didn't exist
    ///   when this row was written."
    /// - `compaction_count > 0` with no `session_compaction` rows. The round-boundary table
    ///   (this PR) is populated in the same transaction as the count, so a row that has the
    ///   count and not the boundaries predates the table. This fires at most once per session:
    ///   the reparse writes the rows, and a session that genuinely compacted always derives at
    ///   least one round from the same events the count came from.
    ///
    /// `compaction_count` alone still can't drive this, for the reason it couldn't before:
    /// its column is `NOT NULL DEFAULT 0`, so the `ALTER TABLE` that added it stamped every
    /// pre-existing row with `0`, indistinguishable from "genuinely never compacted" (see the
    /// caveat on `get_compaction_outcome_correlation`). Only a *non-zero* count is evidence.
    /// `source_mtime_epoch_secs` is joined from `sources.mtime`, which is written in the same
    /// transaction as the session row, so it's never missing for a row that has a `sources`
    /// entry at all. A caller that reparses here backfills everything anyway, since parsing
    /// is all-or-nothing.
    ///
    /// Two further triggers, both of which also terminate on their own:
    ///
    /// - `composite_score IS NULL` -- the row was written without ever being scored. Scoring
    ///   always yields a number (`clamp01` maps even NaN to 0.0), so a rescored row is never
    ///   NULL again and this fires at most once per row. Deliberately *not* keyed on
    ///   `primary_outcome IS NULL`: a NULL outcome is the correct, permanent answer for a
    ///   session that produced no outcome evidence at all, so reparsing on it would rescan
    ///   thousands of rows on every single scan, forever.
    /// - `parser_version < current_parser_version` -- the adapter has since changed what it
    ///   extracts from a file of this shape, so the stored row is stale even though its bytes
    ///   are not. This is what re-derives outcomes and scores after a parse fix (#81) without
    ///   `--force`. The reparse writes the current version, so it also fires at most once.
    pub fn needs_backfill(
        &self,
        source_path: &str,
        current_parser_version: i64,
    ) -> Result<Option<BackfillReason>> {
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stmt = conn.prepare(
            "SELECT s.prompt_preview, s.compaction_count,
                    (SELECT COUNT(*) FROM session_compaction c WHERE c.session_id = s.session_id),
                    s.composite_score, s.parser_version
             FROM sessions s WHERE s.source_path = ?1",
        )?;
        let mut rows = stmt.query(params![source_path])?;
        while let Some(row) = rows.next()? {
            let parser_version: i64 = row.get(4)?;
            if parser_version < current_parser_version {
                return Ok(Some(BackfillReason::StaleParserVersion));
            }
            let prompt_preview: Option<String> = row.get(0)?;
            if prompt_preview.as_deref().unwrap_or("").trim().is_empty() {
                return Ok(Some(BackfillReason::MissingDerivedField));
            }
            let composite_score: Option<f64> = row.get(3)?;
            if composite_score.is_none() {
                return Ok(Some(BackfillReason::MissingDerivedField));
            }
            let compaction_count: i64 = row.get(1)?;
            let stored_rounds: i64 = row.get(2)?;
            if compaction_count > 0 && stored_rounds == 0 {
                return Ok(Some(BackfillReason::MissingDerivedField));
            }
        }
        Ok(None)
    }

    /// Returns `(session_id, adapter, source_path)` for every indexed session that fails
    /// `NEAR_EMPTY_EVENTS_SQL_PREDICATE` -- 0 or 1 normalized events total, the shape left
    /// behind when a non-session file (config, cache, telemetry dump, ...) was accepted as a
    /// session by a since-tightened adapter, or a real session that was abandoned after at
    /// most one turn. Used by the scanner to prune stubs whose source no longer passes the
    /// adapter's current detection. Broader than a plain zero-events check (catches the
    /// one-event shape too), but deliberately does NOT look at `total_tokens` the way
    /// `NON_STUB_SQL_PREDICATE` does for aggregate reporting -- a real multi-event session
    /// with no captured token telemetry is still real and must stay indexed and findable by
    /// `agentworth audit`/`autopsy`/`blind-spots` (which query with `include_stubs: true`
    /// precisely to see it), even though it's excluded from `agentworth stats`/`usage`
    /// aggregates by the separate, stricter predicate.
    pub fn stub_sessions(&self) -> Result<Vec<(String, String, String)>> {
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stmt = conn.prepare(&format!(
            "SELECT session_id, adapter, source_path FROM sessions WHERE {NEAR_EMPTY_EVENTS_SQL_PREDICATE}",
        ))?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Permanently removes one session and its child rows (file modifications, per-model
    /// usage, and its `sources` fingerprint entry). Intended for pruning zero-activity
    /// stub sessions -- callers are responsible for deciding a row is safe to delete.
    pub fn delete_session(&self, session_id: &str, source_path: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM file_modifications WHERE session_id = ?1", params![session_id])?;
        tx.execute("DELETE FROM session_model_usage WHERE session_id = ?1", params![session_id])?;
        tx.execute("DELETE FROM session_compaction WHERE session_id = ?1", params![session_id])?;
        tx.execute("DELETE FROM sessions WHERE session_id = ?1", params![session_id])?;
        tx.execute("DELETE FROM sources WHERE source_path = ?1", params![source_path])?;
        tx.commit()?;
        Ok(())
    }

    /// Upsert an indexed session into the database atomically with verdict and score.
    ///
    /// `parser_version` is the producing adapter's `AgentAdapter::parser_version()`. It is
    /// what lets a later scan tell a row parsed by today's code from one parsed by an older,
    /// wronger version of it -- see `needs_backfill`.
    pub fn upsert_session(
        &self,
        trace: &AgentWorthTrace,
        primary_outcome: Option<&str>,
        composite_score: Option<f64>,
        parser_version: i64,
    ) -> Result<()> {
        let mut conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let tx = conn.transaction()?;

        let scanned_at = Utc::now().to_rfc3339();
        let started_at_str = trace.started_at.to_rfc3339();
        let ended_at_str = trace.ended_at.map(|t| t.to_rfc3339());
        let models_json = serde_json::to_string(&trace.stats.models_used)?;
        let tools_json = serde_json::to_string(&trace.stats.tools_used)?;
        let metadata_json = serde_json::to_string(&trace.metadata)?;

        const PROMPT_PREVIEW_MAX_CHARS: usize = 200;
        let prompt_preview = trace.events.iter().find_map(|event| match &event.payload {
            EventPayload::UserMessage { content } => {
                let trimmed = content.trim();
                if trimmed.is_empty() {
                    None
                } else if trimmed.chars().count() > PROMPT_PREVIEW_MAX_CHARS {
                    let truncated: String = trimmed.chars().take(PROMPT_PREVIEW_MAX_CHARS).collect();
                    Some(format!("{truncated}…"))
                } else {
                    Some(trimmed.to_string())
                }
            }
            _ => None,
        });

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
                primary_outcome, composite_score, prompt_preview,
                compaction_count, compaction_tokens_dropped, parser_version
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)
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
                composite_score = excluded.composite_score,
                prompt_preview = excluded.prompt_preview,
                compaction_count = excluded.compaction_count,
                compaction_tokens_dropped = excluded.compaction_tokens_dropped,
                parser_version = excluded.parser_version;
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
                prompt_preview,
                trace.stats.compaction_count as i64,
                trace.stats.compaction_tokens_dropped as i64,
                parser_version,
            ],
        )?;

        // 3. Replace this session's file-modification records. A full trace is re-parsed from
        // disk on every scan (see Scanner::run_scan), so events are rewritten wholesale rather
        // than diffed to avoid duplicate rows on rescan.
        tx.execute(
            "DELETE FROM file_modifications WHERE session_id = ?1",
            params![trace.session_id],
        )?;

        {
            let mut insert_stmt = tx.prepare(
                r#"
                INSERT INTO file_modifications (session_id, file_path, action, occurred_at, model)
                VALUES (?1, ?2, ?3, ?4, ?5)
                "#,
            )?;

            let mut current_model: Option<String> = None;
            for event in &trace.events {
                match &event.payload {
                    EventPayload::ModelInvocation { model, .. } => {
                        current_model = Some(model.clone());
                    }
                    EventPayload::FileAction { path, action, .. } => {
                        insert_stmt.execute(params![
                            trace.session_id,
                            path,
                            file_action_label(*action),
                            event.timestamp.to_rfc3339(),
                            current_model.as_deref(),
                        ])?;
                    }
                    _ => {}
                }
            }
        }

        // 4. Replace per-model usage rows. Delete-then-insert (rather than upsert)
        // so a rescan correctly drops a model that no longer appears for this session.
        tx.execute(
            "DELETE FROM session_model_usage WHERE session_id = ?1",
            params![trace.session_id],
        )?;
        for (model, usage) in &trace.stats.per_model_token_usage {
            tx.execute(
                r#"
                INSERT INTO session_model_usage (
                    session_id, model, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
                params![
                    trace.session_id,
                    model,
                    usage.input_tokens as i64,
                    usage.output_tokens as i64,
                    usage.cache_read_tokens as i64,
                    usage.cache_creation_tokens as i64,
                ],
            )?;
        }

        // 5. Replace this session's compaction round boundaries. Delete-then-insert for the
        // same reason as step 4: a rescan of a session that has since compacted again must not
        // leave the old round list behind, and a round that moved must not double up.
        tx.execute(
            "DELETE FROM session_compaction WHERE session_id = ?1",
            params![trace.session_id],
        )?;
        for round in compaction_rounds(trace) {
            tx.execute(
                r#"
                INSERT INTO session_compaction (
                    session_id, round, start_seq, end_seq, summary_seq,
                    tokens_before, summary_tokens
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                "#,
                params![
                    trace.session_id,
                    round.round as i64,
                    round.start_seq as i64,
                    round.end_seq as i64,
                    round.summary_seq.map(|s| s as i64),
                    round.tokens_before.map(|t| t as i64),
                    round.summary_tokens.map(|t| t as i64),
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Every stored compaction round for one session, in round order.
    ///
    /// Empty both for a session that never compacted and for one indexed before this table
    /// existed. Those are told apart by `compaction_count`, which is why `needs_backfill`
    /// compares the two rather than treating an empty result as an answer.
    pub fn get_compaction_rounds(&self, session_id: &str) -> Result<Vec<CompactionRound>> {
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stmt = conn.prepare(
            "SELECT round, start_seq, end_seq, summary_seq, tokens_before, summary_tokens
             FROM session_compaction WHERE session_id = ?1 ORDER BY round",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            Ok(CompactionRound {
                round: row.get::<_, i64>(0)? as u32,
                start_seq: row.get::<_, i64>(1)? as u64,
                end_seq: row.get::<_, i64>(2)? as u64,
                summary_seq: row.get::<_, Option<i64>>(3)?.map(|v| v as u64),
                tokens_before: row.get::<_, Option<i64>>(4)?.map(|v| v as u64),
                summary_tokens: row.get::<_, Option<i64>>(5)?.map(|v| v as u64),
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Upsert an indexed trace into the database atomically, with no verdict, no score, and
    /// no parser version. A row written this way always reads as needing a backfill -- which
    /// is correct: nothing about it came from a scan.
    pub fn upsert_trace(&self, trace: &AgentWorthTrace) -> Result<()> {
        self.upsert_session(trace, None, None, 0)
    }

    /// Retrieve summary statistics across the whole indexed database.
    ///
    /// `include_stubs` decides which population every field in the returned `AggregateStats`
    /// describes:
    /// - `false` -- exclude stubs (near-empty, no real activity; see
    ///   `NON_STUB_SQL_PREDICATE`), matching `list_sessions_filtered`'s own default. Use this for
    ///   any verdict/analytics surface that gets shown alongside a `list_sessions_filtered`-backed
    ///   count or ratio (`/api/stats`, `agentworth stats`, the adapter coverage matrix) -- mixing
    ///   an unfiltered total with a stub-excluded breakdown silently corrupts every percentage
    ///   computed from them (docs/DECISION-INBOX.md, stats/stub-count-mismatch entry).
    /// - `true` -- every row, stubs included. Use this only for raw index-inventory /
    ///   health-check reporting that already promises "everything in the index" (`agentworth
    ///   scan`'s "Total Indexed" line, `agentworth doctor`'s `total_indexed_sessions`) -- those
    ///   labels would become false if stubs silently vanished from the count.
    pub fn get_aggregate_stats(&self, include_stubs: bool) -> Result<AggregateStats> {
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

        let mut stats = AggregateStats::default();

        let where_clause = if include_stubs {
            String::new()
        } else {
            format!("WHERE {NON_STUB_SQL_PREDICATE}")
        };

        let mut stmt = conn.prepare(&format!(
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
                COALESCE(SUM(CASE WHEN primary_outcome IN ('ci_or_deployment_verified', 'commit_observed', 'test_or_build_passed') THEN 1 ELSE 0 END), 0)
            FROM sessions
            {where_clause}
            "#,
        ))?;

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

        // Sessions count by adapter -- same where_clause so per-adapter counts here agree with
        // an adapter-filtered `list_sessions_filtered` count for the same adapter (e.g. the
        // coverage matrix's per-adapter `sessions_count` vs. `/api/traces?adapter=X`).
        let mut adapter_stmt = conn.prepare(&format!(
            "SELECT adapter, COUNT(*) FROM sessions {where_clause} GROUP BY adapter ORDER BY COUNT(*) DESC",
        ))?;
        let mut adapter_rows = adapter_stmt.query([])?;
        while let Some(row) = adapter_rows.next()? {
            let adapter: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            stats.sessions_by_adapter.insert(adapter, count as usize);
        }

        // Aggregate models and tools -- same where_clause, so these counts describe the same
        // population as total_sessions rather than a silently larger (or smaller) one.
        let mut items_stmt =
            conn.prepare(&format!("SELECT models_used, tools_used FROM sessions {where_clause}"))?;
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
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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

    /// Groups sessions by whether they were ever compacted (`compaction_count > 0`) and by
    /// `primary_outcome`, with a session count for each combination -- answers the open
    /// question from issue #32 ("do compacted sessions reach lower outcome rungs?") as a
    /// single indexed aggregate query, no per-session trace loading required.
    ///
    /// Excludes stubs (`NON_STUB_SQL_PREDICATE`), matching `list_sessions_filtered`'s own
    /// default and `get_aggregate_stats(false)` -- this is an analytics/verdict surface in
    /// the same sense those are (see their doc comments and docs/DECISION-INBOX.md's
    /// stats/stub-count-mismatch entry for why that population has to agree across every
    /// query like this one, not just the ones that happened to get audited first).
    ///
    /// Caveat that matters when reading the result: `compaction_count` only reflects reality
    /// for sessions that have been scanned since this column was introduced. A row carried
    /// over from before this feature existed reads as `compacted = false` regardless of
    /// whether that session was actually compacted, until it's rescanned (`agentworth scan
    /// --force`) from its original source file. This query doesn't know the difference
    /// between "never compacted" and "not yet rescanned" -- treat a near-zero compacted count
    /// against a large, old index as a sign a rescan hasn't happened yet, not as the finding.
    pub fn get_compaction_outcome_correlation(&self) -> Result<Vec<CompactionOutcomeBucket>> {
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stmt = conn.prepare(&format!(
            r#"
            SELECT
                CASE WHEN compaction_count > 0 THEN 1 ELSE 0 END AS compacted,
                COALESCE(primary_outcome, 'Unresolved') AS outcome,
                COUNT(*) AS session_count
            FROM sessions
            WHERE {NON_STUB_SQL_PREDICATE}
            GROUP BY compacted, COALESCE(primary_outcome, 'Unresolved')
            ORDER BY compacted, session_count DESC
            "#,
        ))?;

        let mut rows = stmt.query([])?;
        let mut results = Vec::new();

        while let Some(row) = rows.next()? {
            let compacted: i64 = row.get(0)?;
            let outcome: String = row.get(1)?;
            let session_count: i64 = row.get(2)?;

            results.push(CompactionOutcomeBucket {
                compacted: compacted != 0,
                outcome,
                session_count: session_count as usize,
            });
        }

        Ok(results)
    }

    /// Retrieve a single session summary by its unique session ID.
    pub fn get_session_by_id(&self, session_id: &str) -> Result<Option<SessionSummary>> {
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stmt = conn.prepare(
            r#"
            SELECT sessions.session_id, sessions.adapter, sessions.source_path, sessions.started_at,
                   sessions.duration_seconds, sessions.total_tokens, sessions.total_events,
                   sessions.tool_calls_count, sessions.models_used, sessions.primary_outcome,
                   sessions.composite_score, sessions.prompt_preview, sessions.compaction_count,
                   sessions.compaction_tokens_dropped, sources.mtime
            FROM sessions
            LEFT JOIN sources ON sessions.source_path = sources.source_path
            WHERE sessions.session_id = ?1
            "#,
        )?;

        let mut rows = stmt.query(params![session_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_session_summary(row)?))
        } else {
            Ok(None)
        }
    }

    /// Sessions whose id starts with `prefix`, newest first, capped at `limit`. Used by
    /// `inspect` to resolve a short id the caller typed instead of the full one -- `%` and
    /// `_` in `prefix` are escaped so they match themselves rather than acting as SQL LIKE
    /// wildcards, since a session id can legitimately contain either character.
    pub fn find_sessions_by_id_prefix(&self, prefix: &str, limit: usize) -> Result<Vec<SessionSummary>> {
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let escaped = prefix.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        let pattern = format!("{}%", escaped);
        let mut stmt = conn.prepare(
            r#"
            SELECT sessions.session_id, sessions.adapter, sessions.source_path, sessions.started_at,
                   sessions.duration_seconds, sessions.total_tokens, sessions.total_events,
                   sessions.tool_calls_count, sessions.models_used, sessions.primary_outcome,
                   sessions.composite_score, sessions.prompt_preview, sessions.compaction_count,
                   sessions.compaction_tokens_dropped, sources.mtime
            FROM sessions
            LEFT JOIN sources ON sessions.source_path = sources.source_path
            WHERE sessions.session_id LIKE ?1 ESCAPE '\'
            ORDER BY sessions.started_at DESC
            LIMIT ?2
            "#,
        )?;

        let mut rows = stmt.query(params![pattern, limit as i64])?;
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            results.push(row_to_session_summary(row)?);
        }
        Ok(results)
    }

    /// Retrieve the per-model token usage breakdown for a single session.
    pub fn get_session_model_usage(&self, session_id: &str) -> Result<Vec<(String, TokenUsage)>> {
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stmt = conn.prepare(
            r#"
            SELECT model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens
            FROM session_model_usage
            WHERE session_id = ?1
            ORDER BY model
            "#,
        )?;

        let mut rows = stmt.query(params![session_id])?;
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            let model: String = row.get(0)?;
            let input: i64 = row.get(1)?;
            let output: i64 = row.get(2)?;
            let cache_read: i64 = row.get(3)?;
            let cache_creation: i64 = row.get(4)?;
            results.push((
                model,
                TokenUsage::new(
                    input as u64,
                    output as u64,
                    cache_read as u64,
                    cache_creation as u64,
                ),
            ));
        }
        Ok(results)
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
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

        let mut sql = String::from(
            r#"
            SELECT sessions.session_id, sessions.adapter, sessions.source_path, sessions.started_at,
                   sessions.duration_seconds, sessions.total_tokens, sessions.total_events,
                   sessions.tool_calls_count, sessions.models_used, sessions.primary_outcome,
                   sessions.composite_score, sessions.prompt_preview, sessions.compaction_count,
                   sessions.compaction_tokens_dropped, sources.mtime
            FROM sessions
            LEFT JOIN sources ON sessions.source_path = sources.source_path
            WHERE 1=1
            "#,
        );

        let mut param_values: Vec<Box<dyn ToSql>> = Vec::new();

        if !filter.include_stubs.unwrap_or(false) {
            sql.push_str(&format!(" AND ({NON_STUB_SQL_PREDICATE})"));
        }

        if let Some(ref adapter) = filter.adapter {
            sql.push_str(" AND sessions.adapter = ?");
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
                sql.push_str(" AND (sessions.session_id LIKE ? OR sessions.source_path LIKE ? OR models_used LIKE ? OR sessions.adapter LIKE ?)");
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

        match filter.limit {
            Some(limit) => {
                sql.push_str(" LIMIT ?");
                param_values.push(Box::new(limit as i64));
            }
            // SQLite requires a LIMIT clause to precede OFFSET; -1 means "unbounded" so an
            // explicit offset with no limit still works instead of producing a syntax error.
            None if filter.offset.is_some() => sql.push_str(" LIMIT -1"),
            None => {}
        }

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

    /// Verified-outcome rate per model, adapter, or repo -- see
    /// `docs/specs/verified-outcome-rate.md`.
    ///
    /// Fetches every session in `[since, until]` (non-stub unless `include_stubs`), then groups
    /// in Rust: a straight `adapter` group, `extract_repository_or_workspace(source_path)` for
    /// `repo`, and a fan-out over each session's `models_used` for `model` (a session that used
    /// two models counts once per model -- the same fan the spec's own ad hoc `json_each` SQL
    /// gives) -- the same fetch-then-filter shape `list_sessions_filtered`'s `repo` post-filter
    /// already uses. "Claimed" is any session with a non-null `primary_outcome`; "verified" is
    /// rung 3 (`TestOrBuildPassed`) or higher via `agentworth_outcomes::outcome_rank`, the one
    /// place this ladder is defined -- not re-typed as a second SQL CASE ladder here.
    ///
    /// A group is included even when zero of its sessions are claimed (`n: 0, reason:
    /// Some("no_outcome_detection")`) -- that is a different, and more informative, signal than
    /// "too little data," which is what `min_n` suppression means instead. See
    /// `OutcomeRateRow`'s doc comment.
    pub fn get_outcome_rate(
        &self,
        group_by: OutcomeRateGroupBy,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
        min_n: usize,
        include_stubs: bool,
    ) -> Result<OutcomeRateResult> {
        let counted_at = Utc::now();
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

        let mut sql = String::from(
            "SELECT session_id, adapter, source_path, primary_outcome, models_used FROM sessions WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn ToSql>> = Vec::new();

        if !include_stubs {
            sql.push_str(&format!(" AND ({NON_STUB_SQL_PREDICATE})"));
        }
        if let Some(since) = since {
            sql.push_str(" AND started_at >= ?");
            param_values.push(Box::new(since.to_rfc3339()));
        }
        if let Some(until) = until {
            sql.push_str(" AND started_at <= ?");
            param_values.push(Box::new(until.to_rfc3339()));
        }

        struct FetchedRow {
            session_id: String,
            adapter: String,
            source_path: String,
            claimed_rung: Option<u8>,
            models_used: Vec<String>,
        }

        let mut fetched = Vec::new();
        {
            let mut stmt = conn.prepare(&sql)?;
            let params_refs: Vec<&dyn ToSql> = param_values.iter().map(|b| b.as_ref()).collect();
            let mut rows = stmt.query(params_refs.as_slice())?;

            while let Some(row) = rows.next()? {
                let session_id: String = row.get(0)?;
                let adapter: String = row.get(1)?;
                let source_path: String = row.get(2)?;
                let primary_outcome: Option<String> = row.get(3)?;
                let models_str: String = row.get(4)?;
                let models_used =
                    serde_json::from_str::<Vec<String>>(&models_str).unwrap_or_default();
                let claimed_rung = primary_outcome.as_deref().and_then(|s| {
                    serde_json::from_value::<OutcomeKind>(serde_json::Value::String(s.to_string()))
                        .ok()
                        .map(outcome_rank)
                });
                fetched.push(FetchedRow {
                    session_id,
                    adapter,
                    source_path,
                    claimed_rung,
                    models_used,
                });
            }
        }

        // The whole index's newest session, not just this query's window/stub slice of it --
        // see `OutcomeRateReceipt::index_last_session_at`'s doc comment.
        let index_last_session_at: Option<String> = conn
            .query_row("SELECT MAX(started_at) FROM sessions", [], |r| {
                r.get::<_, Option<String>>(0)
            })
            .ok()
            .flatten();
        let index_last_session_at = index_last_session_at.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        });
        drop(conn);

        // Baseline: distinct sessions, never fanned by model -- a session using two models
        // must not count twice toward the caller's own overall rate.
        let mut baseline_n = 0usize;
        let mut baseline_verified = 0usize;
        for r in &fetched {
            if let Some(rung) = r.claimed_rung {
                baseline_n += 1;
                if rung >= 3 {
                    baseline_verified += 1;
                }
            }
        }
        let baseline_rate = if baseline_n > 0 {
            baseline_verified as f64 / baseline_n as f64
        } else {
            0.0
        };

        struct GroupAcc {
            n: usize,
            verified: usize,
            rungs: BTreeMap<u8, usize>,
            session_ids: Vec<String>,
        }

        let mut groups: BTreeMap<String, GroupAcc> = BTreeMap::new();
        let mut push = |key: String, rung: Option<u8>, session_id: &str| {
            let acc = groups.entry(key).or_insert_with(|| GroupAcc {
                n: 0,
                verified: 0,
                rungs: BTreeMap::new(),
                session_ids: Vec::new(),
            });
            if let Some(rung) = rung {
                acc.n += 1;
                if rung >= 3 {
                    acc.verified += 1;
                }
                *acc.rungs.entry(rung).or_insert(0) += 1;
                if acc.session_ids.len() < OUTCOME_RATE_SESSION_IDS_CAP {
                    acc.session_ids.push(session_id.to_string());
                }
            }
        };

        for r in &fetched {
            match group_by {
                OutcomeRateGroupBy::Adapter => {
                    push(r.adapter.clone(), r.claimed_rung, &r.session_id);
                }
                OutcomeRateGroupBy::Repo => {
                    let repo = extract_repository_or_workspace(&r.source_path);
                    push(repo, r.claimed_rung, &r.session_id);
                }
                OutcomeRateGroupBy::Model => {
                    for model in &r.models_used {
                        push(model.clone(), r.claimed_rung, &r.session_id);
                    }
                }
            }
        }

        let mut rows_out = Vec::new();
        let mut suppressed_groups = 0usize;

        for (key, acc) in groups {
            let rungs: BTreeMap<String, usize> = (1..=5u8)
                .map(|i| (i.to_string(), *acc.rungs.get(&i).unwrap_or(&0)))
                .collect();

            if acc.n == 0 {
                rows_out.push(OutcomeRateRow {
                    key,
                    n: 0,
                    verified: 0,
                    rate: None,
                    delta_vs_baseline: None,
                    rungs,
                    reason: Some("no_outcome_detection".to_string()),
                    session_ids: Vec::new(),
                    session_ids_truncated: false,
                });
                continue;
            }

            if acc.n < min_n {
                suppressed_groups += 1;
                continue;
            }

            let rate = acc.verified as f64 / acc.n as f64;
            let truncated = acc.n > acc.session_ids.len();
            rows_out.push(OutcomeRateRow {
                key,
                n: acc.n,
                verified: acc.verified,
                rate: Some(rate),
                delta_vs_baseline: Some(rate - baseline_rate),
                rungs,
                reason: None,
                session_ids: acc.session_ids,
                session_ids_truncated: truncated,
            });
        }

        rows_out.sort_by(|a, b| match (a.rate, b.rate) {
            (Some(ra), Some(rb)) => rb
                .partial_cmp(&ra)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.key.cmp(&b.key)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.key.cmp(&b.key),
        });

        let db_path = self
            .db_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| ":memory:".to_string());

        Ok(OutcomeRateResult {
            group_by,
            min_n,
            window: OutcomeRateWindow {
                since,
                until: Some(until.unwrap_or(counted_at)),
            },
            baseline: OutcomeRateBaseline {
                n: baseline_n,
                verified: baseline_verified,
                rate: baseline_rate,
            },
            rows: rows_out,
            suppressed_groups,
            receipt: OutcomeRateReceipt {
                counted_at,
                index_last_session_at,
                db_path,
                non_stub_predicate: NON_STUB_SQL_PREDICATE.to_string(),
            },
        })
    }

    /// Extract and rank top repository / workspace paths from indexed sessions.
    pub fn get_top_repositories(&self) -> Result<Vec<(String, usize)>> {
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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

    /// Retrieve per-model usage rollups bucketed by day, week, or month — the
    /// same periods as `get_daily_usage`/`get_weekly_usage`/`get_monthly_usage`,
    /// grouped by model instead of adapter.
    pub fn get_model_usage(&self, period: &str, limit: usize) -> Result<Vec<ModelUsagePeriodSummary>> {
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

        let period_expr = match period {
            "week" => "strftime('%Y-W%W', s.started_at)",
            "month" => "strftime('%Y-%m', s.started_at)",
            _ => "DATE(s.started_at)",
        };

        let sql = format!(
            r#"
            SELECT
                {period_expr} AS period,
                smu.model AS model,
                COUNT(DISTINCT smu.session_id) AS session_count,
                COALESCE(SUM(smu.input_tokens), 0) AS input_tokens,
                COALESCE(SUM(smu.output_tokens), 0) AS output_tokens,
                COALESCE(SUM(smu.cache_read_tokens), 0) AS cache_read_tokens,
                COALESCE(SUM(smu.cache_creation_tokens), 0) AS cache_creation_tokens,
                COALESCE(SUM(smu.input_tokens + smu.output_tokens + smu.cache_read_tokens + smu.cache_creation_tokens), 0) AS total_tokens
            FROM session_model_usage smu
            JOIN sessions s ON s.session_id = smu.session_id
            WHERE s.started_at > '2020-01-01' AND {NON_STUB_SQL_PREDICATE}
            GROUP BY {period_expr}, smu.model
            ORDER BY period DESC, total_tokens DESC
            LIMIT ?1
            "#,
            period_expr = period_expr
        );

        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params![limit as i64])?;
        let mut results = Vec::new();

        while let Some(row) = rows.next()? {
            let period: String = row.get(0)?;
            let model: String = row.get(1)?;
            let session_count: i64 = row.get(2)?;
            let input: i64 = row.get(3)?;
            let output: i64 = row.get(4)?;
            let cache_read: i64 = row.get(5)?;
            let cache_creation: i64 = row.get(6)?;
            let total: i64 = row.get(7)?;

            let estimated_cost_usd = estimate_model_tokens_cost_usd(
                Some(&model),
                input as u64,
                output as u64,
                cache_read as u64,
                cache_creation as u64,
            );
            let cache_hit_ratio =
                calculate_cache_hit_ratio(input as u64, cache_read as u64, cache_creation as u64);

            results.push(ModelUsagePeriodSummary {
                period,
                model,
                session_count: session_count as usize,
                input_tokens: input as u64,
                output_tokens: output as u64,
                cache_read_tokens: cache_read as u64,
                cache_creation_tokens: cache_creation as u64,
                total_tokens: total as u64,
                estimated_cost_usd,
                cache_hit_ratio,
            });
        }

        Ok(results)
    }

    /// Calculate rolling pacing summary for the last N hours.
    pub fn get_pacing_window(&self, hours: i64) -> Result<PacingSummary> {
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

        // Get max date in DB as anchor if current real-time clock has no recent sessions
        let mut max_stmt = conn.prepare("SELECT MAX(started_at) FROM sessions")?;
        let max_str: Option<String> = max_stmt.query_row([], |r| r.get(0)).ok();
        let anchor_time = max_str
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc)))
            .unwrap_or_else(Utc::now);

        let window_start = anchor_time - chrono::Duration::hours(hours);
        let start_str = window_start.to_rfc3339();

        // Same stub exclusion as everywhere else (`NON_STUB_SQL_PREDICATE`): this window's
        // `session_count` must describe the same population `agentworth stats`/`agentworth
        // usage` do, and the token sums are unaffected since a stub contributes zero tokens
        // by definition of the predicate.
        let mut stmt = conn.prepare(&format!(
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
            WHERE started_at >= ?1 AND {NON_STUB_SQL_PREDICATE}
            "#,
        ))?;

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

        // Query active adapters in this window -- same stub exclusion as the count above so
        // an adapter doesn't show as "active" solely because of stub rows.
        let mut ad_stmt = conn.prepare(&format!(
            "SELECT DISTINCT adapter FROM sessions WHERE started_at >= ?1 AND {NON_STUB_SQL_PREDICATE} ORDER BY adapter",
        ))?;
        let mut ad_rows = ad_stmt.query(params![start_str])?;
        let mut active_adapters = Vec::new();
        while let Some(r) = ad_rows.next()? {
            active_adapters.push(r.get(0)?);
        }

        let mut mod_stmt = conn.prepare(&format!(
            "SELECT models_used FROM sessions WHERE started_at >= ?1 AND {NON_STUB_SQL_PREDICATE}",
        ))?;
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
    ///
    /// Matches against recorded `file_modifications` rows (populated from each trace's
    /// `EventPayload::FileAction` events during scan), not session-level metadata — a session's
    /// own transcript path or tool-name counters never contain the paths of files it edited.
    /// Each matching session is represented once, by its most recent touch to a matching path.
    pub fn find_sessions_for_blame(&self, file_path_pattern: &str) -> Result<Vec<BlameMatch>> {
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let pattern = format!("%{}%", file_path_pattern);

        let mut stmt = conn.prepare(
            r#"
            SELECT session_id, adapter, source_path, started_at, models_used, total_tokens,
                   tool_calls_count, file_path, action, occurred_at, model
            FROM (
                SELECT
                    s.session_id, s.adapter, s.source_path, s.started_at, s.models_used,
                    s.total_tokens, s.tool_calls_count,
                    fm.file_path, fm.action, fm.occurred_at, fm.model,
                    ROW_NUMBER() OVER (
                        PARTITION BY fm.session_id ORDER BY fm.occurred_at DESC
                    ) AS rn
                FROM file_modifications fm
                JOIN sessions s ON s.session_id = fm.session_id
                WHERE fm.file_path LIKE ?1
            )
            WHERE rn = 1
            ORDER BY occurred_at DESC
            LIMIT 25
            "#,
        )?;

        let mut rows = stmt.query(params![pattern])?;
        let mut results = Vec::new();

        while let Some(row) = rows.next()? {
            results.push(row_to_blame_match(row)?);
        }

        Ok(results)
    }

    /// Reverse of `find_sessions_for_blame`: every file a single session touched, each
    /// represented once by its most recent action within that session. Same underlying
    /// `file_modifications` index and the same `BlameMatch` shape, just grouped by file
    /// instead of by session — this is the query the Blunder-to-Blame Bridge uses to go
    /// from "here's a blunder session" to "here's exactly which files AI Code Blame
    /// attributes to it." An unknown `session_id` returns an empty list, not an error.
    pub fn find_files_for_session(&self, session_id: &str) -> Result<Vec<BlameMatch>> {
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

        let mut stmt = conn.prepare(
            r#"
            SELECT session_id, adapter, source_path, started_at, models_used, total_tokens,
                   tool_calls_count, file_path, action, occurred_at, model
            FROM (
                SELECT
                    s.session_id, s.adapter, s.source_path, s.started_at, s.models_used,
                    s.total_tokens, s.tool_calls_count,
                    fm.file_path, fm.action, fm.occurred_at, fm.model,
                    ROW_NUMBER() OVER (
                        PARTITION BY fm.file_path ORDER BY fm.occurred_at DESC
                    ) AS rn
                FROM file_modifications fm
                JOIN sessions s ON s.session_id = fm.session_id
                WHERE fm.session_id = ?1
            )
            WHERE rn = 1
            ORDER BY occurred_at DESC
            LIMIT 500
            "#,
        )?;

        let mut rows = stmt.query(params![session_id])?;
        let mut results = Vec::new();

        while let Some(row) = rows.next()? {
            results.push(row_to_blame_match(row)?);
        }

        Ok(results)
    }

    /// When this index last took a write, i.e. the newest `scanned_at` across all sessions.
    ///
    /// A handoff's receipt has to say how stale the answer is, and "newest session start" is
    /// not that -- a scan run today over a session from last week updates the index without
    /// moving any `started_at`. `None` for an empty index.
    pub fn last_scanned_at(&self) -> Result<Option<DateTime<Utc>>> {
        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let raw: Option<String> = conn
            .query_row("SELECT MAX(scanned_at) FROM sessions", [], |row| row.get(0))
            .unwrap_or(None);

        Ok(raw.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        }))
    }

    /// The most recent non-stub sessions whose derived repository/workspace name equals
    /// `repo`, newest first. Backs `carry_forward` and the "most recent session for the
    /// caller's cwd" default on `session_handoff` (docs/specs/handoff.md).
    ///
    /// **`repo` is not a stored column.** There is only `source_path`, and
    /// `extract_repository_or_workspace` derives the key from it at read time -- the same
    /// derivation `get_top_repositories` and the MCP `sessions_find` tool already do. So this
    /// walks sessions newest-first and filters in Rust, stopping as soon as `limit` matches
    /// are found. That is a scan, so it is bounded by `REPO_SCAN_BUDGET` rows, and
    /// `scan_exhausted` says plainly when the budget ran out before `limit` was reached
    /// rather than letting a partial answer look complete.
    ///
    /// Deliberately no schema change: the derivation is a pure string function over a column
    /// that is already indexed and already read, and adding a stored `repo` column would need
    /// a migration plus a rescan to backfill, for a query that runs at most a handful of times
    /// per session.
    pub fn list_sessions_for_repo(&self, repo: &str, limit: usize) -> Result<RepoSessionPage> {
        if limit == 0 {
            return Ok(RepoSessionPage {
                sessions: Vec::new(),
                scan_exhausted: false,
            });
        }

        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stmt = conn.prepare(&format!(
            r#"
            SELECT sessions.session_id, sessions.adapter, sessions.source_path, sessions.started_at,
                   sessions.duration_seconds, sessions.total_tokens, sessions.total_events,
                   sessions.tool_calls_count, sessions.models_used, sessions.primary_outcome,
                   sessions.composite_score, sessions.prompt_preview, sessions.compaction_count,
                   sessions.compaction_tokens_dropped, sources.mtime
            FROM sessions
            LEFT JOIN sources ON sessions.source_path = sources.source_path
            WHERE ({NON_STUB_SQL_PREDICATE})
            ORDER BY sessions.started_at DESC
            LIMIT {REPO_SCAN_BUDGET}
            "#
        ))?;

        let mut rows = stmt.query([])?;
        let mut sessions = Vec::new();
        let mut scanned = 0usize;

        while let Some(row) = rows.next()? {
            scanned += 1;
            let source_path: String = row.get(2)?;
            if extract_repository_or_workspace(&source_path) != repo {
                continue;
            }
            sessions.push(row_to_session_summary(row)?);
            if sessions.len() >= limit {
                break;
            }
        }

        Ok(RepoSessionPage {
            scan_exhausted: sessions.len() < limit && scanned >= REPO_SCAN_BUDGET,
            sessions,
        })
    }
    /// Every recorded file modification that provably happened inside `repo_root`, plus a count
    /// of the ones that could not be placed anywhere.
    ///
    /// This is `find_sessions_for_blame`'s suffix match replaced by a containment test. The
    /// suffix match is right for "who touched a file called `engine.rs`" and wrong for "who
    /// touched a file in *this* checkout" — measured on a real index, a bare `Cargo.lock`
    /// suffix-matches every Rust repository on the disk, which turned a 2.6% flag rate into a
    /// 33% one that was 90% false (`docs/specs/suspect-commits.md`). The anchoring rule lives
    /// in `crate::anchoring` with its own tests.
    ///
    /// `since` bounds `occurred_at`; pass `None` for the whole index. Results are deduplicated
    /// to one row per (session, path) — the most recent touch — and ordered newest first.
    pub fn blame_for_repo(
        &self,
        repo_root: &str,
        since: Option<DateTime<Utc>>,
    ) -> Result<AnchoredBlame> {
        let root = anchoring::normalize_root(repo_root);
        let identity = anchoring::repo_identity(&root);

        // Two candidate sets, unioned by SQL so the whole table is never pulled into memory:
        // paths under this root, and every relative path (relative rows carry no root to filter
        // on, and there are ~156 of them in a 25,206-row index).
        let like_prefix = format!("{}/%", anchoring::like_prefix_escaped(&root));
        let since_str = since.map(|t| t.to_rfc3339());

        let conn = self.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stmt = conn.prepare(
            r#"
            SELECT fm.file_path, fm.action, fm.occurred_at, fm.model,
                   s.session_id, s.adapter, s.source_path, s.models_used, s.primary_outcome
            FROM file_modifications fm
            JOIN sessions s ON s.session_id = fm.session_id
            WHERE (fm.file_path LIKE ?1 ESCAPE '\' OR fm.file_path = ?2
                   OR fm.file_path NOT LIKE '/%')
              AND (?3 IS NULL OR fm.occurred_at >= ?3)
            ORDER BY fm.occurred_at DESC
            "#,
        )?;

        let mut rows = stmt.query(params![like_prefix, root, since_str])?;

        // Keyed by (session, repo-relative path) so a file touched twenty times in one session
        // contributes one row — the most recent, which the DESC ordering makes the first seen.
        let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
        let mut anchored = Vec::new();
        let mut unanchored_rows = 0usize;

        while let Some(row) = rows.next()? {
            let file_path: String = row.get(0)?;
            let source_path: String = row.get(6)?;

            let (anchor, relative) =
                anchoring::anchor_path(&file_path, &root, &identity, &source_path);
            let relative = match (anchor, relative) {
                (anchoring::Anchor::Unanchored, _) => {
                    unanchored_rows += 1;
                    continue;
                }
                (anchoring::Anchor::ElsewhereAbsolute, _) => continue,
                (_, Some(rel)) => rel,
                (_, None) => continue,
            };

            let session_id: String = row.get(4)?;
            if !seen.insert((session_id.clone(), relative.clone())) {
                continue;
            }

            let occurred_str: String = row.get(2)?;
            let models_str: String = row.get(7)?;
            anchored.push(AnchoredBlameRow {
                repo_relative_path: relative,
                file_path,
                session_id,
                adapter: row.get(5)?,
                source_path,
                action: row.get(1)?,
                modified_at: DateTime::parse_from_rfc3339(&occurred_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                model: row.get(3)?,
                models_used: serde_json::from_str::<Vec<String>>(&models_str).unwrap_or_default(),
                primary_outcome: row.get(8)?,
            });
        }

        Ok(AnchoredBlame {
            repo_root: root,
            repo_identity: identity,
            rows: anchored,
            unanchored_rows,
        })
    }
}

/// How many sessions `list_sessions_for_repo` will walk before giving up. Every row is one
/// `extract_repository_or_workspace` call over an already-fetched string, so this is cheap;
/// the budget exists so an index that grows to six figures can't turn one handoff lookup into
/// a full table scan.
const REPO_SCAN_BUDGET: usize = 5_000;

/// What `Storage::list_sessions_for_repo` found, and whether it ran out of budget looking.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RepoSessionPage {
    pub sessions: Vec<SessionSummary>,
    /// True when the newest-first scan hit `REPO_SCAN_BUDGET` rows without finding `limit`
    /// matches -- older sessions for this repo may exist past it. Never true when the
    /// requested number of sessions was found.
    pub scan_exhausted: bool,
}

/// Decode one row of the shared 11-column shape produced by both `find_sessions_for_blame`
/// and `find_files_for_session` into a `BlameMatch`.
fn row_to_blame_match(row: &rusqlite::Row) -> Result<BlameMatch> {
    let session_id: String = row.get(0)?;
    let adapter: String = row.get(1)?;
    let source_path: String = row.get(2)?;
    let started_str: String = row.get(3)?;
    let models_str: String = row.get(4)?;
    let total_tokens: i64 = row.get(5)?;
    let tool_calls_count: i64 = row.get(6)?;
    let file_path: String = row.get(7)?;
    let action: String = row.get(8)?;
    let occurred_str: String = row.get(9)?;
    let model: Option<String> = row.get(10)?;

    let started_at = DateTime::parse_from_rfc3339(&started_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let modified_at = DateTime::parse_from_rfc3339(&occurred_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or(started_at);

    let models_used = serde_json::from_str::<Vec<String>>(&models_str).unwrap_or_default();

    Ok(BlameMatch {
        session_id,
        adapter,
        source_path,
        started_at,
        models_used,
        total_tokens: total_tokens as u64,
        tool_calls_count: tool_calls_count as usize,
        file_path,
        action,
        modified_at,
        model,
    })
}

/// Estimate developer token cost in USD based on standard blended pricing.
///
/// This collapses to `ModelRates::default()` (Claude 3.5 Sonnet's rate) regardless of
/// what model actually ran — correct only when there is genuinely no model to attribute
/// tokens to (e.g. summing across models/adapters in one SQL aggregate). Any call site
/// that has a specific model, or a per-model breakdown, should use
/// `estimate_model_tokens_cost_usd` or `estimate_total_cost_from_per_model_usage` instead;
/// using this on a single known session silently mis-prices every non-Sonnet model.
pub fn estimate_tokens_cost_usd(
    input: u64,
    output: u64,
    cache_read: u64,
    cache_creation: u64,
) -> f64 {
    estimate_model_tokens_cost_usd(None, input, output, cache_read, cache_creation)
}

/// Sum a per-model token usage breakdown into one total cost, pricing each model's own
/// tokens at that model's own real rate. This is what a session's total spend should look
/// like whenever a per-model breakdown (`TraceStats.per_model_token_usage`) is available —
/// never collapse it to `estimate_tokens_cost_usd`'s single blended rate, which mis-prices
/// every model except the default. Mirrors `TraceScorer::compute_per_model_attribution`'s
/// summation for call sites that only need the total, not the full per-model/outcome
/// breakdown a `TraceScorer` computes.
pub fn estimate_total_cost_from_per_model_usage(
    per_model_usage: &BTreeMap<String, TokenUsage>,
) -> f64 {
    per_model_usage
        .iter()
        .map(|(model, usage)| {
            estimate_model_tokens_cost_usd(
                Some(model),
                usage.input_tokens,
                usage.output_tokens,
                usage.cache_read_tokens,
                usage.cache_creation_tokens,
            )
        })
        .sum()
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
    let prompt_preview: Option<String> = row.get(11).ok();
    let compaction_count: i64 = row.get(12).unwrap_or(0);
    let compaction_tokens_dropped: i64 = row.get(13).unwrap_or(0);
    let source_mtime_epoch_secs: Option<i64> = row.get(14).ok();

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
        prompt_preview,
        source_mtime_epoch_secs,
        compaction_count: compaction_count as usize,
        compaction_tokens_dropped: compaction_tokens_dropped as u64,
    })
}

/// Extract repository or workspace name/path from a session source path.
///
/// Lives in `agentworth-schema` now (`agentworth-redaction` needs it too, and shouldn't have
/// to pull in this crate's SQLite dependency to get one pure string function) — re-exported
/// here so existing callers importing it from `agentworth_storage` don't need to change.
pub use agentworth_schema::extract_repository_or_workspace;

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
    use agentworth_schema::{NormalizedEvent, Provenance};
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
        let stats = storage.get_aggregate_stats(false).expect("get stats");
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
    fn test_find_sessions_by_id_prefix() {
        let storage = Storage::open_in_memory().expect("open storage");
        let base_time = Utc::now() - Duration::hours(3);

        // "a_b1" and "aXb1" differ only in whether the second character is a literal
        // underscore or some other single character -- a giveaway if `_` in the prefix
        // were left as a raw SQL LIKE wildcard instead of being escaped.
        for (i, id) in ["abc123", "abc456", "a_b1", "aXb1", "xyz789"].iter().enumerate() {
            let prov = Provenance::new(
                format!("/Users/dev/code/org/repo/.claude/{}.jsonl", id),
                "claude_code",
                100,
                100,
                format!("fp{}", i),
            );
            let trace = AgentWorthTrace::new(*id, "claude_code", prov, base_time + Duration::minutes(i as i64));
            storage.upsert_trace(&trace).expect("upsert");
        }

        // Unique prefix resolves to exactly one session.
        let unique = storage.find_sessions_by_id_prefix("abc1", 10).expect("prefix query");
        assert_eq!(unique.len(), 1);
        assert_eq!(unique[0].session_id, "abc123");

        // Ambiguous prefix returns every match, newest first.
        let ambiguous = storage.find_sessions_by_id_prefix("abc", 10).expect("prefix query");
        assert_eq!(ambiguous.len(), 2);
        assert_eq!(ambiguous[0].session_id, "abc456");
        assert_eq!(ambiguous[1].session_id, "abc123");

        // A literal underscore in the prefix must not act as a SQL LIKE single-char
        // wildcard: "a_" matches "a_b1" only, not "aXb1".
        let escaped = storage.find_sessions_by_id_prefix("a_", 10).expect("prefix query");
        assert_eq!(escaped.len(), 1);
        assert_eq!(escaped[0].session_id, "a_b1");

        // No match at all.
        let none = storage.find_sessions_by_id_prefix("nope", 10).expect("prefix query");
        assert!(none.is_empty());

        // `limit` caps the result set.
        let capped = storage.find_sessions_by_id_prefix("a", 1).expect("prefix query");
        assert_eq!(capped.len(), 1);
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
        trace1.stats.total_events = 10;
        trace1.stats.token_usage = TokenUsage::new(5000, 1000, 20000, 0);
        trace1.stats.models_used = vec!["claude-3-5-sonnet".to_string()];
        storage.upsert_trace(&trace1).expect("upsert 1");

        let prov2 = Provenance::new("/path/s2.jsonl", "codex", 100, 100, "fp2");
        let mut trace2 =
            AgentWorthTrace::new("sess_old", "codex", prov2, now - Duration::hours(10));
        trace2.stats.total_events = 10;
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
    fn test_session_model_usage_breakdown_and_rescan_replacement() {
        let storage = Storage::open_in_memory().expect("open storage");
        let started_at = DateTime::parse_from_rfc3339("2026-08-30T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        let prov = Provenance::new("/path/multi_model.jsonl", "claude_code", 100, 100, "fp_multi");
        let mut trace = AgentWorthTrace::new("sess_multi", "claude_code", prov, started_at);
        trace.stats.total_events = 4;
        trace.stats.models_used = vec![
            "claude-opus-5".to_string(),
            "claude-fable-5".to_string(),
        ];
        trace
            .stats
            .per_model_token_usage
            .insert("claude-opus-5".to_string(), TokenUsage::new(600, 160, 200, 50));
        trace
            .stats
            .per_model_token_usage
            .insert("claude-fable-5".to_string(), TokenUsage::new(300, 80, 0, 0));
        trace.stats.token_usage = TokenUsage::new(900, 240, 200, 50);

        storage.upsert_trace(&trace).expect("upsert multi-model trace");

        // 1. Per-session breakdown is queryable and exact per model.
        let breakdown = storage
            .get_session_model_usage("sess_multi")
            .expect("get session model usage");
        assert_eq!(breakdown.len(), 2);
        let as_map: std::collections::BTreeMap<_, _> = breakdown.into_iter().collect();
        assert_eq!(
            as_map.get("claude-opus-5"),
            Some(&TokenUsage::new(600, 160, 200, 50))
        );
        assert_eq!(
            as_map.get("claude-fable-5"),
            Some(&TokenUsage::new(300, 80, 0, 0))
        );

        // 2. The period-bucketed rollup groups by model, not adapter, and totals match.
        let rollup = storage.get_model_usage("day", 10).expect("get model usage");
        assert_eq!(rollup.len(), 2);
        let fable_row = rollup
            .iter()
            .find(|r| r.model == "claude-fable-5")
            .expect("fable row present");
        assert_eq!(fable_row.period, "2026-08-30");
        assert_eq!(fable_row.session_count, 1);
        assert_eq!(fable_row.input_tokens, 300);
        assert_eq!(fable_row.output_tokens, 80);
        assert_eq!(fable_row.total_tokens, 380);
        assert!(fable_row.estimated_cost_usd > 0.0);

        let opus_row = rollup
            .iter()
            .find(|r| r.model == "claude-opus-5")
            .expect("opus row present");
        assert_eq!(opus_row.total_tokens, 1010);

        // 3. A rescan (e.g. `agentworth scan --force`) that now attributes everything
        // to a single model must replace the stale per-model rows, not accumulate onto them.
        let prov2 = Provenance::new("/path/multi_model.jsonl", "claude_code", 100, 100, "fp_multi");
        let mut rescanned = AgentWorthTrace::new("sess_multi", "claude_code", prov2, started_at);
        rescanned.stats.total_events = 4;
        rescanned.stats.models_used = vec!["claude-opus-5".to_string()];
        rescanned
            .stats
            .per_model_token_usage
            .insert("claude-opus-5".to_string(), TokenUsage::new(900, 240, 200, 50));
        rescanned.stats.token_usage = TokenUsage::new(900, 240, 200, 50);

        storage.upsert_trace(&rescanned).expect("upsert rescanned trace");

        let after_rescan = storage
            .get_session_model_usage("sess_multi")
            .expect("get session model usage after rescan");
        assert_eq!(after_rescan.len(), 1);
        assert_eq!(after_rescan[0].0, "claude-opus-5");
        assert_eq!(after_rescan[0].1, TokenUsage::new(900, 240, 200, 50));
    }

    #[test]
    fn test_find_sessions_for_blame() {
        use agentworth_schema::NormalizedEvent;

        let storage = Storage::open_in_memory().expect("open storage");

        // Deliberately does NOT mention "engine.rs" anywhere, so a match can only come from a
        // real recorded FileAction event, not the old (buggy) source_path/metadata substring hack.
        let prov = Provenance::new(
            "/Users/dev/.claude/projects/-Users-dev-code-motionvector/sess-blame-1.jsonl",
            "claude_code",
            100,
            100,
            "fp_blame",
        );
        let start = Utc::now() - Duration::minutes(5);
        let mut trace = AgentWorthTrace::new("sess_blame_1", "claude_code", prov, start);
        trace.stats.models_used = vec!["claude-3-5-sonnet".to_string()];
        trace.stats.tools_used.insert("Edit".to_string(), 1);

        let edit_ts = start + Duration::minutes(1);
        trace.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::ModelInvocation {
                model: "claude-3-5-sonnet".to_string(),
                token_usage: TokenUsage::new(100, 20, 0, 0),
                cost_usd: None,
                latency_ms: None,
            },
        ));
        trace.events.push(NormalizedEvent::new(
            2,
            edit_ts,
            EventPayload::FileAction {
                path: "crates/engine/src/engine.rs".to_string(),
                action: FileActionType::Edit,
                diff: None,
                lines_changed: None,
            },
        ));
        storage.upsert_trace(&trace).expect("upsert");

        let matches = storage
            .find_sessions_for_blame("engine.rs")
            .expect("blame matches");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].session_id, "sess_blame_1");
        assert_eq!(matches[0].adapter, "claude_code");
        assert_eq!(matches[0].file_path, "crates/engine/src/engine.rs");
        assert_eq!(matches[0].action, "edit");
        assert_eq!(matches[0].model.as_deref(), Some("claude-3-5-sonnet"));
        assert_eq!(matches[0].modified_at.timestamp(), edit_ts.timestamp());

        // Rescanning (upsert again) must replace, not duplicate, file_modifications rows.
        storage.upsert_trace(&trace).expect("upsert again");
        let matches_after_rescan = storage
            .find_sessions_for_blame("engine.rs")
            .expect("blame matches after rescan");
        assert_eq!(matches_after_rescan.len(), 1);

        let no_match = storage
            .find_sessions_for_blame("no_such_file_anywhere.rs")
            .expect("blame matches for absent file");
        assert!(no_match.is_empty());
    }

    #[test]
    fn test_find_files_for_session() {
        use agentworth_schema::NormalizedEvent;

        let storage = Storage::open_in_memory().expect("open storage");

        let prov = Provenance::new(
            "/Users/dev/.claude/projects/-Users-dev-code-motionvector/sess-blame-2.jsonl",
            "claude_code",
            100,
            100,
            "fp_blame2",
        );
        let start = Utc::now() - Duration::minutes(10);
        let mut trace = AgentWorthTrace::new("sess_blame_2", "claude_code", prov, start);
        trace.stats.models_used = vec!["claude-opus-5".to_string()];

        // Touch engine.rs twice (edit then write) -- only the later write should survive
        // the per-file ROW_NUMBER() dedup, mirroring find_sessions_for_blame's own dedup.
        trace.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::ModelInvocation {
                model: "claude-opus-5".to_string(),
                token_usage: TokenUsage::new(50, 10, 0, 0),
                cost_usd: None,
                latency_ms: None,
            },
        ));
        trace.events.push(NormalizedEvent::new(
            2,
            start + Duration::minutes(1),
            EventPayload::FileAction {
                path: "crates/engine/src/engine.rs".to_string(),
                action: FileActionType::Edit,
                diff: None,
                lines_changed: None,
            },
        ));
        let final_touch = start + Duration::minutes(2);
        trace.events.push(NormalizedEvent::new(
            3,
            final_touch,
            EventPayload::FileAction {
                path: "crates/engine/src/engine.rs".to_string(),
                action: FileActionType::Write,
                diff: None,
                lines_changed: None,
            },
        ));
        // A second, distinct file touched once.
        trace.events.push(NormalizedEvent::new(
            4,
            start + Duration::minutes(3),
            EventPayload::FileAction {
                path: "crates/engine/src/lib.rs".to_string(),
                action: FileActionType::Read,
                diff: None,
                lines_changed: None,
            },
        ));
        storage.upsert_trace(&trace).expect("upsert");

        let files = storage
            .find_files_for_session("sess_blame_2")
            .expect("files for session");
        assert_eq!(files.len(), 2, "engine.rs dedups to its latest touch, plus lib.rs");

        let engine = files
            .iter()
            .find(|f| f.file_path == "crates/engine/src/engine.rs")
            .expect("engine.rs present");
        assert_eq!(engine.action, "write", "later Write must win over the earlier Edit");
        assert_eq!(engine.modified_at.timestamp(), final_touch.timestamp());
        assert_eq!(engine.session_id, "sess_blame_2");
        assert_eq!(engine.model.as_deref(), Some("claude-opus-5"));

        let lib = files
            .iter()
            .find(|f| f.file_path == "crates/engine/src/lib.rs")
            .expect("lib.rs present");
        assert_eq!(lib.action, "read");

        // A session with zero recorded file touches returns an empty list, not an error.
        let prov_empty = Provenance::new("/tmp/empty.jsonl", "claude_code", 10, 10, "fp_empty");
        let empty_trace = AgentWorthTrace::new(
            "sess_no_files",
            "claude_code",
            prov_empty,
            Utc::now(),
        );
        storage.upsert_trace(&empty_trace).expect("upsert empty");
        let no_files = storage
            .find_files_for_session("sess_no_files")
            .expect("files for session with none");
        assert!(no_files.is_empty());

        // An unknown session_id is also an empty list, not an error.
        let unknown = storage
            .find_files_for_session("no_such_session_anywhere")
            .expect("files for unknown session");
        assert!(unknown.is_empty());
    }

    /// Seed one session that touched `path`, with `source_path` deciding which repository the
    /// session itself belongs to.
    fn seed_file_touch(
        storage: &Storage,
        session_id: &str,
        source_path: &str,
        path: &str,
        at: DateTime<Utc>,
    ) {
        use agentworth_schema::NormalizedEvent;

        let prov = Provenance::new(source_path, "claude_code", 100, 100, format!("fp_{session_id}"));
        let mut trace = AgentWorthTrace::new(session_id, "claude_code", prov, at);
        trace.stats.models_used = vec!["claude-opus-5".to_string()];
        trace.events.push(NormalizedEvent::new(
            1,
            at,
            EventPayload::FileAction {
                path: path.to_string(),
                action: FileActionType::Edit,
                diff: None,
                lines_changed: None,
            },
        ));
        storage.upsert_trace(&trace).expect("upsert");
    }

    /// The measured trap from `docs/specs/suspect-commits.md`, reproduced end to end: a bare
    /// relative `Cargo.lock`, written by a session that worked in a *different* repository,
    /// suffix-matches this repository and must be excluded — and counted, not silently dropped.
    #[test]
    fn test_blame_for_repo_excludes_the_cargo_lock_collision() {
        let storage = Storage::open_in_memory().expect("open storage");
        let root = "/Users/dev/code/unfoundbox/agentworth";
        let now = Utc::now();

        seed_file_touch(
            &storage,
            "sess_here_abs",
            "/Users/dev/.claude/projects/-Users-dev-code-unfoundbox-agentworth/a.jsonl",
            &format!("{root}/crates/storage/src/lib.rs"),
            now - Duration::minutes(10),
        );
        seed_file_touch(
            &storage,
            "sess_elsewhere_rel",
            "/Users/dev/.local/share/opencode/project/-Users-dev-code-motionvector-studio/b.json",
            "Cargo.lock",
            now - Duration::minutes(8),
        );
        seed_file_touch(
            &storage,
            "sess_elsewhere_abs",
            "/Users/dev/.claude/projects/-Users-dev-code-motionvector-studio/c.jsonl",
            "/Users/dev/code/motionvector/studio/Cargo.lock",
            now - Duration::minutes(6),
        );

        let found = storage.blame_for_repo(root, None).expect("anchored blame");

        assert_eq!(found.repo_identity, "unfoundbox/agentworth");
        assert_eq!(
            found.rows.len(),
            1,
            "only the absolute in-repo touch is evidence about this repo, got {:?}",
            found.rows.iter().map(|r| &r.file_path).collect::<Vec<_>>()
        );
        assert_eq!(found.rows[0].session_id, "sess_here_abs");
        assert_eq!(found.rows[0].repo_relative_path, "crates/storage/src/lib.rs");

        // The other repo's *relative* row could not be placed, so it is reported.
        assert_eq!(found.unanchored_rows, 1);
        // The other repo's *absolute* row was placed — just not here — so it is not an
        // unanchored row. Anchored elsewhere is a different fact from unplaceable.
    }

    #[test]
    fn test_blame_for_repo_anchors_a_relative_path_from_this_repos_own_session() {
        let storage = Storage::open_in_memory().expect("open storage");
        let root = "/Users/dev/code/unfoundbox/agentworth";

        seed_file_touch(
            &storage,
            "sess_here_rel",
            "/Users/dev/.local/share/opencode/project/-Users-dev-code-unfoundbox-agentworth/d.json",
            "Cargo.lock",
            Utc::now() - Duration::minutes(3),
        );

        let found = storage.blame_for_repo(root, None).expect("anchored blame");
        assert_eq!(found.rows.len(), 1);
        assert_eq!(found.rows[0].repo_relative_path, "Cargo.lock");
        assert_eq!(found.unanchored_rows, 0);
    }

    #[test]
    fn test_blame_for_repo_sibling_prefix_and_since_bound() {
        let storage = Storage::open_in_memory().expect("open storage");
        let root = "/Users/dev/code/unfoundbox/agentworth";
        let now = Utc::now();

        // A sibling checkout whose path shares the root as a *string* prefix.
        seed_file_touch(
            &storage,
            "sess_sibling",
            "/Users/dev/.claude/projects/-Users-dev-code-unfoundbox-agentworth-web/e.jsonl",
            "/Users/dev/code/unfoundbox/agentworth-web/Cargo.lock",
            now - Duration::minutes(30),
        );
        seed_file_touch(
            &storage,
            "sess_old",
            "/Users/dev/.claude/projects/-Users-dev-code-unfoundbox-agentworth/f.jsonl",
            &format!("{root}/README.md"),
            now - Duration::hours(48),
        );
        seed_file_touch(
            &storage,
            "sess_recent",
            "/Users/dev/.claude/projects/-Users-dev-code-unfoundbox-agentworth/g.jsonl",
            &format!("{root}/README.md"),
            now - Duration::hours(2),
        );

        let all = storage.blame_for_repo(root, None).expect("anchored blame");
        let ids: Vec<&str> = all.rows.iter().map(|r| r.session_id.as_str()).collect();
        assert!(!ids.contains(&"sess_sibling"), "agentworth-web is a different tree");
        assert_eq!(ids.len(), 2);

        let recent = storage
            .blame_for_repo(root, Some(now - Duration::hours(24)))
            .expect("anchored blame since");
        assert_eq!(recent.rows.len(), 1);
        assert_eq!(recent.rows[0].session_id, "sess_recent");
    }

    #[test]
    fn test_primary_outcome_and_composite_score_persistence() {
        let storage = Storage::open_in_memory().expect("open storage");
        let prov = Provenance::new("/path/to/log.jsonl", "claude_code", 100, 100, "fp_test");
        let mut trace = AgentWorthTrace::new("sess_verdict_1", "claude_code", prov, Utc::now());
        trace.stats.total_events = 10;
        trace.stats.token_usage = TokenUsage::new(1000, 200, 0, 0);

        storage
            .upsert_session(&trace, Some("commit_observed"), Some(0.88), 1)
            .expect("upsert session");

        let loaded = storage
            .get_session_by_id("sess_verdict_1")
            .expect("get session")
            .expect("found");
        assert_eq!(loaded.primary_outcome.as_deref(), Some("commit_observed"));
        assert_eq!(loaded.composite_score, Some(0.88));

        let list = storage
            .list_sessions_filtered(&SessionFilter {
                outcome: Some("commit_observed".to_string()),
                ..Default::default()
            })
            .expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].primary_outcome.as_deref(), Some("commit_observed"));
        assert_eq!(list[0].composite_score, Some(0.88));
    }

    #[test]
    fn test_verified_outcomes_count_and_distribution() {
        let storage = Storage::open_in_memory().expect("open storage");

        let specs = [
            ("s1", Some("ci_or_deployment_verified"), 0.98, 1000u64),
            ("s2", Some("commit_observed"), 0.85, 2000u64),
            ("s3", Some("test_or_build_passed"), 0.75, 3000u64),
            ("s4", Some("artifact_changed"), 0.50, 4000u64),
            ("s5", Some("done_claimed"), 0.20, 5000u64),
            ("s6", None, 0.0, 6000u64),
        ];

        for (id, outcome, score, tokens) in specs {
            let prov = Provenance::new(format!("/path/{}.jsonl", id), "claude_code", 100, 100, format!("fp_{}", id));
            let mut trace = AgentWorthTrace::new(id, "claude_code", prov, Utc::now());
            trace.stats.total_events = 5;
            trace.stats.token_usage = TokenUsage::new(tokens, 100, 0, 0);
            storage
                .upsert_session(&trace, outcome, Some(score), 1)
                .expect("upsert");
        }

        let stats = storage.get_aggregate_stats(false).expect("aggregate stats");
        assert_eq!(stats.total_sessions, 6);
        // Verified count should only include: ci_or_deployment_verified, commit_observed, test_or_build_passed
        assert_eq!(stats.verified_outcomes_count, 3);

        let dist = storage.get_outcome_distribution().expect("distribution");
        assert_eq!(dist.len(), 6);
        let dist_map: BTreeMap<String, usize> = dist.into_iter().map(|(outcome, count, _, _)| (outcome, count)).collect();
        assert_eq!(dist_map.get("ci_or_deployment_verified"), Some(&1));
        assert_eq!(dist_map.get("commit_observed"), Some(&1));
        assert_eq!(dist_map.get("test_or_build_passed"), Some(&1));
        assert_eq!(dist_map.get("artifact_changed"), Some(&1));
        assert_eq!(dist_map.get("done_claimed"), Some(&1));
        assert_eq!(dist_map.get("Unresolved"), Some(&1));
    }

    #[test]
    fn test_compaction_count_and_dropped_tokens_persistence() {
        let storage = Storage::open_in_memory().expect("open storage");

        // Never-compacted session: must read back as 0, not absent/error -- matches
        // tool_calls_count's own convention (see SessionSummary's doc comment).
        let prov1 = Provenance::new("/path/plain.jsonl", "claude_code", 100, 100, "fp_plain");
        let mut plain = AgentWorthTrace::new("sess_plain", "claude_code", prov1, Utc::now());
        plain.stats.total_events = 3;
        plain.stats.token_usage = TokenUsage::new(50, 20, 0, 0);
        storage.upsert_trace(&plain).expect("upsert plain");

        let loaded_plain = storage
            .get_session_by_id("sess_plain")
            .expect("get session")
            .expect("found");
        assert_eq!(loaded_plain.compaction_count, 0);
        assert_eq!(loaded_plain.compaction_tokens_dropped, 0);

        // Compacted session: values set on trace.stats (as ClaudeCodeAdapter::parse's
        // recalculate_stats call would produce) must round-trip through both
        // get_session_by_id and list_sessions_filtered.
        let prov2 = Provenance::new("/path/compacted.jsonl", "claude_code", 100, 100, "fp_compacted");
        let mut compacted = AgentWorthTrace::new("sess_compacted", "claude_code", prov2, Utc::now());
        compacted.stats.total_events = 20;
        compacted.stats.token_usage = TokenUsage::new(5000, 1200, 0, 0);
        compacted.stats.compaction_count = 3;
        compacted.stats.compaction_tokens_dropped = 1_234_567;
        storage.upsert_trace(&compacted).expect("upsert compacted");

        let loaded_compacted = storage
            .get_session_by_id("sess_compacted")
            .expect("get session")
            .expect("found");
        assert_eq!(loaded_compacted.compaction_count, 3);
        assert_eq!(loaded_compacted.compaction_tokens_dropped, 1_234_567);

        let list = storage
            .list_sessions_filtered(&SessionFilter::default())
            .expect("list");
        let listed_compacted = list
            .iter()
            .find(|s| s.session_id == "sess_compacted")
            .expect("compacted session present in list");
        assert_eq!(listed_compacted.compaction_count, 3);
        assert_eq!(listed_compacted.compaction_tokens_dropped, 1_234_567);

        // A rescan (upsert of the same session_id again, e.g. after a fix landed
        // upstream) must overwrite, not accumulate.
        compacted.stats.compaction_count = 4;
        compacted.stats.compaction_tokens_dropped = 2_000_000;
        storage.upsert_trace(&compacted).expect("re-upsert compacted");
        let rescanned = storage
            .get_session_by_id("sess_compacted")
            .expect("get session")
            .expect("found");
        assert_eq!(rescanned.compaction_count, 4);
        assert_eq!(rescanned.compaction_tokens_dropped, 2_000_000);
    }

    #[test]
    fn test_get_compaction_outcome_correlation() {
        let storage = Storage::open_in_memory().expect("open storage");

        // (session_id, primary_outcome, compaction_count)
        let specs: [(&str, Option<&str>, usize); 6] = [
            ("c1", Some("commit_observed"), 2),
            ("c2", Some("done_claimed"), 5),
            ("c3", Some("done_claimed"), 1),
            ("u1", Some("commit_observed"), 0),
            ("u2", Some("commit_observed"), 0),
            ("u3", None, 0),
        ];

        for (id, outcome, compactions) in specs {
            let prov = Provenance::new(format!("/path/{id}.jsonl"), "claude_code", 100, 100, format!("fp_{id}"));
            let mut trace = AgentWorthTrace::new(id, "claude_code", prov, Utc::now());
            trace.stats.total_events = 5;
            trace.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
            trace.stats.compaction_count = compactions;
            storage
                .upsert_session(&trace, outcome, Some(0.5), 1)
                .expect("seed row");
        }

        let buckets = storage
            .get_compaction_outcome_correlation()
            .expect("correlation query");

        let find = |compacted: bool, outcome: &str| -> Option<usize> {
            buckets
                .iter()
                .find(|b| b.compacted == compacted && b.outcome == outcome)
                .map(|b| b.session_count)
        };

        // Compacted bucket: c1 (commit_observed), c2 + c3 (done_claimed).
        assert_eq!(find(true, "commit_observed"), Some(1));
        assert_eq!(find(true, "done_claimed"), Some(2));
        assert_eq!(find(true, "Unresolved"), None);

        // Uncompacted bucket: u1 + u2 (commit_observed), u3 (Unresolved).
        assert_eq!(find(false, "commit_observed"), Some(2));
        assert_eq!(find(false, "Unresolved"), Some(1));
        assert_eq!(find(false, "done_claimed"), None);

        // Every session accounted for exactly once.
        let total: usize = buckets.iter().map(|b| b.session_count).sum();
        assert_eq!(total, 6);
    }

    /// Real regression test for the PascalCase/snake_case `primary_outcome` encoding bug: builds
    /// a file-backed fixture DB with rows written the way the old, buggy `outcome_kind_name`
    /// wrote them (PascalCase, e.g. "CommitObserved"), then re-runs `initialize_schema` — exactly
    /// what happens the next time a real `~/.agentworth/agentworth.db` is opened by the fixed
    /// binary — and asserts every legacy row is corrected to the new snake_case encoding.
    #[test]
    fn test_migrates_legacy_pascalcase_primary_outcome_on_reopen() {
        let tmp = tempfile::NamedTempFile::new().expect("create fixture db file");
        let storage = Storage::open_path(tmp.path()).expect("open fixture db");

        // Seed rows exactly as the old buggy writer would have: hand-inserted PascalCase,
        // bypassing `outcome_kind_name` entirely so this test doesn't depend on it already
        // being fixed. One row also simulates data that a *fixed* binary already wrote
        // (snake_case) before an older row got migrated, and one row simulates a session that
        // was never scored (NULL) — both must survive untouched.
        let legacy_specs = [
            ("legacy_ci", "CiOrDeploymentVerified"),
            ("legacy_commit", "CommitObserved"),
            ("legacy_test", "TestOrBuildPassed"),
            ("legacy_artifact", "ArtifactChanged"),
            ("legacy_done", "DoneClaimed"),
        ];
        for (id, outcome) in legacy_specs {
            let prov = Provenance::new(format!("/path/{}.jsonl", id), "claude_code", 100, 100, format!("fp_{}", id));
            let mut trace = AgentWorthTrace::new(id, "claude_code", prov, Utc::now());
            trace.stats.total_events = 5;
            trace.stats.token_usage = TokenUsage::new(100, 10, 0, 0);
            storage
                .upsert_session(&trace, Some(outcome), Some(0.5), 1)
                .expect("seed legacy row");
        }

        let prov_new = Provenance::new("/path/already_fixed.jsonl", "claude_code", 100, 100, "fp_already_fixed");
        let mut already_fixed = AgentWorthTrace::new("already_fixed", "claude_code", prov_new, Utc::now());
        already_fixed.stats.total_events = 5;
        already_fixed.stats.token_usage = TokenUsage::new(100, 10, 0, 0);
        storage
            .upsert_session(&already_fixed, Some("commit_observed"), Some(0.9), 1)
            .expect("seed already-migrated row");

        let prov_unscored = Provenance::new("/path/unscored.jsonl", "claude_code", 100, 100, "fp_unscored");
        let mut unscored = AgentWorthTrace::new("unscored", "claude_code", prov_unscored, Utc::now());
        unscored.stats.total_events = 5;
        unscored.stats.token_usage = TokenUsage::new(100, 10, 0, 0);
        storage
            .upsert_session(&unscored, None, None, 1)
            .expect("seed unscored row");

        // Sanity check: the fixture really does carry the old, broken encoding before migration.
        assert_eq!(
            storage
                .get_session_by_id("legacy_commit")
                .unwrap()
                .unwrap()
                .primary_outcome
                .as_deref(),
            Some("CommitObserved"),
            "fixture setup must reproduce the pre-fix PascalCase encoding"
        );

        // Simulate the next process opening this same on-disk database with the fixed binary.
        storage
            .initialize_schema()
            .expect("re-run schema init / migration");

        let expect_outcome = |id: &str, want: Option<&str>| {
            let got = storage.get_session_by_id(id).unwrap().unwrap().primary_outcome;
            assert_eq!(got.as_deref(), want, "unexpected primary_outcome for {id}");
        };
        expect_outcome("legacy_ci", Some("ci_or_deployment_verified"));
        expect_outcome("legacy_commit", Some("commit_observed"));
        expect_outcome("legacy_test", Some("test_or_build_passed"));
        expect_outcome("legacy_artifact", Some("artifact_changed"));
        expect_outcome("legacy_done", Some("done_claimed"));
        // Already-correct and never-scored rows must pass through untouched.
        expect_outcome("already_fixed", Some("commit_observed"));
        expect_outcome("unscored", None);

        // Aggregate queries must now see the corrected values too.
        let stats = storage.get_aggregate_stats(false).expect("aggregate stats");
        assert_eq!(stats.total_sessions, 7);
        // real-verified tiers: legacy_ci, legacy_commit, legacy_test, already_fixed = 4
        assert_eq!(stats.verified_outcomes_count, 4);

        // Idempotency: running the migration again (a second reopen) must not change anything
        // further — the WHERE clause can no longer match any row.
        storage
            .initialize_schema()
            .expect("re-run schema init a second time");
        expect_outcome("legacy_commit", Some("commit_observed"));
        expect_outcome("already_fixed", Some("commit_observed"));
        expect_outcome("unscored", None);
        let stats_after_second_run = storage.get_aggregate_stats(false).expect("aggregate stats");
        assert_eq!(stats_after_second_run.verified_outcomes_count, 4);
    }

    /// Regression test for the missing `busy_timeout` pragma (docs/DECISION-INBOX.md): with no
    /// timeout set, two independent connections to the same file-backed database (e.g. `serve`
    /// and `scan` running concurrently) would return `SQLITE_BUSY` immediately on a write
    /// collision instead of waiting. Assert the pragma is actually in effect on a real
    /// connection, not just present in the schema SQL text.
    #[test]
    fn test_busy_timeout_pragma_is_set() {
        let storage = Storage::open_in_memory().expect("open storage");
        let conn = storage.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let busy_timeout: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .expect("read busy_timeout pragma");
        assert_eq!(busy_timeout, 5000);
    }

    /// Regression test for mutex-poisoning recovery: `agentworth serve` shares one
    /// `Storage` across every request handler on a multi-threaded Axum server, so a
    /// panic inside one handler while the lock is held must not wedge it for every
    /// other endpoint. Poison the lock on another thread, then confirm a normal
    /// `Storage` call still succeeds instead of panic-propagating the poison.
    #[test]
    fn test_recovers_from_poisoned_mutex() {
        let storage = Storage::open_in_memory().expect("open storage");
        let conn = Arc::clone(&storage.conn);

        let panicked = std::thread::spawn(move || {
            let _guard = conn.lock().unwrap();
            panic!("simulated panic while holding the connection lock");
        })
        .join();
        assert!(panicked.is_err(), "spawned thread should have panicked");
        assert!(
            storage.conn.is_poisoned(),
            "the mutex should now be poisoned"
        );

        storage
            .get_aggregate_stats(false)
            .expect("Storage must recover from a poisoned mutex instead of propagating it");
    }

    /// Regression test for `prompt_preview`: previously always empty (zero Rust population,
    /// existed only in the frontend's mock data/types). Covers extraction of the first
    /// non-empty `UserMessage`, truncation past 200 chars with an ellipsis, an all-whitespace
    /// message being skipped in favor of the next real one, and no `UserMessage` at all.
    #[test]
    fn test_prompt_preview_extracted_from_first_user_message() {
        let storage = Storage::open_in_memory().expect("open storage");
        let start = Utc::now();

        let prov = Provenance::new("/path/short.jsonl", "claude_code", 100, 100, "fp_short");
        let mut trace = AgentWorthTrace::new("short", "claude_code", prov, start);
        trace.stats.total_events = 2;
        trace.stats.token_usage = TokenUsage::new(100, 10, 0, 0);
        trace.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::UserMessage { content: "   \n  ".to_string() },
        ));
        trace.events.push(NormalizedEvent::new(
            2,
            start,
            EventPayload::UserMessage { content: "  fix the flaky test  ".to_string() },
        ));
        storage.upsert_trace(&trace).expect("upsert short");
        let summary = storage.get_session_by_id("short").unwrap().unwrap();
        assert_eq!(summary.prompt_preview.as_deref(), Some("fix the flaky test"));

        let prov_long = Provenance::new("/path/long.jsonl", "claude_code", 100, 100, "fp_long");
        let mut trace_long = AgentWorthTrace::new("long", "claude_code", prov_long, start);
        trace_long.stats.total_events = 1;
        trace_long.stats.token_usage = TokenUsage::new(100, 10, 0, 0);
        trace_long.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::UserMessage { content: "x".repeat(250) },
        ));
        storage.upsert_trace(&trace_long).expect("upsert long");
        let long_summary = storage.get_session_by_id("long").unwrap().unwrap();
        let preview = long_summary.prompt_preview.expect("expected a preview");
        assert_eq!(preview.chars().count(), 201);
        assert!(preview.ends_with('…'));
        assert!(preview.starts_with(&"x".repeat(200)));

        let prov_none = Provenance::new("/path/none.jsonl", "claude_code", 100, 100, "fp_none");
        let mut trace_none = AgentWorthTrace::new("none", "claude_code", prov_none, start);
        trace_none.stats.total_events = 1;
        trace_none.stats.token_usage = TokenUsage::new(100, 10, 0, 0);
        trace_none.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::AssistantMessage { content: "hi".to_string(), thinking: None },
        ));
        storage.upsert_trace(&trace_none).expect("upsert none");
        let none_summary = storage.get_session_by_id("none").unwrap().unwrap();
        assert_eq!(none_summary.prompt_preview, None);
    }

    /// Seeds one row with a user message (so `prompt_preview` is derivable) at the given
    /// score and parser version.
    fn seed_backfill_row(
        storage: &Storage,
        id: &str,
        path: &str,
        composite_score: Option<f64>,
        parser_version: i64,
    ) {
        let start = Utc::now();
        let prov = Provenance::new(path, "claude_code", 100, 100, "fp");
        let mut trace = AgentWorthTrace::new(id, "claude_code", prov, start);
        trace.stats.total_events = 1;
        trace.stats.token_usage = TokenUsage::new(100, 10, 0, 0);
        trace.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::UserMessage { content: "already has a preview".to_string() },
        ));
        storage
            .upsert_session(&trace, Some("done_claimed"), composite_score, parser_version)
            .expect("seed row");
    }

    /// `needs_backfill` is the predicate the scanner uses to reparse an unchanged source
    /// whose row predates a derived-field extractor. It must say yes for a row with an
    /// empty/missing `prompt_preview`, no for a row that already has one, and no for a
    /// source_path with no row at all (a brand-new source is a normal scan, not a backfill).
    #[test]
    fn test_needs_backfill_predicate() {
        let storage = Storage::open_in_memory().expect("open storage");
        let start = Utc::now();

        let prov_missing = Provenance::new("/path/missing.jsonl", "claude_code", 100, 100, "fp_missing");
        let mut trace_missing = AgentWorthTrace::new("missing", "claude_code", prov_missing, start);
        trace_missing.stats.total_events = 1;
        trace_missing.stats.token_usage = TokenUsage::new(100, 10, 0, 0);
        trace_missing.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::AssistantMessage { content: "hi".to_string(), thinking: None },
        ));
        storage
            .upsert_session(&trace_missing, None, Some(0.4), 1)
            .expect("upsert missing");
        assert_eq!(
            storage.needs_backfill("/path/missing.jsonl", 1).unwrap(),
            Some(BackfillReason::MissingDerivedField),
            "a row with no prompt_preview needs backfill"
        );

        seed_backfill_row(&storage, "complete", "/path/complete.jsonl", Some(0.5), 1);
        assert_eq!(
            storage.needs_backfill("/path/complete.jsonl", 1).unwrap(),
            None,
            "a row with a non-empty prompt_preview must not be re-flagged"
        );

        assert_eq!(
            storage.needs_backfill("/path/never-indexed.jsonl", 1).unwrap(),
            None,
            "no existing row is a normal new-source scan, not a backfill"
        );
    }

    fn compacted_trace(session_id: &str, source_path: &str) -> AgentWorthTrace {
        let start = Utc::now();
        let prov = Provenance::new(source_path, "claude_code", 100, 100, "fp_compacted");
        let mut trace = AgentWorthTrace::new(session_id, "claude_code", prov, start);
        trace.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::UserMessage { content: "do the thing".to_string() },
        ));
        trace.events.push(NormalizedEvent::new(
            2,
            start + Duration::seconds(1),
            EventPayload::AssistantMessage { content: "on it".to_string(), thinking: None },
        ));
        trace.events.push(NormalizedEvent::new(
            3,
            start + Duration::seconds(2),
            EventPayload::Compaction(agentworth_schema::CompactionEvent {
                trigger: "manual".to_string(),
                pre_tokens: Some(700_000),
                post_tokens: Some(21_000),
                dropped_tokens: Some(679_000),
                duration_ms: None,
            }),
        ));
        trace.events.push(NormalizedEvent::new(
            4,
            start + Duration::seconds(3),
            EventPayload::Custom {
                kind: agentworth_schema::COMPACT_SUMMARY_KIND.to_string(),
                data: serde_json::json!({"message": {"content": "summary text"}}),
            },
        ));
        trace.recalculate_stats();
        trace
    }

    /// The round-boundary table is written by the same transaction as the session row, and a
    /// rescan replaces it rather than appending -- the failure a plain insert would produce on
    /// the second scan of an unchanged session.
    #[test]
    fn test_compaction_rounds_persist_and_replace_on_rescan() {
        let storage = Storage::open_in_memory().expect("open storage");
        let trace = compacted_trace("sess_rounds", "/path/rounds.jsonl");
        storage.upsert_trace(&trace).expect("upsert");

        let rounds = storage.get_compaction_rounds("sess_rounds").expect("rounds");
        assert_eq!(rounds.len(), 1);
        assert_eq!(rounds[0].round, 1);
        assert_eq!((rounds[0].start_seq, rounds[0].end_seq), (1, 2));
        assert_eq!(rounds[0].summary_seq, Some(4));
        assert_eq!(rounds[0].tokens_before, Some(700_000));
        assert_eq!(rounds[0].summary_tokens, Some(21_000));

        storage.upsert_trace(&trace).expect("rescan");
        assert_eq!(
            storage.get_compaction_rounds("sess_rounds").unwrap().len(),
            1,
            "a rescan must replace the round list, not append to it"
        );

        assert!(
            storage.get_compaction_rounds("no_such_session").unwrap().is_empty(),
            "an unknown session has no rounds, and that is not an error"
        );
    }

    /// A session indexed before `session_compaction` existed carries `compaction_count > 0`
    /// and no round rows. That shape must reparse exactly once: flagged before the reparse,
    /// quiet after it, so the scanner doesn't re-read a 68 MB JSONL on every run forever.
    #[test]
    fn test_needs_backfill_flags_a_compacted_session_with_no_round_rows_once() {
        let storage = Storage::open_in_memory().expect("open storage");
        let trace = compacted_trace("sess_backfill", "/path/backfill.jsonl");
        // Written the way a scan writes it -- scored, and stamped with the parser version --
        // so the only thing this test can trip is the round-boundary condition.
        storage
            .upsert_session(&trace, None, Some(0.5), 1)
            .expect("upsert");

        assert_eq!(
            storage.needs_backfill("/path/backfill.jsonl", 1).unwrap(),
            None,
            "a freshly written row has its rounds already"
        );

        // Reproduce the pre-migration shape: the count without the boundaries.
        {
            let conn = storage.conn.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            conn.execute(
                "DELETE FROM session_compaction WHERE session_id = 'sess_backfill'",
                [],
            )
            .expect("drop round rows");
        }
        assert_eq!(
            storage.needs_backfill("/path/backfill.jsonl", 1).unwrap(),
            Some(BackfillReason::MissingDerivedField),
            "compaction_count > 0 with no round rows is a row that predates the table"
        );

        storage
            .upsert_session(&trace, None, Some(0.5), 1)
            .expect("reparse");
        assert_eq!(
            storage.needs_backfill("/path/backfill.jsonl", 1).unwrap(),
            None,
            "the reparse must settle it -- this predicate fires at most once per session"
        );
    }

    /// The 8,816-NULL-outcome finding: a row that was indexed without ever being scored has
    /// to come back through the scanner once, without `--force`. Scoring always produces a
    /// number, so the flag clears on that one pass and the row is not rescanned forever.
    #[test]
    fn test_needs_backfill_flags_an_unscored_row_once() {
        let storage = Storage::open_in_memory().expect("open storage");

        seed_backfill_row(&storage, "unscored", "/path/unscored.jsonl", None, 1);
        assert_eq!(
            storage.needs_backfill("/path/unscored.jsonl", 1).unwrap(),
            Some(BackfillReason::MissingDerivedField),
            "a row with a NULL composite_score was never scored and must be rescanned"
        );

        seed_backfill_row(&storage, "unscored", "/path/unscored.jsonl", Some(0.0), 1);
        assert_eq!(
            storage.needs_backfill("/path/unscored.jsonl", 1).unwrap(),
            None,
            "a score of 0.0 is still a score -- rescanning must stop after one pass"
        );
    }

    /// A NULL `primary_outcome` is the correct, permanent answer for a trace with no outcome
    /// evidence in it. It must never trigger a backfill on its own, or every scan would
    /// reparse thousands of rows forever.
    #[test]
    fn test_null_primary_outcome_alone_does_not_trigger_backfill() {
        let storage = Storage::open_in_memory().expect("open storage");
        let start = Utc::now();
        let prov = Provenance::new("/path/no-outcome.jsonl", "claude_code", 100, 100, "fp_no");
        let mut trace = AgentWorthTrace::new("no-outcome", "claude_code", prov, start);
        trace.stats.total_events = 2;
        trace.stats.token_usage = TokenUsage::new(100, 10, 0, 0);
        trace.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::UserMessage { content: "what does this do?".to_string() },
        ));
        storage
            .upsert_session(&trace, None, Some(0.12), 1)
            .expect("seed scored row with no outcome");

        assert_eq!(
            storage.needs_backfill("/path/no-outcome.jsonl", 1).unwrap(),
            None,
            "no outcome evidence is a real answer, not a missing field"
        );
    }

    /// A parse fix changes what an unchanged file yields, so the stored row is stale even
    /// though its bytes are not. Bumping the adapter's parser version has to pull it back
    /// through the scanner exactly once.
    #[test]
    fn test_needs_backfill_flags_a_stale_parser_version_once() {
        let storage = Storage::open_in_memory().expect("open storage");

        seed_backfill_row(&storage, "v1row", "/path/v1.jsonl", Some(0.7), 1);
        assert_eq!(
            storage.needs_backfill("/path/v1.jsonl", 1).unwrap(),
            None,
            "a row at the adapter's current version is up to date"
        );
        assert_eq!(
            storage.needs_backfill("/path/v1.jsonl", 2).unwrap(),
            Some(BackfillReason::StaleParserVersion),
            "the adapter now parses this file differently, so the row is stale"
        );

        seed_backfill_row(&storage, "v1row", "/path/v1.jsonl", Some(0.7), 2);
        assert_eq!(
            storage.needs_backfill("/path/v1.jsonl", 2).unwrap(),
            None,
            "the reparse recorded the new version, so it must not repeat"
        );
    }

    /// Every row written before this column existed reads as version 0, below every
    /// adapter's version 1 -- so an index built by an older binary reparses itself once
    /// rather than serving stale parses forever.
    #[test]
    fn test_rows_predating_the_parser_version_column_are_stale() {
        let storage = Storage::open_in_memory().expect("open storage");
        let start = Utc::now();
        let prov = Provenance::new("/path/legacy.jsonl", "claude_code", 100, 100, "fp_legacy");
        let mut trace = AgentWorthTrace::new("legacy", "claude_code", prov, start);
        trace.stats.total_events = 2;
        trace.stats.token_usage = TokenUsage::new(100, 10, 0, 0);
        trace.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::UserMessage { content: "a real prompt".to_string() },
        ));
        // upsert_trace is the pre-scan seeding path: no verdict, no score, no version.
        storage.upsert_trace(&trace).expect("seed legacy row");

        assert_eq!(
            storage.needs_backfill("/path/legacy.jsonl", 1).unwrap(),
            Some(BackfillReason::StaleParserVersion)
        );
    }

    /// Regression test for `source_mtime_epoch_secs`: exposes the existing `sources.mtime`
    /// column (already used for incremental scanning) via `SessionSummary`, through the
    /// `LEFT JOIN sources` added to both `get_session_by_id` and `list_sessions_filtered`.
    #[test]
    fn test_source_mtime_epoch_secs_joined_from_sources_table() {
        let storage = Storage::open_in_memory().expect("open storage");
        let prov = Provenance::new("/path/mtime.jsonl", "claude_code", 100, 1_725_000_000, "fp_mtime");
        let mut trace = AgentWorthTrace::new("mtime_sess", "claude_code", prov, Utc::now());
        trace.stats.total_events = 5;
        trace.stats.token_usage = TokenUsage::new(100, 10, 0, 0);
        storage.upsert_trace(&trace).expect("upsert");

        let by_id = storage.get_session_by_id("mtime_sess").unwrap().unwrap();
        assert_eq!(by_id.source_mtime_epoch_secs, Some(1_725_000_000));

        let listed = storage
            .list_sessions_filtered(&SessionFilter::default())
            .expect("list sessions");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].source_mtime_epoch_secs, Some(1_725_000_000));
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

    /// Regression test for the `get_aggregate_stats`/`list_sessions_filtered` stub-count
    /// mismatch (docs/DECISION-INBOX.md): `get_aggregate_stats` used to run an unconditional
    /// `COUNT(*)`/`SUM(...)` over every row, including stubs, while `list_sessions_filtered`
    /// excludes them by default -- so a dashboard showing `total_sessions` next to a
    /// `list_sessions_filtered`-backed traces count (or a ratio of the two, like a verified
    /// rate) saw two numbers that looked like they should describe the same population but
    /// didn't. Seeds a mix of stub and real sessions -- including a stub that itself carries a
    /// "verified" outcome, so a fix that only touches `total_sessions` and forgets
    /// `verified_outcomes_count` still fails this test.
    #[test]
    fn test_aggregate_stats_matches_list_sessions_filtered_population() {
        let storage = Storage::open_in_memory().expect("open storage");

        // Two stubs: one under the event-count floor, one with zero tokens. Neither should
        // count toward the non-stub population, even though one of them (stub_verified) has a
        // "verified" primary_outcome -- verified_outcomes_count must exclude it too, or the
        // verified/total ratio silently mixes populations.
        let stub_low_events_prov =
            Provenance::new("/path/stub_low_events.jsonl", "claude_code", 100, 100, "fp_stub_low_events");
        let mut stub_low_events =
            AgentWorthTrace::new("stub_low_events", "claude_code", stub_low_events_prov, Utc::now());
        stub_low_events.stats.total_events = 1;
        stub_low_events.stats.token_usage = TokenUsage::new(500, 50, 0, 0);
        storage.upsert_trace(&stub_low_events).expect("upsert stub_low_events");

        let stub_verified_prov =
            Provenance::new("/path/stub_verified.jsonl", "codex", 100, 100, "fp_stub_verified");
        let mut stub_verified =
            AgentWorthTrace::new("stub_verified", "codex", stub_verified_prov, Utc::now());
        stub_verified.stats.total_events = 5;
        stub_verified.stats.token_usage = TokenUsage::new(0, 0, 0, 0);
        storage
            .upsert_session(&stub_verified, Some("commit_observed"), Some(0.9), 1)
            .expect("upsert stub_verified");

        // Three real sessions across two adapters, one carrying a verified outcome.
        let real_specs = [
            ("real_1", "claude_code", Some("ci_or_deployment_verified")),
            ("real_2", "claude_code", None),
            ("real_3", "codex", Some("done_claimed")),
        ];
        for (id, adapter, outcome) in real_specs {
            let prov = Provenance::new(format!("/path/{id}.jsonl"), adapter, 100, 100, format!("fp_{id}"));
            let mut trace = AgentWorthTrace::new(id, adapter, prov, Utc::now());
            trace.stats.total_events = 10;
            trace.stats.token_usage = TokenUsage::new(1000, 200, 0, 0);
            storage
                .upsert_session(&trace, outcome, Some(0.5), 1)
                .expect("upsert real session");
        }

        // The population get_aggregate_stats(false) must agree with: list_sessions_filtered's
        // own stub-excluded default (what /api/traces and the CLI's `traces` command return).
        let non_stub_sessions = storage
            .list_sessions_filtered(&SessionFilter::default())
            .expect("list non-stub sessions");
        assert_eq!(
            non_stub_sessions.len(),
            3,
            "fixture sanity check: exactly the 3 real_* sessions should pass the non-stub filter"
        );

        let stats_excl = storage
            .get_aggregate_stats(false)
            .expect("aggregate stats excluding stubs");

        assert_eq!(
            stats_excl.total_sessions,
            non_stub_sessions.len(),
            "total_sessions must match list_sessions_filtered's default (non-stub) count"
        );
        assert_eq!(stats_excl.total_sessions, 3);
        assert_eq!(stats_excl.total_events, 30, "sum of total_events over the 3 real sessions only");
        assert_eq!(
            stats_excl.token_usage.total(),
            3 * 1200,
            "token sums must exclude the two stubs' tokens"
        );
        assert_eq!(
            stats_excl.verified_outcomes_count, 1,
            "only real_1's ci_or_deployment_verified should count -- stub_verified's \
             commit_observed must be excluded even though it's a 'verified' outcome kind"
        );
        assert_eq!(stats_excl.sessions_by_adapter.get("claude_code"), Some(&2));
        assert_eq!(stats_excl.sessions_by_adapter.get("codex"), Some(&1));

        // The raw-inventory mode (used by `agentworth scan`'s "Total Indexed" line and
        // `agentworth doctor`) must still see every row, stubs included.
        let stats_incl = storage
            .get_aggregate_stats(true)
            .expect("aggregate stats including stubs");
        assert_eq!(stats_incl.total_sessions, 5, "all 3 real + 2 stub sessions");
        assert_eq!(
            stats_incl.verified_outcomes_count, 2,
            "including stubs, both real_1 and stub_verified count as verified"
        );
    }

    /// `agentworth stats` and `agentworth usage` used to disagree on how many sessions were
    /// indexed because `get_aggregate_stats` excluded stubs (`NON_STUB_SQL_PREDICATE`) while
    /// the usage views (`v_daily_usage`/`v_weekly_usage`/`v_monthly_usage`) and
    /// `get_model_usage` only filtered `started_at > '2020-01-01'` -- same word ("sessions"),
    /// two different populations. Seeds one real session and two stub shapes (one event with
    /// zero tokens; zero events and zero tokens) and asserts every one of these surfaces --
    /// `get_aggregate_stats`, `get_daily_usage`, `get_monthly_usage`, and `get_model_usage` --
    /// counts exactly the one real session, not three.
    #[test]
    fn test_stats_and_usage_surfaces_agree_on_non_stub_population() {
        let storage = Storage::open_in_memory().expect("open storage");
        let started_at = DateTime::parse_from_rfc3339("2026-09-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        // Real session: multiple events, real token usage, one model.
        let real_prov = Provenance::new("/path/real.jsonl", "claude_code", 100, 100, "fp_real");
        let mut real = AgentWorthTrace::new("real_session", "claude_code", real_prov, started_at);
        real.stats.total_events = 8;
        real.stats.token_usage = TokenUsage::new(500, 100, 0, 0);
        real.stats.models_used = vec!["claude-3-5-sonnet".to_string()];
        real.stats
            .per_model_token_usage
            .insert("claude-3-5-sonnet".to_string(), TokenUsage::new(500, 100, 0, 0));
        storage.upsert_trace(&real).expect("upsert real session");

        // Stub shape 1: exactly one event, zero tokens -- the shape PR #68's zero-events-only
        // scanner prune left behind, and that the plain `started_at > '2020-01-01'` usage
        // views counted before this fix.
        let one_event_prov =
            Provenance::new("/path/one_event.jsonl", "claude_code", 100, 100, "fp_one_event");
        let mut one_event_stub =
            AgentWorthTrace::new("one_event_stub", "claude_code", one_event_prov, started_at);
        one_event_stub.stats.total_events = 1;
        one_event_stub.stats.token_usage = TokenUsage::new(0, 0, 0, 0);
        // Also carries a per-model usage row (a model was invoked but produced no usable
        // tokens) -- without the predicate fix, `get_model_usage`'s JOIN would still pick
        // this up via `COUNT(DISTINCT smu.session_id)` and inflate session_count to 2 even
        // though this row is exactly the stub the other assertions in this test exclude.
        one_event_stub.stats.models_used = vec!["claude-3-5-sonnet".to_string()];
        one_event_stub
            .stats
            .per_model_token_usage
            .insert("claude-3-5-sonnet".to_string(), TokenUsage::new(0, 0, 0, 0));
        storage.upsert_trace(&one_event_stub).expect("upsert one-event stub");

        // Stub shape 2: zero events, zero tokens.
        let zero_event_prov =
            Provenance::new("/path/zero_event.jsonl", "claude_code", 100, 100, "fp_zero_event");
        let mut zero_event_stub =
            AgentWorthTrace::new("zero_event_stub", "claude_code", zero_event_prov, started_at);
        zero_event_stub.stats.total_events = 0;
        zero_event_stub.stats.token_usage = TokenUsage::new(0, 0, 0, 0);
        storage.upsert_trace(&zero_event_stub).expect("upsert zero-event stub");

        // Fixture sanity check: 3 rows are actually indexed, stubs included.
        assert_eq!(storage.get_aggregate_stats(true).unwrap().total_sessions, 3);

        let stats = storage.get_aggregate_stats(false).expect("aggregate stats");
        assert_eq!(stats.total_sessions, 1, "stats must count only the real session");

        let daily = storage.get_daily_usage(None).expect("daily usage");
        assert_eq!(daily.len(), 1, "one adapter/day bucket, for the real session only");
        assert_eq!(daily[0].session_count, 1);

        let monthly = storage.get_monthly_usage(None).expect("monthly usage");
        assert_eq!(monthly.len(), 1);
        assert_eq!(monthly[0].session_count, 1);

        let model_usage = storage.get_model_usage("day", 10).expect("model usage");
        assert_eq!(model_usage.len(), 1, "one model bucket, for the real session only");
        assert_eq!(model_usage[0].session_count, 1);
        assert_eq!(model_usage[0].model, "claude-3-5-sonnet");
    }

    /// Seeds one non-stub session for `get_outcome_rate` fixtures: real events/tokens (so it
    /// clears `NON_STUB_SQL_PREDICATE`), a chosen adapter/repo/model set, and a snake_case
    /// `primary_outcome` (or `None` for "claimed nothing").
    fn seed_outcome_session(
        storage: &Storage,
        session_id: &str,
        adapter: &str,
        source_path: &str,
        models: &[&str],
        primary_outcome: Option<&str>,
        started_at: DateTime<Utc>,
    ) {
        let prov = Provenance::new(source_path, adapter, 100, 12345, format!("fp_{session_id}"));
        let mut trace = AgentWorthTrace::new(session_id, adapter, prov, started_at);
        trace.stats.total_events = 5;
        trace.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
        trace.stats.models_used = models.iter().map(|m| m.to_string()).collect();
        storage
            .upsert_session(&trace, primary_outcome, None, 1)
            .expect("seed outcome session");
    }

    /// Fixtures across two models and two repos: `get_outcome_rate` must compute the right
    /// n/verified/rate per group, suppress a group under `min_n` (folding it into
    /// `suppressed_groups` rather than showing a row), and return a real `n: 0,
    /// reason: "no_outcome_detection"` row for a group whose sessions never claimed an
    /// outcome at all -- a different signal than suppression (docs/specs/verified-outcome-rate.md).
    #[test]
    fn test_get_outcome_rate_across_models_and_repos_with_n_floor_suppression() {
        let storage = Storage::open_in_memory().expect("open storage");
        let t0 = Utc::now();

        // repo-a / model-x: 4 claimed (2 verified, 2 not) + 1 never-claimed session.
        seed_outcome_session(
            &storage, "sess-a1", "claude_code", "/home/u/code/org-a/repo-a/s1.jsonl",
            &["model-x"], Some("test_or_build_passed"), t0,
        );
        seed_outcome_session(
            &storage, "sess-a2", "claude_code", "/home/u/code/org-a/repo-a/s2.jsonl",
            &["model-x"], Some("commit_observed"), t0 + Duration::seconds(1),
        );
        seed_outcome_session(
            &storage, "sess-a3", "claude_code", "/home/u/code/org-a/repo-a/s3.jsonl",
            &["model-x"], Some("done_claimed"), t0 + Duration::seconds(2),
        );
        seed_outcome_session(
            &storage, "sess-a4", "claude_code", "/home/u/code/org-a/repo-a/s4.jsonl",
            &["model-x"], Some("artifact_changed"), t0 + Duration::seconds(3),
        );
        seed_outcome_session(
            &storage, "sess-a5", "claude_code", "/home/u/code/org-a/repo-a/s5.jsonl",
            &["model-x"], None, t0 + Duration::seconds(4),
        );

        // repo-b / model-y: 2 claimed (1 verified) -- below a min_n of 3.
        seed_outcome_session(
            &storage, "sess-b1", "claude_code", "/home/u/code/org-b/repo-b/s1.jsonl",
            &["model-y"], Some("ci_or_deployment_verified"), t0 + Duration::seconds(5),
        );
        seed_outcome_session(
            &storage, "sess-b2", "claude_code", "/home/u/code/org-b/repo-b/s2.jsonl",
            &["model-y"], Some("done_claimed"), t0 + Duration::seconds(6),
        );

        // repo-c / model-z / adapter codex: sessions exist, none ever claimed an outcome.
        seed_outcome_session(
            &storage, "sess-c1", "codex", "/home/u/code/org-c/repo-c/s1.jsonl",
            &["model-z"], None, t0 + Duration::seconds(7),
        );
        seed_outcome_session(
            &storage, "sess-c2", "codex", "/home/u/code/org-c/repo-c/s2.jsonl",
            &["model-z"], None, t0 + Duration::seconds(8),
        );

        let min_n = 3;

        // -- repo --
        let by_repo = storage
            .get_outcome_rate(OutcomeRateGroupBy::Repo, None, None, min_n, false)
            .expect("get_outcome_rate by repo");

        assert_eq!(by_repo.suppressed_groups, 1, "repo-b's n=2 must be suppressed under min_n=3");
        assert_eq!(by_repo.rows.len(), 2, "repo-a (a real row) and repo-c (a reason row)");

        let repo_a = by_repo.rows.iter().find(|r| r.key == "org-a/repo-a").expect("repo-a row");
        assert_eq!(repo_a.n, 4);
        assert_eq!(repo_a.verified, 2);
        assert!((repo_a.rate.unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(repo_a.reason, None);
        assert_eq!(repo_a.rungs.get("1"), Some(&1));
        assert_eq!(repo_a.rungs.get("2"), Some(&1));
        assert_eq!(repo_a.rungs.get("3"), Some(&1));
        assert_eq!(repo_a.rungs.get("4"), Some(&1));
        assert_eq!(repo_a.rungs.get("5"), Some(&0));
        let mut session_ids = repo_a.session_ids.clone();
        session_ids.sort();
        assert_eq!(
            session_ids,
            vec!["sess-a1", "sess-a2", "sess-a3", "sess-a4"],
            "session_ids must be exactly the 4 claimed sessions, not the never-claimed sess-a5"
        );
        assert!(!repo_a.session_ids_truncated);

        assert!(
            by_repo.rows.iter().all(|r| r.key != "org-b/repo-b"),
            "a suppressed group must not appear as a row"
        );

        let repo_c = by_repo.rows.iter().find(|r| r.key == "org-c/repo-c").expect("repo-c row");
        assert_eq!(repo_c.n, 0);
        assert_eq!(repo_c.verified, 0);
        assert_eq!(repo_c.rate, None);
        assert_eq!(repo_c.delta_vs_baseline, None);
        assert_eq!(repo_c.reason.as_deref(), Some("no_outcome_detection"));
        assert!(repo_c.session_ids.is_empty());

        // baseline: distinct claimed sessions across the whole window, not fanned by model --
        // 5 claimed (a1..a4, b1, b2) minus... a1-a4 (4) + b1,b2 (2) = 6 claimed, 3 verified
        // (a1 test_or_build_passed, a2 commit_observed, b1 ci_or_deployment_verified).
        assert_eq!(by_repo.baseline.n, 6);
        assert_eq!(by_repo.baseline.verified, 3);
        assert!((by_repo.baseline.rate - 0.5).abs() < 1e-9);
        assert!((repo_a.delta_vs_baseline.unwrap()).abs() < 1e-9, "repo-a's rate equals baseline");

        // receipt: checkable against the real index.
        assert_eq!(by_repo.receipt.non_stub_predicate, NON_STUB_SQL_PREDICATE);
        let expected_last = t0 + Duration::seconds(8);
        let actual_last = by_repo
            .receipt
            .index_last_session_at
            .expect("index_last_session_at must be set for a non-empty index");
        assert!(
            (actual_last - expected_last).num_milliseconds().abs() < 1000,
            "index_last_session_at should be sess-c2's started_at (round-tripped through RFC \
             3339 text storage): expected ~{expected_last}, got {actual_last}"
        );
        assert!(storage.db_path().is_none(), "sanity: this fixture is in-memory");
        assert_eq!(by_repo.receipt.db_path, ":memory:");

        // -- model: model-x clears the floor, model-y is suppressed, model-z is a reason row.
        let by_model = storage
            .get_outcome_rate(OutcomeRateGroupBy::Model, None, None, min_n, false)
            .expect("get_outcome_rate by model");
        assert_eq!(by_model.suppressed_groups, 1);
        let model_x = by_model.rows.iter().find(|r| r.key == "model-x").expect("model-x row");
        assert_eq!(model_x.n, 4);
        assert_eq!(model_x.verified, 2);
        assert!(by_model.rows.iter().all(|r| r.key != "model-y"));
        let model_z = by_model.rows.iter().find(|r| r.key == "model-z").expect("model-z row");
        assert_eq!(model_z.reason.as_deref(), Some("no_outcome_detection"));

        // -- adapter: claude_code (repo-a + repo-b sessions) clears the floor; codex is a
        // reason row (2 sessions, zero claimed).
        let by_adapter = storage
            .get_outcome_rate(OutcomeRateGroupBy::Adapter, None, None, min_n, false)
            .expect("get_outcome_rate by adapter");
        let claude_code = by_adapter
            .rows
            .iter()
            .find(|r| r.key == "claude_code")
            .expect("claude_code row");
        assert_eq!(claude_code.n, 6);
        assert_eq!(claude_code.verified, 3);
        let codex = by_adapter.rows.iter().find(|r| r.key == "codex").expect("codex row");
        assert_eq!(codex.n, 0);
        assert_eq!(codex.reason.as_deref(), Some("no_outcome_detection"));
    }

    /// `since`/`until` narrow the population `get_outcome_rate` counts, the same way they do
    /// for `list_sessions_filtered`; a session outside the window must not appear in any
    /// group's `n`, `rungs`, `session_ids`, or the baseline.
    #[test]
    fn test_get_outcome_rate_since_until_window() {
        let storage = Storage::open_in_memory().expect("open storage");
        let t0 = Utc::now();

        seed_outcome_session(
            &storage, "sess-old", "claude_code", "/home/u/code/org-a/repo-a/old.jsonl",
            &["model-x"], Some("done_claimed"), t0,
        );
        seed_outcome_session(
            &storage, "sess-in-window", "claude_code", "/home/u/code/org-a/repo-a/in.jsonl",
            &["model-x"], Some("test_or_build_passed"), t0 + Duration::hours(1),
        );
        seed_outcome_session(
            &storage, "sess-too-new", "claude_code", "/home/u/code/org-a/repo-a/new.jsonl",
            &["model-x"], Some("commit_observed"), t0 + Duration::hours(5),
        );

        let result = storage
            .get_outcome_rate(
                OutcomeRateGroupBy::Repo,
                Some(t0 + Duration::minutes(30)),
                Some(t0 + Duration::hours(2)),
                1,
                false,
            )
            .expect("get_outcome_rate with a window");

        assert_eq!(result.baseline.n, 1, "only sess-in-window falls inside [since, until]");
        assert_eq!(result.rows.len(), 1, "only one repo has any session inside the window");
        let repo_a = &result.rows[0];
        assert_eq!(repo_a.n, 1);
        assert_eq!(repo_a.session_ids, vec!["sess-in-window"]);
        assert_eq!(result.window.since, Some(t0 + Duration::minutes(30)));
        assert_eq!(result.window.until, Some(t0 + Duration::hours(2)));

        // Omitting `until` reports the effective upper bound as the moment the query ran,
        // not a bare `null` -- the window should never read as "whenever this happened to run."
        let open_ended = storage
            .get_outcome_rate(OutcomeRateGroupBy::Repo, None, None, 1, false)
            .expect("get_outcome_rate with no window");
        assert!(open_ended.window.since.is_none());
        assert!(open_ended.window.until.is_some());
        assert_eq!(open_ended.baseline.n, 3, "no window means every claimed session counts");
    }

    #[test]
    fn test_list_sessions_for_repo_collapses_worktrees_and_orders_newest_first() {
        let storage = Storage::open_in_memory().expect("open storage");
        let base = Utc::now();

        // Three Claude Code project slugs for the SAME repo: the plain checkout and two
        // worktrees. `extract_repository_or_workspace` prunes the `--` suffix, so all three
        // collapse to one key -- which is exactly what carry-forward wants.
        let paths = [
            ("old", "/Users/x/.claude/projects/-Users-x-code-unfoundbox-agentworth/a.jsonl", 0),
            (
                "middle",
                "/Users/x/.claude/projects/-Users-x-code-unfoundbox-agentworth--claude-worktrees-feat-a/b.jsonl",
                60,
            ),
            (
                "newest",
                "/Users/x/.claude/projects/-Users-x-code-unfoundbox-agentworth--claude-worktrees-feat-b/c.jsonl",
                120,
            ),
            ("other", "/Users/x/.claude/projects/-Users-x-code-unfoundbox-memes/d.jsonl", 90),
        ];

        for (id, path, offset_secs) in paths {
            let prov = Provenance::new(path, "claude_code", 100, 1, format!("fp_{id}"));
            let mut trace = AgentWorthTrace::new(
                id,
                "claude_code",
                prov,
                base + Duration::seconds(offset_secs),
            );
            trace.stats.total_events = 5;
            trace.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
            storage.upsert_trace(&trace).expect("seed session");
        }

        let page = storage
            .list_sessions_for_repo("unfoundbox/agentworth", 10)
            .expect("repo lookup");
        let ids: Vec<&str> = page.sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["newest", "middle", "old"],
            "all three worktrees answer to one repo key, newest first, and the other repo \
             is excluded"
        );
        assert!(!page.scan_exhausted);

        // A limit smaller than the match count truncates without claiming exhaustion.
        let page = storage
            .list_sessions_for_repo("unfoundbox/agentworth", 1)
            .expect("repo lookup");
        assert_eq!(page.sessions.len(), 1);
        assert_eq!(page.sessions[0].session_id, "newest");
        assert!(!page.scan_exhausted);

        // An unknown repo is an empty list, not an error.
        let page = storage
            .list_sessions_for_repo("nobody/nothing", 3)
            .expect("repo lookup");
        assert!(page.sessions.is_empty());

        // The receipt's staleness line reads from here, so a write must populate it.
        assert!(
            storage.last_scanned_at().expect("last scanned").is_some(),
            "four upserts must leave a scanned_at behind"
        );
    }

    #[test]
    fn test_last_scanned_at_is_none_on_an_empty_index() {
        let storage = Storage::open_in_memory().expect("open storage");
        assert_eq!(storage.last_scanned_at().expect("last scanned"), None);
    }
}
