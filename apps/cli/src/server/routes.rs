//! Axum HTTP routes and handlers for the AgentWorth local API server.

use std::path::PathBuf;
use std::sync::Arc;

use agentworth_adapter_sdk::ScanOptions;
use agentworth_core::Scanner;
use agentworth_outcomes::{OutcomeDetector, RecoveryDetector};
use agentworth_schema::{AgentWorthTrace, OutcomeEvidence};
use agentworth_scoring::{TraceScore, TraceScorer};
use agentworth_storage::{SessionFilter, SessionOrderBy, SessionSummary, Storage};
use anyhow::Result;
use axum::extract::{Path, Query, State};
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
use axum::{body::Body, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use super::archaeology::{compute_archaeology_highlights, ArchaeologyHighlights};
use super::static_files::serve_static_or_spa;

/// Shared application state across API handlers.
#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<Storage>,
    pub scanner: Arc<Scanner>,
    pub dist_dir: Option<PathBuf>,
}

/// Query parameters for listing and filtering indexed traces.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TracesQuery {
    pub adapter: Option<String>,
    pub model: Option<String>,
    pub search: Option<String>,
    pub min_tokens: Option<u64>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub order_by: Option<SessionOrderBy>,
}

/// Detailed response for inspecting a full trace session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceDetailResponse {
    pub trace: AgentWorthTrace,
    pub score: TraceScore,
    pub outcomes: Vec<OutcomeEvidence>,
    pub recoveries: Vec<agentworth_outcomes::RecoverySignal>,
}

/// Optional payload to configure scan execution.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScanRequest {
    pub paths: Option<Vec<PathBuf>>,
    pub force: Option<bool>,
}

/// Request payload for trace export.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExportRequest {
    pub redact: Option<bool>,
    pub format: Option<String>,
}

/// Response payload for trace export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportResponse {
    pub session_id: String,
    pub format: String,
    pub content: String,
}

/// Builds the complete Axum router with all API routes, CORS, tracing, and static fallback.
pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api_routes = Router::new()
        .route("/stats", get(get_stats_handler))
        .route("/traces", get(get_traces_handler))
        .route("/traces/:id", get(get_trace_by_id_handler))
        .route("/archaeology", get(get_archaeology_handler))
        .route("/scan", post(post_scan_handler))
        .route("/export/:id", post(post_export_handler));

    let dist_dir_for_fallback = state.dist_dir.clone();

    Router::new()
        .nest("/api", api_routes)
        .fallback(move |req: Request<Body>| {
            let dist_clone = dist_dir_for_fallback.clone();
            async move { serve_static_or_spa(dist_clone, req).await }
        })
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// GET /api/stats -> machine-wide experience stats JSON
async fn get_stats_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let stats = state.storage.get_aggregate_stats().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to retrieve stats: {}", e) })),
        )
    })?;

    let top_repos = state.storage.get_top_repositories().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to retrieve top repositories: {}", e) })),
        )
    })?;

    let response = json!({
        "total_sessions": stats.total_sessions,
        "total_events": stats.total_events,
        "date_range": {
            "first_session_at": stats.first_session_at,
            "last_session_at": stats.last_session_at,
        },
        "token_usage": {
            "input_tokens": stats.token_usage.input_tokens,
            "output_tokens": stats.token_usage.output_tokens,
            "cache_read_tokens": stats.token_usage.cache_read_tokens,
            "cache_creation_tokens": stats.token_usage.cache_creation_tokens,
            "total_tokens": stats.token_usage.total(),
        },
        "sessions_by_adapter": stats.sessions_by_adapter,
        "models_usage_count": stats.models_usage_count,
        "tools_usage_count": stats.tools_usage_count,
        "top_repositories": top_repos.iter().map(|(path, count)| json!({
            "repository": path,
            "sessions_count": count
        })).collect::<Vec<_>>()
    });

    Ok(Json(response))
}

/// GET /api/traces -> filtered list of sessions
async fn get_traces_handler(
    State(state): State<AppState>,
    Query(query): Query<TracesQuery>,
) -> Result<Json<Vec<SessionSummary>>, (StatusCode, Json<serde_json::Value>)> {
    let filter = SessionFilter {
        adapter: query.adapter,
        model: query.model,
        search: query.search,
        min_tokens: query.min_tokens,
        limit: query.limit.or(Some(50)),
        offset: query.offset,
        order_by: query.order_by.or(Some(SessionOrderBy::StartedAtDesc)),
        ..Default::default()
    };

    let sessions = state.storage.list_sessions_filtered(&filter).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to list traces: {}", e) })),
        )
    })?;

    Ok(Json(sessions))
}

/// GET /api/traces/:id -> full trace details (metadata, stats, 5-factor score, outcome evidence, timeline)
async fn get_trace_by_id_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<TraceDetailResponse>, (StatusCode, Json<serde_json::Value>)> {
    let trace = state.scanner.load_trace(&id).map_err(|e| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("Trace session '{}' not found: {}", id, e) })),
        )
    })?;

    let scorer = TraceScorer::default();
    let score = scorer.score(&trace);
    let outcomes = OutcomeDetector::new().detect_outcomes(&trace);
    let recoveries = RecoveryDetector::new().detect_recoveries(&trace);

    Ok(Json(TraceDetailResponse {
        trace,
        score,
        outcomes,
        recoveries,
    }))
}

/// GET /api/archaeology -> archaeology highlights
async fn get_archaeology_handler(
    State(state): State<AppState>,
) -> Result<Json<ArchaeologyHighlights>, (StatusCode, Json<serde_json::Value>)> {
    let highlights =
        compute_archaeology_highlights(&state.storage, &state.scanner).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Failed computing archaeology: {}", e) })),
            )
        })?;

    Ok(Json(highlights))
}

/// POST /api/scan -> triggers scanner background sync
async fn post_scan_handler(
    State(state): State<AppState>,
    body: Option<Json<ScanRequest>>,
) -> Result<Json<agentworth_core::ScanSummary>, (StatusCode, Json<serde_json::Value>)> {
    let req = body.map(|Json(b)| b).unwrap_or_default();
    let options = ScanOptions {
        custom_paths: req.paths.unwrap_or_default(),
        force: req.force.unwrap_or(false),
    };

    let summary = state.scanner.run_scan(&options, |_, _| {}).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Scan failed: {}", e) })),
        )
    })?;

    Ok(Json(summary))
}

/// POST /api/export/:id -> exports trace with optional redaction in JSON or ATIF format
async fn post_export_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Option<Json<ExportRequest>>,
) -> Result<Json<ExportResponse>, (StatusCode, Json<serde_json::Value>)> {
    let req = body.map(|Json(b)| b).unwrap_or_default();
    let mut trace = state.scanner.load_trace(&id).map_err(|e| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("Trace session '{}' not found: {}", id, e) })),
        )
    })?;

    if req.redact.unwrap_or(false) {
        trace = agentworth_redaction::redact_trace(&trace);
    }

    let format = req.format.as_deref().unwrap_or("json");
    let content = match format {
        "atif" => agentworth_export_atif::export_to_atif(&trace, true).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("ATIF export failed: {}", e) })),
            )
        })?,
        _ => serde_json::to_string_pretty(&trace).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("JSON serialization failed: {}", e) })),
            )
        })?,
    };

    Ok(Json(ExportResponse {
        session_id: id,
        format: format.to_string(),
        content,
    }))
}
