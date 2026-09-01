//! CI Blind-Spot Report command for AgentWorth.
//!
//! Subcommand: `agentworth blind-spots [--limit N] [--json]`
//! Generates a forensic report of sessions whose outcome claims never advanced past the
//! bottom two rungs of the verification ladder (`DoneClaimed` / `ArtifactChanged`).

use std::path::PathBuf;
use std::sync::Arc;
use agentworth_core::Scanner;
use agentworth_storage::{estimate_tokens_cost_usd, SessionFilter, SessionOrderBy, Storage};
use anyhow::Result;
use console::style;
use serde::{Deserialize, Serialize};

/// Confidence weight for a `primary_outcome` label, matching the constants
/// `OutcomeDetector` itself assigns when it first classifies these outcome kinds
/// (see `crates/outcomes/src/outcome.rs`). The index only stores the winning label,
/// not the original per-event confidence, so blind-spot reporting re-derives it here.
///
/// Matches the snake_case form `outcome_kind_name` writes (e.g. "done_claimed"), not the old
/// PascalCase encoding — see the fix in crates/outcomes/src/outcome.rs.
fn confidence_for_outcome(outcome: &str) -> f64 {
    match outcome {
        "done_claimed" => 0.35,
        "artifact_changed" => 0.60,
        _ => 0.50,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlindSpotEntry {
    pub session_id: String,
    pub adapter: String,
    pub started_at: String,
    pub total_events: usize,
    pub total_tokens: u64,
    pub spend_usd: f64,
    pub primary_outcome: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlindSpotsReport {
    pub total_blind_spots: usize,
    pub total_unverified_tokens: u64,
    pub total_unverified_spend_usd: f64,
    pub entries: Vec<BlindSpotEntry>,
}

/// Query and compute CI blind-spot sessions from local storage index.
pub fn generate_blind_spots_report(
    storage: &Arc<Storage>,
    limit: Option<usize>,
) -> Result<BlindSpotsReport> {
    // include_stubs: true also surfaces thin/short sessions -- a session that barely did
    // anything before claiming "done" is exactly the kind of blind spot this report exists
    // to catch, so it should not be hidden behind the normal stub filter.
    let filter = SessionFilter {
        limit: Some(10000),
        order_by: Some(SessionOrderBy::StartedAtDesc),
        include_stubs: Some(true),
        ..Default::default()
    };

    let all_sessions = storage.list_sessions_filtered(&filter)?;
    let scanner = Scanner::new(storage.clone());

    let mut entries = Vec::new();
    let mut total_unverified_tokens = 0u64;
    let mut total_unverified_spend_usd = 0.0f64;

    for s in all_sessions {
        let outcome = s.primary_outcome.as_deref().unwrap_or("done_claimed");
        // Blind spots: only self-claimed done or unverified file action -- the two rungs
        // below TestOrBuildPassed on the outcome hierarchy (see agentworth_schema::OutcomeKind).
        if outcome == "done_claimed" || outcome == "artifact_changed" {
            // The indexed SessionSummary only carries an aggregate total_tokens; the
            // input/output/cache breakdown estimate_tokens_cost_usd needs lives on the full
            // trace, so load it lazily just for the sessions that actually match.
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

            total_unverified_tokens += s.total_tokens;
            total_unverified_spend_usd += spend;

            entries.push(BlindSpotEntry {
                session_id: s.session_id,
                adapter: s.adapter,
                started_at: s.started_at.to_string(),
                total_events: s.total_events,
                total_tokens: s.total_tokens,
                spend_usd: spend,
                primary_outcome: outcome.to_string(),
                confidence: confidence_for_outcome(outcome),
            });
        }
    }

    let total_count = entries.len();
    if let Some(lim) = limit {
        entries.truncate(lim);
    }

    Ok(BlindSpotsReport {
        total_blind_spots: total_count,
        total_unverified_tokens,
        total_unverified_spend_usd,
        entries,
    })
}

/// Execute the `agentworth blind-spots` subcommand.
pub fn run_blind_spots_command(
    limit: usize,
    json: bool,
    db_path: Option<PathBuf>,
) -> Result<()> {
    let storage = Arc::new(match db_path {
        Some(p) => Storage::open_path(&p)?,
        None => Storage::open_default()?,
    });

    let report = generate_blind_spots_report(&storage, Some(limit))?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!();
    println!(
        "{}",
        style("┌─ 🙈 AgentWorth CI Blind-Spot Report ────────────────────────┐").bold().yellow()
    );
    println!(
        "│ Unverified Sessions:  {:<37} │",
        style(report.total_blind_spots).bold().yellow()
    );
    println!(
        "│ Unverified Token Burn:{:<37} │",
        style(format!("{} tokens", report.total_unverified_tokens)).bold()
    );
    println!(
        "│ Unverified Spend:     {:<37} │",
        style(format!("${:.2} USD", report.total_unverified_spend_usd)).bold().magenta()
    );
    println!(
        "{}",
        style("├────────────────────────────────────────────────────────────┤").bold()
    );

    if report.entries.is_empty() {
        println!("│ ✓ Excellent! All indexed sessions are verified by CI/tests.│");
    } else {
        println!("│ Showing top uncorroborated sessions:                       │");
        for (i, entry) in report.entries.iter().enumerate() {
            println!(
                "│ [{}] {:<24} Adapter: {:<18} │",
                i + 1,
                style(&entry.session_id).bold().cyan(),
                style(&entry.adapter).green()
            );
            println!(
                "│     Claim:  {:<16} Spend:   ${:<18.2} │",
                style(&entry.primary_outcome).yellow(),
                entry.spend_usd
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
    use agentworth_outcomes::{highest_outcome, outcome_kind_name, OutcomeHierarchyDetector};
    use agentworth_schema::{AgentWorthTrace, EventPayload, NormalizedEvent, Provenance};
    use chrono::Utc;
    use tempfile::NamedTempFile;

    /// Store a trace the same way `Scanner::run_scan` does: detect outcomes first and persist
    /// the winning label, rather than `upsert_trace`'s convenience path (which always stores
    /// `primary_outcome = NULL`). Blind-spot filtering keys entirely off that stored label.
    fn upsert_with_detected_outcome(storage: &Storage, trace: &AgentWorthTrace) {
        let detector = OutcomeHierarchyDetector::new();
        let outcomes = detector.detect_outcomes(trace);
        let strongest = highest_outcome(&outcomes).map(|o| outcome_kind_name(o.kind));
        storage
            .upsert_session(trace, strongest.as_deref(), None)
            .unwrap();
    }

    #[test]
    fn test_blind_spots_filtering() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Arc::new(Storage::open_path(tmp.path()).unwrap());

        let now = Utc::now();
        let prov1 = Provenance::new("/tmp/1.jsonl", "claude_code", 100, 1000, "fp1");
        let mut trace1 = AgentWorthTrace::new("sess-unverified-1", "claude_code", prov1, now);
        trace1.events.push(NormalizedEvent::new(
            1,
            now,
            EventPayload::AssistantMessage {
                content: "I have completed the task!".to_string(),
                thinking: None,
            },
        ));
        trace1.recalculate_stats();
        upsert_with_detected_outcome(&storage, &trace1);

        let prov2 = Provenance::new("/tmp/2.jsonl", "codex", 200, 2000, "fp2");
        let mut trace2 = AgentWorthTrace::new("sess-verified-2", "codex", prov2, now);
        trace2.events.push(NormalizedEvent::new(
            1,
            now,
            EventPayload::ShellCommand(agentworth_schema::ShellCommand {
                command: "cargo test".to_string(),
                cwd: None,
                exit_code: Some(0),
                output: Some("test result: ok. 5 passed".to_string()),
            }),
        ));
        trace2.recalculate_stats();
        upsert_with_detected_outcome(&storage, &trace2);

        let report = generate_blind_spots_report(&storage, None).unwrap();
        assert_eq!(report.total_blind_spots, 1);
        assert_eq!(report.entries[0].session_id, "sess-unverified-1");
    }
}
