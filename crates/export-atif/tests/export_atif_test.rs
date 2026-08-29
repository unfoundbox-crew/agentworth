use agentworth_export_atif::{export_redacted_atif, export_to_atif, AtifTrajectory};
use agentworth_redaction::Redactor;
use agentworth_schema::{
    AgentWorthTrace, EventPayload, FileActionType, HumanIntervention, NormalizedEvent,
    OutcomeEvidence, OutcomeKind, Provenance, ShellCommand, TokenUsage, ToolCall, ToolResult,
};
use chrono::{Duration, Utc};
use serde_json::json;

fn sample_trace() -> AgentWorthTrace {
    let start = Utc::now();
    let prov = Provenance::new(
        "/Users/saurabh/projects/app/trace.jsonl",
        "claude_code",
        1024,
        123456,
        "fp_abc123",
    );
    let mut trace = AgentWorthTrace::new("sess-12345", "claude_code", prov, start);

    trace.metadata = json!({
        "git_branch": "main",
        "user_email": "saurabh@example.com"
    });

    trace.events.push(NormalizedEvent::new(
        1,
        start,
        EventPayload::UserMessage {
            content: "Fix bug in /Users/saurabh/projects/app/server.ts with token sk-abcdef1234567890abcdef1234567890".to_string(),
        },
    ));

    trace.events.push(NormalizedEvent::new(
        2,
        start + Duration::seconds(1),
        EventPayload::ModelInvocation {
            model: "claude-3-7-sonnet".to_string(),
            token_usage: TokenUsage::new(200, 100, 50, 10),
            cost_usd: Some(0.015),
            latency_ms: Some(1500),
        },
    ));

    trace.events.push(NormalizedEvent::new(
        3,
        start + Duration::seconds(2),
        EventPayload::AssistantMessage {
            content: "I will check the file structure.".to_string(),
            thinking: Some("Need to list directory first.".to_string()),
        },
    ));

    trace.events.push(NormalizedEvent::new(
        4,
        start + Duration::seconds(3),
        EventPayload::ToolCall(ToolCall {
            id: Some("call_bash_1".to_string()),
            name: "Bash".to_string(),
            arguments: json!({"command": "ls -la"}),
        }),
    ));

    trace.events.push(NormalizedEvent::new(
        5,
        start + Duration::seconds(4),
        EventPayload::ToolResult(ToolResult {
            call_id: Some("call_bash_1".to_string()),
            name: Some("Bash".to_string()),
            output: json!({"stdout": "server.ts\npackage.json", "exit_code": 0}),
            is_error: false,
        }),
    ));

    trace.events.push(NormalizedEvent::new(
        6,
        start + Duration::seconds(5),
        EventPayload::ShellCommand(ShellCommand {
            command: "npm test".to_string(),
            cwd: Some("/Users/saurabh/projects/app".to_string()),
            exit_code: Some(0),
            output: Some("PASS 5 tests".to_string()),
        }),
    ));

    trace.events.push(NormalizedEvent::new(
        7,
        start + Duration::seconds(6),
        EventPayload::FileAction {
            path: "/Users/saurabh/projects/app/server.ts".to_string(),
            action: FileActionType::Edit,
            diff: Some("--- a/server.ts\n+++ b/server.ts\n@@ -1 +1 @@\n-old\n+new".to_string()),
            lines_changed: Some(2),
        },
    ));

    trace.events.push(NormalizedEvent::new(
        8,
        start + Duration::seconds(7),
        EventPayload::OutcomeEvidence(OutcomeEvidence {
            kind: OutcomeKind::TestOrBuildPassed,
            summary: "All unit tests passed successfully".to_string(),
            confidence: 0.95,
        }),
    ));

    trace.events.push(NormalizedEvent::new(
        9,
        start + Duration::seconds(8),
        EventPayload::HumanIntervention(HumanIntervention {
            action: "user_approval".to_string(),
            details: Some("Approved command execution".to_string()),
        }),
    ));

    trace.recalculate_stats();
    trace
}

#[test]
fn test_export_to_atif_valid_json_and_fields() {
    let trace = sample_trace();
    let json_str = export_to_atif(&trace, true).expect("export to atif string");

    let val: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON output");
    assert_eq!(val["schema_version"], "atif-v1.0");
    assert_eq!(val["session_id"], "sess-12345");
    assert_eq!(val["agent"]["name"], "claude_code");
    assert_eq!(val["agent"]["models"], json!(["claude-3-7-sonnet"]));
    assert_eq!(val["environment"]["adapter"], "claude_code");
    assert_eq!(
        val["environment"]["source_path"],
        "/Users/saurabh/projects/app/trace.jsonl"
    );

    // Steps
    let steps = val["steps"].as_array().expect("steps array");
    assert_eq!(steps.len(), 9);

    // Verify step types and sources
    assert_eq!(steps[0]["source"], "user");
    assert_eq!(steps[0]["step_type"], "user_message");
    assert!(steps[0]["content"].as_str().unwrap().contains("Fix bug"));

    assert_eq!(steps[1]["source"], "agent");
    assert_eq!(steps[1]["step_type"], "model_invocation");

    assert_eq!(steps[2]["source"], "agent");
    assert_eq!(steps[2]["step_type"], "assistant_message");
    assert_eq!(
        steps[2]["thinking"].as_str(),
        Some("Need to list directory first.")
    );

    assert_eq!(steps[3]["source"], "agent");
    assert_eq!(steps[3]["step_type"], "tool_call");

    assert_eq!(steps[4]["source"], "tool");
    assert_eq!(steps[4]["step_type"], "tool_result");

    assert_eq!(steps[5]["source"], "environment");
    assert_eq!(steps[5]["step_type"], "shell_command");

    assert_eq!(steps[6]["source"], "agent");
    assert_eq!(steps[6]["step_type"], "file_action");

    assert_eq!(steps[7]["source"], "environment");
    assert_eq!(steps[7]["step_type"], "outcome_evidence");

    assert_eq!(steps[8]["source"], "user");
    assert_eq!(steps[8]["step_type"], "human_intervention");

    // Metrics & Tokens
    assert_eq!(val["metrics"]["total_events"], 9);
    assert_eq!(val["metrics"]["user_messages_count"], 1);
    assert_eq!(val["tokens"]["input_tokens"], 200);
    assert_eq!(val["tokens"]["output_tokens"], 100);
    assert_eq!(val["tokens"]["cache_read_tokens"], 50);
    assert_eq!(val["tokens"]["cache_creation_tokens"], 10);
    assert_eq!(val["tokens"]["total_tokens"], 360);

    // Roundtrip deserialize into AtifTrajectory
    let trajectory: AtifTrajectory =
        serde_json::from_str(&json_str).expect("deserialize AtifTrajectory");
    assert_eq!(trajectory.session_id, "sess-12345");
    assert_eq!(trajectory.steps.len(), 9);
    assert_eq!(trajectory.tokens.total_tokens, 360);
}

#[test]
fn test_export_redacted_atif() {
    let trace = sample_trace();
    let redactor = Redactor::new();

    let json_str = export_redacted_atif(&trace, &redactor, false).expect("export redacted atif");
    assert!(!json_str.contains("sk-abcdef1234567890abcdef1234567890"));
    assert!(!json_str.contains("/Users/saurabh"));
    assert!(!json_str.contains("saurabh@example.com"));

    let val: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON");
    assert_eq!(
        val["environment"]["source_path"],
        "~/projects/app/trace.jsonl"
    );
    assert_eq!(val["metadata"]["user_email"], "[REDACTED_EMAIL]");
    assert!(val["steps"][0]["content"]
        .as_str()
        .unwrap()
        .contains("[REDACTED_API_KEY]"));
    assert!(val["steps"][0]["content"]
        .as_str()
        .unwrap()
        .contains("~/projects/app/server.ts"));
}

#[test]
fn test_atif_exporter_builder_and_formatting() {
    use agentworth_export_atif::AtifExporter;

    let trace = sample_trace();
    let redactor = Redactor::new();

    // Test pretty without redactor
    let exporter_pretty = AtifExporter::new().pretty(true);
    let output_pretty = exporter_pretty.export(&trace).unwrap();
    assert!(output_pretty.contains('\n'));
    assert!(output_pretty.contains("  \"schema_version\": \"atif-v1.0\""));

    // Test compact with redactor
    let exporter_compact_redacted = AtifExporter::new().pretty(false).with_redactor(redactor);
    let output_compact = exporter_compact_redacted.export(&trace).unwrap();
    assert!(!output_compact.contains("sk-abcdef1234567890abcdef1234567890"));
    assert!(!output_compact.contains("\n"));
}

#[test]
fn test_atif_empty_and_diverse_events() {
    let start = Utc::now();
    let prov = Provenance::new("/var/log/empty.jsonl", "test_adapter", 0, 0, "hash000");
    let mut trace = AgentWorthTrace::new("empty-sess", "test_adapter", prov, start);

    // Empty trace export
    let json_str = export_to_atif(&trace, false).unwrap();
    let trajectory: AtifTrajectory = serde_json::from_str(&json_str).unwrap();
    assert_eq!(trajectory.steps.len(), 0);
    assert_eq!(trajectory.metrics.total_events, 0);

    // Add Error and FileAction variants
    trace.events.push(NormalizedEvent::new(
        1,
        start,
        EventPayload::Error {
            message: "Fatal compiler error".to_string(),
            is_recovered: false,
        },
    ));
    trace.events.push(NormalizedEvent::new(
        2,
        start,
        EventPayload::FileAction {
            path: "test.txt".to_string(),
            action: FileActionType::Delete,
            diff: None,
            lines_changed: None,
        },
    ));
    trace.events.push(NormalizedEvent::new(
        3,
        start,
        EventPayload::Custom {
            kind: "metrics_checkpoint".to_string(),
            data: json!({"checkpoint_id": 42}),
        },
    ));

    let json_str2 = export_to_atif(&trace, false).unwrap();
    let trajectory2: AtifTrajectory = serde_json::from_str(&json_str2).unwrap();
    assert_eq!(trajectory2.steps.len(), 3);
    assert_eq!(trajectory2.steps[0].step_type, "error");
    assert_eq!(
        trajectory2.steps[0].error.as_ref().unwrap().message,
        "Fatal compiler error"
    );
    assert_eq!(trajectory2.steps[1].step_type, "file_action");
    assert_eq!(
        trajectory2.steps[1].file_action.as_ref().unwrap().action,
        FileActionType::Delete
    );
    assert_eq!(trajectory2.steps[2].step_type, "custom_metrics_checkpoint");
}
