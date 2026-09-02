//! `GET /api/config` and `POST /api/config`.
//!
//! These live in their own integration binary, and in one test function, on purpose:
//! the routes resolve their file through the process-wide `AGENTWORTH_CONFIG_PATH`
//! override, so a second test running beside them could point the first one's write at
//! the developer's real `~/.agentworth/config.toml`.

use std::sync::Arc;

use agentworth_cli::server::{create_router, AppState, LIVE_TAIL_CHANNEL_CAPACITY};
use agentworth_core::Scanner;
use agentworth_storage::Storage;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tokio::sync::broadcast;
use tower::ServiceExt;

fn test_app() -> axum::Router {
    let storage = Arc::new(Storage::open_in_memory().expect("open in-memory storage"));
    let scanner = Arc::new(Scanner::new(storage.clone()));
    let (live_tail, _rx) = broadcast::channel(LIVE_TAIL_CHANNEL_CAPACITY);
    create_router(AppState {
        storage,
        scanner,
        dist_dir: None,
        live_tail,
    })
}

async fn call(method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    let req = match body {
        Some(v) => builder.body(Body::from(serde_json::to_vec(&v).unwrap())).unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let response = test_app().oneshot(req).await.expect("execute request");
    let status = response.status();
    let bytes = response.into_body().collect().await.expect("collect body").to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn config_route_reads_writes_and_refuses_a_key_it_does_not_know() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    std::env::set_var("AGENTWORTH_CONFIG_PATH", &path);

    // Nothing written yet: every key is present and null, so a client can tell "unset"
    // from "key I have never heard of".
    let (status, body) = call("GET", "/api/config", None).await;
    assert_eq!(status, StatusCode::OK);
    for key in ["json", "limit", "period", "archie.accessory", "archie.colourway"] {
        assert!(body.get(key).is_some_and(Value::is_null), "{} should be null", key);
    }

    // A partial write, echoed back as the whole config.
    let (status, body) = call(
        "POST",
        "/api/config",
        Some(json!({ "archie.accessory": "goggles", "archie.colourway": "c2" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["archie.accessory"], "goggles");
    assert_eq!(body["archie.colourway"], "C2", "a colourway is stored canonical");

    // It lands in the same file `agentworth config set` writes.
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(on_disk.contains("[archie]"), "config.toml:\n{}", on_disk);
    assert!(on_disk.contains("goggles"));

    // A second write leaves the key it did not mention alone.
    let (_, body) = call("POST", "/api/config", Some(json!({ "limit": 12 }))).await;
    assert_eq!(body["limit"], 12);
    assert_eq!(body["archie.accessory"], "goggles");

    // Null clears one key back to the built-in default.
    let (_, body) = call("POST", "/api/config", Some(json!({ "limit": Value::Null }))).await;
    assert!(body["limit"].is_null());

    // The whole GET payload, config_path included, round-trips unchanged.
    let (status, _) = call("POST", "/api/config", Some(body.clone())).await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = call("POST", "/api/config", Some(json!({ "archie.hat": "fedora" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("unknown config key"));

    let (status, body) =
        call("POST", "/api/config", Some(json!({ "archie.colourway": "C9" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("invalid value"));

    let (status, _) = call("POST", "/api/config", Some(json!(["not", "an", "object"]))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // A rejected write changed nothing.
    let (_, body) = call("GET", "/api/config", None).await;
    assert_eq!(body["archie.colourway"], "C2");

    std::env::remove_var("AGENTWORTH_CONFIG_PATH");
}
