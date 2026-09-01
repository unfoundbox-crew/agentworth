//! PR Blame-Aware Overlay command for AgentWorth.
//!
//! Subcommand: `agentworth pr-blame [FILES...] [--json]`
//! Annotates PR or git diff file modifications with AI provenance:
//! which AI session authored the file, models used, tokens invested, and outcome verdict.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use agentworth_core::Scanner;
use agentworth_storage::{estimate_tokens_cost_usd, Storage};
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
pub struct FileAiProvenance {
    pub file_path: String,
    pub ai_touched: bool,
    pub session_id: Option<String>,
    pub adapter: Option<String>,
    pub models_used: Vec<String>,
    pub primary_outcome: Option<String>,
    pub outcome_confidence: Option<f64>,
    pub spend_usd: Option<f64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrBlameReport {
    pub files_analyzed: usize,
    pub ai_authored_files: usize,
    pub annotations: Vec<FileAiProvenance>,
}

/// Annotate a list of changed files with AI authoring provenance from SQLite index.
pub fn annotate_pr_files(
    storage: &Arc<Storage>,
    files: &[String],
) -> Result<PrBlameReport> {
    let scanner = Scanner::new(storage.clone());
    let mut annotations = Vec::new();
    let mut ai_count = 0usize;

    for path in files {
        let blame_matches = storage.find_sessions_for_blame(path)?;
        if let Some(first) = blame_matches.first() {
            ai_count += 1;
            let session_opt = storage.get_session_by_id(&first.session_id)?;

            let (outcome, conf, spend) = if let Some(s) = session_opt {
                let outcome = s.primary_outcome.unwrap_or_else(|| "DoneClaimed".to_string());
                let conf = confidence_for_outcome(&outcome);
                // BlameMatch/SessionSummary only carry an aggregate total_tokens; the
                // input/output/cache breakdown estimate_tokens_cost_usd needs lives on the
                // full trace. Fall back to zero spend if the source file has since moved.
                let token_usage = scanner
                    .load_trace(&first.session_id)
                    .map(|t| t.stats.token_usage)
                    .unwrap_or_default();
                let spend = estimate_tokens_cost_usd(
                    token_usage.input_tokens,
                    token_usage.output_tokens,
                    token_usage.cache_read_tokens,
                    token_usage.cache_creation_tokens,
                );
                (Some(outcome), Some(conf), Some(spend))
            } else {
                (None, None, None)
            };

            annotations.push(FileAiProvenance {
                file_path: path.clone(),
                ai_touched: true,
                session_id: Some(first.session_id.clone()),
                adapter: Some(first.adapter.clone()),
                models_used: first.models_used.clone(),
                primary_outcome: outcome,
                outcome_confidence: conf,
                spend_usd: spend,
                total_tokens: Some(first.total_tokens as u64),
            });
        } else {
            annotations.push(FileAiProvenance {
                file_path: path.clone(),
                ai_touched: false,
                session_id: None,
                adapter: None,
                models_used: Vec::new(),
                primary_outcome: None,
                outcome_confidence: None,
                spend_usd: None,
                total_tokens: None,
            });
        }
    }

    Ok(PrBlameReport {
        files_analyzed: files.len(),
        ai_authored_files: ai_count,
        annotations,
    })
}

/// Execute the `agentworth pr-blame` subcommand.
pub fn run_pr_blame_command(
    files: Vec<String>,
    json: bool,
    db_path: Option<PathBuf>,
) -> Result<()> {
    let storage = Arc::new(match db_path {
        Some(p) => Storage::open_path(&p)?,
        None => Storage::open_default()?,
    });

    // If files list is empty, attempt to infer from `git diff --name-only HEAD`
    let target_files = if files.is_empty() {
        let output = Command::new("git")
            .args(["diff", "--name-only", "HEAD"])
            .output();
        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                stdout
                    .lines()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            }
            _ => Vec::new(),
        }
    } else {
        files
    };

    if target_files.is_empty() {
        println!("No changed files detected. Provide paths: `agentworth pr-blame <path1> <path2>`");
        return Ok(());
    }

    let report = annotate_pr_files(&storage, &target_files)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!();
    println!(
        "{}",
        style("┌─ 📋 AgentWorth AI Provenance PR Overlay ──────────────────┐").bold().cyan()
    );
    println!(
        "│ Changed Files:    {:<39} │",
        style(report.files_analyzed).bold()
    );
    println!(
        "│ AI-Authored Files:{:<39} │",
        style(format!("{} ({}% AI generated)", report.ai_authored_files, if report.files_analyzed > 0 { (report.ai_authored_files * 100) / report.files_analyzed } else { 0 })).bold().green()
    );
    println!(
        "{}",
        style("├──────────────────────────────────────────────────────────┤").bold()
    );

    for ann in &report.annotations {
        println!(
            "│ File: {:<50} │",
            style(&ann.file_path).bold()
        );
        if ann.ai_touched {
            println!(
                "│   • Touched by: {:<40} │",
                style(format!("{} ({})", ann.session_id.as_deref().unwrap_or(""), ann.adapter.as_deref().unwrap_or(""))).cyan()
            );
            if !ann.models_used.is_empty() {
                println!(
                    "│   • Models:     {:<40} │",
                    style(ann.models_used.join(", ")).yellow()
                );
            }
            if let Some(outcome) = &ann.primary_outcome {
                println!(
                    "│   • Outcome:    {:<40} │",
                    style(format!("{} ({:.0}% conf)", outcome, ann.outcome_confidence.unwrap_or(0.5) * 100.0)).green()
                );
            }
        } else {
            println!(
                "│   • Provenance: {:<40} │",
                style("Human / Unindexed").dim()
            );
        }
        println!(
            "│                                                          │"
        );
    }

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
    use agentworth_schema::{AgentWorthTrace, EventPayload, FileActionType, NormalizedEvent, Provenance};
    use chrono::Utc;
    use tempfile::NamedTempFile;

    #[test]
    fn test_annotate_pr_files() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Arc::new(Storage::open_path(tmp.path()).unwrap());

        let now = Utc::now();
        let prov = Provenance::new("/tmp/test.jsonl", "claude_code", 100, 1000, "fp1");
        let mut trace = AgentWorthTrace::new("sess-pr-1", "claude_code", prov, now);

        trace.events.push(NormalizedEvent::new(
            1,
            now,
            EventPayload::FileAction {
                path: "crates/storage/src/chunker.rs".to_string(),
                action: FileActionType::Write,
                diff: Some("+ diff".to_string()),
                lines_changed: Some(5),
            },
        ));
        storage.upsert_trace(&trace).unwrap();

        let files = vec![
            "crates/storage/src/chunker.rs".to_string(),
            "docs/README.md".to_string(),
        ];

        let report = annotate_pr_files(&storage, &files).unwrap();
        assert_eq!(report.files_analyzed, 2);
        assert_eq!(report.ai_authored_files, 1);
        assert!(report.annotations[0].ai_touched);
        assert!(!report.annotations[1].ai_touched);
    }
}
