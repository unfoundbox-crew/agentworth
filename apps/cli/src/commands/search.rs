//! Semantic latent vector search command for AgentWorth.
//!
//! Subcommand: `archie session search "<query>" [--limit N] [--min-score F] [--kind KIND] [--json]`
//! Embeds the query, tops up the vector store with anything scanned since the last run,
//! ranks by cosine similarity, and renders the matched turns through the `ui` module.

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use agentworth_core::Scanner;
use agentworth_schema::vector::ChunkKind;
use agentworth_storage::vector::VectorStore;
use agentworth_storage::{
    extract_repository_or_workspace, LocalEmbedder, SessionFilter, Storage, TrajectoryChunker,
};
use anyhow::Result;

use crate::ui::views::{self, SearchRow, SearchView};
use crate::ui::Ui;

/// Per-invocation ceiling on how many not-yet-indexed sessions get embedded in one
/// `agwt search` bootstrap pass.
///
/// Embedding is real model inference when the `fastembed` feature is on (ONNX
/// BGE-Small-EN-v1.5 / MiniLM) -- the most expensive per-session step in this
/// codebase. That feature is off by default today (checked: no crate in this
/// workspace enables `agentworth-storage/fastembed`, so `LocalEmbedder` runs the
/// cheap hash fallback in the real build), but this cap is sized for the inference
/// path, since turning that feature on later shouldn't silently turn `agwt search`
/// into an unbounded hang on its first run.
///
/// This bounds one run's latency only, not the corpus. `bootstrap_vector_store` is
/// incremental and resumable -- every invocation tops up whatever's still unindexed,
/// instead of the old one-shot `if total_chunks == 0` gate, which silently stopped
/// indexing forever after the first successful pass. 25,000 sits well above the
/// largest known real index (10,188 sessions, see docs/DECISION-INBOX.md), so a full
/// backfill still finishes in one run for every index seen in practice.
const MAX_SESSIONS_TO_INDEX_PER_RUN: usize = 25_000;

/// Execute the `agwt search` subcommand.
#[allow(clippy::too_many_arguments)]
pub fn run_search_command(
    query: &str,
    limit: usize,
    min_score: f32,
    kind: Option<String>,
    json_output: bool,
    db_path: Option<PathBuf>,
    ui: &Ui,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let vector_store = storage.vector_store()?;
    let embedder = LocalEmbedder::new();

    // 1. Incrementally top up the vector store with any session not yet embedded. This is
    // the one genuinely slow step -- a disk read plus an adapter parse plus an embed per
    // not-yet-indexed session -- and it used to draw an off-brand `indicatif` bar built out
    // of braille spinner glyphs no allowed face carries.
    let bootstrap = crate::ui::with_status(ui, "embedding new sessions", || {
        bootstrap_vector_store(&storage, &vector_store, &embedder, MAX_SESSIONS_TO_INDEX_PER_RUN)
    })?;

    // 2. Generate embedding for search query
    let query_vector = embedder.embed_text(query)?;

    // 3. Parse optional kind filter
    let kind_filter = kind.as_deref().and_then(|k| ChunkKind::from_str(k).ok());

    // 4. Query Vector Store with cosine similarity ranking
    let results =
        vector_store.search_filtered(&query_vector, limit, min_score, None, kind_filter)?;

    // 5. Output rendering
    if json_output {
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(());
    }

    let engine = if embedder.is_onnx() {
        "ONNX (BGE-Small-EN-v1.5)"
    } else {
        "offline semantic vectors (384-dim)"
    };

    // One `get_session_by_id` per row, off the index, for the repo the turn came from.
    let projects: Vec<String> = results
        .iter()
        .map(|res| {
            storage
                .get_session_by_id(&res.session_id)
                .ok()
                .flatten()
                .map(|s| extract_repository_or_workspace(&s.source_path))
                .unwrap_or_else(|| "workspace".to_string())
        })
        .collect();
    let turns: Vec<String> = results
        .iter()
        .map(|res| {
            if res.turn_index > 0 {
                format!("turn #{}", res.turn_index)
            } else {
                "summary".to_string()
            }
        })
        .collect();

    let rows: Vec<SearchRow<'_>> = results
        .iter()
        .zip(projects.iter().zip(turns.iter()))
        .map(|(res, (project, turn))| SearchRow {
            session_id: &res.session_id,
            adapter: &res.adapter,
            model: res.model.as_deref().unwrap_or("unknown"),
            project,
            started_at: res.started_at.as_deref().unwrap_or("unknown"),
            kind: res.kind.as_str(),
            turn,
            total_tokens: res.total_tokens,
            score: res.score,
            snippet: &res.text_content,
        })
        .collect();

    print!(
        "{}",
        views::search(
            ui,
            &SearchView {
                query,
                engine,
                indexed_chunks: bootstrap.chunks_embedded,
                still_pending: bootstrap.sessions_still_pending,
                rows,
            }
        )
    );
    Ok(())
}

fn open_storage(db_path: Option<PathBuf>) -> Result<Arc<Storage>> {
    if let Some(path) = db_path {
        Ok(Arc::new(Storage::open_path(&path)?))
    } else {
        Ok(Arc::new(Storage::open_default()?))
    }
}

/// Result of one incremental vector-store bootstrap pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct BootstrapOutcome {
    /// Sessions this run attempted to embed (took out of the pending pool).
    sessions_embedded: usize,
    /// Trajectory chunks actually inserted into the vector store this run.
    chunks_embedded: usize,
    /// Sessions still not indexed after this run, because they exceeded the cap.
    sessions_still_pending: usize,
}

/// Embed every session not yet present in the vector store, up to
/// `max_sessions_per_run` sessions in this call.
///
/// Runs on every `agwt search` invocation instead of gating on the vector store
/// being empty, so a session scanned after the first successful bootstrap -- or
/// left over from a previous run's cap -- still gets picked up. Finding which
/// sessions are pending is a cheap SQL scan (`list_sessions_filtered` with
/// `limit: None`, matching `SessionFilter::limit`'s "unlimited" contract, plus an
/// indexed-column `SELECT DISTINCT`); only the embedding step below is bounded,
/// because that's the expensive part.
fn bootstrap_vector_store<V: VectorStore>(
    storage: &Arc<Storage>,
    vector_store: &V,
    embedder: &LocalEmbedder,
    max_sessions_per_run: usize,
) -> Result<BootstrapOutcome> {
    let already_indexed = vector_store.indexed_session_ids()?;
    let all_sessions = storage.list_sessions_filtered(&SessionFilter::default())?;
    let mut pending_sessions: Vec<_> = all_sessions
        .into_iter()
        .filter(|s| !already_indexed.contains(&s.session_id))
        .collect();

    if pending_sessions.is_empty() {
        return Ok(BootstrapOutcome::default());
    }

    let total_pending = pending_sessions.len();
    pending_sessions.truncate(max_sessions_per_run);
    let sessions_embedded = pending_sessions.len();
    let sessions_still_pending = total_pending - sessions_embedded;

    let scanner = Scanner::new(Arc::clone(storage));
    let mut chunks_embedded = 0;

    for sess in &pending_sessions {
        if let Ok(trace) = scanner.load_trace(&sess.session_id) {
            let chunks = TrajectoryChunker::extract_chunks(&trace);
            if !chunks.is_empty() {
                let texts: Vec<String> = chunks.iter().map(|c| c.text_content.clone()).collect();
                if let Ok(embeddings) = embedder.embed_batch(&texts) {
                    let _ = vector_store.insert_embeddings(&chunks, &embeddings);
                    chunks_embedded += chunks.len();
                }
            }
        }
    }

    Ok(BootstrapOutcome {
        sessions_embedded,
        chunks_embedded,
        sessions_still_pending,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_adapter_sdk::ScanOptions;
    use agentworth_adapters::ClaudeCodeAdapter;
    use agentworth_storage::vector::SqliteVectorStore;
    use serde_json::json;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Writes a minimal but real Claude Code JSONL session file, matching the shape
    /// `threat_digest.rs`'s tests use -- going through the real adapter parse path
    /// rather than constructing an `AgentWorthTrace` by hand, since `load_trace`
    /// (called inside `bootstrap_vector_store`) re-parses the file from disk.
    fn write_claude_session(user_content: &str, assistant_content: &str) -> NamedTempFile {
        let mut temp = tempfile::Builder::new()
            .suffix(".jsonl")
            .tempfile()
            .unwrap();
        let line1 = json!({
            "type": "user",
            "timestamp": "2026-08-29T10:00:00Z",
            "content": user_content,
        });
        let line2 = json!({
            "type": "assistant",
            "timestamp": "2026-08-29T10:00:05Z",
            "model": "claude-3-5-sonnet",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_read_input_tokens": 0,
                "cache_creation_input_tokens": 0
            },
            "content": [{"type": "text", "text": assistant_content}],
        });
        writeln!(temp, "{}", line1).unwrap();
        writeln!(temp, "{}", line2).unwrap();
        temp
    }

    fn session_id_of(temp: &NamedTempFile) -> String {
        temp.path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string()
    }

    /// Scans exactly one fixture file via `custom_paths`, so this never touches real
    /// session files that might exist on the machine running the test.
    fn scan_one(scanner: &Scanner, temp: &NamedTempFile) {
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: true,
            ..Default::default()
        };
        let summary = scanner.run_scan(&options, |_, _| {}).expect("scan run");
        assert_eq!(summary.scanned_sessions, 1);
    }

    #[test]
    fn test_bootstrap_is_incremental_not_gated_on_vector_store_being_empty() {
        // Regression test for the reported bug: a session scanned after the vector
        // store's first successful bootstrap must still get embedded on the next
        // `agwt search` call, not silently skipped forever because `total_chunks`
        // is already > 0 (the old `if stats.total_chunks == 0` one-shot gate).
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner =
            Scanner::with_adapters(vec![Box::new(ClaudeCodeAdapter::new())], storage.clone());
        let vector_store = SqliteVectorStore::open_in_memory().expect("open vector store");
        let embedder = LocalEmbedder::new_deterministic();

        let session_a = write_claude_session("please add retry logic", "done, added backoff");
        scan_one(&scanner, &session_a);

        let outcome_1 = bootstrap_vector_store(&storage, &vector_store, &embedder, 100)
            .expect("first bootstrap");
        assert_eq!(outcome_1.sessions_embedded, 1);
        assert_eq!(outcome_1.sessions_still_pending, 0);
        assert_eq!(vector_store.stats().unwrap().total_sessions, 1);

        // A second session appears, as if `agwt scan` ran again between two `agwt
        // search` invocations.
        let session_b = write_claude_session(
            "refactor the parser module please",
            "done, split into two files",
        );
        scan_one(&scanner, &session_b);

        let outcome_2 = bootstrap_vector_store(&storage, &vector_store, &embedder, 100)
            .expect("second bootstrap");
        assert_eq!(
            outcome_2.sessions_embedded, 1,
            "the newly-scanned session must be embedded on the very next invocation"
        );
        assert_eq!(vector_store.stats().unwrap().total_sessions, 2);

        let indexed = vector_store.indexed_session_ids().unwrap();
        assert!(indexed.contains(&session_id_of(&session_a)));
        assert!(indexed.contains(&session_id_of(&session_b)));

        // Prove it's not just a row in the table but actually retrievable by search --
        // the exact user-visible symptom the bug report described.
        let query_vector = embedder
            .embed_text("refactor the parser module please")
            .unwrap();
        let results = vector_store
            .search_filtered(&query_vector, 5, 0.0, None, None)
            .expect("search");
        assert!(results
            .iter()
            .any(|r| r.session_id == session_id_of(&session_b)));
    }

    #[test]
    fn test_bootstrap_per_run_cap_truncates_and_resumes_on_next_call() {
        // With a small per-run cap and more pending sessions than the cap, one call
        // must only embed up to the cap and report the rest as still pending; a
        // second call must finish the remainder. Proves the cap bounds a single
        // run's cost without ever becoming a permanent ceiling on the corpus, unlike
        // the old fixed `Some(10000)` bootstrap limit.
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner =
            Scanner::with_adapters(vec![Box::new(ClaudeCodeAdapter::new())], storage.clone());
        let vector_store = SqliteVectorStore::open_in_memory().expect("open vector store");
        let embedder = LocalEmbedder::new_deterministic();

        let sessions: Vec<NamedTempFile> = (0..3)
            .map(|i| {
                let temp = write_claude_session(&format!("task {i}"), &format!("done {i}"));
                scan_one(&scanner, &temp);
                temp
            })
            .collect();

        let outcome_1 = bootstrap_vector_store(&storage, &vector_store, &embedder, 2)
            .expect("first bootstrap");
        assert_eq!(outcome_1.sessions_embedded, 2);
        assert_eq!(outcome_1.sessions_still_pending, 1);
        assert_eq!(vector_store.stats().unwrap().total_sessions, 2);

        let outcome_2 = bootstrap_vector_store(&storage, &vector_store, &embedder, 2)
            .expect("second bootstrap");
        assert_eq!(outcome_2.sessions_embedded, 1);
        assert_eq!(outcome_2.sessions_still_pending, 0);
        assert_eq!(vector_store.stats().unwrap().total_sessions, 3);

        let indexed = vector_store.indexed_session_ids().unwrap();
        for s in &sessions {
            assert!(indexed.contains(&session_id_of(s)));
        }
    }

    #[test]
    fn test_bootstrap_is_noop_when_nothing_pending() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner =
            Scanner::with_adapters(vec![Box::new(ClaudeCodeAdapter::new())], storage.clone());
        let vector_store = SqliteVectorStore::open_in_memory().expect("open vector store");
        let embedder = LocalEmbedder::new_deterministic();

        let outcome = bootstrap_vector_store(&storage, &vector_store, &embedder, 100)
            .expect("bootstrap on empty index");
        assert_eq!(outcome, BootstrapOutcome::default());

        let session = write_claude_session("hello", "hi there");
        scan_one(&scanner, &session);
        bootstrap_vector_store(&storage, &vector_store, &embedder, 100)
            .expect("bootstrap indexes the one session");

        // Nothing new since the last run -- must do no work, not re-embed.
        let outcome_again = bootstrap_vector_store(&storage, &vector_store, &embedder, 100)
            .expect("bootstrap with nothing pending");
        assert_eq!(outcome_again, BootstrapOutcome::default());
    }
}
