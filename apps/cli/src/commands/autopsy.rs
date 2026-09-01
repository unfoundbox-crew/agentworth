//! Prompt Autopsy command for AgentWorth.
//!
//! Subcommand: `agentworth autopsy [--min-occurrences N] [--json]`
//! Scans user prompt turns across all indexed sessions to surface recurring human corrections,
//! frustrations, and guardrail reminders, aggregating frequency and estimated token expenditure.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use agentworth_core::Scanner;
use agentworth_schema::EventPayload;
use agentworth_storage::{estimate_tokens_cost_usd, SessionFilter, Storage};
use anyhow::Result;
use console::style;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptCorrectionCluster {
    pub normalized_phrase: String,
    pub sample_raw_prompt: String,
    pub occurrences: usize,
    pub sessions_affected: Vec<String>,
    pub total_wasted_tokens: u64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutopsyReport {
    pub total_user_messages_analyzed: usize,
    pub recurring_correction_count: usize,
    pub total_recurrent_spend_usd: f64,
    pub clusters: Vec<PromptCorrectionCluster>,
}

/// Normalize prompt string for clustering (lowercase, trimmed punctuation, whitespace normalized).
pub fn normalize_prompt_phrase(text: &str) -> String {
    let lower = text.to_lowercase();
    let cleaned: String = lower
        .chars()
        .map(|c| if c.is_alphanumeric() || c.is_whitespace() { c } else { ' ' })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Identify if a prompt represents a human correction or steering intervention.
pub fn is_correction_intent(normalized: &str) -> bool {
    let patterns = [
        "no that is wrong",
        "no that s wrong",
        "don t do that",
        "dont do that",
        "stop",
        "why did you",
        "revert",
        "undo that",
        "use proper permissions",
        "fix permissions",
        "do not touch",
        "don t touch",
        "dont touch",
        "you forgot",
        "again the same",
        "not what i asked",
        "that failed",
        "why keep asking",
        "never touch main",
    ];

    patterns.iter().any(|p| normalized.contains(p)) || normalized.starts_with("no ")
}

/// Perform autopsy analysis across all indexed sessions.
pub fn perform_prompt_autopsy(
    storage: &Arc<Storage>,
    min_occurrences: usize,
) -> Result<AutopsyReport> {
    let scanner = Scanner::new(storage.clone());
    let sessions = storage.list_sessions_filtered(&SessionFilter {
        limit: Some(10000),
        include_stubs: Some(true),
        ..Default::default()
    })?;

    let mut phrase_map: HashMap<String, (String, Vec<String>, u64, f64)> = HashMap::new();
    let mut total_user_msgs = 0usize;

    for summary in &sessions {
        let trace = match scanner.load_trace(&summary.session_id) {
            Ok(t) => t,
            // Source file no longer on disk, or unreadable -- skip, don't fail the whole report.
            Err(_) => continue,
        };
        let session_spend = estimate_tokens_cost_usd(
            trace.stats.token_usage.input_tokens,
            trace.stats.token_usage.output_tokens,
            trace.stats.token_usage.cache_read_tokens,
            trace.stats.token_usage.cache_creation_tokens,
        );

        for event in &trace.events {
            if let EventPayload::UserMessage { content } = &event.payload {
                total_user_msgs += 1;
                let norm = normalize_prompt_phrase(content);
                if is_correction_intent(&norm) && norm.len() >= 5 {
                    let entry = phrase_map
                        .entry(norm.clone())
                        .or_insert_with(|| (content.clone(), Vec::new(), 0, 0.0));
                    if !entry.1.contains(&trace.session_id) {
                        entry.1.push(trace.session_id.clone());
                    }
                    entry.2 += trace.stats.token_usage.total();
                    entry.3 += session_spend;
                }
            }
        }
    }

    let mut clusters = Vec::new();
    let mut total_recurrent_spend = 0.0f64;

    for (norm, (sample, sessions, tokens, spend)) in phrase_map {
        let occurrences = sessions.len();
        if occurrences >= min_occurrences {
            total_recurrent_spend += spend;
            clusters.push(PromptCorrectionCluster {
                normalized_phrase: norm,
                sample_raw_prompt: sample,
                occurrences,
                sessions_affected: sessions,
                total_wasted_tokens: tokens,
                estimated_cost_usd: spend,
            });
        }
    }

    clusters.sort_by_key(|c| std::cmp::Reverse(c.occurrences));

    Ok(AutopsyReport {
        total_user_messages_analyzed: total_user_msgs,
        recurring_correction_count: clusters.len(),
        total_recurrent_spend_usd: total_recurrent_spend,
        clusters,
    })
}

/// Execute the `agentworth autopsy` subcommand.
pub fn run_autopsy_command(
    min_occurrences: usize,
    json: bool,
    db_path: Option<PathBuf>,
) -> Result<()> {
    let storage = Arc::new(match db_path {
        Some(p) => Storage::open_path(&p)?,
        None => Storage::open_default()?,
    });

    let report = perform_prompt_autopsy(&storage, min_occurrences)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!();
    println!(
        "{}",
        style("┌─ 🔬 AgentWorth Prompt Autopsy: Recurring Human Corrections ┐").bold().cyan()
    );
    println!(
        "│ User Prompts Analyzed: {:<39} │",
        style(report.total_user_messages_analyzed).bold()
    );
    println!(
        "│ Recurring Friction Clusters: {:<33} │",
        style(report.recurring_correction_count).bold().yellow()
    );
    println!(
        "│ Friction Token Burn:   {:<39} │",
        style(format!("${:.2} USD", report.total_recurrent_spend_usd)).bold().magenta()
    );
    println!(
        "{}",
        style("├────────────────────────────────────────────────────────────┤").bold()
    );

    if report.clusters.is_empty() {
        println!("│ ✓ No recurring friction or correction phrases detected.    │");
    } else {
        println!("│ Top Repeated Developer Steering Phrases:                   │");
        for (i, c) in report.clusters.iter().enumerate() {
            println!(
                "│ [{}] \"{:<38}\" │",
                i + 1,
                style(if c.sample_raw_prompt.len() > 38 {
                    format!("{}...", &c.sample_raw_prompt[..35])
                } else {
                    c.sample_raw_prompt.clone()
                }).bold().yellow()
            );
            println!(
                "│     Frequency: {:<14} Spend Burn: ${:<17.2} │",
                format!("{} sessions", c.occurrences),
                c.estimated_cost_usd
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

    #[test]
    fn test_prompt_normalization_and_intent() {
        let raw = "No, that is wrong! Don't touch that file!";
        let norm = normalize_prompt_phrase(raw);
        assert!(norm.contains("no that is wrong"));
        assert!(norm.contains("don t touch that file"));
        assert!(is_correction_intent(&norm));
    }
}
