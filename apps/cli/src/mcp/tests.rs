use std::io::Write;
use std::sync::Arc;

use agentworth_schema::{AgentWorthTrace, Provenance, TokenUsage};
use agentworth_storage::Storage;
use chrono::Utc;
use rmcp::handler::server::wrapper::Parameters;
use serde_json::Value;

use super::params::{OutcomeRateParams, SessionGetParams, SessionsFindParams, SESSION_GET_DEFAULT_EVENTS_LIMIT};
use super::server::AgentWorthMcpServer;

/// Builds `n` minimal single-line Claude Code "user message" events, one per line, so a real
/// `Scanner::load_trace` parse produces a trace with exactly `n` events (unlike
/// `seed_non_stub_session`, which only ever populates the SQLite summary row -- `session_show`
/// goes through `Scanner::load_trace`, which re-parses the on-disk history file rather than
/// reading events back out of storage).
fn build_n_event_claude_jsonl(n: usize) -> String {
    (0..n)
        .map(|i| {
            format!(
                r#"{{"type":"user","timestamp":"2026-08-29T10:{:02}:{:02}Z","content":"event {i}"}}"#,
                (i / 60) % 60,
                i % 60
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Writes `contents` to a real temp `.jsonl` file and seeds a matching (non-stub) session row
/// pointing at it, so `Scanner::load_trace` can find and parse it. Returns the session ID and
/// the temp file (which must stay alive for the file to still exist on disk).
fn seed_trace_from_jsonl(storage: &Storage, contents: &str) -> (String, tempfile::NamedTempFile) {
    let mut temp_file = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .expect("create temp jsonl file");
    temp_file
        .write_all(contents.as_bytes())
        .expect("write temp jsonl contents");

    let session_id = temp_file
        .path()
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let prov = Provenance::new(
        temp_file.path().to_string_lossy().to_string(),
        "claude_code",
        1024,
        1700000000,
        format!("sha256:mock_hash_{session_id}"),
    );
    let trace = AgentWorthTrace::new(session_id.clone(), "claude_code", prov, Utc::now());
    storage.upsert_trace(&trace).expect("seed trace");

    (session_id, temp_file)
}

fn empty_sessions_find_params(limit: usize) -> SessionsFindParams {
    SessionsFindParams {
        repo: None,
        adapter: None,
        model: None,
        outcome: None,
        search: None,
        start_date: None,
        end_date: None,
        min_tokens: None,
        order_by: None,
        limit,
        offset: None,
        include_stubs: None,
    }
}

fn seed_non_stub_session(storage: &Storage, session_id: &str, source_path: &str) {
    let prov = Provenance::new(source_path, "claude_code", 100, 12345, format!("fp_{session_id}"));
    let mut trace = AgentWorthTrace::new(session_id, "claude_code", prov, Utc::now());
    // Non-stub per `NON_STUB_SQL_PREDICATE`: total_events > 1 AND total_tokens > 0.
    trace.stats.total_events = 5;
    trace.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
    storage.upsert_trace(&trace).expect("seed session");
}

fn call_result_json(result: rmcp::model::CallToolResult) -> Value {
    let text = result
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.clone())
        .expect("tool result should carry a text content block");
    serde_json::from_str(&text).expect("tool result text should be valid JSON")
}

#[test]
fn test_sessions_find_params_limit_is_required() {
    // `limit` has no `Option` wrapper on `SessionsFindParams` -- omitting it from the call
    // arguments must fail deserialization outright, not silently default to some value. This
    // is what makes `limit` "required" per docs/specs/mcp-server.md's "limit default trap"
    // note, distinct from the ceiling check exercised below.
    let missing_limit = serde_json::json!({});
    let result: Result<SessionsFindParams, _> = serde_json::from_value(missing_limit);
    assert!(
        result.is_err(),
        "SessionsFindParams must reject a call with no `limit` field"
    );
}

#[test]
fn test_session_get_params_include_raw_defaults_false() {
    let params: SessionGetParams =
        serde_json::from_value(serde_json::json!({ "session_id": "abc" })).unwrap();
    assert!(
        !params.include_raw,
        "include_raw must default to false when omitted -- redacted is the default output"
    );
}

#[tokio::test]
async fn test_sessions_find_rejects_zero_limit() {
    let storage = Arc::new(Storage::open_in_memory().unwrap());
    let server = AgentWorthMcpServer::new(storage);

    let err = server
        .session_list(Parameters(empty_sessions_find_params(0)))
        .await
        .expect_err("limit=0 must be rejected");
    assert!(err.message.contains("between 1 and 200"));
}

#[tokio::test]
async fn test_sessions_find_rejects_limit_over_ceiling() {
    let storage = Arc::new(Storage::open_in_memory().unwrap());
    let server = AgentWorthMcpServer::new(storage);

    let err = server
        .session_list(Parameters(empty_sessions_find_params(201)))
        .await
        .expect_err("limit=201 must be rejected -- the hard ceiling is 200");
    assert!(err.message.contains("between 1 and 200"));
}

#[tokio::test]
async fn test_sessions_find_accepts_limit_at_ceiling() {
    let storage = Arc::new(Storage::open_in_memory().unwrap());
    let server = AgentWorthMcpServer::new(storage);

    let result = server
        .session_list(Parameters(empty_sessions_find_params(200)))
        .await
        .expect("limit=200 is exactly the ceiling and must be accepted");
    let value = call_result_json(result);
    assert_eq!(value["sessions"].as_array().unwrap().len(), 0);
    assert_eq!(value["truncated"], false);
}

#[tokio::test]
async fn test_sessions_find_redacts_source_path_by_default() {
    let storage = Storage::open_in_memory().unwrap();
    seed_non_stub_session(
        &storage,
        "sess-1",
        "/Users/saurabh/code/unfoundbox/agentworth/sess-1.jsonl",
    );
    let server = AgentWorthMcpServer::new(Arc::new(storage));

    let result = server
        .session_list(Parameters(empty_sessions_find_params(10)))
        .await
        .unwrap();
    let value = call_result_json(result);
    let sessions = value["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    let source_path = sessions[0]["source_path"].as_str().unwrap();
    assert!(
        !source_path.contains("unfoundbox/agentworth"),
        "source_path must not leak the raw repository identity: {source_path}"
    );
    assert!(
        source_path.contains("[REDACTED_REPOSITORY]"),
        "source_path should carry the repository-identity redaction marker: {source_path}"
    );
}

#[tokio::test]
async fn test_sessions_find_repo_filter_sets_truncated_flag() {
    let storage = Storage::open_in_memory().unwrap();
    // Three sessions in the same repo, one in a different repo -- `repo` isn't a stored
    // column, so this exercises the over-fetch-then-post-filter path documented in
    // docs/specs/mcp-server.md's session_list section.
    seed_non_stub_session(
        &storage,
        "sess-a1",
        "/Users/saurabh/code/unfoundbox/agentworth/sess-a1.jsonl",
    );
    seed_non_stub_session(
        &storage,
        "sess-a2",
        "/Users/saurabh/code/unfoundbox/agentworth/sess-a2.jsonl",
    );
    seed_non_stub_session(
        &storage,
        "sess-a3",
        "/Users/saurabh/code/unfoundbox/agentworth/sess-a3.jsonl",
    );
    seed_non_stub_session(
        &storage,
        "sess-b1",
        "/Users/saurabh/code/othercorp/otherrepo/sess-b1.jsonl",
    );
    let server = AgentWorthMcpServer::new(Arc::new(storage));

    let mut params = empty_sessions_find_params(2);
    params.repo = Some("unfoundbox/agentworth".to_string());

    let result = server.session_list(Parameters(params)).await.unwrap();
    let value = call_result_json(result);
    let sessions = value["sessions"].as_array().unwrap();

    assert_eq!(
        sessions.len(),
        2,
        "must return exactly the requested limit, not every matching repo session"
    );
    assert_eq!(
        value["truncated"], true,
        "3 repo-A sessions exist but limit=2 -- truncated must be true"
    );
}

#[tokio::test]
async fn test_sessions_find_repo_filter_no_truncation_when_limit_covers_all_matches() {
    let storage = Storage::open_in_memory().unwrap();
    seed_non_stub_session(
        &storage,
        "sess-a1",
        "/Users/saurabh/code/unfoundbox/agentworth/sess-a1.jsonl",
    );
    seed_non_stub_session(
        &storage,
        "sess-b1",
        "/Users/saurabh/code/othercorp/otherrepo/sess-b1.jsonl",
    );
    let server = AgentWorthMcpServer::new(Arc::new(storage));

    let mut params = empty_sessions_find_params(10);
    params.repo = Some("unfoundbox/agentworth".to_string());

    let result = server.session_list(Parameters(params)).await.unwrap();
    let value = call_result_json(result);
    let sessions = value["sessions"].as_array().unwrap();

    assert_eq!(sessions.len(), 1);
    assert_eq!(value["truncated"], false);
}

#[tokio::test]
async fn test_session_get_not_found_returns_resource_not_found_error() {
    let storage = Arc::new(Storage::open_in_memory().unwrap());
    let server = AgentWorthMcpServer::new(storage);

    let err = server
        .session_show(Parameters(SessionGetParams {
            session_id: "does-not-exist".to_string(),
            include_raw: false,
            events_offset: None,
            events_limit: None,
        }))
        .await
        .expect_err("an unknown session_id must fail");
    assert!(err.message.contains("does-not-exist"));
}

#[tokio::test]
async fn test_session_get_default_events_limit_caps_large_trace() {
    // A session with more events than the default cap must never come back in full just
    // because the caller didn't pass events_limit -- that's the exact 64MB-by-accident shape
    // this default exists to prevent.
    let storage = Storage::open_in_memory().unwrap();
    let n = SESSION_GET_DEFAULT_EVENTS_LIMIT + 100;
    let (session_id, _temp_file) = seed_trace_from_jsonl(&storage, &build_n_event_claude_jsonl(n));
    let server = AgentWorthMcpServer::new(Arc::new(storage));

    let result = server
        .session_show(Parameters(SessionGetParams {
            session_id,
            include_raw: true,
            events_offset: None,
            events_limit: None,
        }))
        .await
        .expect("session_show should succeed");
    let value = call_result_json(result);

    assert_eq!(
        value["trace"]["events"].as_array().unwrap().len(),
        SESSION_GET_DEFAULT_EVENTS_LIMIT,
        "omitting events_limit must cap at the default, not return every event"
    );
    assert_eq!(value["events_total"], n as u64);
    assert_eq!(value["events_offset"], 0);
}

#[tokio::test]
async fn test_session_get_events_offset_and_limit_page_through_trace() {
    let storage = Storage::open_in_memory().unwrap();
    let (session_id, _temp_file) = seed_trace_from_jsonl(&storage, &build_n_event_claude_jsonl(10));
    let server = AgentWorthMcpServer::new(Arc::new(storage));

    let result = server
        .session_show(Parameters(SessionGetParams {
            session_id,
            include_raw: true,
            events_offset: Some(7),
            events_limit: Some(5),
        }))
        .await
        .expect("session_show should succeed");
    let value = call_result_json(result);

    // 10 events total, offset 7, limit 5 -- only 3 remain (indices 7, 8, 9).
    let events = value["trace"]["events"].as_array().unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(value["events_total"], 10);
    assert_eq!(value["events_offset"], 7);
}

#[tokio::test]
async fn test_session_get_events_offset_past_end_returns_empty_page() {
    let storage = Storage::open_in_memory().unwrap();
    let (session_id, _temp_file) = seed_trace_from_jsonl(&storage, &build_n_event_claude_jsonl(5));
    let server = AgentWorthMcpServer::new(Arc::new(storage));

    let result = server
        .session_show(Parameters(SessionGetParams {
            session_id,
            include_raw: true,
            events_offset: Some(100),
            events_limit: None,
        }))
        .await
        .expect("an out-of-range offset is not an error, just an empty page");
    let value = call_result_json(result);

    assert!(value["trace"]["events"].as_array().unwrap().is_empty());
    assert_eq!(value["events_total"], 5);
    assert_eq!(value["events_offset"], 100);
}

#[tokio::test]
async fn test_session_get_events_limit_zero_is_rejected() {
    let storage = Storage::open_in_memory().unwrap();
    let (session_id, _temp_file) = seed_trace_from_jsonl(&storage, &build_n_event_claude_jsonl(5));
    let server = AgentWorthMcpServer::new(Arc::new(storage));

    let err = server
        .session_show(Parameters(SessionGetParams {
            session_id,
            include_raw: true,
            events_offset: None,
            events_limit: Some(0),
        }))
        .await
        .expect_err("events_limit=0 must be rejected");
    assert!(err.message.contains("events_limit"));
}

#[tokio::test]
async fn test_coverage_stats_include_matrix_toggle() {
    let storage = Arc::new(Storage::open_in_memory().unwrap());
    let server = AgentWorthMcpServer::new(storage);

    let without_matrix = server
        .agent_list(Parameters(super::params::CoverageStatsParams {
            include_matrix: false,
        }))
        .await
        .unwrap();
    let value = call_result_json(without_matrix);
    assert!(value.get("matrix").is_none());

    let with_matrix = server
        .agent_list(Parameters(super::params::CoverageStatsParams {
            include_matrix: true,
        }))
        .await
        .unwrap();
    let value = call_result_json(with_matrix);
    assert!(value.get("matrix").is_some());
    assert!(value["matrix"]["total_adapters"].as_u64().unwrap() > 0);
}

fn seed_claimed_session(
    storage: &Storage,
    session_id: &str,
    adapter: &str,
    source_path: &str,
    models: &[&str],
    primary_outcome: Option<&str>,
) {
    let prov = Provenance::new(source_path, adapter, 100, 12345, format!("fp_{session_id}"));
    let mut trace = AgentWorthTrace::new(session_id, adapter, prov, Utc::now());
    // Non-stub per `NON_STUB_SQL_PREDICATE`: total_events > 1 AND total_tokens > 0.
    trace.stats.total_events = 5;
    trace.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
    trace.stats.models_used = models.iter().map(|m| m.to_string()).collect();
    storage
        .upsert_session(&trace, primary_outcome, None, 1)
        .expect("seed claimed session");
}

fn outcome_rate_params(
    group_by: &str,
    min_n: Option<usize>,
) -> serde_json::Value {
    serde_json::json!({ "group_by": group_by, "since": null, "until": null, "min_n": min_n })
}

#[test]
fn test_outcome_rate_group_by_rejects_unknown_value() {
    let params: Result<OutcomeRateParams, _> =
        serde_json::from_value(outcome_rate_params("worktree", None));
    assert!(
        params.is_err(),
        "group_by must reject anything outside model/adapter/repo"
    );
}

#[test]
fn test_outcome_rate_group_by_accepts_the_three_documented_values() {
    for value in ["model", "adapter", "repo"] {
        let params: Result<OutcomeRateParams, _> =
            serde_json::from_value(outcome_rate_params(value, None));
        assert!(params.is_ok(), "group_by={value} must deserialize");
    }
}

#[tokio::test]
async fn test_outcome_rate_min_n_defaults_to_20() {
    let storage = Arc::new(Storage::open_in_memory().unwrap());
    let server = AgentWorthMcpServer::new(storage);

    let result = server
        .stats_outcomes(Parameters(
            serde_json::from_value(outcome_rate_params("repo", None)).unwrap(),
        ))
        .await
        .unwrap();
    let value = call_result_json(result);
    assert_eq!(
        value["min_n"], 20,
        "min_n must default to 20 per docs/specs/verified-outcome-rate.md"
    );
}

#[tokio::test]
async fn test_outcome_rate_min_n_explicit_override() {
    let storage = Arc::new(Storage::open_in_memory().unwrap());
    let server = AgentWorthMcpServer::new(storage);

    let result = server
        .stats_outcomes(Parameters(
            serde_json::from_value(outcome_rate_params("repo", Some(3))).unwrap(),
        ))
        .await
        .unwrap();
    let value = call_result_json(result);
    assert_eq!(value["min_n"], 3);
}

#[tokio::test]
async fn test_outcome_rate_end_to_end_suppression_and_reason() {
    let storage = Storage::open_in_memory().unwrap();

    // repo-a: 3 claimed sessions, 2 verified -- clears a min_n of 3.
    seed_claimed_session(
        &storage,
        "sess-a1",
        "claude_code",
        "/home/u/code/org-a/repo-a/s1.jsonl",
        &["model-x"],
        Some("test_or_build_passed"),
    );
    seed_claimed_session(
        &storage,
        "sess-a2",
        "claude_code",
        "/home/u/code/org-a/repo-a/s2.jsonl",
        &["model-x"],
        Some("commit_observed"),
    );
    seed_claimed_session(
        &storage,
        "sess-a3",
        "claude_code",
        "/home/u/code/org-a/repo-a/s3.jsonl",
        &["model-x"],
        Some("done_claimed"),
    );

    // repo-b: 2 claimed sessions -- below a min_n of 3, must be suppressed.
    seed_claimed_session(
        &storage,
        "sess-b1",
        "claude_code",
        "/home/u/code/org-b/repo-b/s1.jsonl",
        &["model-y"],
        Some("ci_or_deployment_verified"),
    );
    seed_claimed_session(
        &storage,
        "sess-b2",
        "claude_code",
        "/home/u/code/org-b/repo-b/s2.jsonl",
        &["model-y"],
        Some("done_claimed"),
    );

    // repo-c: sessions exist but none ever claimed a primary_outcome -- a real "no
    // detection" row, not the same claim as "too little data."
    seed_claimed_session(
        &storage,
        "sess-c1",
        "codex",
        "/home/u/code/org-c/repo-c/s1.jsonl",
        &["model-z"],
        None,
    );

    let server = AgentWorthMcpServer::new(Arc::new(storage));
    let result = server
        .stats_outcomes(Parameters(
            serde_json::from_value(outcome_rate_params("repo", Some(3))).unwrap(),
        ))
        .await
        .unwrap();
    let value = call_result_json(result);

    assert_eq!(value["suppressed_groups"], 1, "repo-b's n=2 must be suppressed under min_n=3");

    let rows = value["rows"].as_array().unwrap();
    let repo_a = rows
        .iter()
        .find(|r| r["key"] == "org-a/repo-a")
        .expect("repo-a row must be present");
    assert_eq!(repo_a["n"], 3);
    assert_eq!(repo_a["verified"], 2);
    assert!((repo_a["rate"].as_f64().unwrap() - (2.0 / 3.0)).abs() < 1e-9);
    assert!(rows.iter().all(|r| r["key"] != "org-b/repo-b"), "suppressed rows must not appear");

    let repo_c = rows
        .iter()
        .find(|r| r["key"] == "org-c/repo-c")
        .expect("repo-c must still appear, unsuppressed");
    assert_eq!(repo_c["n"], 0);
    assert!(repo_c["rate"].is_null());
    assert_eq!(repo_c["reason"], "no_outcome_detection");

    // baseline: 5 claimed sessions total (a1..a3, b1, b2), 3 verified (a1, a2, b1).
    assert_eq!(value["baseline"]["n"], 5);
    assert_eq!(value["baseline"]["verified"], 3);
}

// -----------------------------------------------------------------------------
// session_handoff / session_carry_forward (docs/specs/handoff.md)
// -----------------------------------------------------------------------------

use super::params::{CarryForwardParams, SessionHandoffParams};

/// A real Claude Code transcript for one working session: a stated decision, a file edit, a
/// test run and a commit (each a `Bash` tool call plus its result, which is how Claude Code
/// records them), and a commitment that was stated and then handed straight back to the user.
///
/// Written to a real `projects/-Users-...` directory so `extract_repository_or_workspace`
/// derives a stable repo key from it, which is what `session_carry_forward` queries on.
fn write_fixture_session(dir: &std::path::Path, session_id: &str, day: &str) -> std::path::PathBuf {
    let project = dir.join("projects").join("-Users-x-code-unfoundbox-agentworth");
    std::fs::create_dir_all(&project).expect("create project dir");

    let lines = [
        format!(r#"{{"type":"user","timestamp":"{day}T14:02:00Z","content":"port the loose-ends detector to Rust"}}"#),
        format!(r#"{{"type":"assistant","timestamp":"{day}T14:03:00Z","model":"claude-opus-5","usage":{{"input_tokens":500,"output_tokens":120,"cache_read_input_tokens":200,"cache_creation_input_tokens":50}},"content":[{{"type":"text","text":"We decided to keep the exit-code index out of SQLite for now."}}]}}"#),
        format!(r#"{{"type":"assistant","timestamp":"{day}T14:05:00Z","content":[{{"type":"tool_use","id":"t1","name":"Edit","input":{{"file_path":"crates/storage/src/lib.rs","old_string":"a","new_string":"b"}}}}]}}"#),
        format!(r#"{{"type":"assistant","timestamp":"{day}T14:07:00Z","content":[{{"type":"tool_use","id":"t2","name":"Bash","input":{{"command":"cargo test -p agentworth-storage"}}}}]}}"#),
        format!(r#"{{"type":"user","timestamp":"{day}T14:08:00Z","content":[{{"type":"tool_result","tool_use_id":"t2","content":"test result: ok. 12 passed; 0 failed"}}]}}"#),
        format!(r#"{{"type":"assistant","timestamp":"{day}T14:09:00Z","content":[{{"type":"tool_use","id":"t3","name":"Bash","input":{{"command":"git commit -m 'feat: repo-scoped lookup'"}}}}]}}"#),
        format!(r#"{{"type":"user","timestamp":"{day}T14:10:00Z","content":[{{"type":"tool_result","tool_use_id":"t3","content":"[main 9f3e1a2] feat: repo-scoped lookup"}}]}}"#),
        format!(r#"{{"type":"assistant","timestamp":"{day}T14:12:00Z","content":[{{"type":"text","text":"I'll delete the stale worktree sk-ant-abcdefghijklmnopqrstuvwxyz012345 before the next scan."}}]}}"#),
        format!(r#"{{"type":"user","timestamp":"{day}T14:13:00Z","content":"thanks, stop there"}}"#),
    ];

    let path = project.join(format!("{session_id}.jsonl"));
    std::fs::write(&path, lines.join("\n")).expect("write fixture transcript");
    path
}

/// Seeds one fixture session into `storage` and returns its id.
fn seed_fixture_session(storage: &Storage, dir: &std::path::Path, session_id: &str, day: &str) {
    let path = write_fixture_session(dir, session_id, day);
    let prov = Provenance::new(
        path.to_string_lossy().to_string(),
        "claude_code",
        4096,
        1_756_000_000,
        format!("sha256:{session_id}"),
    );
    let mut trace = AgentWorthTrace::new(
        session_id,
        "claude_code",
        prov,
        chrono::DateTime::parse_from_rfc3339(&format!("{day}T14:02:00Z"))
            .unwrap()
            .with_timezone(&Utc),
    );
    trace.stats.total_events = 9;
    trace.stats.token_usage = TokenUsage::new(500, 120, 200, 50);
    storage.upsert_trace(&trace).expect("seed fixture session");
}

fn handoff_params(session_id: &str) -> SessionHandoffParams {
    SessionHandoffParams {
        session_id: Some(session_id.to_string()),
        max_lines: None,
        include_loose_ends: None,
        include_raw: false,
    }
}

#[test]
fn test_session_handoff_include_raw_defaults_false() {
    let params: SessionHandoffParams =
        serde_json::from_value(serde_json::json!({ "session_id": "abc" })).unwrap();
    assert!(
        !params.include_raw,
        "redacted is the default for every tool that can carry event or file content"
    );
    assert_eq!(params.max_lines, None);
    assert_eq!(params.include_loose_ends, None);
}

#[tokio::test]
async fn test_session_handoff_unknown_session_is_not_found() {
    let storage = Arc::new(Storage::open_in_memory().unwrap());
    let server = AgentWorthMcpServer::new(storage);

    let err = server
        .session_handoff(Parameters(handoff_params("no-such-session")))
        .await
        .expect_err("an unknown session id must not resolve to a blank handoff");
    assert!(
        err.message.contains("no-such-session"),
        "the error must name the session that was asked for: {}",
        err.message
    );
}

#[tokio::test]
async fn test_session_handoff_assembles_the_whole_document_from_a_real_transcript() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open_in_memory().unwrap();
    let id = "452c23fd-6e9b-4948-8e8f-6a31f1c3f7dd";
    seed_fixture_session(&storage, dir.path(), id, "2026-09-01");
    let server = AgentWorthMcpServer::new(Arc::new(storage));

    let value = call_result_json(
        server
            .session_handoff(Parameters(SessionHandoffParams {
                include_raw: true,
                ..handoff_params(id)
            }))
            .await
            .expect("handoff for a seeded session"),
    );
    let markdown = value["markdown"].as_str().expect("markdown");

    assert!(markdown.contains("unfoundbox/agentworth"), "{markdown}");
    assert!(markdown.contains("rung 4, commit_observed"), "{markdown}");
    assert!(markdown.contains("crates/storage/src/lib.rs"), "{markdown}");
    assert!(markdown.contains("cargo test -p agentworth-storage"), "{markdown}");
    assert!(markdown.contains("out of SQLite"), "the stated decision is quoted: {markdown}");
    assert!(markdown.contains("delete the stale worktree"), "the dropped commitment: {markdown}");
    assert!(markdown.contains("## Not in this handoff"), "{markdown}");
    assert!(markdown.contains(&format!("session {id}")), "the receipt names the session");
    assert!(markdown.lines().count() <= 60, "the default budget is 60 lines");

    // `prompt_preview` is not filled by the scanner yet, so the one line a handoff most needs
    // is a stated gap rather than a guess.
    let gaps: Vec<&str> = value["gaps"].as_array().unwrap().iter().map(|g| g.as_str().unwrap()).collect();
    assert!(gaps.contains(&"prompt_preview_empty"), "{gaps:?}");
    assert_eq!(value["receipt"]["redacted"], false);
}

#[tokio::test]
async fn test_session_handoff_is_redacted_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open_in_memory().unwrap();
    let id = "452c23fd-6e9b-4948-8e8f-6a31f1c3f7dd";
    seed_fixture_session(&storage, dir.path(), id, "2026-09-01");
    let server = AgentWorthMcpServer::new(Arc::new(storage));

    let value = call_result_json(
        server
            .session_handoff(Parameters(handoff_params(id)))
            .await
            .expect("handoff for a seeded session"),
    );
    let markdown = value["markdown"].as_str().expect("markdown");

    assert!(
        !markdown.contains("sk-ant-abcdefghijklmnopqrstuvwxyz012345"),
        "an API key quoted inside a loose end must not survive the default:\n{markdown}"
    );
    assert!(
        !markdown.contains("unfoundbox/agentworth"),
        "the session's own repository identity must be masked by default:\n{markdown}"
    );
    assert!(markdown.contains("delete the stale worktree"), "the claim itself survives");
    assert_eq!(value["receipt"]["redacted"], true);
    assert!(
        !value["receipt"]["source_path"].as_str().unwrap().contains("unfoundbox/agentworth"),
        "the receipt's own path is redacted too, on the same redactor instance"
    );
}

#[tokio::test]
async fn test_session_handoff_rejects_a_max_lines_out_of_range() {
    let storage = Arc::new(Storage::open_in_memory().unwrap());
    let server = AgentWorthMcpServer::new(storage);

    for bad in [0usize, 121] {
        let err = server
            .session_handoff(Parameters(SessionHandoffParams {
                max_lines: Some(bad),
                ..handoff_params("anything")
            }))
            .await
            .expect_err("out-of-range max_lines is rejected, not clamped");
        assert!(err.message.contains("between 1 and 120"), "{}", err.message);
    }
}

#[tokio::test]
async fn test_carry_forward_returns_the_last_n_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open_in_memory().unwrap();
    for (id, day) in [
        ("aaaaaaaa-0000-0000-0000-000000000001", "2026-08-30"),
        ("bbbbbbbb-0000-0000-0000-000000000002", "2026-08-31"),
        ("cccccccc-0000-0000-0000-000000000003", "2026-09-01"),
    ] {
        seed_fixture_session(&storage, dir.path(), id, day);
    }
    let server = AgentWorthMcpServer::new(Arc::new(storage));

    let value = call_result_json(
        server
            .session_carry_forward(Parameters(CarryForwardParams {
                repo: "unfoundbox/agentworth".to_string(),
                n: Some(2),
                since: None,
                max_lines: None,
                include_raw: true,
                include_subagents: false,
            }))
            .await
            .expect("session_carry_forward for a seeded repo"),
    );

    let handoffs = value["handoffs"].as_array().unwrap();
    assert_eq!(handoffs.len(), 2, "n=2 caps the list");
    assert_eq!(
        handoffs[0]["receipt"]["session_id"], "cccccccc-0000-0000-0000-000000000003",
        "newest first"
    );
    assert_eq!(handoffs[1]["receipt"]["session_id"], "bbbbbbbb-0000-0000-0000-000000000002");
    assert!(value["unreadable"].as_array().unwrap().is_empty());
    assert_eq!(value["scan_exhausted"], false);
}

/// Seeds a subagent transcript for `parent_id`, at
/// `<project>/<parent_id>/subagents/agent-abc.jsonl`, started after the parent.
fn seed_subagent_session(storage: &Storage, dir: &std::path::Path, parent_id: &str, day: &str) {
    let project = dir.join("projects").join("-Users-x-code-unfoundbox-agentworth");
    let subagents_dir = project.join(parent_id).join("subagents");
    std::fs::create_dir_all(&subagents_dir).expect("create subagents dir");
    let path = subagents_dir.join("agent-abc123.jsonl");
    std::fs::write(
        &path,
        format!(
            "{{\"type\":\"user\",\"timestamp\":\"{day}T15:00:00Z\",\"content\":\"find the callers\"}}\n\
             {{\"type\":\"assistant\",\"timestamp\":\"{day}T15:01:00Z\",\"content\":[{{\"type\":\"text\",\"text\":\"found them\"}}]}}"
        ),
    )
    .expect("write subagent transcript");

    let prov = Provenance::new(
        path.to_string_lossy().to_string(),
        "claude_code",
        1024,
        1_756_000_000,
        "sha256:subagent",
    );
    // The Claude Code adapter re-derives a session id from the file stem on every reparse
    // (`derive_session_id`), so the seeded id must equal the file name for
    // `scanner.load_trace` to find this row again.
    let session_id = "agent-abc123";
    let mut trace = AgentWorthTrace::new(
        session_id,
        "claude_code",
        prov,
        chrono::DateTime::parse_from_rfc3339(&format!("{day}T15:00:00Z"))
            .unwrap()
            .with_timezone(&Utc),
    );
    trace.stats.total_events = 2;
    trace.stats.token_usage = TokenUsage::new(50, 10, 0, 0);
    storage.upsert_trace(&trace).expect("seed subagent session");
}

#[tokio::test]
async fn test_carry_forward_excludes_subagents_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open_in_memory().unwrap();
    let parent_id = "aaaaaaaa-0000-0000-0000-000000000001";
    seed_fixture_session(&storage, dir.path(), parent_id, "2026-08-30");
    // Started after the parent, so it would otherwise "win newest".
    seed_subagent_session(&storage, dir.path(), parent_id, "2026-09-01");
    let server = AgentWorthMcpServer::new(Arc::new(storage));

    let value = call_result_json(
        server
            .session_carry_forward(Parameters(CarryForwardParams {
                repo: "unfoundbox/agentworth".to_string(),
                n: Some(2),
                since: None,
                max_lines: None,
                include_raw: true,
                include_subagents: false,
            }))
            .await
            .expect("session_carry_forward for a seeded repo"),
    );
    let handoffs = value["handoffs"].as_array().unwrap();
    assert_eq!(handoffs.len(), 1, "the subagent must not appear by default");
    assert_eq!(handoffs[0]["receipt"]["session_id"], parent_id);

    let value = call_result_json(
        server
            .session_carry_forward(Parameters(CarryForwardParams {
                repo: "unfoundbox/agentworth".to_string(),
                n: Some(2),
                since: None,
                max_lines: None,
                include_raw: true,
                include_subagents: true,
            }))
            .await
            .expect("session_carry_forward with include_subagents"),
    );
    let handoffs = value["handoffs"].as_array().unwrap();
    assert_eq!(handoffs.len(), 2, "include_subagents=true brings it back");
    assert_eq!(handoffs[0]["receipt"]["session_id"], "agent-abc123", "newest first");
}

#[tokio::test]
async fn test_carry_forward_unknown_repo_is_empty_not_an_error() {
    let storage = Arc::new(Storage::open_in_memory().unwrap());
    let server = AgentWorthMcpServer::new(storage);

    let value = call_result_json(
        server
            .session_carry_forward(Parameters(CarryForwardParams {
                repo: "nobody/nothing".to_string(),
                ..Default::default()
            }))
            .await
            .expect("an unknown repo is a legitimate empty answer"),
    );
    assert!(value["handoffs"].as_array().unwrap().is_empty());
    assert_eq!(value["repo"], "nobody/nothing");
}

#[tokio::test]
async fn test_carry_forward_rejects_n_over_the_ceiling() {
    let storage = Arc::new(Storage::open_in_memory().unwrap());
    let server = AgentWorthMcpServer::new(storage);

    let err = server
        .session_carry_forward(Parameters(CarryForwardParams {
            repo: "unfoundbox/agentworth".to_string(),
            n: Some(11),
            ..Default::default()
        }))
        .await
        .expect_err("n=11 is over the ceiling");
    assert!(err.message.contains("between 1 and 10"), "{}", err.message);
}

#[tokio::test]
async fn test_carry_forward_names_a_session_it_could_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open_in_memory().unwrap();
    let id = "dddddddd-0000-0000-0000-000000000004";
    seed_fixture_session(&storage, dir.path(), id, "2026-09-01");
    // The transcript is gone but the index row remains -- exactly what a rotated or deleted
    // history file looks like. "One session, unreadable" is a different answer from "none".
    std::fs::remove_file(
        dir.path()
            .join("projects")
            .join("-Users-x-code-unfoundbox-agentworth")
            .join(format!("{id}.jsonl")),
    )
    .unwrap();
    let server = AgentWorthMcpServer::new(Arc::new(storage));

    let value = call_result_json(
        server
            .session_carry_forward(Parameters(CarryForwardParams {
                repo: "unfoundbox/agentworth".to_string(),
                ..Default::default()
            }))
            .await
            .expect("an unreadable session must not fail the whole call"),
    );
    assert!(value["handoffs"].as_array().unwrap().is_empty());
    let unreadable = value["unreadable"].as_array().unwrap();
    assert_eq!(unreadable.len(), 1);
    assert_eq!(unreadable[0]["session_id"], id);
}

use super::params::ForgottenContextParams;

/// A synthetic two-round compacted session in the exact shape Claude Code writes it: a
/// `system`/`compact_boundary` record carrying `compactMetadata`, immediately followed by a
/// `user` record with `isCompactSummary: true`. Round 1 drops two decision-shaped sentences,
/// one of which the summary restates; round 2 drops one more and restates nothing.
fn write_compacted_session(dir: &std::path::Path, session_id: &str) -> std::path::PathBuf {
    let project = dir.join("projects").join("-Users-x-code-unfoundbox-agentworth");
    std::fs::create_dir_all(&project).expect("create project dir");

    let lines = [
        r#"{"type":"user","timestamp":"2026-09-01T10:00:00Z","content":"build the compaction diff"}"#,
        r#"{"type":"assistant","timestamp":"2026-09-01T10:01:00Z","model":"claude-opus-5","usage":{"input_tokens":500,"output_tokens":120,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"We decided to store the round boundaries rather than rescan the file. The splitter is hand-rolled because the regex crate has no lookbehind."}]}"#,
        r#"{"type":"assistant","timestamp":"2026-09-01T10:02:00Z","content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"crates/storage/src/lib.rs","old_string":"a","new_string":"b"}}]}"#,
        r#"{"type":"system","subtype":"compact_boundary","timestamp":"2026-09-01T10:30:00Z","uuid":"b1","compactMetadata":{"trigger":"manual","preTokens":754356,"postTokens":25834,"durationMs":1000,"cumulativeDroppedTokens":728522}}"#,
        r#"{"type":"user","timestamp":"2026-09-01T10:30:01Z","isCompactSummary":true,"parentUuid":"b1","message":{"role":"user","content":"Carried forward: we decided to store the round boundaries rather than rescan the file."}}"#,
        r#"{"type":"assistant","timestamp":"2026-09-01T11:00:00Z","content":[{"type":"text","text":"Going with 0.6 Jaccard instead of an exact match on the sentence text."}]}"#,
        r#"{"type":"system","subtype":"compact_boundary","timestamp":"2026-09-01T11:30:00Z","uuid":"b2","compactMetadata":{"trigger":"manual","preTokens":851578,"postTokens":19399,"durationMs":1000,"cumulativeDroppedTokens":832179}}"#,
        r#"{"type":"user","timestamp":"2026-09-01T11:30:01Z","isCompactSummary":true,"parentUuid":"b2","message":{"role":"user","content":"The work continues on the extractor and on its tests."}}"#,
        r#"{"type":"assistant","timestamp":"2026-09-01T11:31:00Z","content":[{"type":"text","text":"We decided this last one is after every round and is still in context."}]}"#,
    ];

    let path = project.join(format!("{session_id}.jsonl"));
    std::fs::write(&path, lines.join("\n")).expect("write compacted transcript");
    path
}

/// Seeds a compacted session the way a real scan would: parsed by the adapter, so the round
/// boundaries land in `session_compaction` rather than being hand-written into it.
fn seed_compacted_session(storage: &Storage, dir: &std::path::Path, session_id: &str) {
    use agentworth_adapter_sdk::{AgentAdapter, SessionSource};

    let path = write_compacted_session(dir, session_id);
    let adapter = agentworth_adapters::ClaudeCodeAdapter::new();
    let source = SessionSource::from_path(&path, adapter.name()).expect("source");
    let mut trace = adapter.parse(&source).expect("parse compacted fixture").trace;
    trace.session_id = session_id.to_string();
    storage.upsert_trace(&trace).expect("seed compacted session");
}

fn forgotten_params(session_id: &str) -> ForgottenContextParams {
    ForgottenContextParams {
        session_id: Some(session_id.to_string()),
        include_raw: true,
        ..Default::default()
    }
}

#[test]
fn test_forgotten_context_include_raw_defaults_false() {
    let params: ForgottenContextParams =
        serde_json::from_value(serde_json::json!({ "session_id": "abc" })).unwrap();
    assert!(
        !params.include_raw,
        "everything this tool returns is transcript text; redacted is the default"
    );
    assert_eq!(params.limit, None);
    assert_eq!(params.round, None);
}

#[tokio::test]
async fn test_forgotten_context_diffs_a_two_round_session_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open_in_memory().unwrap();
    let id = "cccccccc-0000-0000-0000-000000000001";
    seed_compacted_session(&storage, dir.path(), id);

    // The boundaries came off the scan, not off a rescan inside the tool.
    assert_eq!(storage.get_compaction_rounds(id).unwrap().len(), 2);

    let server = AgentWorthMcpServer::new(Arc::new(storage));
    let value = call_result_json(
        server
            .session_forgotten(Parameters(forgotten_params(id)))
            .await
            .expect("diff for a seeded compacted session"),
    );

    assert_eq!(value["compactions"], 2);
    assert_eq!(value["receipt"]["rounds_source"], "index");
    assert_eq!(value["receipt"]["method"], "regex_v1");
    assert_eq!(value["receipt"]["no_model"], true);

    // Round 1 dropped two, one of which the summary restates; round 2 dropped one.
    assert_eq!(value["dropped_total"], 3);
    assert_eq!(value["survived_in_summary"], 1);
    assert_eq!(value["forgotten_total"], 2);

    let forgotten = value["forgotten"].as_array().unwrap();
    assert_eq!(forgotten.len(), 2);
    assert_eq!(forgotten[0]["round"], 2, "newest first");
    assert!(forgotten[0]["text"].as_str().unwrap().contains("0.6 Jaccard"));
    assert_eq!(forgotten[1]["round"], 1);
    assert!(
        forgotten[1]["text"].as_str().unwrap().contains("because"),
        "the reason survives extraction even though it did not survive compaction"
    );
    assert!(
        !forgotten
            .iter()
            .any(|f| f["text"].as_str().unwrap().contains("still in context")),
        "a sentence after the last boundary was never dropped"
    );

    // The reason sentence was followed by an Edit, which is what makes it checkable.
    assert_eq!(forgotten[1]["followed_by"][0]["what"], "tool_call:Edit");

    assert!(
        value["headline"]
            .as_str()
            .unwrap()
            .starts_with("Things you decided"),
        "{}",
        value["headline"]
    );
    assert!(value["notes"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_forgotten_context_round_and_class_filters() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open_in_memory().unwrap();
    let id = "cccccccc-0000-0000-0000-000000000002";
    seed_compacted_session(&storage, dir.path(), id);
    let server = AgentWorthMcpServer::new(Arc::new(storage));

    let round_one = call_result_json(
        server
            .session_forgotten(Parameters(ForgottenContextParams {
                round: Some(1),
                ..forgotten_params(id)
            }))
            .await
            .expect("round filter"),
    );
    let rows = round_one["forgotten"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["round"], 1);

    let reasons = call_result_json(
        server
            .session_forgotten(Parameters(ForgottenContextParams {
                classes: Some(vec!["reason".to_string()]),
                ..forgotten_params(id)
            }))
            .await
            .expect("class filter"),
    );
    assert_eq!(reasons["returned"], 1);
    assert_eq!(
        reasons["forgotten_total"], 2,
        "the totals describe the session, not the filter"
    );

    let bad = server
        .session_forgotten(Parameters(ForgottenContextParams {
            classes: Some(vec!["everything".to_string()]),
            ..forgotten_params(id)
        }))
        .await
        .expect_err("an unknown class must be rejected, not ignored");
    assert!(bad.message.contains("everything"), "{}", bad.message);
}

#[tokio::test]
async fn test_forgotten_context_is_redacted_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open_in_memory().unwrap();
    let id = "cccccccc-0000-0000-0000-000000000003";
    seed_compacted_session(&storage, dir.path(), id);
    let server = AgentWorthMcpServer::new(Arc::new(storage));

    let value = call_result_json(
        server
            .session_forgotten(Parameters(ForgottenContextParams {
                session_id: Some(id.to_string()),
                ..Default::default()
            }))
            .await
            .expect("default call"),
    );
    assert_eq!(value["receipt"]["redacted"], true);
    assert!(
        !value["receipt"]["source_path"]
            .as_str()
            .unwrap()
            .contains("unfoundbox/agentworth"),
        "the session's own repository identity must be masked: {}",
        value["receipt"]["source_path"]
    );
}

/// A session that never compacted returns a named "nothing here" rather than an empty list a
/// caller could read as a finding.
#[tokio::test]
async fn test_forgotten_context_never_compacted_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open_in_memory().unwrap();
    let id = "cccccccc-0000-0000-0000-000000000004";
    seed_fixture_session(&storage, dir.path(), id, "2026-09-01");
    let server = AgentWorthMcpServer::new(Arc::new(storage));

    let value = call_result_json(
        server
            .session_forgotten(Parameters(forgotten_params(id)))
            .await
            .expect("a never-compacted session is not an error"),
    );
    assert_eq!(value["compactions"], 0);
    assert!(value["forgotten"].as_array().unwrap().is_empty());
    let notes: Vec<&str> = value["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap())
        .collect();
    assert_eq!(notes, vec!["no_compactions_in_this_session"]);
}

#[tokio::test]
async fn test_forgotten_context_rejects_a_limit_over_the_ceiling() {
    let storage = Arc::new(Storage::open_in_memory().unwrap());
    let server = AgentWorthMcpServer::new(storage);

    let err = server
        .session_forgotten(Parameters(ForgottenContextParams {
            limit: Some(500),
            ..forgotten_params("anything")
        }))
        .await
        .expect_err("a limit over the ceiling is an error, not a silent clamp");
    assert!(err.message.contains("200"), "{}", err.message);
}

/// `sessions.source_path` can point at a file that has since been deleted. Returning a partial
/// diff assembled from the index row would be inventing content, so the tool refuses.
#[tokio::test]
async fn test_forgotten_context_refuses_when_the_transcript_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open_in_memory().unwrap();
    let id = "cccccccc-0000-0000-0000-000000000005";
    seed_compacted_session(&storage, dir.path(), id);
    std::fs::remove_file(
        dir.path()
            .join("projects")
            .join("-Users-x-code-unfoundbox-agentworth")
            .join(format!("{id}.jsonl")),
    )
    .unwrap();
    let server = AgentWorthMcpServer::new(Arc::new(storage));

    let err = server
        .session_forgotten(Parameters(forgotten_params(id)))
        .await
        .expect_err("a missing transcript must refuse rather than answer from the index");
    assert!(err.message.contains(id), "{}", err.message);
}

/// The MCP surface, end to end on a real temp git repo: two commits, one written by a session
/// that never got past `artifact_changed`. Exercises the parameter decoding, the default
/// window, and the redaction pass — none of which the compute-level tests in
/// `commands/suspect/tests.rs` go through.
#[tokio::test]
async fn test_suspect_commits_flags_the_unproven_commit() {
    use agentworth_schema::{EventPayload, FileActionType, NormalizedEvent};
    use std::process::Command;

    let dir = tempfile::tempdir().expect("tempdir");
    // macOS temp dirs live under a symlink; `git rev-parse --show-toplevel` reports the
    // resolved path, and anchoring compares strings, so the fixture has to resolve it too.
    let root = std::fs::canonicalize(dir.path()).expect("canonicalize");
    let run = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run(&["init", "-q", "-b", "main"]);
    run(&["config", "user.email", "test@example.invalid"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "commit.gpgsign", "false"]);

    std::fs::write(root.join("first.rs"), "fn a() {}\n").expect("write");
    run(&["add", "first.rs"]);
    run(&["commit", "-q", "-m", "feat: first"]);
    std::fs::write(root.join("second.rs"), "fn b() {}\n").expect("write");
    run(&["add", "second.rs"]);
    run(&["commit", "-q", "-m", "feat: second"]);

    let storage = Arc::new(Storage::open_in_memory().unwrap());
    // Comfortably inside the attribution window and unambiguously before the commits, so the
    // test does not lean on `COMMIT_TIME_SLACK` to pass. Git records whole seconds; `Utc::now()`
    // does not, and an edit stamped in the same second as its commit reads as later.
    let at = Utc::now() - chrono::Duration::minutes(1);
    let prov = Provenance::new(
        format!(
            "/tmp/claude/projects/{}/s.jsonl",
            root.to_string_lossy().replace('/', "-")
        ),
        "claude_code",
        100,
        100,
        "fp_suspect",
    );
    let mut trace = AgentWorthTrace::new("sess_suspect_mcp", "claude_code", prov, at);
    trace.events.push(NormalizedEvent::new(
        1,
        at,
        EventPayload::FileAction {
            path: root.join("second.rs").to_string_lossy().to_string(),
            action: FileActionType::Edit,
            diff: None,
            lines_changed: None,
        },
    ));
    storage
        .upsert_session(&trace, Some("artifact_changed"), Some(0.4), 1)
        .expect("seed session");

    let server = AgentWorthMcpServer::new(storage);
    let params = serde_json::json!({ "repo": root.to_string_lossy() });
    let result = server
        .repo_suspect(Parameters(serde_json::from_value(params).unwrap()))
        .await
        .expect("repo_suspect");
    let value = call_result_json(result);

    assert_eq!(value["commits_scanned"], 2);
    assert_eq!(value["attributed"], 1);
    assert_eq!(
        value["unattributed"], 1,
        "a commit with no authoring session is unknown, and says so in its own field"
    );
    assert_eq!(value["window_hours"], 24);
    assert_eq!(value["suspect"].as_array().unwrap().len(), 1);
    assert_eq!(value["suspect"][0]["subject"], "feat: second");
    assert_eq!(
        value["suspect"][0]["sessions"][0]["reasons"][0]["code"],
        "no_test_run"
    );
    assert!(
        value["receipt"]["anchoring_rule"].is_string(),
        "the answer states the rule it applied"
    );
    assert!(
        value.get("patch").is_none() && value.get("diff").is_none(),
        "the output is a list and a prompt, never a change"
    );
}

/// A repo path that is not a git checkout comes back as a parameter error naming the missing
/// noun, not an opaque internal one.
#[tokio::test]
async fn test_suspect_commits_rejects_a_non_repo_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let storage = Arc::new(Storage::open_in_memory().unwrap());
    let server = AgentWorthMcpServer::new(storage);
    let params = serde_json::json!({ "repo": dir.path().to_string_lossy() });
    let err = server
        .repo_suspect(Parameters(serde_json::from_value(params).unwrap()))
        .await
        .expect_err("a non-repo path must be rejected");
    assert!(err.message.contains("git repository"), "got: {}", err.message);
}

/// `stats_ladder` answers with all three blocks, and defaults to the shared sample floor
/// rather than a second number invented at the tool boundary.
#[tokio::test]
async fn test_stats_ladder_returns_three_blocks_and_the_shared_floor() {
    let storage = Storage::open_in_memory().unwrap();
    seed_claimed_session(
        &storage,
        "sess-ci",
        "claude_code",
        "/home/u/code/org-a/repo-a/s1.jsonl",
        &["model-x"],
        Some("ci_or_deployment_verified"),
    );
    seed_claimed_session(
        &storage,
        "sess-done",
        "claude_code",
        "/home/u/code/org-a/repo-a/s2.jsonl",
        &["model-x"],
        Some("done_claimed"),
    );
    seed_claimed_session(
        &storage,
        "sess-unflown",
        "claude_code",
        "/home/u/code/org-a/repo-a/s3.jsonl",
        &["model-x"],
        None,
    );
    let server = AgentWorthMcpServer::new(Arc::new(storage));

    let result = server
        .stats_ladder(Parameters(
            serde_json::from_value(serde_json::json!({ "period": "all" })).unwrap(),
        ))
        .await
        .unwrap();
    let value = call_result_json(result);

    assert_eq!(value["min_n"], 20, "the floor comes from storage, not from here");
    assert_eq!(value["period"], "all");
    assert_eq!(value["cost_basis"], "api_list_price_equivalent");
    assert_eq!(value["total_sessions"], 3);

    let rungs = value["rungs"].as_array().expect("rungs");
    assert_eq!(rungs.len(), 6);
    assert_eq!(rungs[0]["outcome"], "ci_or_deployment_verified");
    assert_eq!(rungs[0]["sessions"], 1);
    assert_eq!(rungs[5]["outcome"], "unflown");
    assert_eq!(rungs[5]["sessions"], 1);

    // Two claimed sessions is far under the floor, so the rate is null -- not 50%.
    let groups = value["groups"].as_array().expect("groups");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["key"], "model-x");
    assert_eq!(groups[0]["n"], 2);
    assert!(groups[0]["rate"].is_null());

    assert_eq!(value["recent_verified"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn test_stats_ladder_rejects_a_period_that_is_not_a_window() {
    let storage = Arc::new(Storage::open_in_memory().unwrap());
    let server = AgentWorthMcpServer::new(storage);

    let err = server
        .stats_ladder(Parameters(
            serde_json::from_value(serde_json::json!({ "period": "fortnight" })).unwrap(),
        ))
        .await
        .expect_err("an unknown period is an error, not a silent default");
    assert!(err.to_string().contains("period must be one of"));
}

#[test]
fn test_stats_ladder_group_by_accepts_effort_and_rejects_anything_else() {
    for value in ["model", "repo", "adapter", "effort"] {
        let params: Result<super::params::LadderParams, _> =
            serde_json::from_value(serde_json::json!({ "by": value }));
        assert!(params.is_ok(), "by={value} must deserialize");
    }
    let params: Result<super::params::LadderParams, _> =
        serde_json::from_value(serde_json::json!({ "by": "worktree" }));
    assert!(params.is_err(), "by must reject anything off the four axes");
}
