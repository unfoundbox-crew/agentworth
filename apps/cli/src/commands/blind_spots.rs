//! CI Blind-Spot Report command for AgentWorth.
//!
//! Subcommand: `agentworth blind-spots [--limit N] [--json]`
//! Generates a forensic report of sessions whose outcome claims never advanced past the
//! bottom two rungs of the verification ladder (`DoneClaimed` / `ArtifactChanged`).

use std::path::PathBuf;
use std::sync::Arc;
use agentworth_core::Scanner;
use agentworth_storage::{
    estimate_total_cost_from_per_model_usage, SessionFilter, SessionOrderBy, Storage,
};
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
    //
    // `limit: None` means genuinely unlimited (see SessionFilter::limit's doc comment in
    // crates/storage/src/lib.rs). This used to be `Some(10000)`, which silently dropped older
    // sessions past that cap from `all_sessions` -- so `total_blind_spots`,
    // `total_unverified_tokens`, and `total_unverified_spend_usd` below under-reported on any
    // index bigger than 10,000 non-stub sessions, the same "presented as complete but silently
    // truncated" shape already fixed for get_stats_handler and compute_verdict_breakdown. The
    // `--limit` CLI flag (`run_blind_spots_command`'s own `limit` param, below) is unrelated:
    // it only truncates the *displayed* entries after these totals are already computed (see
    // `entries.truncate(lim)` below), so it can't be used to bound this query.
    //
    // Cost check before removing the cap: this is a manually-invoked CLI report, not a polled
    // server endpoint, and it's lighter than the already-unbounded compute_verdict_breakdown,
    // which does the same per-session `scanner.load_trace()` call unconditionally for every
    // indexed session. Here that load only runs for the outcome-matching subset (see the loop
    // below), never more than the entire index -- so the same removal is safe.
    let filter = SessionFilter {
        limit: None,
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
            // The indexed SessionSummary only carries an aggregate total_tokens with no
            // per-model breakdown; the full per-model usage estimate_total_cost_from_per_model_usage
            // needs lives on the full trace, so load it lazily just for the sessions that
            // actually match. Priced per model (each model's tokens at that model's own
            // rate, then summed), not blended at a single default rate.
            let spend = scanner
                .load_trace(&s.session_id)
                .map(|t| estimate_total_cost_from_per_model_usage(&t.stats.per_model_token_usage))
                .unwrap_or(0.0);

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
    use agentworth_adapter_sdk::ScanOptions;
    use agentworth_adapters::ClaudeCodeAdapter;
    use agentworth_outcomes::{highest_outcome, outcome_kind_name, OutcomeHierarchyDetector};
    use agentworth_schema::{
        AgentWorthTrace, EventPayload, NormalizedEvent, Provenance, TokenUsage,
    };
    use chrono::{Duration, Utc};
    use serde_json::json;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Store a trace the same way `Scanner::run_scan` does: detect outcomes first and persist
    /// the winning label, rather than `upsert_trace`'s convenience path (which always stores
    /// `primary_outcome = NULL`). Blind-spot filtering keys entirely off that stored label.
    fn upsert_with_detected_outcome(storage: &Storage, trace: &AgentWorthTrace) {
        let detector = OutcomeHierarchyDetector::new();
        let outcomes = detector.detect_outcomes(trace);
        let strongest = highest_outcome(&outcomes).map(|o| outcome_kind_name(o.kind));
        storage
            .upsert_session(trace, strongest.as_deref(), None, 1)
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

    /// Regression test for the pricing bug: `generate_blind_spots_report` used to price
    /// every entry's spend via the blended `estimate_tokens_cost_usd` (model_id = None ->
    /// always Claude 3.5 Sonnet's rate), regardless of which model actually ran. Scans a
    /// real, adapter-parsed session run on a cheap non-Sonnet model (DeepSeek Chat) that
    /// only self-claims done (a blind spot) and asserts the reported spend matches
    /// DeepSeek's real rate, not Sonnet's.
    #[test]
    fn test_blind_spots_prices_non_sonnet_model_correctly() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Arc::new(Storage::open_path(tmp.path()).unwrap());

        let mut session_file = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();
        let line1 = json!({
            "type": "user",
            "timestamp": "2026-01-01T00:00:00Z",
            "content": "Please fix the failing build.",
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
            "content": [{"type": "text", "text": "I have completed the task!"}],
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

        let report = generate_blind_spots_report(&storage, None).unwrap();
        assert_eq!(report.total_blind_spots, 1, "the done-claimed session must be a blind spot");

        // Real DeepSeek Chat rate: $0.14/M input, $0.28/M output.
        let expected_real_cost = 1_000_000.0 / 1_000_000.0 * 0.14 + 500_000.0 / 1_000_000.0 * 0.28;
        assert!(
            (report.entries[0].spend_usd - expected_real_cost).abs() < 1e-9,
            "expected deepseek-chat's real rate (${:.4}), got ${:.4}",
            expected_real_cost,
            report.entries[0].spend_usd
        );

        // What the old blended-Sonnet bug would have produced: $3.00/M input, $15.00/M output.
        let wrong_blended_sonnet_cost =
            1_000_000.0 / 1_000_000.0 * 3.00 + 500_000.0 / 1_000_000.0 * 15.00;
        assert!(
            (report.entries[0].spend_usd - wrong_blended_sonnet_cost).abs() > 1.0,
            "entry spend (${:.4}) must not collapse to the blended-Sonnet figure (${:.4})",
            report.entries[0].spend_usd,
            wrong_blended_sonnet_cost
        );
        assert_eq!(report.total_unverified_spend_usd, report.entries[0].spend_usd);
    }

    /// Regression test for the old `Some(10000)` cap on
    /// `generate_blind_spots_report`'s `list_sessions_filtered` call. Seeds more non-stub
    /// sessions than that old cap -- each left with `primary_outcome = NULL` via
    /// `upsert_trace`, which the report treats as the "done_claimed" blind-spot default -- and
    /// asserts `total_blind_spots` and `total_unverified_tokens` account for every one of them.
    /// Before the fix, sessions past the old 10,000-row cap (the oldest ones, since the query
    /// orders `StartedAtDesc`) were silently dropped from `all_sessions` and never counted at
    /// all. The fixture size (10,050) is deliberately chosen to exceed the old cap: a smaller
    /// fixture would pass on both the old and new code and wouldn't actually exercise the fix.
    #[test]
    fn test_blind_spots_scans_beyond_old_10000_cap() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Storage::open_path(tmp.path()).unwrap();
        let start = Utc::now();

        const SESSION_COUNT: i64 = 10_050;
        for i in 0..SESSION_COUNT {
            let prov = Provenance::new(
                format!("/test/blind_spot_cap_{}.jsonl", i),
                "claude_code",
                10,
                100,
                format!("fp_blind_spot_cap_{}", i),
            );
            let mut trace = AgentWorthTrace::new(
                format!("sess-blind-spot-cap-{}", i),
                "claude_code",
                prov,
                start + Duration::seconds(i),
            );
            // Non-stub: list_sessions_filtered's default excludes total_events <= 1 or
            // total_tokens <= 0.
            trace.stats.total_events = 2;
            trace.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
            storage.upsert_trace(&trace).unwrap();
        }

        let storage = Arc::new(storage);
        // limit: None here is the *display* truncation (run_blind_spots_command's --limit
        // flag), not the internal fetch cap under test -- pass None so entries isn't truncated
        // and we can assert its length too, not just the totals computed before truncation.
        let report = generate_blind_spots_report(&storage, None).unwrap();

        assert_eq!(
            report.total_blind_spots, SESSION_COUNT as usize,
            "generate_blind_spots_report must count every session, not silently cap at 10,000"
        );
        assert_eq!(
            report.entries.len(),
            SESSION_COUNT as usize,
            "every seeded session should appear in entries -- a lower count means sessions past \
             the old 10,000 cap were dropped before the outcome filter even ran"
        );

        let expected_tokens_per_session = TokenUsage::new(100, 20, 0, 0).total();
        assert_eq!(
            report.total_unverified_tokens,
            expected_tokens_per_session * SESSION_COUNT as u64,
            "total_unverified_tokens should sum every seeded session's tokens, not just the \
             first 10,000"
        );
    }
}
