//! Rust -> TypeScript API contract fixtures.
//!
//! AGENTS.md item 2 ("Things you cannot learn from the code") names Rust/TypeScript drift as
//! the recurring bug class on this dashboard: three independent field-name mismatches shipped
//! in one day because nothing forced the two sides to agree. This test is the half of the loop
//! that lives in Rust.
//!
//! For each API response type the dashboard consumes, this builds one concrete, representative
//! value using the *real* Rust struct (not `serde_json::json!()` -- a struct literal is what
//! makes a field rename a compile error here, not just a silent reformat), serializes it, and
//! compares it against a checked-in JSON fixture under
//! `apps/dashboard/src/types/__fixtures__/`. A shape change either fails to compile (a field was
//! added/removed/renamed) or fails the value comparison (serialization changed) until the
//! fixture is regenerated.
//!
//! Regenerate after an intentional Rust-side change:
//!
//! ```sh
//! UPDATE_API_FIXTURES=1 cargo test -p agentworth-cli api_contract
//! ```
//!
//! Then run `npm run typecheck` in apps/dashboard/ -- `contract.test.ts` imports these same
//! fixtures and asserts each one `satisfies` its TypeScript type, so a TS type that didn't
//! move with the Rust change fails loudly instead of silently reading `undefined`.
//!
//! Not every route dashboard TS models is covered here. `GET /api/stats` returns a hand-built
//! `serde_json::Value` (apps/cli/src/server/routes.rs::get_stats_handler) whose shape does not
//! match the `AggregateStats` struct at all -- it nests `date_range`, replaces the fixed
//! `outcome_distribution` struct with a dynamic `BTreeMap`, and adds `average_composite_score`
//! and `top_repositories` that don't exist on `AggregateStats`. That is real, live drift
//! (`OverviewPane.tsx` and `VerdictBoard.tsx` both read from it), but fixing it means deciding
//! whether to reshape the handler or the TS type and re-verifying both consumers, which is out
//! of scope for this pass -- flagged separately rather than silently patched in a fixture test.

use agentworth_cli::server::archaeology::{
    ArchaeologyHighlights, CarbonDatingEra, ModelSwitchesHighlight, RecoveryLoopHighlight,
    TokenCarbonDating, UnsolvedTaskHighlight,
};
use agentworth_cli::server::routes::{
    AdapterMatrixItem, AdapterMatrixResponse, EventsPageResponse, TraceDetailResponse,
    UsageResponse,
};
use agentworth_core::ScanSummary;
use agentworth_schema::{
    AgentWorthTrace, EventPayload, EventType, FileActionType, NormalizedEvent, OutcomeEvidence,
    OutcomeKind, Provenance, TokenUsage, TraceStats,
};
use agentworth_scoring::TraceScore;
use agentworth_storage::{
    AggregateStats, BlameMatch, PacingSummary, SessionSummary, UsagePeriodSummary,
};
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn ts(s: &str) -> DateTime<Utc> {
    s.parse().expect("valid RFC3339 fixture timestamp")
}

fn fixtures_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR is apps/cli; fixtures live under the dashboard's own types dir so
    // the TS side imports them with a plain relative path.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../dashboard/src/types/__fixtures__")
}

/// Writes (if `UPDATE_API_FIXTURES=1`) or checks one fixture. Comparison is on parsed
/// `serde_json::Value`, not raw bytes, so pretty-printing whitespace never causes a false
/// failure -- only an actual shape or content change does.
fn assert_fixture<T: serde::Serialize>(name: &str, value: &T) {
    let dir = fixtures_dir();
    let path = dir.join(format!("{name}.json"));
    let actual = serde_json::to_value(value).expect("fixture value must serialize");
    let pretty =
        serde_json::to_string_pretty(&actual).expect("fixture value must pretty-print") + "\n";

    if std::env::var("UPDATE_API_FIXTURES").is_ok() {
        std::fs::create_dir_all(&dir).expect("create fixtures dir");
        std::fs::write(&path, &pretty).unwrap_or_else(|e| panic!("write {path:?}: {e}"));
        return;
    }

    let existing = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing fixture {path:?}: {e}\nRun `UPDATE_API_FIXTURES=1 cargo test -p agentworth-cli api_contract` to create it."
        )
    });
    let expected: serde_json::Value =
        serde_json::from_str(&existing).unwrap_or_else(|e| panic!("parse {path:?}: {e}"));

    assert_eq!(
        actual, expected,
        "\n{name}.json is stale.\nRun `UPDATE_API_FIXTURES=1 cargo test -p agentworth-cli api_contract` to regenerate it,\nthen run `npm run typecheck` in apps/dashboard/ to see what the TS side needs.",
    );
}

fn sample_token_usage() -> TokenUsage {
    TokenUsage::new(12_000, 3_400, 8_000, 1_200)
}

fn sample_session_summary() -> SessionSummary {
    SessionSummary {
        session_id: "sess_fixture_0001".to_string(),
        adapter: "claude_code".to_string(),
        kind: agentworth_schema::TraceKind::Conversation,
        source_path: "/Users/example/.claude/projects/demo/sess_fixture_0001.jsonl".to_string(),
        started_at: ts("2026-09-01T10:00:00Z"),
        duration_seconds: Some(482.5),
        total_tokens: 24_600,
        // 12_000 + 3_400 + 1.25 * 1_200 + 0.1 * 8_000
        cost_weighted_tokens: 17_700,
        input_tokens: 12_000,
        output_tokens: 3_400,
        cache_read_tokens: 8_000,
        cache_creation_tokens: 1_200,
        total_events: 42,
        tool_calls_count: 9,
        models_used: vec!["claude-sonnet-5".to_string()],
        primary_outcome: Some("commit_observed".to_string()),
        composite_score: Some(0.81),
        prompt_preview: Some("Fix the dashboard coverage matrix.".to_string()),
        source_mtime_epoch_secs: Some(1_772_000_000),
        compaction_count: 0,
        compaction_tokens_dropped: 0,
    }
}

fn sample_trace() -> AgentWorthTrace {
    let mut per_model = BTreeMap::new();
    per_model.insert("claude-sonnet-5".to_string(), sample_token_usage());
    let mut tools_used = BTreeMap::new();
    tools_used.insert("Read".to_string(), 5usize);
    tools_used.insert("Edit".to_string(), 4usize);

    AgentWorthTrace {
        session_id: "sess_fixture_0001".to_string(),
        adapter: "claude_code".to_string(),
        kind: agentworth_schema::TraceKind::Conversation,
        provenance: Provenance::new(
            "/Users/example/.claude/projects/demo/sess_fixture_0001.jsonl",
            "claude_code",
            184_320,
            1_772_000_000,
            "blake3:fixture0001",
        ),
        started_at: ts("2026-09-01T10:00:00Z"),
        ended_at: Some(ts("2026-09-01T10:08:02Z")),
        stats: TraceStats {
            total_events: 2,
            user_messages_count: 1,
            assistant_messages_count: 1,
            tool_calls_count: 0,
            token_usage: sample_token_usage(),
            models_used: vec!["claude-sonnet-5".to_string()],
            per_model_token_usage: per_model,
            tools_used,
            duration_seconds: Some(482.5),
            effort: None,
            compaction_count: 0,
            compaction_tokens_dropped: 0,
        },
        // Built as struct literals, not `NormalizedEvent::new` -- that helper stamps a random
        // UUID onto `id`, which would make this fixture non-deterministic across
        // regenerations and defeat the whole point of a checked-in comparison file.
        events: vec![
            NormalizedEvent {
                id: "evt-fixture-0".to_string(),
                sequence: 0,
                timestamp: ts("2026-09-01T10:00:00Z"),
                payload: EventPayload::UserMessage {
                    content: "Fix the dashboard coverage matrix.".to_string(),
                },
                raw_ref: None,
            },
            NormalizedEvent {
                id: "evt-fixture-1".to_string(),
                sequence: 1,
                timestamp: ts("2026-09-01T10:00:05Z"),
                payload: EventPayload::FileAction {
                    path: "apps/dashboard/src/components/CoverageMatrix.tsx".to_string(),
                    action: FileActionType::Edit,
                    diff: Some("- item.tokens === \"yes\"\n+ item.token_accounting".to_string()),
                    lines_changed: Some(6),
                },
                raw_ref: Some("L145".to_string()),
            },
        ],
        metadata: serde_json::Value::Null,
        identities: Vec::new(),
    }
}

fn sample_trace_score() -> TraceScore {
    TraceScore {
        outcome_score: 0.7,
        verifiability_score: 0.6,
        complexity_score: 0.5,
        recovery_score: 0.0,
        provenance_score: 1.0,
        composite_score: 0.64,
        explanations: vec!["commit observed for this session".to_string()],
        total_estimated_cost_usd: 0.14,
        per_model: BTreeMap::new(),
    }
}

#[test]
fn api_contract_matrix() {
    let response = AdapterMatrixResponse {
        total_adapters: 2,
        detected_adapters: 1,
        adapters: vec![
            AdapterMatrixItem {
                adapter: "claude_code".to_string(),
                name: "Claude Code".to_string(),
                detected: true,
                sessions_count: 512,
                identities: vec!["claude_code".to_string()],
                formats: vec!["jsonl".to_string(), "json".to_string()],
                token_accounting: true,
                cache_breakdown: true,
                tool_calls: true,
                file_actions: true,
                shell_commands: true,
                model_switches: true,
                thinking_blocks: true,
                error_recovery: true,
            },
            AdapterMatrixItem {
                adapter: "hermes".to_string(),
                name: "Nous Hermes".to_string(),
                detected: false,
                sessions_count: 0,
                identities: vec!["hermes".to_string()],
                formats: vec![],
                token_accounting: false,
                cache_breakdown: false,
                tool_calls: true,
                file_actions: false,
                shell_commands: false,
                model_switches: false,
                thinking_blocks: false,
                error_recovery: false,
            },
        ],
    };
    assert_fixture("matrix", &response);
}

#[test]
fn api_contract_usage() {
    let daily = vec![
        UsagePeriodSummary {
            period: "2026-09-07".to_string(),
            adapter: "claude_code".to_string(),
            session_count: 3,
            total_events: 210,
            input_tokens: 40_000,
            output_tokens: 12_000,
            cache_read_tokens: 90_000,
            cache_creation_tokens: 5_000,
            total_tokens: 147_000,
            // 40_000 + 12_000 + 1.25 * 5_000 + 0.1 * 90_000
            cost_weighted_tokens: 67_250,
            total_duration_seconds: 5_400.0,
            estimated_cost_usd: 2.15,
            cache_hit_ratio: 0.62,
        },
        UsagePeriodSummary {
            period: "2026-09-06".to_string(),
            adapter: "codex".to_string(),
            session_count: 1,
            total_events: 40,
            input_tokens: 8_000,
            output_tokens: 2_000,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            total_tokens: 10_000,
            // No cache on this row, so the weighted figure equals the raw one -- which is the
            // point of having it: the gap only opens where cache reads do.
            cost_weighted_tokens: 10_000,
            total_duration_seconds: 900.0,
            estimated_cost_usd: 0.30,
            cache_hit_ratio: 0.0,
        },
    ];
    let response = UsageResponse {
        daily,
        weekly: vec![],
        monthly: vec![],
        cost_basis: "api_list_price",
        subscription_tier: None,
    };
    assert_fixture("usage", &response);
}

#[test]
fn api_contract_traces_list() {
    let response = vec![sample_session_summary()];
    assert_fixture("traces", &response);
}

#[test]
fn api_contract_trace_detail() {
    let response = TraceDetailResponse {
        trace: sample_trace(),
        score: sample_trace_score(),
        outcomes: vec![OutcomeEvidence {
            kind: OutcomeKind::CommitObserved,
            summary: "git commit abc123 observed".to_string(),
            confidence: 0.9,
        }],
        recoveries: vec![],
        events_total: 2,
        events_offset: 0,
    };
    assert_fixture("trace_detail", &response);
}

#[test]
fn api_contract_trace_events_page() {
    let response = EventsPageResponse {
        events: sample_trace().events,
        events_total: 2,
        events_offset: 0,
    };
    assert_fixture("trace_events_page", &response);
}

#[test]
fn api_contract_pacing() {
    let response = PacingSummary {
        window_hours: 5,
        started_at: ts("2026-09-07T07:00:00Z"),
        ended_at: ts("2026-09-07T12:00:00Z"),
        session_count: 4,
        total_events: 320,
        input_tokens: 60_000,
        output_tokens: 15_000,
        cache_read_tokens: 100_000,
        cache_creation_tokens: 6_000,
        total_tokens: 181_000,
        burn_rate_tokens_per_hour: 36_200.0,
        estimated_cost_usd: 3.4,
        cache_hit_ratio: 0.55,
        active_adapters: vec!["claude_code".to_string()],
        active_models: vec!["claude-sonnet-5".to_string()],
    };
    assert_fixture("pacing", &response);
}

#[test]
fn api_contract_blame() {
    let response = vec![BlameMatch {
        session_id: "sess_fixture_0001".to_string(),
        adapter: "claude_code".to_string(),
        source_path: "/Users/example/.claude/projects/demo/sess_fixture_0001.jsonl".to_string(),
        started_at: ts("2026-09-01T10:00:00Z"),
        models_used: vec!["claude-sonnet-5".to_string()],
        total_tokens: 24_600,
        tool_calls_count: 9,
        file_path: "apps/dashboard/src/components/CoverageMatrix.tsx".to_string(),
        action: "edit".to_string(),
        modified_at: ts("2026-09-01T10:00:05Z"),
        model: Some("claude-sonnet-5".to_string()),
    }];
    assert_fixture("blame", &response);
}

#[test]
fn api_contract_archaeology() {
    let response = ArchaeologyHighlights {
        most_expensive_unsolved: Some(UnsolvedTaskHighlight {
            session_id: "sess_fixture_0002".to_string(),
            adapter: "codex".to_string(),
            prompt: "Migrate the auth middleware.".to_string(),
            total_tokens: 900_000,
            duration_seconds: Some(12_400.0),
            models_used: vec!["gpt-5-codex".to_string()],
            outcome_summary: "unresolved after 3 attempts".to_string(),
            error_count: 4,
        }),
        longest_recovery_loop: Some(RecoveryLoopHighlight {
            session_id: "sess_fixture_0003".to_string(),
            adapter: "claude_code".to_string(),
            failure_sequence: 12,
            recovery_sequence: 40,
            steps_to_recover: 28,
            corrective_actions_count: 6,
            duration_seconds: Some(600.0),
            failure_summary: "compile error".to_string(),
            recovery_summary: "fixed missing import".to_string(),
        }),
        most_frequent_model_switches: Some(ModelSwitchesHighlight {
            session_id: "sess_fixture_0004".to_string(),
            adapter: "gemini".to_string(),
            switch_count: 3,
            unique_models: vec![
                "gemini-3.7-flash".to_string(),
                "gemini-3.8-flash-high".to_string(),
            ],
            models_sequence: vec![
                "gemini-3.7-flash".to_string(),
                "gemini-3.8-flash-high".to_string(),
                "gemini-3.7-flash".to_string(),
            ],
            total_tokens: 210_000,
        }),
        token_carbon_dating: TokenCarbonDating {
            earliest_session_at: Some(ts("2026-06-01T00:00:00Z")),
            latest_session_at: Some(ts("2026-09-07T00:00:00Z")),
            total_days_active: 61,
            total_tokens: 12_000_000,
            average_tokens_per_session: 45_000,
            timeline: vec![CarbonDatingEra {
                period: "2026-09".to_string(),
                tokens: 3_000_000,
                sessions_count: 60,
            }],
            adapter_tokens: {
                let mut m = BTreeMap::new();
                m.insert("claude_code".to_string(), 9_000_000u64);
                m.insert("codex".to_string(), 3_000_000u64);
                m
            },
        },
    };
    assert_fixture("archaeology", &response);
}

#[test]
fn api_contract_scan() {
    let response = ScanSummary {
        discovered_sources: 120,
        scanned_sessions: 4,
        skipped_unchanged: 116,
        backfilled_sessions: 0,
        reparsed_sessions: 0,
        sources_unavailable: 0,
        errors_encountered: 0,
        total_indexed_sessions: 512,
        aggregate_stats: AggregateStats {
            total_sessions: 512,
            total_events: 18_400,
            token_usage: sample_token_usage(),
            sessions_by_adapter: {
                let mut m = BTreeMap::new();
                m.insert("claude_code".to_string(), 400usize);
                m.insert("codex".to_string(), 112usize);
                m
            },
            models_usage_count: BTreeMap::new(),
            tools_usage_count: BTreeMap::new(),
            verified_outcomes_count: 88,
            first_session_at: Some(ts("2026-06-01T00:00:00Z")),
            last_session_at: Some(ts("2026-09-07T00:00:00Z")),
        },
        stub_sessions_removed: 0,
    };
    assert_fixture("scan", &response);
}

// EventType is re-exported by agentworth_schema and used by NormalizedEvent::event_type();
// referenced here only so an accidental removal of the re-export is caught at compile time
// by this test crate too, not just by whatever else happens to use it.
#[allow(dead_code)]
fn _touch_event_type(e: &EventPayload) -> EventType {
    e.event_type()
}
