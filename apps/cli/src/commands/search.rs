//! Semantic latent vector search command for AgentWorth.
//!
//! Subcommand: `agwt search "<query>" [--limit N] [--min-score F] [--kind KIND] [--json]`
//! Generates query embedding vector, queries the VectorStore, and renders ASCII thermal receipt cards.

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use agentworth_core::Scanner;
use agentworth_schema::vector::{ChunkKind, VectorSearchResult};
use agentworth_storage::vector::VectorStore;
use agentworth_storage::{
    extract_repository_or_workspace, LocalEmbedder, SessionFilter, Storage, TrajectoryChunker,
};
use anyhow::Result;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};

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
pub fn run_search_command(
    query: &str,
    limit: usize,
    min_score: f32,
    kind: Option<String>,
    json_output: bool,
    db_path: Option<PathBuf>,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let vector_store = storage.vector_store()?;
    let embedder = LocalEmbedder::new();

    // 1. Incrementally top up the vector store with any session not yet embedded.
    bootstrap_vector_store(
        &storage,
        &vector_store,
        &embedder,
        MAX_SESSIONS_TO_INDEX_PER_RUN,
        json_output,
    )?;
    let stats = vector_store.stats()?;

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

    if results.is_empty() {
        println!();
        println!(
            "{}",
            style(format!(
                "No semantic matches found for query '{}' (min_score: {:.2}).",
                query, min_score
            ))
            .dim()
        );
        if stats.total_chunks == 0 {
            println!(
                "{}",
                style("Tip: Run `archie scan` (or `agwt scan`) first to index local agent histories.").yellow()
            );
        }
        println!();
        return Ok(());
    }

    render_ascii_thermal_receipts(query, &results, &embedder, &storage);

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
    json_output: bool,
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

    if !json_output {
        println!(
            "{}",
            style(format!(
                "⚡ Indexing {} sessions into vector store for latent semantic retrieval...",
                sessions_embedded
            ))
            .dim()
        );
    }

    let scanner = Scanner::new(Arc::clone(storage));
    let mut chunks_embedded = 0;

    let pb = if !json_output {
        let pb = ProgressBar::new(pending_sessions.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                .template(
                    "{spinner:.cyan.bold} Embedding trajectories ▕{bar:30.cyan/238}▏ {pos}/{len} sessions",
                )
                .unwrap()
                .progress_chars("█▉▊▋▌▍▎▏ "),
        );
        Some(pb)
    } else {
        None
    };

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
        if let Some(ref pb) = pb {
            pb.inc(1);
        }
    }

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    if !json_output && chunks_embedded > 0 {
        println!(
            "{}",
            style(format!(
                "✓ Indexed {} trajectory chunks ({} dimensions)",
                chunks_embedded,
                embedder.dimension()
            ))
            .green()
        );
    }

    if !json_output && sessions_still_pending > 0 {
        println!(
            "{}",
            style(format!(
                "Tip: {} more sessions not yet searchable. Run `agwt search` again to index them.",
                sessions_still_pending
            ))
            .yellow()
        );
    }

    Ok(BootstrapOutcome {
        sessions_embedded,
        chunks_embedded,
        sessions_still_pending,
    })
}

/// Render beautiful ASCII thermal receipt cards with score badges, model, tokens, and turn snippets.
fn render_ascii_thermal_receipts(
    query: &str,
    results: &[VectorSearchResult],
    embedder: &LocalEmbedder,
    storage: &Storage,
) {
    let engine_tag = if embedder.is_onnx() {
        "ONNX (BGE-Small-EN-v1.5)"
    } else {
        "Offline Semantic Vector Engine (384-dim)"
    };

    println!();
    println!(
        "{}",
        style("┌─ ⚡ AgentWorth Semantic Latent Vector Search ──────────────────────────┐")
            .bold()
            .cyan()
    );
    println!(
        "│ Query:   {:<59} │",
        style(format!("\"{}\"", query)).bold().yellow()
    );
    println!(
        "│ Engine:  {:<59} │",
        style(engine_tag).dim()
    );
    println!(
        "│ Matches: {:<59} │",
        style(format!("{} trajectory turn(s) retrieved", results.len())).green()
    );
    println!(
        "{}",
        style("├────────────────────────────────────────────────────────────────────────┤")
            .bold()
    );

    for (i, res) in results.iter().enumerate() {
        let (_badge, badge_styled) = format_thermal_score_badge(res.score);
        let kind_badge = format!("[{}]", res.kind.as_str().to_uppercase());

        println!(
            "│ {}  {:<24} {:>32} │",
            style(format!("#{:02}", i + 1)).bold().dim(),
            badge_styled,
            style(&kind_badge).bold().magenta()
        );

        println!(
            "│ Session:   {:<58} │",
            style(&res.session_id).cyan().bold()
        );

        let model_disp = res.model.as_deref().unwrap_or("unknown");
        println!(
            "│ Adapter:   {:<24} Model:   {:<25} │",
            style(&res.adapter).green(),
            style(model_disp).yellow()
        );

        let started_disp = res
            .started_at
            .as_deref()
            .unwrap_or("Unknown timestamp");
        let token_burn_disp = if res.total_tokens > 1_000_000 {
            format!("{:.2}M tokens", res.total_tokens as f64 / 1_000_000.0)
        } else if res.total_tokens > 0 {
            format!("{} tokens", format_number(res.total_tokens))
        } else {
            "0 tokens".to_string()
        };

        println!(
            "│ Timestamp: {:<24} Tokens:  {:<25} │",
            style(started_disp).dim(),
            style(token_burn_disp).bold()
        );

        // Fetch session path to display repo / project
        let project_disp = storage
            .get_session_by_id(&res.session_id)
            .ok()
            .flatten()
            .map(|s| extract_repository_or_workspace(&s.source_path))
            .unwrap_or_else(|| "workspace".to_string());

        let turn_disp = if res.turn_index > 0 {
            format!("#{}", res.turn_index)
        } else {
            "Summary".to_string()
        };

        println!(
            "│ Project:   {:<24} Turn:    {:<25} │",
            style(project_disp).cyan(),
            style(turn_disp).yellow()
        );

        println!(
            "│ {}",
            style("──────────────────────────────────────────────────────────────────────").dim()
        );

        // Clean & wrap snippet text
        let snippet_lines = clean_and_wrap_text(&res.text_content, 68);
        for line in snippet_lines.iter().take(6) {
            println!("│   {:<66} │", style(line).italic());
        }
        if snippet_lines.len() > 6 {
            println!(
                "│   {:<66} │",
                style(format!("... [{} more lines]", snippet_lines.len() - 6)).dim()
            );
        }

        if i + 1 < results.len() {
            println!(
                "{}",
                style("├────────────────────────────────────────────────────────────────────────┤")
                    .bold()
            );
        }
    }

    println!(
        "{}",
        style("└────────────────────────────────────────────────────────────────────────┘")
            .bold()
    );
    println!();
}

/// Format thermal match score badge with emoji and color.
fn format_thermal_score_badge(score: f32) -> (String, console::StyledObject<String>) {
    let pct = (score * 100.0).clamp(0.0, 100.0);
    if pct >= 85.0 {
        let txt = format!("[MATCH: {:.1}% 🔥]", pct);
        (txt.clone(), style(txt).bold().red())
    } else if pct >= 70.0 {
        let txt = format!("[MATCH: {:.1}% ⚡]", pct);
        (txt.clone(), style(txt).bold().yellow())
    } else if pct >= 50.0 {
        let txt = format!("[MATCH: {:.1}% 💡]", pct);
        (txt.clone(), style(txt).bold().cyan())
    } else {
        let txt = format!("[MATCH: {:.1}% 🔎]", pct);
        (txt.clone(), style(txt).dim())
    }
}

/// Wrap text to a maximum line width.
fn clean_and_wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let mut current = String::new();
        for word in trimmed.split_whitespace() {
            if current.len() + word.len() + 1 > max_width && !current.is_empty() {
                lines.push(current);
                current = String::new();
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    if lines.is_empty() {
        lines.push("No text content recorded.".to_string());
    }
    lines
}

fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (count, c) in s.chars().rev().enumerate() {
        if count > 0 && count % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
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

        let outcome_1 = bootstrap_vector_store(&storage, &vector_store, &embedder, 100, true)
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

        let outcome_2 = bootstrap_vector_store(&storage, &vector_store, &embedder, 100, true)
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

        let outcome_1 = bootstrap_vector_store(&storage, &vector_store, &embedder, 2, true)
            .expect("first bootstrap");
        assert_eq!(outcome_1.sessions_embedded, 2);
        assert_eq!(outcome_1.sessions_still_pending, 1);
        assert_eq!(vector_store.stats().unwrap().total_sessions, 2);

        let outcome_2 = bootstrap_vector_store(&storage, &vector_store, &embedder, 2, true)
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

        let outcome = bootstrap_vector_store(&storage, &vector_store, &embedder, 100, true)
            .expect("bootstrap on empty index");
        assert_eq!(outcome, BootstrapOutcome::default());

        let session = write_claude_session("hello", "hi there");
        scan_one(&scanner, &session);
        bootstrap_vector_store(&storage, &vector_store, &embedder, 100, true)
            .expect("bootstrap indexes the one session");

        // Nothing new since the last run -- must do no work, not re-embed.
        let outcome_again = bootstrap_vector_store(&storage, &vector_store, &embedder, 100, true)
            .expect("bootstrap with nothing pending");
        assert_eq!(outcome_again, BootstrapOutcome::default());
    }
}
