use std::sync::Arc;

use agentworth_schema::{AgentWorthTrace, Provenance, TokenUsage};
use agentworth_storage::Storage;
use chrono::Utc;
use rmcp::handler::server::wrapper::Parameters;
use serde_json::Value;

use super::params::{OutcomeRateParams, SessionGetParams, SessionsFindParams};
use super::server::AgentWorthMcpServer;

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
        .sessions_find(Parameters(empty_sessions_find_params(0)))
        .await
        .expect_err("limit=0 must be rejected");
    assert!(err.message.contains("between 1 and 200"));
}

#[tokio::test]
async fn test_sessions_find_rejects_limit_over_ceiling() {
    let storage = Arc::new(Storage::open_in_memory().unwrap());
    let server = AgentWorthMcpServer::new(storage);

    let err = server
        .sessions_find(Parameters(empty_sessions_find_params(201)))
        .await
        .expect_err("limit=201 must be rejected -- the hard ceiling is 200");
    assert!(err.message.contains("between 1 and 200"));
}

#[tokio::test]
async fn test_sessions_find_accepts_limit_at_ceiling() {
    let storage = Arc::new(Storage::open_in_memory().unwrap());
    let server = AgentWorthMcpServer::new(storage);

    let result = server
        .sessions_find(Parameters(empty_sessions_find_params(200)))
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
        .sessions_find(Parameters(empty_sessions_find_params(10)))
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
    // docs/specs/mcp-server.md's sessions_find section.
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

    let result = server.sessions_find(Parameters(params)).await.unwrap();
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

    let result = server.sessions_find(Parameters(params)).await.unwrap();
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
        .session_get(Parameters(SessionGetParams {
            session_id: "does-not-exist".to_string(),
            include_raw: false,
        }))
        .await
        .expect_err("an unknown session_id must fail");
    assert!(err.message.contains("does-not-exist"));
}

#[tokio::test]
async fn test_coverage_stats_include_matrix_toggle() {
    let storage = Arc::new(Storage::open_in_memory().unwrap());
    let server = AgentWorthMcpServer::new(storage);

    let without_matrix = server
        .coverage_stats(Parameters(super::params::CoverageStatsParams {
            include_matrix: false,
        }))
        .await
        .unwrap();
    let value = call_result_json(without_matrix);
    assert!(value.get("matrix").is_none());

    let with_matrix = server
        .coverage_stats(Parameters(super::params::CoverageStatsParams {
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
        .upsert_session(&trace, primary_outcome, None)
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
        .outcome_rate(Parameters(
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
        .outcome_rate(Parameters(
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
        .outcome_rate(Parameters(
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
