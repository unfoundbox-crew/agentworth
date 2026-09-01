//! Experience Recall command for AgentWorth.
//!
//! Subcommand: `agentworth recall "<query>" [--limit N] [--min-score F] [--json]`
//! Semantically searches prior agent trajectories and joins against session outcome & spend records,
//! answering: "Have I solved this before, and did it actually work?"

use std::path::PathBuf;

use agentworth_storage::{
    estimate_tokens_cost_usd, LocalEmbedder, SessionFilter, Storage, VectorStore,
};
use anyhow::Result;
use console::style;
use serde::{Deserialize, Serialize};

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
    storage: &Storage,
    query: &str,
    limit: usize,
    min_score: f32,
) -> Result<RecallReport> {
    let embedder = LocalEmbedder::new();
    let query_vector = embedder.embed_text(query)?;

    let chunk_matches = storage.search_similar_chunks(&query_vector, limit, min_score, None)?;

    let mut results = Vec::new();

    for m in chunk_matches {
        let session_opt = storage.get_session(&m.session_id)?;

        let (adapter, models, outcome, conf, spend) = if let Some(s) = session_opt {
            let models = serde_json::from_str::<Vec<String>>(&s.models_used).unwrap_or_default();
            let outcome = s.primary_outcome.unwrap_or_else(|| "done_claimed".to_string());
            let conf = s.primary_outcome_confidence.unwrap_or(0.50);
            let spend = estimate_tokens_cost_usd(
                s.input_tokens as u64,
                s.output_tokens as u64,
                s.cache_read_tokens as u64,
                s.cache_creation_tokens as u64,
            );
            (s.adapter, models, outcome, conf, spend)
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
            similarity_score: m.similarity_score,
            chunk_kind: m.chunk_kind,
            snippet: m.content,
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
    let storage = match db_path {
        Some(p) => Storage::open_path(&p)?,
        None => Storage::open_default()?,
    };

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
    use agentworth_schema::{AgentWorthTrace, EventPayload, NormalizedEvent, Provenance};
    use chrono::Utc;
    use tempfile::NamedTempFile;

    #[test]
    fn test_recall_experience_empty_graceful() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Storage::open_path(tmp.path()).unwrap();

        let report = recall_experience(&storage, "fix utf-8 boundary", 5, 0.0).unwrap();
        assert_eq!(report.matches_count, 0);
    }
}
