//! Depth extraction for the Codex adapter: prompts, tool calls, and the outcome ladder.
//!
//! The v2 parser read only metadata and tokens; `response_item` records — where Codex keeps
//! messages, function calls, and their outputs — nothing parsed. These fixtures are
//! synthetic shapes copied from real `~/.codex/sessions` rollouts: every string and call id
//! is invented, no transcript content. Malformed lines, a failed test before its retry, and
//! a broken record mid-line are deliberate.

use agentworth_adapter_sdk::{AgentAdapter, SessionSource};
use agentworth_adapters::CodexAdapter;
use agentworth_schema::{EventPayload, OutcomeKind};
use std::path::{Path, PathBuf};

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn source_for(name: &str) -> SessionSource {
    SessionSource::from_path(fixture_path(name).as_path(), "codex").expect("source")
}

fn payloads(trace: &agentworth_schema::AgentWorthTrace) -> Vec<&EventPayload> {
    trace.events.iter().map(|e| &e.payload).collect()
}

fn evidence(trace: &agentworth_schema::AgentWorthTrace) -> Vec<OutcomeKind> {
    trace
        .events
        .iter()
        .filter_map(|e| match &e.payload {
            EventPayload::OutcomeEvidence(ev) => Some(ev.kind),
            _ => None,
        })
        .collect()
}

/// A clean rollout: one fix, one patch, one passing test, one commit. Prompts come out of
/// `response_item` message records, tool traffic out of function_call/function_call_output.
#[test]
fn codex_depth_extract_on_clean_rollout() {
    let trace = CodexAdapter::new()
        .parse(&source_for("rollout-depth-clean.jsonl"))
        .expect("parse")
        .trace;

    assert_eq!(trace.stats.user_messages_count, 1);
    assert_eq!(trace.stats.assistant_messages_count, 1);
    assert_eq!(trace.stats.tool_calls_count, 3);
    assert_eq!(trace.stats.tools_used.get("exec_command"), Some(&3));

    let shells: Vec<(String, i32)> = trace
        .events
        .iter()
        .filter_map(|e| match &e.payload {
            EventPayload::ShellCommand(cmd) => {
                Some((cmd.command.clone(), cmd.exit_code.unwrap_or(i32::MIN)))
            }
            _ => None,
        })
        .collect();

    assert_eq!(
        shells.len(),
        3,
        "every exec_command becomes a shell command"
    );
    assert!(
        shells.iter().all(|(_, code)| *code == 0),
        "the rollouts envelope states exit 0"
    );

    let file_paths: Vec<String> = trace
        .events
        .iter()
        .filter_map(|e| match &e.payload {
            EventPayload::FileAction { path, action, .. } => Some(format!("{:?} {}", action, path)),
            _ => None,
        })
        .collect();
    assert_eq!(
        file_paths,
        vec!["Edit src/demo.rs"],
        "apply_patch inside exec_command names the modified file for blame"
    );

    let kinds = evidence(&trace);
    assert!(
        kinds.contains(&OutcomeKind::ArtifactChanged),
        "a file edit is an artifact change"
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|k| **k == OutcomeKind::TestOrBuildPassed)
            .count(),
        1,
        "the exit-0 cargo test is a test pass"
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|k| **k == OutcomeKind::CommitObserved)
            .count(),
        1,
        "the exit-0 git commit is a commit"
    );
    assert!(
        !kinds.contains(&OutcomeKind::DoneClaimed),
        "nothing in the rollout claims done on its own word"
    );
    assert!(
        !kinds.contains(&OutcomeKind::CiOrDeploymentVerified),
        "a plain test run proves nothing about CI"
    );

    // Token accounting must not move where the parse is otherwise unchanged.
    assert_eq!(trace.stats.token_usage.input_tokens, 600);
    assert_eq!(trace.stats.token_usage.output_tokens, 200);
    assert_eq!(trace.stats.token_usage.cache_read_tokens, 400);
    assert_eq!(trace.stats.token_usage.total(), 1200);
    assert_eq!(trace.stats.models_used, vec!["gpt-5.4-synth".to_string()]);
}

/// Malformed lines degrade per record; the session survives.
#[test]
fn codex_depth_malformed_lines_do_not_invalidate_the_session() {
    let result = CodexAdapter::new()
        .parse(&source_for("rollout-depth-malformed.jsonl"))
        .expect("parse");

    assert_eq!(
        result.malformed_lines, 3,
        "corrupt, half-written, dangling-record line"
    );
    let trace = result.trace;
    assert_eq!(trace.stats.user_messages_count, 1);
    assert_eq!(trace.stats.assistant_messages_count, 1);
    assert!(
        payloads(&trace)
            .iter()
            .any(|p| matches!(p, EventPayload::Custom { kind, .. } if kind == "session_meta")),
        "unparsable record kinds still index, as before"
    );
}

/// A failing test emits no pass evidence; only its retry does. A non-shell tool (js) is a
/// tool call but not a shell command.
#[test]
fn codex_depth_failed_test_then_retry() {
    let trace = CodexAdapter::new()
        .parse(&source_for("rollout-depth-failed-retry.jsonl"))
        .expect("parse")
        .trace;

    assert_eq!(trace.stats.tool_calls_count, 4);
    assert_eq!(trace.stats.tools_used.get("js"), Some(&1));
    let shell_count = payloads(&trace)
        .iter()
        .filter(|p| matches!(p, EventPayload::ShellCommand(_)))
        .count();
    assert_eq!(shell_count, 3, "exec_command is shell; js is not");

    let failed = trace
        .events
        .iter()
        .filter_map(|e| match &e.payload {
            EventPayload::ShellCommand(cmd) => Some(cmd.exit_code),
            _ => None,
        })
        .filter(|c| *c == Some(1))
        .count();
    assert_eq!(failed, 1, "the failed cargo test records its exit 1");

    let results: Vec<bool> = trace
        .events
        .iter()
        .filter_map(|e| match &e.payload {
            EventPayload::ToolResult(res) => Some(res.is_error),
            _ => None,
        })
        .collect();
    assert_eq!(
        results.iter().filter(|e| **e).count(),
        1,
        "only the failed run is an error result"
    );

    let kinds = evidence(&trace);
    assert_eq!(
        kinds
            .iter()
            .filter(|k| **k == OutcomeKind::TestOrBuildPassed)
            .count(),
        1,
        "the failed run claims nothing; the retry carries the pass"
    );
    assert!(kinds.contains(&OutcomeKind::ArtifactChanged));
    assert!(!kinds.contains(&OutcomeKind::CommitObserved));
}

/// The bump: depth extraction changes what an already-parsed rollout yields, so the
/// incremental scanner must recompute every indexed codex row on the next scan. Version 4
/// adds the `custom_tool_call` pair and the compaction rounds, so it is the current
/// expected value.
#[test]
fn codex_parser_version_moved_past_the_depthless_version() {
    assert_eq!(CodexAdapter::PARSER_VERSION, 4);
    assert_eq!(
        CodexAdapter::new().parser_version(),
        CodexAdapter::PARSER_VERSION
    );
}
