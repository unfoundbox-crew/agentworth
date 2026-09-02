//! Prompt Autopsy command for AgentWorth.
//!
//! Subcommand: `archie session autopsy [--min-occurrences N] [--json]`
//! Scans user prompt turns across all indexed sessions to surface recurring human corrections,
//! frustrations, and guardrail reminders, aggregating frequency and estimated token expenditure.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use agentworth_core::Scanner;
use agentworth_schema::EventPayload;
use agentworth_storage::{estimate_total_cost_from_per_model_usage, SessionFilter, Storage};
use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::ui::views::{self, AutopsyClusterRow, AutopsyView};
use crate::ui::Ui;

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
    // `limit: None` means genuinely unlimited (see SessionFilter::limit's doc comment in
    // crates/storage/src/lib.rs). This used to be `Some(10000)`, which silently dropped
    // sessions past the cap even though this module's own doc comment claims to cover "all
    // indexed sessions" -- the same bug shape already fixed for compute_verdict_breakdown and
    // get_stats_handler. This loop is heavier than those two: it calls scanner.load_trace per
    // session (a disk read plus a full adapter parse), not just a summary-row scan. Still safe
    // to leave unbounded: `autopsy` is a CLI command a person runs on demand, not an HTTP
    // response someone is blocked on, and compute_verdict_breakdown already runs this same
    // unbounded per-session trace-load pattern on every `archie stats` call, which is
    // invoked far more often than autopsy will be.
    let sessions = storage.list_sessions_filtered(&SessionFilter {
        limit: None,
        include_stubs: Some(true),
        ..Default::default()
    })?;

    let mut phrase_map: HashMap<String, (String, Vec<String>, u64, f64)> = HashMap::new();
    let mut total_user_msgs = 0usize;

    for summary in &sessions {
        // The index already knows this session has no user turn, so it cannot carry a
        // correction phrase -- reading and reparsing its transcript to discover that is
        // pure waste. This is the only prefilter the index supports: no table holds
        // per-turn message text, so a session that *does* have user turns still has to be
        // parsed from disk to see what they say.
        if summary.total_events == 0 {
            continue;
        }
        let trace = match scanner.load_trace(&summary.session_id) {
            Ok(t) => t,
            // Source file no longer on disk, or unreadable -- skip, don't fail the whole report.
            Err(_) => continue,
        };
        // Priced per model (each model's tokens at that model's own rate, then summed)
        // rather than blended at a single default rate -- a session run on a non-Sonnet
        // model must not be billed as if it were Sonnet.
        let session_spend =
            estimate_total_cost_from_per_model_usage(&trace.stats.per_model_token_usage);

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

/// Execute the `archie session autopsy` subcommand.
pub fn run_autopsy_command(
    min_occurrences: usize,
    json: bool,
    db_path: Option<PathBuf>,
    ui: &Ui,
) -> Result<()> {
    let storage = Arc::new(match db_path {
        Some(p) => Storage::open_path(&p)?,
        None => Storage::open_default()?,
    });

    // The slowest command in the CLI on a real index, and until now it ran in silence.
    // It cannot be moved onto the index: no table holds per-turn message text, so finding
    // a repeated human correction means reading the transcripts.
    let report = crate::ui::with_status(ui, "reading prompts across every session", || {
        perform_prompt_autopsy(&storage, min_occurrences)
    })?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    let clusters: Vec<AutopsyClusterRow<'_>> = report
        .clusters
        .iter()
        .map(|c| AutopsyClusterRow {
            phrase: &c.normalized_phrase,
            sample: &c.sample_raw_prompt,
            occurrences: c.occurrences,
            spend_usd: c.estimated_cost_usd,
        })
        .collect();

    print!(
        "{}",
        views::autopsy(
            ui,
            &AutopsyView {
                prompts_analyzed: report.total_user_messages_analyzed,
                min_occurrences,
                total_spend_usd: report.total_recurrent_spend_usd,
                clusters,
            }
        )
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_adapter_sdk::ScanOptions;
    use agentworth_adapters::ClaudeCodeAdapter;
    use agentworth_schema::{AgentWorthTrace, Provenance, TokenUsage};
    use chrono::{Duration, Utc};
    use serde_json::json;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_prompt_normalization_and_intent() {
        let raw = "No, that is wrong! Don't touch that file!";
        let norm = normalize_prompt_phrase(raw);
        assert!(norm.contains("no that is wrong"));
        assert!(norm.contains("don t touch that file"));
        assert!(is_correction_intent(&norm));
    }

    /// Regression test for the old `Some(10000)` cap on perform_prompt_autopsy's
    /// list_sessions_filtered call -- same bug shape as the already-fixed
    /// compute_verdict_breakdown and get_stats_handler cases. Seeds one real, on-disk,
    /// adapter-parsed session carrying a correction phrase, dated far older than 10,050 cheap
    /// filler sessions. Under the old cap (most-recent-10,000, since the default order is
    /// started_at DESC), the real session would rank 10,051th and be silently dropped even
    /// though this module's own doc comment claims to cover "all indexed sessions." The fixture
    /// size is deliberately chosen to exceed the old cap: anything smaller would pass on both
    /// the old and new code and wouldn't exercise the fix.
    #[test]
    fn test_perform_prompt_autopsy_scans_beyond_old_10000_cap() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Arc::new(Storage::open_path(tmp.path()).unwrap());

        // The one real, parseable session: oldest of the bunch, carrying a correction phrase.
        // Going through the real ClaudeCodeAdapter parse path (via run_scan), not a hand-built
        // AgentWorthTrace, so the test also proves scanner.load_trace succeeds on it later.
        let mut old_session = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();
        let line1 = json!({
            "type": "user",
            "timestamp": "2000-01-01T00:00:00Z",
            "content": "no, please revert that change",
        });
        let line2 = json!({
            "type": "assistant",
            "timestamp": "2000-01-01T00:00:05Z",
            "content": [{"type": "text", "text": "Reverted."}],
        });
        writeln!(old_session, "{}", line1).unwrap();
        writeln!(old_session, "{}", line2).unwrap();

        let scan_only_claude =
            Scanner::with_adapters(vec![Box::new(ClaudeCodeAdapter::new())], storage.clone());
        let scan_summary = scan_only_claude
            .run_scan(
                &ScanOptions {
                    custom_paths: vec![old_session.path().to_path_buf()],
                    force: true,
                    ..Default::default()
                },
                |_, _| {},
            )
            .expect("scan the real session");
        assert_eq!(scan_summary.scanned_sessions, 1);

        // Filler sessions: cheap SQL-only rows with no backing file on disk, all newer than the
        // real session above, so they outrank it under started_at DESC and push it past the old
        // Some(10000) cap. scanner.load_trace fails fast on these (missing file) and
        // perform_prompt_autopsy skips them via `continue`, so they contribute nothing to the
        // report either way -- they exist purely to inflate the total past the old cap.
        let start = Utc::now();
        const FILLER_SESSION_COUNT: i64 = 10_050;
        for i in 0..FILLER_SESSION_COUNT {
            let prov = Provenance::new(
                format!("/test/autopsy_cap_{}.jsonl", i),
                "claude_code",
                10,
                100,
                format!("fp_autopsy_cap_{}", i),
            );
            let mut trace = AgentWorthTrace::new(
                format!("sess-autopsy-cap-{}", i),
                "claude_code",
                prov,
                start + Duration::seconds(i),
            );
            trace.stats.total_events = 2;
            trace.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
            storage.upsert_trace(&trace).unwrap();
        }

        let report = perform_prompt_autopsy(&storage, 1).expect("autopsy should succeed");

        assert_eq!(
            report.total_user_messages_analyzed, 1,
            "the one real session's user message must be scanned even with 10,050 newer \
             sessions in the index -- the old Some(10000) cap would have pushed it out of the \
             most-recent-10,000 window entirely"
        );
        assert_eq!(report.clusters.len(), 1, "the correction phrase must form a cluster");
        assert_eq!(report.clusters[0].occurrences, 1);
        assert!(report.clusters[0].normalized_phrase.contains("revert"));
    }

    /// Regression test for the pricing bug: `perform_prompt_autopsy` used to price every
    /// session's spend via the blended `estimate_tokens_cost_usd` (model_id = None -> always
    /// Claude 3.5 Sonnet's rate), regardless of which model actually ran. Scans a real,
    /// adapter-parsed session run on a cheap non-Sonnet model (DeepSeek Chat) and asserts the
    /// resulting cluster spend matches DeepSeek's real rate, not Sonnet's.
    #[test]
    fn test_perform_prompt_autopsy_prices_non_sonnet_model_correctly() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Arc::new(Storage::open_path(tmp.path()).unwrap());

        let mut session_file = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();
        let line1 = json!({
            "type": "user",
            "timestamp": "2026-01-01T00:00:00Z",
            "content": "no, please revert that change",
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
            "content": [{"type": "text", "text": "Reverted."}],
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

        let report = perform_prompt_autopsy(&storage, 1).expect("autopsy should succeed");
        assert_eq!(report.clusters.len(), 1, "the correction phrase must form a cluster");

        // Real DeepSeek Chat rate: $0.14/M input, $0.28/M output.
        let expected_real_cost = 1_000_000.0 / 1_000_000.0 * 0.14 + 500_000.0 / 1_000_000.0 * 0.28;
        assert!(
            (report.clusters[0].estimated_cost_usd - expected_real_cost).abs() < 1e-9,
            "expected deepseek-chat's real rate (${:.4}), got ${:.4}",
            expected_real_cost,
            report.clusters[0].estimated_cost_usd
        );

        // What the old blended-Sonnet bug would have produced: $3.00/M input, $15.00/M output.
        let wrong_blended_sonnet_cost =
            1_000_000.0 / 1_000_000.0 * 3.00 + 500_000.0 / 1_000_000.0 * 15.00;
        assert!(
            (report.clusters[0].estimated_cost_usd - wrong_blended_sonnet_cost).abs() > 1.0,
            "cluster spend (${:.4}) must not collapse to the blended-Sonnet figure (${:.4})",
            report.clusters[0].estimated_cost_usd,
            wrong_blended_sonnet_cost
        );
        assert_eq!(report.total_recurrent_spend_usd, report.clusters[0].estimated_cost_usd);
    }
}
