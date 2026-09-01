//! Experience Recall command for AgentWorth.
//!
//! Subcommand: `agentworth recall "<query>" [--limit N] [--min-score F] [--json]`
//! Semantically searches prior agent trajectories and joins against session outcome & spend records,
//! answering: "Have I solved this before, and did it actually work?"

use std::path::PathBuf;
use std::sync::Arc;

use agentworth_core::Scanner;
use agentworth_storage::{estimate_tokens_cost_usd, LocalEmbedder, Storage, VectorStore};
use anyhow::Result;
use console::style;
use serde::{Deserialize, Serialize};

/// Confidence weight for a `primary_outcome` label, matching the constants
/// `OutcomeDetector` assigns when it first classifies these outcome kinds
/// (see `crates/outcomes/src/outcome.rs`).
fn confidence_for_outcome(outcome: &str) -> f64 {
    match outcome {
        "DoneClaimed" => 0.35,
        "ArtifactChanged" => 0.60,
        "TestOrBuildPassed" => 0.85,
        "CommitObserved" => 0.88,
        "CiOrDeploymentVerified" => 0.95,
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
            let outcome = s.primary_outcome.clone().unwrap_or_else(|| "DoneClaimed".to_string());
            let conf = confidence_for_outcome(&outcome);
            // SessionSummary only carries an aggregate total_tokens; the input/output/cache
            // breakdown estimate_tokens_cost_usd needs lives on the full trace.
            let token_usage = scanner
                .load_trace(&s.session_id)
                .map(|t| t.stats.token_usage)
                .unwrap_or_default();
            let spend = estimate_tokens_cost_usd(
                token_usage.input_tokens,
                token_usage.output_tokens,
                token_usage.cache_read_tokens,
                token_usage.cache_creation_tokens,
            );
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

/// Execute the `agentworth recall` subcommand.
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
            style(if r.snippet.len() > 46 {
                format!("{}...", &r.snippet[..43])
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
    use tempfile::NamedTempFile;

    #[test]
    fn test_recall_experience_empty_graceful() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Arc::new(Storage::open_path(tmp.path()).unwrap());

        let report = recall_experience(&storage, "fix utf-8 boundary", 5, 0.0).unwrap();
        assert_eq!(report.matches_count, 0);
    }
}
