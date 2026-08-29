use agentworth_schema::{
    AgentWorthTrace, EventPayload, NormalizedEvent, Provenance, ShellCommand, TokenUsage, ToolCall,
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
