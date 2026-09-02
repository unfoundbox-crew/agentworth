use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use agentworth_cli::server::{
    create_router, embedded_dashboard_is_built, AppState, LiveTailChangeKind, LiveTailEvent,
    LIVE_TAIL_CHANNEL_CAPACITY,
};
use agentworth_core::Scanner;
use agentworth_schema::{
    AgentWorthTrace, EventPayload, FileActionType, NormalizedEvent, Provenance, TokenUsage,
};
use agentworth_storage::Storage;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{Duration, Utc};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::broadcast;
use tower::ServiceExt;

/// Helper function to create an in-memory test AppState and router.
fn setup_test_app(dist_dir: Option<PathBuf>) -> (axum::Router, Arc<Storage>, Arc<Scanner>) {
    let (app, storage, scanner, _live_tail_tx) = setup_test_app_with_live_tail(dist_dir);
    (app, storage, scanner)
}

/// Like `setup_test_app`, but also hands back the live-tail broadcast sender so a test can
/// publish filesystem-change events and assert they reach a subscribed SSE client.
fn setup_test_app_with_live_tail(
    dist_dir: Option<PathBuf>,
) -> (
    axum::Router,
    Arc<Storage>,
    Arc<Scanner>,
    broadcast::Sender<LiveTailEvent>,
) {
    let storage = Arc::new(Storage::open_in_memory().expect("open in-memory storage"));
    let scanner = Arc::new(Scanner::new(storage.clone()));
    let (live_tail_tx, _rx) = broadcast::channel(LIVE_TAIL_CHANNEL_CAPACITY);
    let state = AppState {
        storage: storage.clone(),
        scanner: scanner.clone(),
        dist_dir,
        live_tail: live_tail_tx.clone(),
    };
    let app = create_router(state);
    (app, storage, scanner, live_tail_tx)
}

/// Helper to execute an HTTP request on an Axum router and return status + JSON Value.
async fn request_json(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let req_builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");

    let req = if let Some(json_body) = body {
        req_builder
            .body(Body::from(serde_json::to_vec(&json_body).unwrap()))
            .unwrap()
    } else {
        req_builder.body(Body::empty()).unwrap()
    };

    let response = app.oneshot(req).await.expect("execute request");
    let status = response.status();
    let body_bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let json_val: Value = serde_json::from_slice(&body_bytes).unwrap_or(Value::Null);
    (status, json_val)
}

/// Helper to execute an HTTP request and return status + raw response body as String.
async fn request_raw(app: axum::Router, method: &str, uri: &str) -> (StatusCode, String) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(req).await.expect("execute request");
    let status = response.status();
    let body_bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    let string_val = String::from_utf8_lossy(&body_bytes).to_string();
    (status, string_val)
}

#[tokio::test]
async fn test_api_get_stats_empty_and_populated() {
    let (app, storage, _) = setup_test_app(None);

    // 1. Initial empty stats
    let (status, stats) = request_json(app.clone(), "GET", "/api/stats", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stats["total_sessions"], 0);
    assert_eq!(stats["total_events"], 0);
    assert_eq!(stats["token_usage"]["total_tokens"], 0);

    // 2. Populate storage with two traces
    let now = Utc::now();
    for i in 1..=2 {
        let prov = Provenance::new(
            format!("/Users/dev/code/org/repo1/.claude/session_{}.jsonl", i),
            "claude_code",
            500,
            1700000000,
            format!("fp_{}", i),
        );
        let mut trace = AgentWorthTrace::new(
            format!("session_{}", i),
            "claude_code",
            prov,
            now + Duration::hours(i as i64),
        );
        trace.stats.token_usage = TokenUsage::new(1000 * i as u64, 500 * i as u64, 200, 50);
        trace.stats.total_events = i * 5;
        trace.stats.models_used = vec!["claude-3-5-sonnet".to_string()];
        trace.stats.tools_used.insert("Bash".to_string(), 3);
        storage.upsert_trace(&trace).expect("upsert trace");
    }

    // 3. Verify populated stats
    let (status, stats) = request_json(app, "GET", "/api/stats", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stats["total_sessions"], 2);
    assert_eq!(stats["total_events"], 15);
    assert_eq!(stats["token_usage"]["input_tokens"], 3000);
    assert_eq!(stats["token_usage"]["output_tokens"], 1500);
    assert_eq!(stats["sessions_by_adapter"]["claude_code"], 2);
    assert_eq!(stats["models_usage_count"]["claude-3-5-sonnet"], 2);
    assert_eq!(stats["tools_usage_count"]["Bash"], 6);
    assert!(stats["verified_outcomes_count"].as_u64().is_some());
    assert!(stats["outcome_distribution"].is_object());
}

#[tokio::test]
async fn test_api_get_traces_filtering_search_and_pagination() {
    let (app, storage, _) = setup_test_app(None);
    let now = Utc::now();

    // Insert 4 sessions with diverse parameters
    let session_specs = [
        ("sess_a", "claude_code", "claude-3-5-sonnet", 1000u64),
        ("sess_b", "claude_code", "claude-3-opus", 5000u64),
        ("sess_c", "codex", "gpt-4o", 12000u64),
        ("sess_d", "gemini", "gemini-2.5-flash", 2500u64),
    ];

    for (i, &(id, adapter, model, tokens)) in session_specs.iter().enumerate() {
        let prov = Provenance::new(
            format!("/Users/dev/code/org/project_{}/log.jsonl", id),
            adapter,
            1024,
            1700000000,
            format!("fp_{}", id),
        );
        let mut trace = AgentWorthTrace::new(
            id.to_string(),
            adapter,
            prov,
            now + Duration::minutes(i as i64 * 10),
        );
        trace.stats.token_usage = TokenUsage::new(tokens, 0, 0, 0);
        trace.stats.models_used = vec![model.to_string()];
        trace.stats.total_events = 10;
        storage.upsert_trace(&trace).expect("upsert trace");
    }

    // 1. Get all traces
    let (status, traces) = request_json(app.clone(), "GET", "/api/traces", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(traces.as_array().unwrap().len(), 4);

    // 2. Filter by adapter: claude_code
    let (status, traces) =
        request_json(app.clone(), "GET", "/api/traces?adapter=claude_code", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(traces.as_array().unwrap().len(), 2);

    // 3. Filter by model substring: opus
    let (status, traces) = request_json(app.clone(), "GET", "/api/traces?model=opus", None).await;
    assert_eq!(status, StatusCode::OK);
    let arr = traces.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["session_id"], "sess_b");

    // 4. Search query: project_sess_c
    let (status, traces) = request_json(
        app.clone(),
        "GET",
        "/api/traces?search=project_sess_c",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let arr = traces.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["session_id"], "sess_c");

    // 5. Min tokens: 4000
    let (status, traces) =
        request_json(app.clone(), "GET", "/api/traces?min_tokens=4000", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(traces.as_array().unwrap().len(), 2); // sess_b (5000) and sess_c (12000)

    // 6. Ordering: tokens_desc
    let (status, traces) =
        request_json(app.clone(), "GET", "/api/traces?order_by=tokens_desc", None).await;
    assert_eq!(status, StatusCode::OK);
    let arr = traces.as_array().unwrap();
    assert_eq!(arr[0]["session_id"], "sess_c");
    assert_eq!(arr[3]["session_id"], "sess_a");

    // 7. Pagination: limit=2, offset=2
    let (status, traces) =
        request_json(app.clone(), "GET", "/api/traces?limit=2&offset=2", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(traces.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_api_get_trace_by_id_and_not_found() {
    let (app, storage, _) = setup_test_app(None);

    // Create a real temporary Claude Code JSONL file on disk
    let mut temp_file = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .unwrap();
    let sample_claude_jsonl = r#"
{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"Fix the database connection retry loop"}
{"type":"assistant","timestamp":"2026-08-29T10:00:05Z","model":"claude-3-5-sonnet","usage":{"input_tokens":500,"output_tokens":150,"cache_read_input_tokens":50,"cache_creation_input_tokens":10},"content":[{"type":"text","text":"I will inspect the connection logic."},{"type":"tool_use","id":"call_2","name":"Bash","input":{"command":"cargo test --lib"}}]}
{"type":"tool_result","timestamp":"2026-08-29T10:00:20Z","tool_use_id":"call_2","content":"test result: ok. 8 passed; 0 failed","is_error":false}
"#;
    temp_file.write_all(sample_claude_jsonl.as_bytes()).unwrap();

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
        "sha256:mock_hash",
    );
    let trace = AgentWorthTrace::new(session_id.clone(), "claude_code", prov, Utc::now());
    storage.upsert_trace(&trace).expect("upsert trace");

    // 1. Query existing trace details
    let uri = format!("/api/traces/{}", session_id);
    let (status, detail) = request_json(app.clone(), "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["trace"]["session_id"], session_id);
    assert_eq!(detail["trace"]["adapter"], "claude_code");
    assert!(detail["score"]["composite_score"].as_f64().unwrap() > 0.0);
    assert!(detail["score"]["outcome_score"].as_f64().unwrap() >= 0.50);
    assert!(!detail["outcomes"].as_array().unwrap().is_empty());
    let events = detail["trace"]["events"].as_array().unwrap();
    assert!(!events.is_empty());
    // No offset/limit given -- this must be exactly today's behavior: the full event list,
    // with events_total/events_offset describing that (not silently truncating).
    assert_eq!(detail["events_total"], events.len() as u64);
    assert_eq!(detail["events_offset"], 0);

    // 2. Query non-existent trace -> 404 Not Found
    let (status, err) = request_json(app, "GET", "/api/traces/unknown_session_999", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(err["error"].as_str().unwrap().contains("not found"));
}

/// Builds a temp Claude Code `.jsonl` file with `n` minimal single-line user-message events
/// and seeds a matching session row, returning `(session_id, temp_file)`. The temp file must
/// stay alive for `Scanner::load_trace` to find it on disk.
fn seed_trace_with_n_events(storage: &Storage, n: usize) -> (String, TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("evt_session.jsonl");
    let contents: String = (0..n)
        .map(|i| {
            format!(
                r#"{{"type":"user","timestamp":"2026-08-29T10:{:02}:{:02}Z","content":"event {i}"}}"#,
                (i / 60) % 60,
                i % 60
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&file_path, contents).unwrap();

    let session_id = file_path
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let prov = Provenance::new(
        file_path.to_string_lossy().to_string(),
        "claude_code",
        1024,
        1700000000,
        "sha256:mock_hash_evt",
    );
    let trace = AgentWorthTrace::new(session_id.clone(), "claude_code", prov, Utc::now());
    storage.upsert_trace(&trace).expect("upsert trace");

    (session_id, dir, file_path)
}

#[tokio::test]
async fn test_api_get_trace_by_id_events_pagination_boundaries() {
    let (app, storage, _) = setup_test_app(None);
    let (session_id, _dir, _path) = seed_trace_with_n_events(&storage, 10);

    // offset + limit slices trace.events, and echoes back events_total/events_offset.
    let uri = format!("/api/traces/{session_id}?offset=3&limit=4");
    let (status, detail) = request_json(app.clone(), "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    let events = detail["trace"]["events"].as_array().unwrap();
    assert_eq!(events.len(), 4);
    assert_eq!(detail["events_total"], 10);
    assert_eq!(detail["events_offset"], 3);

    // offset past the end of the trace returns an empty page, not an error.
    let uri = format!("/api/traces/{session_id}?offset=100&limit=5");
    let (status, detail) = request_json(app.clone(), "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(detail["trace"]["events"].as_array().unwrap().is_empty());
    assert_eq!(detail["events_total"], 10);
    assert_eq!(detail["events_offset"], 100);

    // limit=0 is rejected outright rather than silently returning zero events.
    let uri = format!("/api/traces/{session_id}?limit=0");
    let (status, err) = request_json(app.clone(), "GET", &uri, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err["error"].as_str().unwrap().contains("limit"));

    // No params at all keeps today's behavior: every event, offset 0.
    let uri = format!("/api/traces/{session_id}");
    let (status, detail) = request_json(app, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["trace"]["events"].as_array().unwrap().len(), 10);
    assert_eq!(detail["events_total"], 10);
    assert_eq!(detail["events_offset"], 0);
}

#[tokio::test]
async fn test_api_get_trace_events_endpoint_returns_just_the_slice() {
    let (app, storage, _) = setup_test_app(None);
    let (session_id, _dir, _path) = seed_trace_with_n_events(&storage, 10);

    let uri = format!("/api/traces/{session_id}/events?offset=2&limit=3");
    let (status, page) = request_json(app.clone(), "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["events"].as_array().unwrap().len(), 3);
    assert_eq!(page["events_total"], 10);
    assert_eq!(page["events_offset"], 2);
    // Only the slice -- no trace/score/outcomes on this endpoint's shape.
    assert!(page.get("trace").is_none());
    assert!(page.get("score").is_none());

    // 404 for an unknown session, same as /api/traces/:id.
    let (status, err) = request_json(app, "GET", "/api/traces/unknown_999/events", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(err["error"].as_str().unwrap().contains("not found"));
}

#[tokio::test]
async fn test_api_response_compression_negotiated_via_accept_encoding() {
    let (app, storage, _) = setup_test_app(None);
    let (session_id, _dir, _path) = seed_trace_with_n_events(&storage, 200);

    let uri = format!("/api/traces/{session_id}");
    let req = Request::builder()
        .method("GET")
        .uri(&uri)
        .header("accept-encoding", "gzip")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(req).await.expect("execute request");
    assert_eq!(response.status(), StatusCode::OK);
    let content_encoding = response
        .headers()
        .get("content-encoding")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    assert_eq!(
        content_encoding.as_deref(),
        Some("gzip"),
        "a large JSON response with Accept-Encoding: gzip must come back gzip-compressed"
    );

    // No Accept-Encoding header -> no content-encoding on the response.
    let req = Request::builder()
        .method("GET")
        .uri(&uri)
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(req).await.expect("execute request");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("content-encoding").is_none());
}

#[tokio::test]
async fn test_api_get_archaeology_highlights() {
    let (app, storage, _) = setup_test_app(None);

    // Create 3 temporary trace files on disk representing archaeological discoveries:
    // Trace 1: Expensive unsolved task (18.3M tokens, unverified)
    let mut temp1 = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .unwrap();
    let content1 = r#"{"type":"user","timestamp":"2026-08-10T10:00:00Z","content":"Center this div perfectly and fix alignment"}
{"type":"assistant","timestamp":"2026-08-10T10:05:00Z","model":"claude-3-opus","usage":{"input_tokens":18000000,"output_tokens":300000},"content":[{"type":"text","text":"Attempting flexbox..."},{"type":"tool_use","id":"c1","name":"Bash","input":{"command":"npm test"}}]}
{"type":"tool_result","timestamp":"2026-08-10T10:11:00Z","tool_use_id":"c1","is_error":true,"content":"FAIL: tests failed with error"}
"#;
    temp1.write_all(content1.as_bytes()).unwrap();
    temp1.flush().unwrap();
    let sess1_id = temp1
        .path()
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let prov1 = Provenance::new(
        temp1.path().to_string_lossy().to_string(),
        "claude_code",
        2048,
        1700000000,
        "fp1",
    );
    let mut trace1 = AgentWorthTrace::new(
        sess1_id.clone(),
        "claude_code",
        prov1,
        Utc::now() - Duration::days(15),
    );
    trace1.stats.total_events = 3;
    trace1.stats.token_usage = TokenUsage::new(18000000, 300000, 0, 0);
    trace1.stats.models_used = vec!["claude-3-opus".to_string()];
    storage.upsert_trace(&trace1).unwrap();

    // Trace 2: Autonomous Recovery Loop (Failure -> Fix -> Pass)
    let mut temp2 = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .unwrap();
    let start2 = Utc::now() - Duration::days(5);
    let prov2 = Provenance::new(
        temp2.path().to_string_lossy().to_string(),
        "claude_code",
        1024,
        1700000000,
        "fp2",
    );
    let sess2_id = temp2
        .path()
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let mut trace2 = AgentWorthTrace::new(sess2_id.clone(), "claude_code", prov2, start2);
    trace2.stats.total_events = 7;
    trace2.stats.token_usage = TokenUsage::new(1000, 200, 0, 0);
    let sample_claude_rec = format!(
        r#"{{"type":"user","timestamp":"{}","content":"Run tests"}}
{{"type":"assistant","timestamp":"{}","model":"claude-3-5-sonnet","usage":{{"input_tokens":1000,"output_tokens":200}},"content":[{{"type":"tool_use","id":"c2","name":"Bash","input":{{"command":"cargo test"}}}}]}}
{{"type":"tool_result","timestamp":"{}","tool_use_id":"c2","is_error":true,"content":"test result: FAILED. 1 failed"}}
{{"type":"assistant","timestamp":"{}","model":"claude-3-5-sonnet","content":[{{"type":"tool_use","id":"c3","name":"replace_file_content","input":{{"path":"src/logic.rs"}}}}]}}
{{"type":"tool_result","timestamp":"{}","tool_use_id":"c3","content":"Saved"}}
{{"type":"assistant","timestamp":"{}","model":"claude-3-5-sonnet","content":[{{"type":"tool_use","id":"c4","name":"Bash","input":{{"command":"cargo test"}}}}]}}
{{"type":"tool_result","timestamp":"{}","tool_use_id":"c4","content":"test result: ok. 5 passed; 0 failed"}}
"#,
        start2.to_rfc3339(),
        (start2 + Duration::seconds(2)).to_rfc3339(),
        (start2 + Duration::seconds(4)).to_rfc3339(),
        (start2 + Duration::seconds(6)).to_rfc3339(),
        (start2 + Duration::seconds(8)).to_rfc3339(),
        (start2 + Duration::seconds(10)).to_rfc3339(),
        (start2 + Duration::seconds(12)).to_rfc3339(),
    );
    temp2.write_all(sample_claude_rec.as_bytes()).unwrap();
    temp2.flush().unwrap();
    storage.upsert_trace(&trace2).unwrap();

    // Trace 3: Model switches (Claude 3 Opus -> Sonnet -> Haiku)
    let mut temp3 = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .unwrap();
    let start3 = Utc::now() - Duration::days(1);
    let sample_switches = format!(
        r#"{{"type":"user","timestamp":"{}","content":"Multi-model task"}}
{{"type":"assistant","timestamp":"{}","model":"claude-3-opus","usage":{{"input_tokens":500,"output_tokens":100}},"content":[{{"type":"text","text":"Planning..."}}]}}
{{"type":"assistant","timestamp":"{}","model":"claude-3-5-sonnet","usage":{{"input_tokens":500,"output_tokens":100}},"content":[{{"type":"text","text":"Executing..."}}]}}
{{"type":"assistant","timestamp":"{}","model":"claude-3-haiku","usage":{{"input_tokens":500,"output_tokens":100}},"content":[{{"type":"text","text":"Summarizing..."}}]}}
"#,
        start3.to_rfc3339(),
        (start3 + Duration::seconds(2)).to_rfc3339(),
        (start3 + Duration::seconds(4)).to_rfc3339(),
        (start3 + Duration::seconds(6)).to_rfc3339(),
    );
    temp3.write_all(sample_switches.as_bytes()).unwrap();
    temp3.flush().unwrap();
    let prov3 = Provenance::new(
        temp3.path().to_string_lossy().to_string(),
        "claude_code",
        1024,
        1700000000,
        "fp3",
    );
    let mut trace3 = AgentWorthTrace::new(
        temp3
            .path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string(),
        "claude_code",
        prov3,
        start3,
    );
    trace3.stats.total_events = 4;
    trace3.stats.models_used = vec![
        "claude-3-opus".to_string(),
        "claude-3-5-sonnet".to_string(),
        "claude-3-haiku".to_string(),
    ];
    trace3.stats.token_usage = TokenUsage::new(1500, 300, 0, 0);
    storage.upsert_trace(&trace3).unwrap();

    // Call GET /api/archaeology
    let (status, arch) = request_json(app, "GET", "/api/archaeology", None).await;
    assert_eq!(status, StatusCode::OK);

    // Verify Most Expensive Unsolved
    let unsolved = &arch["most_expensive_unsolved"];
    assert!(unsolved.is_object());
    assert_eq!(unsolved["session_id"], sess1_id);
    assert_eq!(unsolved["total_tokens"], 18300000);
    assert!(unsolved["prompt"]
        .as_str()
        .unwrap()
        .contains("Center this div"));

    // Verify Longest Recovery Loop
    let recovery = &arch["longest_recovery_loop"];
    assert!(recovery.is_object());
    assert!(recovery["steps_to_recover"].as_u64().unwrap() >= 1);

    // Verify Model Switches
    let switches = &arch["most_frequent_model_switches"];
    assert!(switches.is_object());
    assert_eq!(switches["switch_count"], 2);
    assert_eq!(switches["unique_models"].as_array().unwrap().len(), 3);

    // Verify Token Carbon Dating
    let carbon = &arch["token_carbon_dating"];
    assert!(carbon["total_tokens"].as_u64().unwrap() > 0);
    assert!(carbon["total_days_active"].as_u64().unwrap() >= 1);
    assert!(!carbon["timeline"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_api_post_scan_endpoint() {
    let (app, storage, _) = setup_test_app(None);

    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("session_1.jsonl");
    let sample = r#"
{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"Hello world"}
{"type":"assistant","timestamp":"2026-08-29T10:00:05Z","model":"claude-3-5-sonnet","usage":{"input_tokens":120,"output_tokens":30,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"Hi!"}]}
"#;
    std::fs::write(&file_path, sample).unwrap();

    let scan_payload = json!({
        "paths": [temp_dir.path().to_string_lossy().to_string()],
        "force": true
    });

    let (status, summary) = request_json(app, "POST", "/api/scan", Some(scan_payload)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(summary["discovered_sources"].as_u64().unwrap() >= 1);
    assert!(summary["scanned_sessions"].as_u64().unwrap() >= 1);
    assert!(summary["total_indexed_sessions"].as_u64().unwrap() >= 1);

    // Verify indexed in storage
    let stats = storage.get_aggregate_stats(false).unwrap();
    assert_eq!(stats.total_sessions, 1);
}

#[tokio::test]
async fn test_api_post_export_json_and_atif_and_redact() {
    let (app, storage, _) = setup_test_app(None);

    let mut temp_file = tempfile::Builder::new()
        .suffix(".jsonl")
        .tempfile()
        .unwrap();
    let sample_with_secret = r#"
{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"Here is my API key sk-ant-api03-abcdef1234567890abcdef1234567890"}
{"type":"assistant","timestamp":"2026-08-29T10:00:05Z","model":"claude-3-5-sonnet","usage":{"input_tokens":200,"output_tokens":50,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"Saved to /Users/saurabh/secret.pem and sent to admin@company.com"}]}
"#;
    temp_file.write_all(sample_with_secret.as_bytes()).unwrap();

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
        "fp_secret",
    );
    let trace = AgentWorthTrace::new(session_id.clone(), "claude_code", prov, Utc::now());
    storage.upsert_trace(&trace).unwrap();

    // 1. Export unredacted JSON
    let uri = format!("/api/export/{}", session_id);
    let (status, res) = request_json(
        app.clone(),
        "POST",
        &uri,
        Some(json!({ "format": "json", "redact": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(res["format"], "json");
    assert!(res["content"]
        .as_str()
        .unwrap()
        .contains("sk-ant-api03-abcdef1234567890abcdef1234567890"));

    // 2. Export REDACTED JSON
    let (status, res) = request_json(
        app.clone(),
        "POST",
        &uri,
        Some(json!({ "format": "json", "redact": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let content = res["content"].as_str().unwrap();
    assert!(!content.contains("sk-ant-api03-abcdef1234567890abcdef1234567890"));
    assert!(content.contains("[REDACTED_API_KEY]"));
    assert!(content.contains("[REDACTED_EMAIL]"));

    // 3. Export ATIF format
    let (status, res) = request_json(
        app,
        "POST",
        &uri,
        Some(json!({ "format": "atif", "redact": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(res["format"], "atif");
    let atif_str = res["content"].as_str().unwrap();
    assert!(atif_str.contains("\"schema_version\": \"atif-v1.0\""));
}

#[tokio::test]
async fn test_api_static_file_serving_and_spa_fallback() {
    // 1. Default embedded fallback: rust_embed pulls apps/dashboard/dist at
    // compile time (see AGENTS.md item 5), so this proves the real built
    // React shell is inside the binary — not the hand-written FALLBACK_HTML
    // stub, which has neither a <title>AgentWorth</title> nor a hashed
    // /assets/ bundle. Checking for "AGENTWORTH" / "Your agents left
    // receipts" text would pass even against the stub, and would also fail
    // against the real dashboard since that copy is rendered client-side by
    // React, not present in the served HTML shell.
    //
    // On a fresh clone nothing is embedded at all (apps/dashboard/dist does not exist until
    // `npm run build` has run), and every route serves the hand-written FALLBACK_HTML stub
    // instead. That is a build state, not a regression, so this half skips loudly rather
    // than failing. CI builds the dashboard before the Rust suite, so there it always runs.
    if embedded_dashboard_is_built() {
        let (app, _, _) = setup_test_app(None);

        // Root GET /
        let (status, html) = request_raw(app.clone(), "GET", "/").await;
        assert_eq!(status, StatusCode::OK);
        assert!(html.contains("<title>AgentWorth</title>"));
        assert!(
            html.contains("/assets/"),
            "expected a hashed Vite bundle reference, got: {html}"
        );

        // SPA client-side route GET /traces/sess_123
        let (status, html) = request_raw(app, "GET", "/traces/sess_123").await;
        assert_eq!(status, StatusCode::OK);
        assert!(html.contains("<title>AgentWorth</title>"));
    } else {
        println!(
            "SKIP: the embedded dashboard is the stub -- apps/dashboard/dist was absent at \
             compile time. Run `npm --prefix apps/dashboard run build` and rebuild to cover \
             the embedded-asset half of this test. The custom-dist half below still runs."
        );
    }

    // 2. Custom dist_dir serving
    let temp_dist = TempDir::new().unwrap();
    let index_file = temp_dist.path().join("index.html");
    std::fs::write(&index_file, "<html><body>Custom Dist Web UI</body></html>").unwrap();

    let asset_file = temp_dist.path().join("app.js");
    std::fs::write(&asset_file, "console.log('custom js');").unwrap();

    let (app_dist, _, _) = setup_test_app(Some(temp_dist.path().to_path_buf()));

    // Exact asset file GET /app.js
    let (status, js) = request_raw(app_dist.clone(), "GET", "/app.js").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(js, "console.log('custom js');");

    // SPA fallback route GET /sessions
    let (status, custom_html) = request_raw(app_dist, "GET", "/sessions").await;
    assert_eq!(status, StatusCode::OK);
    assert!(custom_html.contains("Custom Dist Web UI"));
}

#[tokio::test]
async fn test_api_get_usage_endpoint() {
    let (app, storage, _) = setup_test_app(None);
    let now = Utc::now();

    // Populate with 2 sessions across 2 days
    for day in 0..2 {
        let prov = Provenance::new(
            format!("/Users/dev/code/project/session_{}.jsonl", day),
            "claude_code",
            1024,
            1700000000,
            format!("fp_usage_{}", day),
        );
        let mut trace = AgentWorthTrace::new(
            format!("sess_usage_{}", day),
            "claude_code",
            prov,
            now - Duration::days(day as i64),
        );
        trace.stats.token_usage = TokenUsage::new(2000, 500, 10000, 1000);
        trace.stats.total_events = 20;
        trace.stats.duration_seconds = Some(180.0);
        storage.upsert_trace(&trace).expect("upsert trace");
    }

    let (status, usage) = request_json(app, "GET", "/api/usage", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(usage["daily"].is_array());
    assert!(usage["weekly"].is_array());
    assert!(usage["monthly"].is_array());

    let daily = usage["daily"].as_array().unwrap();
    assert_eq!(daily.len(), 2);
    assert_eq!(daily[0]["adapter"], "claude_code");
    assert_eq!(daily[0]["input_tokens"], 2000);
    assert_eq!(daily[0]["cache_read_tokens"], 10000);
    assert!(daily[0]["estimated_cost_usd"].as_f64().unwrap() > 0.0);
    assert!(daily[0]["cache_hit_ratio"].as_f64().unwrap() > 0.0);
}

#[tokio::test]
async fn test_api_get_pacing_endpoint() {
    let (app, storage, _) = setup_test_app(None);
    let now = Utc::now();

    // Session 1 hour ago (within 5h window)
    let prov1 = Provenance::new(
        "/Users/dev/code/proj/s1.jsonl",
        "claude_code",
        512,
        1700000000,
        "fp_pacing_1",
    );
    let mut trace1 = AgentWorthTrace::new(
        "sess_pacing_1",
        "claude_code",
        prov1,
        now - Duration::hours(1),
    );
    trace1.stats.total_events = 10;
    trace1.stats.token_usage = TokenUsage::new(5000, 1000, 20000, 500);
    trace1.stats.models_used = vec!["claude-3-5-sonnet".to_string()];
    storage.upsert_trace(&trace1).expect("upsert trace1");

    // Session 10 hours ago (outside 5h window)
    let prov2 = Provenance::new(
        "/Users/dev/code/proj/s2.jsonl",
        "codex",
        512,
        1700000000,
        "fp_pacing_2",
    );
    let mut trace2 = AgentWorthTrace::new(
        "sess_pacing_2",
        "codex",
        prov2,
        now - Duration::hours(10),
    );
    trace2.stats.total_events = 10;
    trace2.stats.token_usage = TokenUsage::new(8000, 2000, 0, 0);
    trace2.stats.models_used = vec!["gpt-4o".to_string()];
    storage.upsert_trace(&trace2).expect("upsert trace2");

    let (status, pacing) = request_json(app, "GET", "/api/pacing?hours=5", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pacing["window_hours"], 5);
    assert_eq!(pacing["session_count"], 1);
    assert_eq!(pacing["total_tokens"], 26500);
    assert!(pacing["burn_rate_tokens_per_hour"].as_f64().unwrap() > 0.0);
    assert!(pacing["cache_hit_ratio"].as_f64().unwrap() > 0.0);
    assert_eq!(pacing["active_adapters"].as_array().unwrap(), &["claude_code"]);
    assert_eq!(
        pacing["active_models"].as_array().unwrap(),
        &["claude-3-5-sonnet"]
    );
}

#[tokio::test]
async fn test_api_get_blame_endpoint() {
    let (app, storage, _) = setup_test_app(None);

    let prov = Provenance::new(
        "/Users/dev/.claude/projects/-Users-dev-code-engine/sess-blame-abc.jsonl",
        "claude_code",
        1024,
        1700000000,
        "fp_blame_test",
    );
    let mut trace = AgentWorthTrace::new("sess_blame_abc", "claude_code", prov, Utc::now());
    trace.stats.models_used = vec!["claude-3-5-sonnet".to_string()];
    trace.stats.tools_used.insert("replace_file_content".to_string(), 4);
    trace.stats.token_usage = TokenUsage::new(4000, 1000, 0, 0);
    trace.events.push(NormalizedEvent::new(
        1,
        Utc::now(),
        EventPayload::FileAction {
            path: "src/pipeline.rs".to_string(),
            action: FileActionType::Edit,
            diff: None,
            lines_changed: None,
        },
    ));
    storage.upsert_trace(&trace).expect("upsert blame trace");

    let (status, matches) = request_json(app, "GET", "/api/blame?file=pipeline.rs", None).await;
    assert_eq!(status, StatusCode::OK);
    let arr = matches.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["session_id"], "sess_blame_abc");
    assert_eq!(arr[0]["adapter"], "claude_code");
    assert_eq!(arr[0]["total_tokens"], 5000);
    assert_eq!(arr[0]["file_path"], "src/pipeline.rs");
    assert_eq!(arr[0]["action"], "edit");
}

#[tokio::test]
async fn test_api_get_matrix_endpoint() {
    let (app, _, _) = setup_test_app(None);

    let (status, matrix) = request_json(app, "GET", "/api/matrix", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(matrix["total_adapters"], 20);
    assert!(matrix["detected_adapters"].as_u64().is_some());

    let adapters = matrix["adapters"].as_array().unwrap();
    assert_eq!(adapters.len(), 20);

    // Verify Claude Code capability coverage
    let claude = adapters
        .iter()
        .find(|a| a["adapter"] == "claude_code")
        .expect("claude_code adapter in matrix");
    assert_eq!(claude["name"], "Claude Code");
    assert_eq!(claude["token_accounting"], true);
    assert_eq!(claude["cache_breakdown"], true);
    assert_eq!(claude["tool_calls"], true);
    assert_eq!(claude["file_actions"], true);
    assert_eq!(claude["shell_commands"], true);
    assert_eq!(claude["model_switches"], true);
    assert_eq!(claude["thinking_blocks"], true);
    assert_eq!(claude["error_recovery"], true);
}

#[tokio::test]
async fn test_api_live_tail_sse_stream_delivers_broadcast_event() {
    let (app, _, _, live_tail_tx) = setup_test_app_with_live_tail(None);

    let req = Request::builder()
        .method("GET")
        .uri("/api/live-tail")
        .body(Body::empty())
        .unwrap();

    // The handler subscribes to the broadcast channel synchronously before it ever awaits,
    // so by the time `oneshot` hands back this response the subscription already exists —
    // sending now is safe, not a race against the handler's setup.
    let response = app.oneshot(req).await.expect("execute live-tail request");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/event-stream"
    );

    let sample_event = LiveTailEvent {
        path: PathBuf::from("/Users/dev/.claude/projects/sess-live.jsonl"),
        kind: LiveTailChangeKind::Modified,
        adapter: Some("claude_code".to_string()),
        timestamp: Utc::now(),
    };
    live_tail_tx
        .send(sample_event)
        .expect("broadcast live-tail event to subscriber");

    let mut body = response.into_body();
    let frame = tokio::time::timeout(std::time::Duration::from_secs(5), body.frame())
        .await
        .expect("timed out waiting for an SSE frame")
        .expect("stream ended before yielding a frame")
        .expect("frame error");
    let bytes = frame.into_data().expect("expected a data frame");
    let text = String::from_utf8_lossy(&bytes);

    assert!(text.contains("event: change"), "frame was: {}", text);
    assert!(text.contains("sess-live.jsonl"), "frame was: {}", text);
    assert!(text.contains("\"kind\":\"modified\""), "frame was: {}", text);
    assert!(
        text.contains("\"adapter\":\"claude_code\""),
        "frame was: {}",
        text
    );
}

#[tokio::test]
async fn test_api_live_tail_sse_reports_lag_without_closing_stream() {
    let (app, _, _, live_tail_tx) = setup_test_app_with_live_tail(None);

    let req = Request::builder()
        .method("GET")
        .uri("/api/live-tail")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(req).await.expect("execute live-tail request");

    // Flood past the channel capacity so the subscriber's next recv reports `Lagged`
    // instead of quietly losing events.
    for i in 0..(LIVE_TAIL_CHANNEL_CAPACITY + 10) {
        let _ = live_tail_tx.send(LiveTailEvent {
            path: PathBuf::from(format!("/Users/dev/.claude/projects/sess-{}.jsonl", i)),
            kind: LiveTailChangeKind::Created,
            adapter: Some("claude_code".to_string()),
            timestamp: Utc::now(),
        });
    }

    let mut body = response.into_body();
    let frame = tokio::time::timeout(std::time::Duration::from_secs(5), body.frame())
        .await
        .expect("timed out waiting for an SSE frame")
        .expect("stream ended before yielding a frame")
        .expect("frame error");
    let bytes = frame.into_data().expect("expected a data frame");
    let text = String::from_utf8_lossy(&bytes);

    assert!(text.contains("event: lagged"), "frame was: {}", text);
}

/// Regression test for `agentworth serve --dist <path>` silently falling back to the dashboard
/// embedded in the binary instead of serving from the given directory. `resolve_dist_dir` (used
/// by `main.rs`'s `Serve` command) is what enforces this now; this test exercises the router's
/// serving path directly by writing a marker string into a temp dist dir's `index.html` and
/// asserting the marker -- not anything from the embedded fallback -- comes back for both an
/// exact-path request and an SPA-fallback request.
#[tokio::test]
async fn test_serve_custom_dist_dir_marker_is_returned_not_embedded_fallback() {
    let temp_dist = TempDir::new().unwrap();
    let marker = "AGENTWORTH-CUSTOM-DIST-MARKER-4f1c9a";
    std::fs::write(
        temp_dist.path().join("index.html"),
        format!("<html><body>{marker}</body></html>"),
    )
    .unwrap();

    let (app, _, _) = setup_test_app(Some(temp_dist.path().to_path_buf()));

    let (status, root_html) = request_raw(app.clone(), "GET", "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        root_html.contains(marker),
        "expected the custom dist's index.html marker, got: {}",
        root_html
    );

    // SPA client-side route: still the custom dist's index.html, never the embedded fallback.
    let (status, spa_html) = request_raw(app, "GET", "/some/client/route").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        spa_html.contains(marker),
        "expected the custom dist's index.html marker on SPA fallback, got: {}",
        spa_html
    );
}

/// `resolve_dist_dir` must fail loudly when given a `--dist` path that doesn't exist, rather
/// than silently falling back to the embedded dashboard -- that silent fallback is exactly what
/// made `--dist` look "ignored" (evidence: a 200 response whose served index.html referenced
/// asset hashes absent from the directory the user pointed `--dist` at).
#[test]
fn test_resolve_dist_dir_rejects_nonexistent_path() {
    let missing = std::env::temp_dir().join("agentworth-test-dist-does-not-exist-4f1c9a");
    let err = agentworth_cli::server::resolve_dist_dir(Some(missing.clone()))
        .expect_err("a nonexistent --dist path must be a hard error, not a silent fallback");
    assert!(
        err.to_string().contains(&missing.display().to_string()),
        "error should name the offending path: {}",
        err
    );
}

/// A `--dist` path that exists but isn't a built web frontend (no `index.html`) must also fail
/// loudly rather than silently falling back to the embedded dashboard.
#[test]
fn test_resolve_dist_dir_rejects_directory_without_index_html() {
    let empty_dir = TempDir::new().unwrap();
    let err = agentworth_cli::server::resolve_dist_dir(Some(empty_dir.path().to_path_buf()))
        .expect_err("a --dist dir with no index.html must be a hard error");
    assert!(
        err.to_string().contains("index.html"),
        "error should mention the missing index.html: {}",
        err
    );
}

/// A valid `--dist` directory (exists, is a directory, has an `index.html`) must resolve
/// successfully to that same path.
#[test]
fn test_resolve_dist_dir_accepts_valid_directory() {
    let dist = TempDir::new().unwrap();
    std::fs::write(dist.path().join("index.html"), "<html></html>").unwrap();

    let resolved = agentworth_cli::server::resolve_dist_dir(Some(dist.path().to_path_buf()))
        .expect("a valid --dist directory should resolve");
    assert_eq!(resolved, Some(dist.path().to_path_buf()));
}

/// Regression test: `/api/traces` used to omit `primary_outcome` and `composite_score` from
/// every row via `#[serde(skip_serializing_if = "Option::is_none")]` on `SessionSummary`, which
/// on real (scored) sessions meant the keys should be present but on any never-scored session
/// made the fields disappear from the payload entirely rather than reading as `null`. Seeds one
/// scored and one unscored session and asserts both keys are always present, and that a real
/// outcome value comes back snake_case (matching `OutcomeKind`'s own serde encoding).
#[tokio::test]
async fn test_api_traces_includes_primary_outcome_and_composite_score() {
    let (app, storage, _) = setup_test_app(None);
    let now = Utc::now();

    let scored_prov = Provenance::new(
        "/Users/dev/code/org/scored/log.jsonl",
        "claude_code",
        1024,
        1700000000,
        "fp_scored",
    );
    let mut scored_trace = AgentWorthTrace::new("sess_scored", "claude_code", scored_prov, now);
    // Non-stub: list_sessions_filtered's default excludes total_events <= 1 or total_tokens <= 0
    // (NON_STUB_SQL_PREDICATE), and this test needs the session to survive that filter to prove
    // anything about /api/traces's payload.
    scored_trace.stats.total_events = 10;
    scored_trace.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
    storage
        .upsert_session(&scored_trace, Some("commit_observed"), Some(0.87), 1)
        .expect("upsert scored session");

    let unscored_prov = Provenance::new(
        "/Users/dev/code/org/unscored/log.jsonl",
        "claude_code",
        1024,
        1700000000,
        "fp_unscored",
    );
    let mut unscored_trace = AgentWorthTrace::new(
        "sess_unscored",
        "claude_code",
        unscored_prov,
        now + Duration::minutes(1),
    );
    unscored_trace.stats.total_events = 10;
    unscored_trace.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
    storage.upsert_trace(&unscored_trace).expect("upsert unscored trace");

    let (status, traces) = request_json(app, "GET", "/api/traces", None).await;
    assert_eq!(status, StatusCode::OK);
    let arr = traces.as_array().unwrap();
    assert_eq!(arr.len(), 2);

    let scored = arr
        .iter()
        .find(|t| t["session_id"] == "sess_scored")
        .expect("scored session present");
    assert!(
        scored.get("primary_outcome").is_some(),
        "primary_outcome key must be present: {}",
        scored
    );
    assert_eq!(scored["primary_outcome"], "commit_observed");
    assert!(
        scored.get("composite_score").is_some(),
        "composite_score key must be present: {}",
        scored
    );
    assert_eq!(scored["composite_score"], 0.87);

    let unscored = arr
        .iter()
        .find(|t| t["session_id"] == "sess_unscored")
        .expect("unscored session present");
    assert!(
        unscored.get("primary_outcome").is_some(),
        "primary_outcome key must be present even when null: {}",
        unscored
    );
    assert!(unscored["primary_outcome"].is_null());
    assert!(
        unscored.get("composite_score").is_some(),
        "composite_score key must be present even when null: {}",
        unscored
    );
    assert!(unscored["composite_score"].is_null());
}

