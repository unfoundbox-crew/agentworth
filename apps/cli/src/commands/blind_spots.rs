//! CI Blind-Spot Report command for AgentWorth.
//!
//! Subcommand: `agentworth blind-spots [--limit N] [--json]`
//! Generates a forensic report of sessions whose outcome claims never advanced past the
//! bottom two rungs of the verification ladder (`DoneClaimed` / `ArtifactChanged`).

use std::path::PathBuf;
use agentworth_storage::{estimate_tokens_cost_usd, SessionFilter, SessionOrderBy, Storage};
use anyhow::Result;
use console::style;
use serde::{Deserialize, Serialize};

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
    storage: &Storage,
    limit: Option<usize>,
) -> Result<BlindSpotsReport> {
    let filter = SessionFilter {
        limit: Some(10000),
        order_by: Some(SessionOrderBy::StartedAtDesc),
        ..Default::default()
    };

    let all_sessions = storage.list_sessions_filtered(&filter)?;

    let mut entries = Vec::new();
    let mut total_unverified_tokens = 0u64;
    let mut total_unverified_spend_usd = 0.0f64;

    for s in all_sessions {
        let outcome = s.primary_outcome.as_deref().unwrap_or("done_claimed");
        // Blind spots: only self-claimed done or unverified file action
        if outcome == "done_claimed" || outcome == "artifact_changed" {
            let spend = estimate_tokens_cost_usd(
                s.input_tokens as u64,
                s.output_tokens as u64,
                s.cache_read_tokens as u64,
                s.cache_creation_tokens as u64,
            );

            total_unverified_tokens += s.total_tokens as u64;
            total_unverified_spend_usd += spend;

            entries.push(BlindSpotEntry {
                session_id: s.session_id,
                adapter: s.adapter,
                started_at: s.started_at.to_string(),
                total_events: s.total_events as usize,
                total_tokens: s.total_tokens as u64,
                spend_usd: spend,
                primary_outcome: outcome.to_string(),
                confidence: s.primary_outcome_confidence.unwrap_or(0.50),
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
    let storage = match db_path {
        Some(p) => Storage::open_path(&p)?,
        None => Storage::open_default()?,
    };

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
    use agentworth_schema::{AgentWorthTrace, EventPayload, NormalizedEvent, Provenance};
    use chrono::Utc;
    use tempfile::NamedTempFile;

    #[test]
    fn test_blind_spots_filtering() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Storage::open_path(tmp.path()).unwrap();

        let now = Utc::now();
        let prov1 = Provenance::new("/tmp/1.jsonl", "claude_code", 100, 1000, "fp1");
        let mut trace1 = AgentWorthTrace::new("sess-unverified-1", "claude_code", prov1, now);
        trace1.events.push(NormalizedEvent::new(
            1,
            now,
            EventPayload::AssistantMessage {
                content: "I'm done!".to_string(),
                thinking: None,
            },
        ));
        storage.insert_trace(&trace1).unwrap();

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
        storage.insert_trace(&trace2).unwrap();

        let report = generate_blind_spots_report(&storage, None).unwrap();
        assert_eq!(report.total_blind_spots, 1);
        assert_eq!(report.entries[0].session_id, "sess-unverified-1");
    }
}
