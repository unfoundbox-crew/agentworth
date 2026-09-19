//! Lane `lane/remove-legacy-dashboard`: the v0.1.27 legacy dashboard
//! ("Your agents left receipts" HTML at `/`) is gone from the binary.
//!
//! After: `/` serves the home deck when built; without a built deck it
//! returns the API root (JSON) -- never legacy HTML. Every `/api/*` route
//! the new deck needs stays registered.

use std::sync::Arc;

use agentworth_cli::server::{create_router, AppState, LIVE_TAIL_CHANNEL_CAPACITY};
use agentworth_core::Scanner;
use agentworth_storage::Storage;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tokio::sync::broadcast;
use tower::ServiceExt;

fn test_app() -> axum::Router {
    let storage = Arc::new(Storage::open_in_memory().expect("open in-memory storage"));
    let scanner = Arc::new(Scanner::new(storage.clone()));
    let (live_tail_tx, _rx) = broadcast::channel(LIVE_TAIL_CHANNEL_CAPACITY);
    let state = AppState {
        storage: storage.clone(),
        scanner: scanner.clone(),
        live_tail: live_tail_tx,
        #[cfg(unix)]
        home: None,
    };
    create_router(state)
}

async fn get_raw(app: axum::Router, uri: &str) -> (StatusCode, String, String) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.expect("execute request");
    let status = res.status();
    let content_type = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = res.into_body().collect().await.expect("body").to_bytes();
    (status, content_type, String::from_utf8_lossy(&bytes).to_string())
}

#[tokio::test]
async fn root_never_serves_legacy_dashboard_html() {
    let (status, content_type, body) = get_raw(test_app(), "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !body.contains("Your agents left receipts"),
        "legacy dashboard copy still served at /"
    );
    assert!(
        !body.contains("AgentWorth — Your agents left receipts"),
        "legacy dashboard title still served at /"
    );
    assert!(
        !body.contains("<title>AgentWorth</title>"),
        "old dashboard bundle shell still served at /"
    );
    if agentworth_cli::server::embedded_home_deck_is_built() {
        assert!(
            content_type.starts_with("text/html"),
            "/ should be deck HTML when the deck is built, got: {content_type}"
        );
        assert!(body.contains("<title>home</title>"));
    } else {
        assert!(
            content_type.starts_with("application/json"),
            "/ should be the JSON API root without a built deck, got: {content_type} :: {body:.120}"
        );
        assert!(body.contains("/api/"));
    }
}

#[tokio::test]
async fn no_route_serves_the_old_dashboard_bundle() {
    // The legacy dashboard shell referenced a hashed Vite bundle at /assets/*.
    // The home deck (base '/home/') never serves anything there, so a 200 with
    // JS at a dashboard-shaped path means the old bundle is still wired up.
    for path in ["/assets/index-legacy.js", "/favicon.ico"] {
        let (status, _, body) = get_raw(test_app(), path).await;
        assert!(
            !(status == StatusCode::OK && body.contains("Your agents left receipts")),
            "legacy bundle still served at {path}"
        );
    }
    // Unknown non-API paths fall back to the deck (SPA) when built, or a JSON 404
    // without a deck -- never to the legacy stub.
    let (status, content_type, body) = get_raw(test_app(), "/traces/sess_123").await;
    assert!(!body.contains("Your agents left receipts"));
    if agentworth_cli::server::embedded_home_deck_is_built() {
        assert_eq!(status, StatusCode::OK);
        assert!(content_type.starts_with("text/html"));
    } else {
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(content_type.starts_with("application/json"));
    }
}

#[tokio::test]
async fn every_api_route_the_new_deck_needs_survives() {
    // Mirrors docs/REFERENCE.md: these are current, not legacy.
    for path in [
        "/api/stats",
        "/api/traces",
        "/api/usage",
        "/api/pacing",
        "/api/blame",
        "/api/matrix",
        "/api/archaeology",
        "/api/config",
    ] {
        let (status, _, _) = get_raw(test_app(), path).await;
        assert_eq!(status, StatusCode::OK, "API route gone: {path}");
    }
}
