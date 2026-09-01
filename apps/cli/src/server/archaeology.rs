//! Archaeology highlight detection and analytics across agent histories.

use std::collections::BTreeMap;

use agentworth_core::Scanner;
use agentworth_outcomes::{OutcomeDetector, RecoveryDetector};
use agentworth_schema::{EventPayload, OutcomeKind};
use agentworth_storage::{SessionFilter, SessionOrderBy, Storage};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// High-level archaeological discoveries across local agent histories.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ArchaeologyHighlights {
    pub most_expensive_unsolved: Option<UnsolvedTaskHighlight>,
    pub longest_recovery_loop: Option<RecoveryLoopHighlight>,
    pub most_frequent_model_switches: Option<ModelSwitchesHighlight>,
    pub token_carbon_dating: TokenCarbonDating,
}

/// Details of the most token-expensive task that failed or remained unresolved.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnsolvedTaskHighlight {
    pub session_id: String,
    pub adapter: String,
    pub prompt: String,
    pub total_tokens: u64,
    pub duration_seconds: Option<f64>,
    pub models_used: Vec<String>,
    pub outcome_summary: String,
    pub error_count: usize,
}

/// Details of the longest autonomous error recovery loop.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecoveryLoopHighlight {
    pub session_id: String,
    pub adapter: String,
    pub failure_sequence: u64,
    pub recovery_sequence: u64,
    pub steps_to_recover: usize,
    pub corrective_actions_count: usize,
    pub duration_seconds: Option<f64>,
    pub failure_summary: String,
    pub recovery_summary: String,
}

/// Details of sessions with the highest model switches / transitions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelSwitchesHighlight {
    pub session_id: String,
    pub adapter: String,
    pub switch_count: usize,
    pub unique_models: Vec<String>,
    pub models_sequence: Vec<String>,
    pub total_tokens: u64,
}

/// Chronological milestone / era in token accumulation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CarbonDatingEra {
    pub period: String,
    pub tokens: u64,
    pub sessions_count: usize,
}

/// Carbon-dating analysis of token exhaust across time.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TokenCarbonDating {
    pub earliest_session_at: Option<DateTime<Utc>>,
    pub latest_session_at: Option<DateTime<Utc>>,
    pub total_days_active: u64,
    pub total_tokens: u64,
    pub average_tokens_per_session: u64,
    pub timeline: Vec<CarbonDatingEra>,
    pub adapter_tokens: BTreeMap<String, u64>,
}

/// Computes archaeology highlights across all indexed agent sessions.
pub fn compute_archaeology_highlights(
    storage: &Storage,
    scanner: &Scanner,
) -> Result<ArchaeologyHighlights> {
    let stats = storage.get_aggregate_stats()?;
    // `limit: None` = unlimited (SessionFilter::limit's doc comment, crates/storage/src/lib.rs).
    // This was `Some(1000)` ordered oldest-first (StartedAtAsc) -- worse than a plain
    // undercount: on any index over 1000 non-stub sessions it silently dropped every session
    // past the 1000 oldest, so "archaeology" (most-expensive-unsolved, longest-recovery-loop,
    // top model-switchers, and a full first-session-to-latest carbon-dating timeline) showed
    // only ancient history and missed all recent activity. Same cap-presented-as-complete bug
    // already fixed in compute_verdict_breakdown and get_stats_handler.
    //
    // Safe to remove for the same reasons as get_stats_handler: this pulls lightweight
    // SessionSummary rows (no event payloads), get_aggregate_stats above already scans the
    // same table unbounded, the response is fixed-shape highlights (not the session list), and
    // both callers (dashboard, legacy static UI) fetch this once per page load or explicit
    // rescan, never on a poll. It also doesn't touch the expensive part of this function:
    // full-trace loading stays capped at the <=40 sessions picked as candidates below,
    // regardless of how many summaries this scan returns.
    //
    // Order flipped to most-recent-first: nothing here actually depends on row order (monthly
    // buckets key off a BTreeMap below, and the top-20-by-tokens/top-20-by-events candidate
    // picks each do their own explicit sort), so once unbounded this only affects which session
    // wins an exact-value tie. Kept as StartedAtDesc anyway -- if a cap ever comes back here,
    // "missing old history" is a safer failure than "missing today's session," which is the bug
    // this fix closes.
    let all_sessions = storage.list_sessions_filtered(&SessionFilter {
        limit: None,
        order_by: Some(SessionOrderBy::StartedAtDesc),
        ..Default::default()
    })?;

    if all_sessions.is_empty() {
        return Ok(ArchaeologyHighlights::default());
    }

    let mut most_expensive_unsolved: Option<UnsolvedTaskHighlight> = None;
    let mut longest_recovery_loop: Option<RecoveryLoopHighlight> = None;
    let mut most_frequent_model_switches: Option<ModelSwitchesHighlight> = None;

    let recovery_detector = RecoveryDetector::new();
    let outcome_detector = OutcomeDetector::new();

    let mut monthly_buckets: BTreeMap<String, (u64, usize)> = BTreeMap::new();
    let mut adapter_tokens: BTreeMap<String, u64> = BTreeMap::new();

    // 1. Build Token Carbon Dating (Shallow Pass)
    for summary in &all_sessions {
        // Track adapter tokens
        *adapter_tokens.entry(summary.adapter.clone()).or_insert(0) += summary.total_tokens;

        // Group into monthly bucket YYYY-MM
        let month_key = summary.started_at.format("%Y-%m").to_string();
        let bucket = monthly_buckets.entry(month_key).or_insert((0, 0));
        bucket.0 += summary.total_tokens;
        bucket.1 += 1;
    }

    // 2. Select top candidates for deep trace inspection
    let mut candidate_ids = std::collections::HashSet::new();
    
    // Top 20 by tokens (for most_expensive_unsolved)
    let mut by_tokens = all_sessions.clone();
    by_tokens.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));
    for s in by_tokens.iter().take(20) {
        candidate_ids.insert(s.session_id.clone());
    }

    // Top 20 by events (for recovery loops and model switches)
    let mut by_events = all_sessions.clone();
    by_events.sort_by(|a, b| b.total_events.cmp(&a.total_events));
    for s in by_events.iter().take(20) {
        candidate_ids.insert(s.session_id.clone());
    }

    let candidate_sessions: Vec<_> = all_sessions
        .iter()
        .filter(|s| candidate_ids.contains(&s.session_id))
        .collect();

    // 3. Deep Trace Inspection on Candidates Only
    for summary in candidate_sessions {
        // Attempt to load full trace for deep inspection
        let trace_opt = scanner.load_trace(&summary.session_id).ok();

        if let Some(trace) = trace_opt {
            let outcomes = outcome_detector.detect_outcomes(&trace);
            let recoveries = recovery_detector.detect_recoveries(&trace);

            // 1. Check Unsolved Tasks
            let has_verified_outcome = outcomes.iter().any(|o| {
                matches!(
                    o.kind,
                    OutcomeKind::CiOrDeploymentVerified
                        | OutcomeKind::CommitObserved
                        | OutcomeKind::TestOrBuildPassed
                )
            });

            let error_count = trace
                .events
                .iter()
                .filter(|ev| match &ev.payload {
                    EventPayload::Error { .. } => true,
                    EventPayload::ToolResult(tr) => tr.is_error,
                    EventPayload::ShellCommand(sc) => sc.exit_code.is_some_and(|c| c != 0),
                    _ => false,
                })
                .count();

            if (!has_verified_outcome || error_count > 0)
                && most_expensive_unsolved
                    .as_ref()
                    .map(|u| summary.total_tokens > u.total_tokens)
                    .unwrap_or(true)
            {
                let prompt = trace
                    .events
                    .iter()
                    .find_map(|ev| match &ev.payload {
                        EventPayload::UserMessage { content } => {
                            let trimmed = content.trim();
                            if !trimmed.is_empty() {
                                let first_line = trimmed.lines().next().unwrap_or(trimmed);
                                Some(if first_line.len() > 120 {
                                    format!("{}...", &first_line[..117])
                                } else {
                                    first_line.to_string()
                                })
                            } else {
                                None
                            }
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| "Unknown Task".to_string());

                let outcome_summary = if !outcomes.is_empty() {
                    outcomes[0].summary.clone()
                } else if error_count > 0 {
                    format!("Unresolved errors ({} failure points)", error_count)
                } else {
                    "Unverified execution without test or commit receipts".to_string()
                };

                most_expensive_unsolved = Some(UnsolvedTaskHighlight {
                    session_id: summary.session_id.clone(),
                    adapter: summary.adapter.clone(),
                    prompt,
                    total_tokens: summary.total_tokens,
                    duration_seconds: summary.duration_seconds,
                    models_used: summary.models_used.clone(),
                    outcome_summary,
                    error_count,
                });
            }

            // 2. Check Longest Recovery Loop
            for rec in recoveries {
                let is_longer = longest_recovery_loop
                    .as_ref()
                    .map(|curr| {
                        rec.steps_to_recover > curr.steps_to_recover
                            || (rec.steps_to_recover == curr.steps_to_recover
                                && rec.duration_seconds.unwrap_or(0.0)
                                    > curr.duration_seconds.unwrap_or(0.0))
                    })
                    .unwrap_or(true);

                if is_longer {
                    let failure_summary = trace
                        .events
                        .iter()
                        .find(|e| e.sequence == rec.failure_sequence)
                        .map(|e| match &e.payload {
                            EventPayload::Error { message, .. } => message.clone(),
                            EventPayload::ToolResult(tr) => {
                                format!("Tool {} failed", tr.name.as_deref().unwrap_or("unknown"))
                            }
                            EventPayload::ShellCommand(sc) => {
                                format!("Command '{}' failed", sc.command)
                            }
                            _ => "Observed error state".to_string(),
                        })
                        .unwrap_or_else(|| "Observed error state".to_string());

                    let recovery_summary = trace
                        .events
                        .iter()
                        .find(|e| e.sequence == rec.recovery_sequence)
                        .map(|e| match &e.payload {
                            EventPayload::ShellCommand(sc) => {
                                format!("Command '{}' passed with exit code 0", sc.command)
                            }
                            EventPayload::OutcomeEvidence(oe) => oe.summary.clone(),
                            EventPayload::ToolResult(_) => "Tool returned success".to_string(),
                            _ => "Verified resolution".to_string(),
                        })
                        .unwrap_or_else(|| "Verified resolution".to_string());

                    longest_recovery_loop = Some(RecoveryLoopHighlight {
                        session_id: summary.session_id.clone(),
                        adapter: summary.adapter.clone(),
                        failure_sequence: rec.failure_sequence,
                        recovery_sequence: rec.recovery_sequence,
                        steps_to_recover: rec.steps_to_recover,
                        corrective_actions_count: rec.corrective_actions_count,
                        duration_seconds: rec.duration_seconds,
                        failure_summary,
                        recovery_summary,
                    });
                }
            }

            // 3. Check Model Switches
            let mut models_sequence = Vec::new();
            let mut switch_count = 0;
            let mut last_model: Option<String> = None;

            for ev in &trace.events {
                if let EventPayload::ModelInvocation { model, .. } = &ev.payload {
                    if let Some(ref prev) = last_model {
                        if prev != model {
                            switch_count += 1;
                        }
                    }
                    models_sequence.push(model.clone());
                    last_model = Some(model.clone());
                }
            }

            if switch_count > 0
                && most_frequent_model_switches
                    .as_ref()
                    .map(|curr| switch_count > curr.switch_count)
                    .unwrap_or(true)
            {
                let mut unique = summary.models_used.clone();
                unique.dedup();
                most_frequent_model_switches = Some(ModelSwitchesHighlight {
                    session_id: summary.session_id.clone(),
                    adapter: summary.adapter.clone(),
                    switch_count,
                    unique_models: unique,
                    models_sequence,
                    total_tokens: summary.total_tokens,
                });
            }
        }
    }

    // 4. Token Carbon Dating Calculation
    let earliest = stats.first_session_at;
    let latest = stats.last_session_at;
    let total_days_active = if let (Some(e), Some(l)) = (earliest, latest) {
        let diff = (l - e).num_days();
        if diff <= 0 {
            1
        } else {
            diff as u64
        }
    } else {
        0
    };

    let total_tokens = stats.token_usage.total();
    let average_tokens_per_session = if stats.total_sessions > 0 {
        total_tokens / stats.total_sessions as u64
    } else {
        0
    };

    let timeline = monthly_buckets
        .into_iter()
        .map(|(period, (tokens, sessions_count))| CarbonDatingEra {
            period,
            tokens,
            sessions_count,
        })
        .collect();

    let token_carbon_dating = TokenCarbonDating {
        earliest_session_at: earliest,
        latest_session_at: latest,
        total_days_active,
        total_tokens,
        average_tokens_per_session,
        timeline,
        adapter_tokens,
    };

    Ok(ArchaeologyHighlights {
        most_expensive_unsolved,
        longest_recovery_loop,
        most_frequent_model_switches,
        token_carbon_dating,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{AgentWorthTrace, Provenance, TokenUsage};
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    /// Regression test for the old `Some(1000)` cap + oldest-first (`StartedAtAsc`) ordering on
    /// compute_archaeology_highlights's `list_sessions_filtered` call. That combination was worse
    /// than a plain undercount: on an index over 1000 non-stub sessions, oldest-first plus a
    /// 1000-row cap means every session past the 1000 oldest is invisible to this function, so an
    /// "archaeology"/highlights view whose entire point is scanning full history (a
    /// first-session-to-latest carbon-dating timeline, most-expensive-unsolved, longest-recovery,
    /// top model-switchers) instead showed only ancient history and silently missed all recent
    /// activity.
    ///
    /// Seeds 1000 "old" sessions clustered in one month plus 50 "recent" sessions ~400 days
    /// later in a different month -- deliberately more than the old 1000 cap, and deliberately
    /// the *most recent* activity, exactly what oldest-first + the cap dropped. Uses a distinct
    /// adapter and token total for each cohort so both `timeline` (keyed by month) and
    /// `adapter_tokens` (keyed by adapter) independently confirm the recent cohort is fully
    /// counted, not partially or not at all. Deep per-trace inspection (most_expensive_unsolved
    /// etc.) is untouched by this test: `scanner.load_trace` fails for these synthetic sessions
    /// (no real file on disk), so it's exercised only up to the `.ok()` no-op -- the assertions
    /// below cover the shallow-pass fields that come straight from the previously-capped
    /// `all_sessions` scan.
    #[test]
    fn test_archaeology_highlights_includes_sessions_beyond_old_1000_cap() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Storage::open_path(tmp.path()).unwrap();

        let old_base: DateTime<Utc> = "2020-01-15T00:00:00Z".parse().unwrap();
        let recent_base: DateTime<Utc> = old_base + chrono::Duration::days(400);

        const OLD_COUNT: i64 = 1000;
        const RECENT_COUNT: i64 = 50;

        for i in 0..OLD_COUNT {
            let prov = Provenance::new(
                format!("/test/arch_old_{}.jsonl", i),
                "claude_code",
                10,
                100,
                format!("fp_arch_old_{}", i),
            );
            let mut trace = AgentWorthTrace::new(
                format!("sess-arch-old-{}", i),
                "claude_code",
                prov,
                old_base + chrono::Duration::seconds(i),
            );
            trace.stats.total_events = 2;
            trace.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
            storage.upsert_trace(&trace).unwrap();
        }

        for i in 0..RECENT_COUNT {
            let prov = Provenance::new(
                format!("/test/arch_recent_{}.jsonl", i),
                "codex_cli",
                10,
                100,
                format!("fp_arch_recent_{}", i),
            );
            let mut trace = AgentWorthTrace::new(
                format!("sess-arch-recent-{}", i),
                "codex_cli",
                prov,
                recent_base + chrono::Duration::seconds(i),
            );
            trace.stats.total_events = 2;
            trace.stats.token_usage = TokenUsage::new(200, 50, 0, 0);
            storage.upsert_trace(&trace).unwrap();
        }

        let old_month = old_base.format("%Y-%m").to_string();
        let recent_month = recent_base.format("%Y-%m").to_string();
        assert_ne!(
            old_month, recent_month,
            "test fixture bug: old/recent cohorts must land in different months"
        );

        let storage = Arc::new(storage);
        let scanner = Scanner::new(storage.clone());

        let highlights = compute_archaeology_highlights(&storage, &scanner)
            .expect("compute_archaeology_highlights should succeed");

        let timeline = &highlights.token_carbon_dating.timeline;

        let old_era = timeline
            .iter()
            .find(|e| e.period == old_month)
            .unwrap_or_else(|| panic!("old month bucket '{}' missing from timeline", old_month));
        assert_eq!(
            old_era.sessions_count, OLD_COUNT as usize,
            "old month bucket should count all {} old sessions",
            OLD_COUNT
        );
        assert_eq!(old_era.tokens, (OLD_COUNT as u64) * 120);

        let recent_era = timeline.iter().find(|e| e.period == recent_month);
        assert!(
            recent_era.is_some(),
            "recent month bucket '{}' is missing from the timeline -- \
             compute_archaeology_highlights is still capping/misordering list_sessions_filtered \
             and silently dropping the newest {} sessions",
            recent_month,
            RECENT_COUNT
        );
        let recent_era = recent_era.unwrap();
        assert_eq!(
            recent_era.sessions_count, RECENT_COUNT as usize,
            "recent month bucket should count all {} recent sessions, not a partial/zero count",
            RECENT_COUNT
        );
        assert_eq!(recent_era.tokens, (RECENT_COUNT as u64) * 250);

        // adapter_tokens is populated from the same previously-capped scan; the recent cohort's
        // distinct adapter name makes this an independent confirmation of the same fix.
        let adapter_tokens = &highlights.token_carbon_dating.adapter_tokens;
        assert_eq!(
            adapter_tokens.get("claude_code").copied().unwrap_or(0),
            (OLD_COUNT as u64) * 120
        );
        assert_eq!(
            adapter_tokens.get("codex_cli").copied().unwrap_or(0),
            (RECENT_COUNT as u64) * 250,
            "codex_cli (the recent cohort's adapter) should have its tokens fully counted"
        );
    }
}
