use agentworth_schema::{
    AgentWorthTrace, EventPayload, NormalizedEvent, OutcomeKind, Provenance, ShellCommand,
    TokenUsage, ToolCall,
};
use agentworth_scoring::TraceScorer;
use chrono::{Duration, Utc};

#[test]
fn test_unrecovered_errors_penalized() {
    let start = Utc::now();
    let prov = Provenance::new("/tmp/test.jsonl", "claude_code", 100, 1000, "fp123");
    let mut trace = AgentWorthTrace::new("sess_unrecovered", "claude_code", prov, start);

    // Failing test never fixed
    trace.events.push(NormalizedEvent::new(
        1,
        start + Duration::seconds(1),
        EventPayload::ShellCommand(ShellCommand {
            command: "cargo test".to_string(),
            cwd: None,
            exit_code: Some(101),
            output: Some("test result: FAILED".to_string()),
        }),
    ));

    let scorer = TraceScorer::new();
    let score = scorer.score(&trace);

    assert_eq!(
        score.recovery_score, 0.0,
        "Unrecovered errors must yield 0 recovery score"
    );
    assert_eq!(score.outcome_score, 0.0);
}

#[test]
fn test_multiturn_depth_and_tool_breadth_scoring() {
    let start = Utc::now();
    let prov = Provenance::new("/tmp/test.jsonl", "claude_code", 5000, 1000, "fp123");
    let mut trace = AgentWorthTrace::new("sess_deep", "claude_code", prov, start);

    for i in 1..=20 {
        trace.events.push(NormalizedEvent::new(
            i,
            start + Duration::seconds(i as i64),
            EventPayload::ToolCall(ToolCall {
                id: Some(format!("call_{}", i)),
                name: if i % 3 == 0 {
                    "Bash".to_string()
                } else if i % 3 == 1 {
                    "replace_file_content".to_string()
                } else {
                    "write_to_file".to_string()
                },
                arguments: serde_json::json!({}),
            }),
        ));
    }

    trace.stats.token_usage = TokenUsage::new(25000, 8000, 1000, 500);
    trace.recalculate_stats();

    let scorer = TraceScorer::new();
    let score = scorer.score(&trace);

    assert!(
        score.complexity_score >= 0.50,
        "Rich multiturn tool usage should have solid complexity score"
    );
    assert!(score.composite_score >= 0.0 && score.composite_score <= 1.0);
}

/// A session that starts on a cheap model and switches to Opus mid-task must not have its
/// cost or verdict lumped onto one blended number: the cheap model only touches a file,
/// Opus is the one that tests and commits, and the two carry very different per-token rates.
#[test]
fn test_multi_model_session_cost_and_outcome_attribution() {
    let start = Utc::now();
    let prov = Provenance::new("/tmp/multi_model.jsonl", "claude_code", 100, 1000, "fp_multi");
    let mut trace = AgentWorthTrace::new("sess_multi_model", "claude_code", prov, start);

    // Phase 1: cheap model does exploratory work, only touches a file -- no test, no commit.
    trace.events.push(NormalizedEvent::new(
        1,
        start + Duration::seconds(1),
        EventPayload::ModelInvocation {
            model: "claude-3-5-haiku".to_string(),
            token_usage: TokenUsage::new(10_000, 2_000, 0, 0),
            cost_usd: None,
            latency_ms: None,
            effort: None,
        },
    ));
    trace.events.push(NormalizedEvent::new(
        2,
        start + Duration::seconds(2),
        EventPayload::ToolCall(ToolCall {
            id: Some("t1".to_string()),
            name: "replace_file_content".to_string(),
            arguments: serde_json::json!({"path": "src/lib.rs"}),
        }),
    ));

    // Phase 2: session switches to Opus, which runs the tests and commits the fix.
    trace.events.push(NormalizedEvent::new(
        3,
        start + Duration::seconds(3),
        EventPayload::ModelInvocation {
            model: "claude-3-opus".to_string(),
            token_usage: TokenUsage::new(10_000, 2_000, 0, 0),
            cost_usd: None,
            latency_ms: None,
            effort: None,
        },
    ));
    trace.events.push(NormalizedEvent::new(
        4,
        start + Duration::seconds(4),
        EventPayload::ShellCommand(ShellCommand {
            command: "cargo test --all".to_string(),
            cwd: Some("/repo".to_string()),
            exit_code: Some(0),
            output: Some("test result: ok. 3 passed; 0 failed".to_string()),
        }),
    ));
    trace.events.push(NormalizedEvent::new(
        5,
        start + Duration::seconds(5),
        EventPayload::ShellCommand(ShellCommand {
            command: "git commit -m 'fix: resolve issue'".to_string(),
            cwd: Some("/repo".to_string()),
            exit_code: Some(0),
            output: Some("[main abc1234] fix: resolve issue\n 1 file changed".to_string()),
        }),
    ));

    trace.recalculate_stats();

    let scorer = TraceScorer::new();
    let score = scorer.score(&trace);

    assert_eq!(
        score.per_model.len(),
        2,
        "both models invoked in the session must appear in the per-model breakdown"
    );

    let haiku = score
        .per_model
        .get("claude-3-5-haiku")
        .expect("haiku entry present");
    let opus = score
        .per_model
        .get("claude-3-opus")
        .expect("opus entry present");

    // Same token volume on both models (10_000 in / 2_000 out), priced very differently:
    // haiku at $0.80/$4.00 per M, opus at $15/$75 per M.
    assert!((haiku.estimated_cost_usd - 0.016).abs() < 1e-9);
    assert!((opus.estimated_cost_usd - 0.30).abs() < 1e-9);
    assert!(
        opus.cost_share > haiku.cost_share,
        "Opus did identical token volume at a far higher rate, so it should carry more of the cost share"
    );
    assert!((haiku.cost_share + opus.cost_share - 1.0).abs() < 1e-9);
    assert!(
        (score.total_estimated_cost_usd - (haiku.estimated_cost_usd + opus.estimated_cost_usd))
            .abs()
            < 1e-9
    );

    // The correctly-priced total must not collapse to what pricing the blended token total
    // at a single default rate would have produced -- that collapse is the exact "lumped
    // onto one number" failure mode this feature exists to avoid.
    let blended_total = agentworth_storage::estimate_tokens_cost_usd(20_000, 4_000, 0, 0);
    assert!(
        (score.total_estimated_cost_usd - blended_total).abs() > 0.05,
        "a correctly priced multi-model total should differ meaningfully from the single-rate blended figure"
    );

    // Verdict attribution: the cheap model only touched a file; Opus earned the commit.
    assert_eq!(haiku.strongest_outcome, Some(OutcomeKind::ArtifactChanged));
    assert_eq!(opus.strongest_outcome, Some(OutcomeKind::CommitObserved));
    assert!(
        opus.outcome_score > haiku.outcome_score,
        "the model that produced the verified commit should score higher than the model that only touched a file"
    );

    // Session-level scores stay whole-session numbers; the per-model split is additive.
    assert_eq!(score.explanations.len(), 5);

    // `per_model`'s keys must match `TraceStats.per_model_token_usage` exactly, and each
    // entry's token usage must match what the schema layer already attributed to that model.
    let score_models: Vec<&String> = score.per_model.keys().collect();
    let stats_models: Vec<&String> = trace.stats.per_model_token_usage.keys().collect();
    assert_eq!(score_models, stats_models);
    for (model, usage) in &trace.stats.per_model_token_usage {
        assert_eq!(&score.per_model[model].token_usage, usage);
    }
}

/// Single-model sessions should get a populated (not omitted) per-model breakdown too,
/// with exactly one entry that carries the whole session's cost.
#[test]
fn test_single_model_session_gets_full_cost_share() {
    let start = Utc::now();
    let prov = Provenance::new("/tmp/single_model.jsonl", "claude_code", 100, 1000, "fp_single");
    let mut trace = AgentWorthTrace::new("sess_single_model", "claude_code", prov, start);

    trace.events.push(NormalizedEvent::new(
        1,
        start + Duration::seconds(1),
        EventPayload::ModelInvocation {
            model: "claude-3-5-sonnet".to_string(),
            token_usage: TokenUsage::new(5_000, 1_000, 0, 0),
            cost_usd: None,
            latency_ms: None,
            effort: None,
        },
    ));

    trace.recalculate_stats();

    let scorer = TraceScorer::new();
    let score = scorer.score(&trace);

    assert_eq!(score.per_model.len(), 1);
    let sonnet = score
        .per_model
        .get("claude-3-5-sonnet")
        .expect("sonnet entry present");
    assert_eq!(sonnet.cost_share, 1.0);
    assert!(sonnet.estimated_cost_usd > 0.0);
    assert_eq!(sonnet.estimated_cost_usd, score.total_estimated_cost_usd);
}

/// A trace with no model invocations at all must not fabricate a per-model entry or cost.
#[test]
fn test_no_model_invocations_yields_empty_attribution() {
    let start = Utc::now();
    let prov = Provenance::new("/tmp/no_model.jsonl", "claude_code", 100, 1000, "fp_none");
    let mut trace = AgentWorthTrace::new("sess_no_model", "claude_code", prov, start);

    trace.events.push(NormalizedEvent::new(
        1,
        start + Duration::seconds(1),
        EventPayload::AssistantMessage {
            content: "Done with everything.".to_string(),
            thinking: None,
        },
    ));
    trace.recalculate_stats();

    let scorer = TraceScorer::new();
    let score = scorer.score(&trace);

    assert!(score.per_model.is_empty());
    assert_eq!(score.total_estimated_cost_usd, 0.0);
}
