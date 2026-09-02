//! Experience Recall command for AgentWorth.
//!
//! Subcommand: `archie session recall "<query>" [--limit N] [--min-score F] [--json]`
//! Semantically searches prior agent trajectories and joins against session outcome & spend records,
//! answering: "Have I solved this before, and did it actually work?"

use std::path::PathBuf;
use std::sync::Arc;

use agentworth_core::Scanner;
use agentworth_storage::{
    estimate_total_cost_from_per_model_usage, LocalEmbedder, Storage, VectorStore,
};
use anyhow::Result;
use console::style;
use serde::{Deserialize, Serialize};

/// Confidence weight for a `primary_outcome` label, matching the constants
/// `OutcomeDetector` assigns when it first classifies these outcome kinds
/// (see `crates/outcomes/src/outcome.rs`).
///
/// Matches the snake_case form `outcome_kind_name` writes (e.g. "done_claimed"), not the old
/// PascalCase encoding — see the fix in crates/outcomes/src/outcome.rs.
fn confidence_for_outcome(outcome: &str) -> f64 {
    match outcome {
        "done_claimed" => 0.35,
        "artifact_changed" => 0.60,
        "test_or_build_passed" => 0.85,
        "commit_observed" => 0.88,
        "ci_or_deployment_verified" => 0.95,
        _ => 0.50,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecalledSolution {
    pub session_id: String,
    pub adapter: String,
    pub models_used: Vec<String>,
    pub primary_outcome: String,
    pub outcome_confidence: f64,
    pub spend_usd: f64,
    pub similarity_score: f32,
    pub chunk_kind: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallReport {
    pub query: String,
    pub matches_count: usize,
    pub results: Vec<RecalledSolution>,
}

/// Execute recall search joining vector results with session metadata.
pub fn recall_experience(
    storage: &Arc<Storage>,
    query: &str,
    limit: usize,
    min_score: f32,
) -> Result<RecallReport> {
    let embedder = LocalEmbedder::new();
    let query_vector = embedder.embed_text(query)?;

    let vector_store = storage.vector_store()?;
    let chunk_matches = vector_store.search_filtered(&query_vector, limit, min_score, None, None)?;
    let scanner = Scanner::new(storage.clone());

    let mut results = Vec::new();

    for m in chunk_matches {
        let session_opt = storage.get_session_by_id(&m.session_id)?;

        let (adapter, models, outcome, conf, spend) = if let Some(s) = session_opt {
            let outcome = s.primary_outcome.clone().unwrap_or_else(|| "done_claimed".to_string());
            let conf = confidence_for_outcome(&outcome);
            // SessionSummary only carries an aggregate total_tokens with no per-model
            // breakdown; the full per-model usage estimate_total_cost_from_per_model_usage
            // needs lives on the full trace. Priced per model (each model's tokens at that
            // model's own rate, then summed), not blended at a single default rate.
            let spend = scanner
                .load_trace(&s.session_id)
                .map(|t| estimate_total_cost_from_per_model_usage(&t.stats.per_model_token_usage))
                .unwrap_or(0.0);
            (s.adapter, s.models_used, outcome, conf, spend)
        } else {
            ("unknown".to_string(), Vec::new(), "unknown".to_string(), 0.0, 0.0)
        };

        results.push(RecalledSolution {
            session_id: m.session_id,
            adapter,
            models_used: models,
            primary_outcome: outcome,
            outcome_confidence: conf,
            spend_usd: spend,
            similarity_score: m.score,
            chunk_kind: m.kind.as_str().to_string(),
            snippet: m.text_content,
        });
    }

    Ok(RecallReport {
        query: query.to_string(),
        matches_count: results.len(),
        results,
    })
}

/// Execute the `archie session recall` subcommand.
pub fn run_recall_command(
    query: &str,
    limit: usize,
    min_score: f32,
    json: bool,
    db_path: Option<PathBuf>,
) -> Result<()> {
    let storage = Arc::new(match db_path {
        Some(p) => Storage::open_path(&p)?,
        None => Storage::open_default()?,
    });

    let report = recall_experience(&storage, query, limit, min_score)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!();
    println!(
        "{}",
        style(format!("┌─ 🧠 AgentWorth Solution Recall: \"{}\" ─────────────────────┐", query))
            .bold()
            .cyan()
    );

    if report.results.is_empty() {
        println!("│ No prior trajectories matched this query.                  │");
        println!(
            "{}",
            style("└────────────────────────────────────────────────────────────┘").bold()
        );
        println!();
        return Ok(());
    }

    for (i, r) in report.results.iter().enumerate() {
        println!(
            "│ [{}] Similarity: {:<11} Outcome: {:<20} │",
            i + 1,
            style(format!("{:.1}%", r.similarity_score * 100.0)).bold().green(),
            style(&r.primary_outcome).bold().yellow()
        );
        println!(
            "│     Session: {:<21} Spend: ${:<21.2} │",
            style(&r.session_id).bold().cyan(),
            r.spend_usd
        );
        println!(
            "│     Snippet: {:<49} │",
            style(if r.snippet.chars().count() > 46 {
                format!("{}...", agentworth_schema::text::truncate_chars(&r.snippet, 43))
            } else {
                r.snippet.clone()
            }).italic().dim()
        );
        if i + 1 < report.results.len() {
            println!(
                "│ {}",
                style("────────────────────────────────────────────────────────────").dim()
            );
        }
    }

    println!(
        "{}",
        style("└────────────────────────────────────────────────────────────┘").bold()
    );
    println!();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_adapter_sdk::ScanOptions;
    use agentworth_adapters::ClaudeCodeAdapter;
    use agentworth_schema::{ChunkKind, TrajectoryChunk};
    use agentworth_storage::SessionFilter;
    use serde_json::json;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_recall_experience_empty_graceful() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Arc::new(Storage::open_path(tmp.path()).unwrap());

        let report = recall_experience(&storage, "fix utf-8 boundary", 5, 0.0).unwrap();
        assert_eq!(report.matches_count, 0);
    }

    /// Regression test for the pricing bug: `recall_experience` used to price a recalled
    /// session's spend via the blended `estimate_tokens_cost_usd` (model_id = None -> always
    /// Claude 3.5 Sonnet's rate), regardless of which model actually ran. Scans a real,
    /// adapter-parsed session run on a cheap non-Sonnet model (DeepSeek Chat), indexes one
    /// chunk for it in the vector store, and asserts the recalled spend matches DeepSeek's
    /// real rate, not Sonnet's.
    #[test]
    fn test_recall_experience_prices_non_sonnet_model_correctly() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Arc::new(Storage::open_path(tmp.path()).unwrap());

        let mut session_file = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();
        let line1 = json!({
            "type": "user",
            "timestamp": "2026-01-01T00:00:00Z",
            "content": "fix the utf-8 boundary panic in the tokenizer",
        });
        let line2 = json!({
            "type": "assistant",
            "timestamp": "2026-01-01T00:00:05Z",
            "model": "deepseek-chat",
            "usage": {
                "input_tokens": 1_000_000,
                "output_tokens": 500_000,
                "cache_read_input_tokens": 0,
                "cache_creation_input_tokens": 0,
            },
            "content": [{"type": "text", "text": "Fixed the boundary check."}],
        });
        writeln!(session_file, "{}", line1).unwrap();
        writeln!(session_file, "{}", line2).unwrap();

        let scan_only_claude =
            Scanner::with_adapters(vec![Box::new(ClaudeCodeAdapter::new())], storage.clone());
        let scan_summary = scan_only_claude
            .run_scan(
                &ScanOptions {
                    custom_paths: vec![session_file.path().to_path_buf()],
                    force: true,
                    ..Default::default()
                },
                |_, _| {},
            )
            .expect("scan the real session");
        assert_eq!(scan_summary.scanned_sessions, 1);

        let indexed = storage
            .list_sessions_filtered(&SessionFilter {
                include_stubs: Some(true),
                ..Default::default()
            })
            .expect("list the scanned session");
        assert_eq!(indexed.len(), 1);
        let session_id = indexed[0].session_id.clone();

        // Index one chunk for the scanned session, embedded under the exact text we will
        // query with -- the deterministic offline embedder is a pure function of the text,
        // so querying with the same string guarantees a top hit.
        let query = "fix utf-8 boundary panic in tokenizer";
        let embedder = LocalEmbedder::new_deterministic();
        let chunk = TrajectoryChunk::new(
            session_id.clone(),
            "claude_code",
            ChunkKind::SessionSummary,
            0,
            "2026-01-01T00:00:00Z",
            query,
            "{}",
        );
        let embedding = embedder.embed_text(query).expect("embed chunk text");
        storage
            .vector_store()
            .expect("vector store")
            .insert_embeddings(&[chunk], &[embedding])
            .expect("insert chunk embedding");

        let report = recall_experience(&storage, query, 5, 0.0).expect("recall should succeed");
        assert_eq!(report.matches_count, 1);
        assert_eq!(report.results[0].session_id, session_id);

        // Real DeepSeek Chat rate: $0.14/M input, $0.28/M output.
        let expected_real_cost = 1_000_000.0 / 1_000_000.0 * 0.14 + 500_000.0 / 1_000_000.0 * 0.28;
        assert!(
            (report.results[0].spend_usd - expected_real_cost).abs() < 1e-9,
            "expected deepseek-chat's real rate (${:.4}), got ${:.4}",
            expected_real_cost,
            report.results[0].spend_usd
        );

        // What the old blended-Sonnet bug would have produced: $3.00/M input, $15.00/M output.
        let wrong_blended_sonnet_cost =
            1_000_000.0 / 1_000_000.0 * 3.00 + 500_000.0 / 1_000_000.0 * 15.00;
        assert!(
            (report.results[0].spend_usd - wrong_blended_sonnet_cost).abs() > 1.0,
            "recalled spend (${:.4}) must not collapse to the blended-Sonnet figure (${:.4})",
            report.results[0].spend_usd,
            wrong_blended_sonnet_cost
        );
    }
}
