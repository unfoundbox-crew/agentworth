//! PR Blame-Aware Overlay command for AgentWorth.
//!
//! Subcommand: `agentworth pr-blame [FILES...] [--json]`
//! Annotates PR or git diff file modifications with AI provenance:
//! which AI session authored the file, models used, tokens invested, and outcome verdict.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use agentworth_core::Scanner;
use agentworth_storage::{estimate_total_cost_from_per_model_usage, Storage};
use anyhow::Result;
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
                let outcome = s.primary_outcome.unwrap_or_else(|| "done_claimed".to_string());
                let conf = confidence_for_outcome(&outcome);
                // BlameMatch/SessionSummary only carry an aggregate total_tokens with no
                // per-model breakdown; the full per-model usage
                // estimate_total_cost_from_per_model_usage needs lives on the full trace.
                // Fall back to zero spend if the source file has since moved. Priced per
                // model (each model's tokens at that model's own rate, then summed), not
                // blended at a single default rate.
                let spend = scanner
                    .load_trace(&first.session_id)
                    .map(|t| estimate_total_cost_from_per_model_usage(&t.stats.per_model_token_usage))
                    .unwrap_or(0.0);
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
                total_tokens: Some(first.total_tokens),
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
    ui: &crate::ui::Ui,
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

    let report = crate::ui::with_status(ui, "matching sessions to files", || {
        annotate_pr_files(&storage, &target_files)
    })?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    let view = crate::ui::views::PrBlameView {
        files_analyzed: report.files_analyzed,
        ai_authored_files: report.ai_authored_files,
        rows: report
            .annotations
            .iter()
            .map(|ann| crate::ui::views::PrBlameRow {
                file_path: &ann.file_path,
                ai_touched: ann.ai_touched,
                session_id: ann.session_id.as_deref(),
                adapter: ann.adapter.as_deref(),
                models: &ann.models_used,
                outcome: ann.primary_outcome.as_deref(),
                confidence: ann.outcome_confidence,
            })
            .collect(),
    };
    print!("{}", crate::ui::views::pr_blame(ui, &view));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_adapter_sdk::ScanOptions;
    use agentworth_adapters::ClaudeCodeAdapter;
    use agentworth_schema::{AgentWorthTrace, EventPayload, FileActionType, NormalizedEvent, Provenance};
    use chrono::Utc;
    use serde_json::json;
    use std::io::Write;
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

    /// Regression test for the pricing bug: `annotate_pr_files` used to price a file's
    /// blamed spend via the blended `estimate_tokens_cost_usd` (model_id = None -> always
    /// Claude 3.5 Sonnet's rate), regardless of which model actually touched the file.
    /// Scans a real, adapter-parsed session that edits a target file on a cheap non-Sonnet
    /// model (DeepSeek Chat) and asserts the reported spend matches DeepSeek's real rate.
    #[test]
    fn test_annotate_pr_files_prices_non_sonnet_model_correctly() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Arc::new(Storage::open_path(tmp.path()).unwrap());

        let mut session_file = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();
        let line1 = json!({
            "type": "user",
            "timestamp": "2026-01-01T00:00:00Z",
            "content": "Please add a helper function.",
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
            "content": [
                {"type": "text", "text": "Adding the helper now."},
                {
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "Write",
                    "input": {"file_path": "src/pr_blame_target.rs", "content": "fn helper() {}"},
                },
            ],
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

        let report =
            annotate_pr_files(&storage, &["src/pr_blame_target.rs".to_string()]).unwrap();
        assert_eq!(report.ai_authored_files, 1, "the file must resolve to the scanned session");
        let spend = report.annotations[0]
            .spend_usd
            .expect("spend must be computed for an AI-touched file");

        // Real DeepSeek Chat rate: $0.14/M input, $0.28/M output.
        let expected_real_cost = 1_000_000.0 / 1_000_000.0 * 0.14 + 500_000.0 / 1_000_000.0 * 0.28;
        assert!(
            (spend - expected_real_cost).abs() < 1e-9,
            "expected deepseek-chat's real rate (${:.4}), got ${:.4}",
            expected_real_cost,
            spend
        );

        // What the old blended-Sonnet bug would have produced: $3.00/M input, $15.00/M output.
        let wrong_blended_sonnet_cost =
            1_000_000.0 / 1_000_000.0 * 3.00 + 500_000.0 / 1_000_000.0 * 15.00;
        assert!(
            (spend - wrong_blended_sonnet_cost).abs() > 1.0,
            "blamed spend (${:.4}) must not collapse to the blended-Sonnet figure (${:.4})",
            spend,
            wrong_blended_sonnet_cost
        );
    }
}
