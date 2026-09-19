//! Serving the `apps/home` deck and the JSON API root.
//!
//! The v0.1.27 legacy dashboard ("Your agents left receipts" HTML at `/`,
//! `apps/dashboard`, its compiled-in bundle and the `--dist` flag) was removed
//! on `lane/remove-legacy-dashboard`: it stays in git history, not in the
//! binary. `/` serves the home deck when built; without a built deck it returns
//! the API root (JSON), never legacy HTML. The `/api/*` surface is untouched --
//! every route lives in `routes::route_entries()`, which also builds the live
//! router, so the API the new deck needs cannot drift from what is served.

use axum::body::Body;
use axum::http::{header, HeaderValue, Request, Response, StatusCode};
use axum::response::IntoResponse;
use rust_embed::RustEmbed;

/// rust_embed::Metadata carries hashes and timestamps, not a mimetype, so the
/// content type is derived from the extension. The deck build emits only
/// these six; anything else falls back to octet-stream rather than guessing.
fn content_type_for(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

/// The `apps/home` deck, compiled into the binary. Populated by
/// `npm run build` in apps/home, which emits `apps/home/dist` with
/// `base: '/home/'` baked into every asset URL (`apps/home/vite.config.ts`) --
/// so deck assets live under the `/home/` prefix, and `/` serves the same
/// `index.html` shell (its asset URLs are absolute, so they resolve back under
/// `/home/` whichever entry path the browser landed on).
#[derive(RustEmbed)]
#[folder = "../../apps/home/dist"]
struct HomeDeckAssets;

/// Whether `npm run build` in apps/home ran before this binary was compiled.
/// `archie home` checks this before starting the server, since a blank page is
/// a worse failure mode than a command that refuses to start with an
/// instruction attached.
pub fn embedded_home_deck_is_built() -> bool {
    HomeDeckAssets::get("index.html").is_some()
}

fn home_deck_response(path: &str) -> Option<Response<Body>> {
    let asset = HomeDeckAssets::get(path)?;
    let mime = content_type_for(path);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .body(Body::from(asset.data.into_owned()))
        .ok()
}

fn plain_text(status: StatusCode, body: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        )
        .body(Body::from(body.to_string()))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap()
        })
}

/// The JSON body answered at `/` when no deck was built into the binary, and
/// (with a 404 status) at any path no route owns. The route list is read off
/// `routes::route_entries()` -- the same table that builds the live router --
/// so a bare `curl localhost:3000/` always names routes the server serves.
fn api_root_body() -> String {
    let routes: Vec<String> = super::routes::route_entries()
        .iter()
        .map(|e| format!("{} /api{}", e.method, e.path))
        .collect();
    serde_json::json!({
        "name": "agentworth",
        "version": env!("CARGO_PKG_VERSION"),
        "ui": "the home deck was not built into this binary; run npm run build in apps/home and rebuild",
        "api": "/api/",
        "routes": routes,
    })
    .to_string()
}

fn api_root_response() -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )
        .body(Body::from(api_root_body()))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap()
        })
}

/// `GET /`: the home deck shell when built, otherwise the JSON API root.
/// Never legacy HTML -- the v0.1.27 dashboard is gone from this binary.
pub async fn serve_root() -> impl IntoResponse {
    if let Some(res) = home_deck_response("index.html") {
        return res;
    }
    api_root_response()
}

/// Serves the embedded `apps/home` deck under `/home` and `/home/*`, with
/// `index.html` as the SPA history fallback for any client-side route the deck
/// itself owns. Static deck assets need no live gateway, so this answers
/// whenever the deck was built in -- `--home` only gates `/ws`
/// (`home::gateway::ws_handler`).
pub async fn serve_home_deck(req: Request<Body>) -> impl IntoResponse {
    let path = req.uri().path();
    let rel = path
        .strip_prefix("/home")
        .unwrap_or("")
        .trim_start_matches('/');
    let asset_path = if rel.is_empty() { "index.html" } else { rel };

    if let Some(res) = home_deck_response(asset_path) {
        return res;
    }
    if let Some(res) = home_deck_response("index.html") {
        return res;
    }

    plain_text(
        StatusCode::NOT_FOUND,
        "the deck was not built into this binary; run npm run build in apps/home and rebuild",
    )
}

/// Fallback for every path no route owns: the deck's SPA shell when built
/// (client-side routes), otherwise a JSON 404 that points at `/api/`.
pub async fn serve_fallback(req: Request<Body>) -> impl IntoResponse {
    if req.uri().path().starts_with("/home") {
        return serve_home_deck(req).await.into_response();
    }
    if let Some(res) = home_deck_response("index.html") {
        return res;
    }
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )
        .body(Body::from(api_root_body()))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap()
        })
}
