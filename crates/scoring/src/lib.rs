//! Explainable scoring engine for AgentWorth traces.
//!
//! Provides transparent, inspectable scoring of AI agent trace sessions across
//! outcome hierarchy, verifiability, complexity, error recovery, and provenance.

mod context_rot;
mod scorer;

pub use context_rot::{
    detect_context_rot, ContextRotDetector, ContextRotSegment, ContextRotSignal, SegmentLabel,
};
pub use scorer::{ScoringWeights, TraceScore, TraceScorer};

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{
        AgentWorthTrace, EventPayload, FileActionType, NormalizedEvent, OutcomeEvidence,
        OutcomeKind, Provenance, ShellCommand, TokenUsage, ToolCall,
    };
    use chrono::{Duration, Utc};

    fn make_test_trace() -> AgentWorthTrace {
        let start = Utc::now();
        let prov = Provenance::new(
            "/Users/test/.claude/sessions/session-123.jsonl",
            "claude_code",
            4096,
            1720000000,
            "sha256:abc123def456",
        );
        AgentWorthTrace::new("session-123", "claude_code", prov, start)
    }

    #[test]
    fn test_scoring_high_quality_verified_trace() {
        let mut trace = make_test_trace();
        let start = trace.started_at;

        // 1. User message
        trace.events.push(NormalizedEvent::new(
            1,
            start + Duration::seconds(1),
            EventPayload::UserMessage {
                content: "Fix the memory leak and run test suite".to_string(),
            },
        ));

        // 2. Model invocation (with token usage)
        trace.events.push(NormalizedEvent::new(
            2,
            start + Duration::seconds(2),
            EventPayload::ModelInvocation {
                model: "claude-3-5-sonnet".to_string(),
                token_usage: TokenUsage::new(5000, 1500, 200, 100),
                cost_usd: Some(0.04),
                latency_ms: Some(850),
            },
        ));

        // 3. Tool call to edit
        trace.events.push(NormalizedEvent::new(
            3,
            start + Duration::seconds(3),
            EventPayload::ToolCall(ToolCall {
                id: Some("t1".to_string()),
                name: "replace_file_content".to_string(),
                arguments: serde_json::json!({"path": "src/lib.rs"}),
            }),
        ));

        // 4. File action
        trace.events.push(NormalizedEvent::new(
            4,
            start + Duration::seconds(4),
            EventPayload::FileAction {
                path: "src/lib.rs".to_string(),
                action: FileActionType::Edit,
                diff: Some("- old()\n+ new()".to_string()),
                lines_changed: Some(2),
            },
        ));

        // 5. Test command
        trace.events.push(NormalizedEvent::new(
            5,
            start + Duration::seconds(5),
            EventPayload::ShellCommand(ShellCommand {
                command: "cargo test --all".to_string(),
                cwd: Some("/repo".to_string()),
                exit_code: Some(0),
                output: Some("test result: ok. 10 passed; 0 failed".to_string()),
            }),
        ));

        // 6. Git commit command
        trace.events.push(NormalizedEvent::new(
            6,
            start + Duration::seconds(7),
            EventPayload::ShellCommand(ShellCommand {
                command: "git commit -m 'fix: resolve memory leak'".to_string(),
                cwd: Some("/repo".to_string()),
                exit_code: Some(0),
                output: Some(
                    "[main a1b2c3d] fix: resolve memory leak\n 1 file changed".to_string(),
                ),
            }),
        ));

        trace.recalculate_stats();

        let scorer = TraceScorer::new();
        let score = scorer.score(&trace);

        assert!(
            score.outcome_score >= 0.70,
            "Outcome score should be high for committed/tested trace"
        );
        assert!(
            score.verifiability_score >= 0.85,
            "Verifiability should be high for git commit and tests"
        );
        assert!(score.complexity_score > 0.0);
        assert!(
            score.provenance_score >= 0.90,
            "Provenance should be near 1.0"
        );
        assert!(
            score.composite_score >= 0.60,
            "Composite score should be solid"
        );
        assert_eq!(score.explanations.len(), 5);
    }

    #[test]
    fn test_scoring_with_recovery_bonus() {
        let mut trace = make_test_trace();
        let start = trace.started_at;

        // 1. Initial failing test
        trace.events.push(NormalizedEvent::new(
            1,
            start + Duration::seconds(1),
            EventPayload::ShellCommand(ShellCommand {
                command: "cargo test".to_string(),
                cwd: None,
                exit_code: Some(101),
                output: Some("error[E0308]: mismatched types\ntest result: FAILED".to_string()),
            }),
        ));

        // 2. Corrective edit
        trace.events.push(NormalizedEvent::new(
            2,
            start + Duration::seconds(2),
            EventPayload::FileAction {
                path: "src/main.rs".to_string(),
                action: FileActionType::Edit,
                diff: Some("+ fixed()".to_string()),
                lines_changed: Some(1),
            },
        ));

        // 3. Passing test
        trace.events.push(NormalizedEvent::new(
            3,
            start + Duration::seconds(4),
            EventPayload::ShellCommand(ShellCommand {
                command: "cargo test".to_string(),
                cwd: None,
                exit_code: Some(0),
                output: Some("test result: ok. 1 passed; 0 failed".to_string()),
            }),
        ));

        trace.events.push(NormalizedEvent::new(
            4,
            start + Duration::seconds(5),
            EventPayload::ModelInvocation {
                model: "claude-3-5-sonnet".to_string(),
                token_usage: TokenUsage::new(2000, 500, 0, 0),
                cost_usd: None,
                latency_ms: None,
            },
        ));

        trace.recalculate_stats();

        let scorer = TraceScorer::new();
        let score = scorer.score(&trace);

        assert!(
            score.recovery_score >= 0.70,
            "Recovery score should reward successful diagnosis and fix"
        );
    }

    #[test]
    fn test_scoring_unverified_done_claim() {
        let mut trace = make_test_trace();
        let start = trace.started_at;

        // Trace with only an assistant saying "I have completed all tasks" but no tool runs or file changes
        trace.events.push(NormalizedEvent::new(
            1,
            start + Duration::seconds(1),
            EventPayload::AssistantMessage {
                content: "I have completed everything successfully!".to_string(),
                thinking: None,
            },
        ));

        let scorer = TraceScorer::new();
        let score = scorer.score(&trace);

        assert!(
            score.verifiability_score <= 0.20,
            "Verifiability should be low for unverified self claims"
        );
        assert!(score.outcome_score <= 0.20);
    }

    #[test]
    fn test_custom_scoring_weights() {
        let trace = make_test_trace();
        let outcomes = vec![OutcomeEvidence {
            kind: OutcomeKind::CiOrDeploymentVerified,
            summary: "Deployed to production".to_string(),
            confidence: 1.0,
        }];
        let recoveries = vec![];

        let weights = ScoringWeights {
            outcome_weight: 1.0,
            verifiability_weight: 0.0,
            complexity_weight: 0.0,
            recovery_weight: 0.0,
            provenance_weight: 0.0,
        };

        let scorer = TraceScorer::with_weights(weights);
        let score = scorer.score_trace(&trace, &outcomes, &recoveries);

        assert_eq!(score.outcome_score, 1.0);
        assert_eq!(score.composite_score, 1.0);
    }
}
