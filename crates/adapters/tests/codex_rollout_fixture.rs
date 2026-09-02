//! The Codex adapter against a rollout shaped like the real thing.
//!
//! `tests/fixtures/rollout-*.jsonl` is hand-written and carries no transcript content: only
//! the record envelopes and the four fields the format actually holds -- `session_meta.cwd`,
//! `turn_context.model`, `turn_context.effort`, and the `token_count` counters. Every number
//! in it is invented; the shapes are copied from files under `~/.codex/sessions`.

use agentworth_adapter_sdk::{AgentAdapter, ScanOptions, SessionSource};
use agentworth_adapters::CodexAdapter;
use agentworth_schema::{extract_repository_or_workspace, EventPayload, TokenUsage};
use std::path::{Path, PathBuf};

const FIXTURE: &str = "rollout-2026-01-01T00-00-00-00000000-0000-7000-8000-000000000000.jsonl";

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(FIXTURE)
}

/// Copy the fixture into a throwaway `~/.codex/sessions/<y>/<m>/<d>/` tree so the test drives
/// the real discovery path (`is_candidate_codex_file`, `build_codex_source`) and not just
/// `parse`.
fn staged_codex_home() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("tempdir");
    let sessions = temp
        .path()
        .join(".codex")
        .join("sessions")
        .join("2026")
        .join("01")
        .join("01");
    std::fs::create_dir_all(&sessions).expect("create sessions dir");
    std::fs::copy(fixture_path(), sessions.join(FIXTURE)).expect("copy fixture");
    temp
}

fn enumerate_fixture(temp: &tempfile::TempDir) -> SessionSource {
    let adapter = CodexAdapter::new();
    let options = ScanOptions {
        custom_paths: vec![temp.path().to_path_buf()],
        ..Default::default()
    };
    let mut sources = adapter.enumerate(&options).expect("enumerate");
    assert_eq!(sources.len(), 1, "exactly one rollout in the staged tree");
    sources.remove(0)
}

fn model_invocations(
    trace: &agentworth_schema::AgentWorthTrace,
) -> Vec<(String, TokenUsage, Option<String>)> {
    trace
        .events
        .iter()
        .filter_map(|event| match &event.payload {
            EventPayload::ModelInvocation {
                model,
                token_usage,
                effort,
                ..
            } => Some((model.clone(), *token_usage, effort.clone())),
            _ => None,
        })
        .collect()
}

/// `session_meta.cwd` is the only thing in a rollout that names the repository -- the file
/// itself lives under `~/.codex/sessions/...`, which resolves to the home directory.
#[test]
fn codex_repository_comes_from_session_meta_cwd() {
    let temp = staged_codex_home();
    let source = enumerate_fixture(&temp);
    let adapter = CodexAdapter::new();
    let trace = adapter.parse(&source).expect("parse").trace;

    assert_eq!(
        extract_repository_or_workspace(&trace.provenance.source_path),
        "acme/widget"
    );
    // The identity string enumeration produced and the one parse recorded must be the same
    // string, or every rescan sees the source as changed and every stub prune misfires.
    assert_eq!(
        trace.provenance.source_path,
        source.path.to_string_lossy().to_string()
    );
    // The synthetic identity is not a real path, so presence has to go through the adapter.
    assert!(adapter.source_exists(&source));
    assert!(!source.path.exists());
    assert_eq!(trace.session_id, FIXTURE.trim_end_matches(".jsonl"));
}

/// A rollout with no readable workspace keeps its real path as its identity rather than
/// inventing one.
#[test]
fn codex_source_identity_is_the_real_path_when_no_workspace_is_readable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let sessions = temp.path().join(".codex").join("sessions");
    std::fs::create_dir_all(&sessions).expect("create sessions dir");
    let path = sessions.join(FIXTURE);
    std::fs::write(&path, "{\"type\":\"event_msg\",\"payload\":{}}\n").expect("write");

    let adapter = CodexAdapter::new();
    let options = ScanOptions {
        custom_paths: vec![temp.path().to_path_buf()],
        ..Default::default()
    };
    let sources = adapter.enumerate(&options).expect("enumerate");
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].path, path);
    assert!(adapter.source_exists(&sources[0]));
}

#[test]
fn codex_model_and_effort_come_from_turn_context() {
    let temp = staged_codex_home();
    let source = enumerate_fixture(&temp);
    let trace = CodexAdapter::new().parse(&source).expect("parse").trace;

    assert_eq!(
        trace.stats.models_used,
        vec!["gpt-5.6-sol".to_string(), "codex-auto-review".to_string()],
        "the reviewer sub-thread is a real model that really spent tokens; which one counts \
         as the session's model is a question for the query side"
    );
    // `high` on two turns, `low` on one.
    assert_eq!(trace.stats.effort.as_deref(), Some("high"));

    let invocations = model_invocations(&trace);
    assert_eq!(
        invocations
            .iter()
            .map(|(model, _, effort)| (model.as_str(), effort.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            ("gpt-5.6-sol", Some("high")),
            ("codex-auto-review", Some("low")),
            ("gpt-5.6-sol", Some("high")),
        ]
    );
}

/// Codex re-emits a turn's `last_token_usage` more than once (86 of 425 real rollout files
/// that carry token events overshoot their own cumulative total when you sum it). The
/// adapter reads the monotonic `total_token_usage` and attributes deltas, so the repeated
/// record on line 4 of the fixture contributes nothing and the session total lands on
/// exactly what the last record says it is.
#[test]
fn codex_tokens_are_deltas_of_the_cumulative_counter_not_a_sum_of_per_turn_counts() {
    let temp = staged_codex_home();
    let source = enumerate_fixture(&temp);
    let trace = CodexAdapter::new().parse(&source).expect("parse").trace;

    let usage = trace.stats.token_usage;
    // Cached input nests inside Codex's `input_tokens`; AgentWorth keeps the two disjoint.
    assert_eq!(usage.input_tokens, 1_100);
    assert_eq!(usage.output_tokens, 350);
    assert_eq!(usage.cache_read_tokens, 700);
    assert_eq!(usage.cache_creation_tokens, 0);
    // The fixture's final `total_token_usage.total_tokens`. Summing `last_token_usage`
    // instead would give 2,800.
    assert_eq!(usage.total(), 2_150);

    assert_eq!(
        model_invocations(&trace)
            .iter()
            .map(|(_, usage, _)| *usage)
            .collect::<Vec<_>>(),
        vec![
            TokenUsage::new(600, 200, 400, 0),
            TokenUsage::new(300, 100, 200, 0),
            TokenUsage::new(200, 50, 100, 0),
        ],
        "the repeated token_count must contribute a zero delta and emit no event"
    );

    assert_eq!(
        trace.stats.per_model_token_usage.get("gpt-5.6-sol"),
        Some(&TokenUsage::new(800, 250, 500, 0))
    );
    assert_eq!(
        trace.stats.per_model_token_usage.get("codex-auto-review"),
        Some(&TokenUsage::new(300, 100, 200, 0))
    );
}

/// Reading these fields is a change to what an unchanged file yields, which is exactly the
/// case `parser_version` exists for: without the bump, an incremental scan keeps serving the
/// empty rows version 1 produced.
#[test]
fn codex_parser_version_moved_past_the_version_that_read_nothing() {
    assert_eq!(CodexAdapter::PARSER_VERSION, 2);
    assert_eq!(CodexAdapter::new().parser_version(), 2);
}
