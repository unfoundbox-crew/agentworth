//! SQLite-backed vector store implementation.
//!
//! Stores trajectory chunks and their raw float embeddings in SQLite,
//! performing high-efficiency cosine distance vector ranking.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use agentworth_schema::vector::{ChunkKind, TrajectoryChunk, VectorSearchResult, VectorStats};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};

use super::{bytes_to_f32_vec, cosine_similarity, f32_slice_to_bytes, VectorStore};

/// SQLite-backed implementation of `VectorStore`.
pub struct SqliteVectorStore {
    conn: Arc<Mutex<Connection>>,
    db_path: Option<PathBuf>,
}

impl SqliteVectorStore {
    /// Open or create a vector store at the given database file path.
    pub fn open_path(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open SQLite database at {:?}", path))?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: Some(path.to_path_buf()),
        };
        store.initialize()?;
        Ok(store)
    }

    /// Open an ephemeral in-memory vector store (ideal for unit testing).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: None,
        };
        store.initialize()?;
        Ok(store)
    }

    /// Construct a vector store sharing an existing SQLite connection handle.
    pub fn from_shared_connection(conn: Arc<Mutex<Connection>>) -> Result<Self> {
        let store = Self {
            conn,
            db_path: None,
        };
        store.initialize()?;
        Ok(store)
    }

    /// Database file path, if not in-memory.
    pub fn db_path(&self) -> Option<&Path> {
        self.db_path.as_deref()
    }
}

impl VectorStore for SqliteVectorStore {
    fn initialize(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                session_id TEXT PRIMARY KEY,
                adapter TEXT NOT NULL,
                source_path TEXT NOT NULL DEFAULT '',
                fingerprint TEXT NOT NULL DEFAULT '',
                started_at TEXT NOT NULL DEFAULT '',
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
                models_used TEXT NOT NULL DEFAULT '[]',
                tools_used TEXT NOT NULL DEFAULT '{}',
                metadata TEXT,
                scanned_at TEXT NOT NULL DEFAULT ''
            );

            CREATE TABLE IF NOT EXISTS trajectory_chunks (
                chunk_id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                adapter TEXT NOT NULL,
                kind TEXT NOT NULL,
                turn_index INTEGER NOT NULL,
                timestamp TEXT NOT NULL,
                text_content TEXT NOT NULL,
                metadata_json TEXT,
                embedding BLOB
            );

            CREATE INDEX IF NOT EXISTS idx_chunks_session ON trajectory_chunks(session_id);
            CREATE INDEX IF NOT EXISTS idx_chunks_kind ON trajectory_chunks(kind);
            CREATE INDEX IF NOT EXISTS idx_chunks_adapter ON trajectory_chunks(adapter);
            "#,
        )?;

        Ok(())
    }

    fn insert_embeddings(
        &self,
        chunks: &[TrajectoryChunk],
        embeddings: &[Vec<f32>],
    ) -> Result<()> {
        if chunks.len() != embeddings.len() {
            bail!(
                "Mismatch between chunk count ({}) and embedding count ({})",
                chunks.len(),
                embeddings.len()
            );
        }

        if chunks.is_empty() {
            return Ok(());
        }

        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        {
            let mut stmt = tx.prepare(
                r#"
                INSERT INTO trajectory_chunks (
                    chunk_id, session_id, adapter, kind, turn_index, timestamp,
                    text_content, metadata_json, embedding
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                ON CONFLICT(chunk_id) DO UPDATE SET
                    session_id = excluded.session_id,
                    adapter = excluded.adapter,
                    kind = excluded.kind,
                    turn_index = excluded.turn_index,
                    timestamp = excluded.timestamp,
                    text_content = excluded.text_content,
                    metadata_json = excluded.metadata_json,
                    embedding = excluded.embedding;
                "#,
            )?;

            for (chunk, emb) in chunks.iter().zip(embeddings.iter()) {
                let emb_bytes = f32_slice_to_bytes(emb);
                stmt.execute(params![
                    chunk.chunk_id,
                    chunk.session_id,
                    chunk.adapter,
                    chunk.kind.as_str(),
                    chunk.turn_index as i64,
                    chunk.timestamp,
                    chunk.text_content,
                    chunk.metadata_json,
                    emb_bytes,
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    fn search_filtered(
        &self,
        query_vector: &[f32],
        limit: usize,
        min_score: f32,
        adapter_filter: Option<&str>,
        kind_filter: Option<ChunkKind>,
    ) -> Result<Vec<VectorSearchResult>> {
        if query_vector.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let conn = self.conn.lock().unwrap();

        let mut sql = String::from(
            r#"
            SELECT c.chunk_id, c.session_id, c.adapter, c.kind, c.turn_index, c.text_content, c.metadata_json, c.embedding,
                   s.started_at, s.total_tokens, s.models_used
            FROM trajectory_chunks c
            LEFT JOIN sessions s ON c.session_id = s.session_id
            WHERE c.embedding IS NOT NULL
            "#,
        );

        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(adapter) = adapter_filter {
            sql.push_str(" AND c.adapter = ?");
            param_values.push(Box::new(adapter.to_string()));
        }

        if let Some(kind) = kind_filter {
            sql.push_str(" AND c.kind = ?");
            param_values.push(Box::new(kind.as_str().to_string()));
        }

        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|b| b.as_ref()).collect();
        let mut rows = stmt.query(params_refs.as_slice())?;

        let mut scored_results: Vec<VectorSearchResult> = Vec::new();

        while let Some(row) = rows.next()? {
            let chunk_id: String = row.get(0)?;
            let session_id: String = row.get(1)?;
            let adapter: String = row.get(2)?;
            let kind_str: String = row.get(3)?;
            let turn_index: i64 = row.get(4)?;
            let text_content: String = row.get(5)?;
            let metadata_json: Option<String> = row.get(6)?;
            let emb_bytes: Option<Vec<u8>> = row.get(7)?;
            let started_at: Option<String> = row.get(8)?;
            let total_tokens: Option<i64> = row.get(9)?;
            let models_str: Option<String> = row.get(10)?;

            let model = models_str.and_then(|m| {
                serde_json::from_str::<Vec<String>>(&m)
                    .ok()
                    .and_then(|v| v.into_iter().next())
            });

            if let Some(bytes) = emb_bytes {
                let candidate_vec = bytes_to_f32_vec(&bytes);
                let score = cosine_similarity(query_vector, &candidate_vec);

                if score >= min_score {
                    let kind = ChunkKind::from_str(&kind_str).unwrap_or(ChunkKind::SessionSummary);
                    scored_results.push(VectorSearchResult {
                        chunk_id,
                        session_id,
                        adapter,
                        kind,
                        turn_index: turn_index as usize,
                        score,
                        text_content,
                        metadata_json,
                        started_at,
                        total_tokens: total_tokens.unwrap_or(0) as u64,
                        model,
                    });
                }
            }
        }

        // Sort descending by cosine similarity score
        scored_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if scored_results.len() > limit {
            scored_results.truncate(limit);
        }

        Ok(scored_results)
    }

    fn delete_session(&self, session_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM trajectory_chunks WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    fn stats(&self) -> Result<VectorStats> {
        let conn = self.conn.lock().unwrap();

        let mut count_stmt = conn.prepare(
            r#"
            SELECT
                COUNT(*),
                COUNT(DISTINCT session_id)
            FROM trajectory_chunks
            "#,
        )?;

        let mut rows = count_stmt.query([])?;
        let (total_chunks, total_sessions) = if let Some(row) = rows.next()? {
            (row.get::<_, i64>(0)? as usize, row.get::<_, i64>(1)? as usize)
        } else {
            (0, 0)
        };

        // Determine dimension from first available embedding
        let mut dim_stmt = conn.prepare(
            "SELECT LENGTH(embedding) FROM trajectory_chunks WHERE embedding IS NOT NULL LIMIT 1",
        )?;
        let byte_len: Option<i64> = dim_stmt.query_row([], |r| r.get(0)).ok();
        let dimension = byte_len.map(|len| (len / 4) as usize).unwrap_or(384);

        // Group chunks by kind
        let mut kind_stmt = conn.prepare(
            "SELECT kind, COUNT(*) FROM trajectory_chunks GROUP BY kind",
        )?;
        let mut kind_rows = kind_stmt.query([])?;
        let mut chunks_by_kind = std::collections::BTreeMap::new();
        while let Some(r) = kind_rows.next()? {
            let k: String = r.get(0)?;
            let c: i64 = r.get(1)?;
            chunks_by_kind.insert(k, c as usize);
        }

        Ok(VectorStats {
            total_chunks,
            total_sessions,
            dimensions: dimension,
            index_backend: "sqlite-vector".to_string(),
            chunks_by_kind,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqlite_vector_store_crud_and_search() {
        let store = SqliteVectorStore::open_in_memory().expect("open in memory");

        // 1. Check initial stats
        let initial_stats = store.stats().expect("stats");
        assert_eq!(initial_stats.total_chunks, 0);
        assert_eq!(initial_stats.total_sessions, 0);

        // 2. Prepare mock chunks and embeddings
        let chunks = vec![
            TrajectoryChunk::new(
                "sess_100",
                "claude_code",
                ChunkKind::ApologyPanic,
                14,
                "2026-08-30T12:00:00Z",
                "That was my mistake - I used rm -rf, which is a hard rule I should never invoke.",
                "{}",
            ).with_chunk_id("chunk_1"),
            TrajectoryChunk::new(
                "sess_100",
                "claude_code",
                ChunkKind::ErrorRecovery,
                15,
                "2026-08-30T12:01:00Z",
                "Bash tool failed with Permission Denied. Retrying with proper file path.",
                "{}",
            ).with_chunk_id("chunk_2"),
            TrajectoryChunk::new(
                "sess_200",
                "codex",
                ChunkKind::SessionSummary,
                1,
                "2026-08-30T14:00:00Z",
                "Successfully built the React component with Tailwind CSS styling.",
                "{}",
            ).with_chunk_id("chunk_3"),
        ];

        let emb1 = vec![0.9, 0.1, 0.0, 0.0];
        let emb2 = vec![0.1, 0.9, 0.0, 0.0];
        let emb3 = vec![0.0, 0.0, 0.9, 0.1];
        let embeddings = vec![emb1.clone(), emb2.clone(), emb3.clone()];

        store
            .insert_embeddings(&chunks, &embeddings)
            .expect("insert");

        // 3. Stats after insert
        let stats = store.stats().expect("stats");
        assert_eq!(stats.total_chunks, 3);
        assert_eq!(stats.total_sessions, 2);
        assert_eq!(stats.dimensions, 4);

        // 4. Search with query matching chunk_1
        let query1 = vec![0.95, 0.05, 0.0, 0.0];
        let results = store.search(&query1, 2, 0.5).expect("search");
        assert!(!results.is_empty());
        assert_eq!(results[0].chunk_id, "chunk_1");
        assert_eq!(results[0].kind, ChunkKind::ApologyPanic);
        assert!(results[0].score > 0.95);

        // 5. Search with kind filter
        let results_filtered = store
            .search_filtered(&query1, 5, 0.0, None, Some(ChunkKind::ErrorRecovery))
            .expect("search filtered");
        assert_eq!(results_filtered.len(), 1);
        assert_eq!(results_filtered[0].chunk_id, "chunk_2");

        // 6. Delete session
        store.delete_session("sess_100").expect("delete");
        let stats_after_delete = store.stats().expect("stats after delete");
        assert_eq!(stats_after_delete.total_chunks, 1);
        assert_eq!(stats_after_delete.total_sessions, 1);
    }
}
