//! Axum HTTP routes and handlers for the AgentWorth local API server.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agentworth_adapter_sdk::{AgentAdapter, ScanOptions};
use agentworth_adapters::{
    AiderAdapter, ClaudeCodeAdapter, ClineAdapter, CodexAdapter, CursorAdapter, DeepSeekAdapter,
    GeminiAdapter, GooseAdapter, GrokAdapter, HerdrAdapter, HermesAdapter, KimiAdapter, ManusAdapter,
    MiniMaxAdapter, OpenClawAdapter, OpenCodeAdapter, PiAdapter, QwenAdapter, WindsurfAdapter,
    ZhipuAdapter,
};
use agentworth_core::Scanner;
use agentworth_outcomes::{OutcomeDetector, RecoveryDetector};
use agentworth_schema::{AgentWorthTrace, OutcomeEvidence};
use agentworth_scoring::{TraceScore, TraceScorer};
use agentworth_storage::{
    BlameMatch, PacingSummary, SessionFilter, SessionOrderBy, SessionSummary, Storage,
    UsagePeriodSummary,
};
use anyhow::Result;
use axum::extract::{Path, Query, State};
use axum::http::{Request, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{body::Body, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast;
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};
use tokio_stream::{Stream, StreamExt};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use super::archaeology::{compute_archaeology_highlights, ArchaeologyHighlights};
use super::live_tail::LiveTailEvent;
use super::static_files::serve_static_or_spa;

/// Shared application state across API handlers.
#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<Storage>,
    pub scanner: Arc<Scanner>,
    pub dist_dir: Option<PathBuf>,
    /// Sender side of the live-tail filesystem-event broadcast. Each SSE connection calls
    /// `.subscribe()` for its own receiver; cloning the sender itself is cheap.
    pub live_tail: broadcast::Sender<LiveTailEvent>,
}

/// Query parameters for listing and filtering indexed traces.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TracesQuery {
    pub adapter: Option<String>,
    pub model: Option<String>,
    pub search: Option<String>,
    pub outcome: Option<String>,
    pub min_tokens: Option<u64>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub order_by: Option<SessionOrderBy>,
}

/// Query parameters for usage rollups.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UsageQuery {
    pub daily_limit: Option<usize>,
    pub weekly_limit: Option<usize>,
    pub monthly_limit: Option<usize>,
}

/// Aggregated usage response containing daily, weekly, and monthly rollups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageResponse {
    pub daily: Vec<UsagePeriodSummary>,
    pub weekly: Vec<UsagePeriodSummary>,
    pub monthly: Vec<UsagePeriodSummary>,
}

/// Query parameters for rolling pacing window.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PacingQuery {
    pub hours: Option<i64>,
}

/// Query parameters for AI Code Blame search.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BlameQuery {
    pub file: Option<String>,
    pub path: Option<String>,
}

/// Adapter capabilities and extraction coverage matrix item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterMatrixItem {
    pub adapter: String,
    pub name: String,
    pub detected: bool,
    pub sessions_count: usize,
    pub formats: Vec<String>,
    pub token_accounting: bool,
    pub cache_breakdown: bool,
    pub tool_calls: bool,
    pub file_actions: bool,
    pub shell_commands: bool,
    pub model_switches: bool,
    pub thinking_blocks: bool,
    pub error_recovery: bool,
}

/// Complete adapter matrix response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterMatrixResponse {
    pub total_adapters: usize,
    pub detected_adapters: usize,
    pub adapters: Vec<AdapterMatrixItem>,
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
        .route("/usage", get(get_usage_handler))
        .route("/pacing", get(get_pacing_handler))
        .route("/blame", get(get_blame_handler))
        .route("/matrix", get(get_matrix_handler))
        .route("/archaeology", get(get_archaeology_handler))
        .route("/live-tail", get(get_live_tail_handler))
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

/// Maps a stored `primary_outcome` value to the snake_case key the web dashboard's
/// `OutcomeDistribution` type expects.
///
/// `agentworth_outcomes::outcome_kind_name` now writes the snake_case form directly (it defers
/// to `OutcomeKind`'s own serde encoding — see `crates/outcomes/src/outcome.rs`), and the
/// storage-layer migration in `crates/storage/src/lib.rs::initialize_schema` corrects any
/// pre-existing PascalCase rows on open. So in the common case this is close to an identity
/// mapping. It still recognizes the legacy PascalCase literals (e.g. `"CommitObserved"`) as a
/// defense-in-depth fallback — belt and suspenders in case some row is ever read before a
/// migration runs — and anything else unrecognized (including the "Unresolved" sentinel used
/// below for a session that hasn't been scored) folds into "unresolved" rather than silently
/// minting a new bucket.
fn outcome_distribution_key(outcome: &str) -> &'static str {
    match outcome {
        "ci_or_deployment_verified" | "CiOrDeploymentVerified" => "ci_or_deployment_verified",
        "commit_observed" | "CommitObserved" => "commit_observed",
        "test_or_build_passed" | "TestOrBuildPassed" => "test_or_build_passed",
        "artifact_changed" | "ArtifactChanged" => "artifact_changed",
        "done_claimed" | "DoneClaimed" => "done_claimed",
        _ => "unresolved",
    }
}

/// GET /api/stats -> machine-wide experience stats JSON with outcome distributions and verification telemetry
async fn get_stats_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let storage = state.storage.clone();

    let (stats_res, top_repos_res, all_sessions_res) = tokio::task::spawn_blocking(move || {
        (
            storage.get_aggregate_stats(),
            storage.get_top_repositories(),
            storage.list_sessions_filtered(&SessionFilter {
                limit: Some(10000),
                ..Default::default()
            }),
        )
    })
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Task joining failed: {}", e) })),
        )
    })?;

    let stats = stats_res.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to retrieve stats: {}", e) })),
        )
    })?;

    let top_repos = top_repos_res.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to retrieve top repositories: {}", e) })),
        )
    })?;

    let all_sessions = all_sessions_res.unwrap_or_default();

    let mut outcome_dist: BTreeMap<String, usize> = BTreeMap::new();
    let mut total_score = 0.0f64;
    let mut scored_count = 0usize;

    for s in &all_sessions {
        // `primary_outcome` is stored as the snake_case OutcomeKind variant name (see
        // crates/outcomes/src/outcome.rs), matching the keys below directly. An unpopulated
        // column used to fall back to the literal string "done_claimed", which fabricated a
        // fake verified-outcome bucket for every un-scored session instead of reporting it as
        // unresolved — "Unresolved" here is just a sentinel for outcome_distribution_key to
        // fold into the "unresolved" bucket, not an OutcomeKind value.
        let outcome_str = s.primary_outcome.as_deref().unwrap_or("Unresolved");
        let key = outcome_distribution_key(outcome_str);
        *outcome_dist.entry(key.to_string()).or_insert(0) += 1;

        if let Some(score) = s.composite_score {
            total_score += score;
            scored_count += 1;
        }
    }

    let average_composite_score = if scored_count > 0 {
        total_score / (scored_count as f64)
    } else {
        0.0
    };

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
        "verified_outcomes_count": stats.verified_outcomes_count,
        "outcome_distribution": outcome_dist,
        "average_composite_score": average_composite_score,
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

    let storage = state.storage.clone();
    let sessions_res = tokio::task::spawn_blocking(move || storage.list_sessions_filtered(&filter))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Task joining failed: {}", e) })),
            )
        })?;

    let sessions = sessions_res.map_err(|e| {
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
    let scanner = state.scanner.clone();
    let id_clone = id.clone();
    let trace_res = tokio::task::spawn_blocking(move || scanner.load_trace(&id_clone))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Task joining failed: {}", e) })),
            )
        })?;

    let trace = trace_res.map_err(|e| {
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

/// GET /api/usage -> daily, weekly, monthly usage rollups
async fn get_usage_handler(
    State(state): State<AppState>,
    Query(query): Query<UsageQuery>,
) -> Result<Json<UsageResponse>, (StatusCode, Json<serde_json::Value>)> {
    let storage = state.storage.clone();
    let usage_res = tokio::task::spawn_blocking(move || {
        let daily = storage.get_daily_usage(query.daily_limit.or(Some(30)))?;
        let weekly = storage.get_weekly_usage(query.weekly_limit.or(Some(20)))?;
        let monthly = storage.get_monthly_usage(query.monthly_limit.or(Some(12)))?;
        Ok::<_, anyhow::Error>(UsageResponse {
            daily,
            weekly,
            monthly,
        })
    })
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Task joining failed: {}", e) })),
        )
    })?;

    let usage = usage_res.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to retrieve usage rollups: {}", e) })),
        )
    })?;

    Ok(Json(usage))
}

/// GET /api/pacing -> rolling pacing window (e.g. 5-hour), burn velocity, and cache hit ratio
async fn get_pacing_handler(
    State(state): State<AppState>,
    Query(query): Query<PacingQuery>,
) -> Result<Json<PacingSummary>, (StatusCode, Json<serde_json::Value>)> {
    let storage = state.storage.clone();
    let hours = query.hours.unwrap_or(5);
    let pacing_res = tokio::task::spawn_blocking(move || storage.get_pacing_window(hours))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Task joining failed: {}", e) })),
            )
        })?;

    let pacing = pacing_res.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed computing pacing window: {}", e) })),
        )
    })?;

    Ok(Json(pacing))
}

/// GET /api/blame?file=<path> -> file change lineage matching session histories
async fn get_blame_handler(
    State(state): State<AppState>,
    Query(query): Query<BlameQuery>,
) -> Result<Json<Vec<BlameMatch>>, (StatusCode, Json<serde_json::Value>)> {
    let storage = state.storage.clone();
    let target = query.file.or(query.path).unwrap_or_default();
    let matches_res = tokio::task::spawn_blocking(move || storage.find_sessions_for_blame(&target))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Task joining failed: {}", e) })),
            )
        })?;

    let matches = matches_res.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed executing file blame: {}", e) })),
        )
    })?;

    Ok(Json(matches))
}

/// GET /api/matrix -> adapter extraction coverage and capabilities matrix
async fn get_matrix_handler(
    State(state): State<AppState>,
) -> Result<Json<AdapterMatrixResponse>, (StatusCode, Json<serde_json::Value>)> {
    let storage = state.storage.clone();
    let matrix_res = tokio::task::spawn_blocking(move || compute_adapter_matrix(&storage))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Task joining failed: {}", e) })),
            )
        })?;

    Ok(Json(matrix_res))
}

/// Compute capability and detection matrix across all registered adapters.
fn compute_adapter_matrix(storage: &Storage) -> AdapterMatrixResponse {
    let stats = storage.get_aggregate_stats().unwrap_or_default();
    let scan_opts = ScanOptions::default();

    #[allow(clippy::type_complexity)]
    let adapter_defs: Vec<(
        Box<dyn AgentAdapter>,
        &'static str,
        Vec<&'static str>,
        bool,
        bool,
        bool,
        bool,
        bool,
        bool,
        bool,
        bool,
    )> = vec![
        (
            Box::new(ClaudeCodeAdapter::new()),
            "Claude Code",
            vec!["jsonl", "json"],
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        (
            Box::new(CodexAdapter::new()),
            "Codex",
            vec!["jsonl"],
            true,
            false,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        (
            Box::new(GeminiAdapter::new()),
            "Gemini / Antigravity",
            vec!["jsonl", "json"],
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        (
            Box::new(OpenCodeAdapter::new()),
            "OpenCode",
            vec!["jsonl", "db"],
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        (
            Box::new(CursorAdapter::new()),
            "Cursor Composer",
            vec!["jsonl", "vscdb"],
            true,
            false,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        (
            Box::new(ClineAdapter::new()),
            "Cline",
            vec!["json"],
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        (
            Box::new(WindsurfAdapter::new()),
            "Windsurf Cascade",
            vec!["json", "jsonl"],
            true,
            false,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        (
            Box::new(AiderAdapter::new()),
            "Aider",
            vec!["chat.history.md", "jsonl"],
            true,
            false,
            true,
            true,
            true,
            true,
            false,
            true,
        ),
        (
            Box::new(DeepSeekAdapter::new()),
            "DeepSeek Coder",
            vec!["jsonl"],
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        (
            Box::new(GrokAdapter::new()),
            "Grok / xAI",
            vec!["jsonl"],
            true,
            false,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        (
            Box::new(KimiAdapter::new()),
            "Kimi K1.5",
            vec!["jsonl"],
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        (
            Box::new(MiniMaxAdapter::new()),
            "MiniMax",
            vec!["jsonl"],
            true,
            false,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        (
            Box::new(QwenAdapter::new()),
            "Qwen / Alibaba",
            vec!["jsonl"],
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        (
            Box::new(ZhipuAdapter::new()),
            "GLM / Zhipu",
            vec!["jsonl"],
            true,
            false,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        (
            Box::new(GooseAdapter::new()),
            "Goose",
            vec!["jsonl", "db"],
            true,
            false,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        (
            Box::new(ManusAdapter::new()),
            "Manus",
            vec!["jsonl"],
            true,
            false,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        (
            Box::new(HerdrAdapter::new()),
            "Herdr",
            vec!["jsonl"],
            true,
            false,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        (
            Box::new(HermesAdapter::new()),
            "Hermes Agent",
            vec!["jsonl"],
            true,
            false,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        (
            Box::new(OpenClawAdapter::new()),
            "OpenClaw",
            vec!["jsonl"],
            true,
            false,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
        (
            Box::new(PiAdapter::new()),
            "Pi / Inflection",
            vec!["jsonl"],
            true,
            false,
            true,
            true,
            true,
            true,
            true,
            true,
        ),
    ];

    let mut detected_count = 0;
    let mut adapters = Vec::new();

    for (
        adapter_impl,
        display_name,
        formats,
        tok_acc,
        cache_bd,
        tool_c,
        file_a,
        shell_c,
        model_sw,
        thinking_b,
        err_rec,
    ) in adapter_defs
    {
        let is_detected = adapter_impl
            .detect(&scan_opts)
            .map(|d| d.is_present)
            .unwrap_or(false);
        if is_detected {
            detected_count += 1;
        }
        let sess_count = stats
            .sessions_by_adapter
            .get(adapter_impl.name())
            .copied()
            .unwrap_or(0);

        adapters.push(AdapterMatrixItem {
            adapter: adapter_impl.name().to_string(),
            name: display_name.to_string(),
            detected: is_detected,
            sessions_count: sess_count,
            formats: formats.into_iter().map(String::from).collect(),
            token_accounting: tok_acc,
            cache_breakdown: cache_bd,
            tool_calls: tool_c,
            file_actions: file_a,
            shell_commands: shell_c,
            model_switches: model_sw,
            thinking_blocks: thinking_b,
            error_recovery: err_rec,
        });
    }

    let total_adapters = adapters.len();
    AdapterMatrixResponse {
        total_adapters,
        detected_adapters: detected_count,
        adapters,
    }
}

/// GET /api/archaeology -> archaeology highlights
async fn get_archaeology_handler(
    State(state): State<AppState>,
) -> Result<Json<ArchaeologyHighlights>, (StatusCode, Json<serde_json::Value>)> {
    let storage = state.storage.clone();
    let scanner = state.scanner.clone();
    let highlights_res = tokio::task::spawn_blocking(move || {
        compute_archaeology_highlights(&storage, &scanner)
    })
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Task joining failed: {}", e) })),
        )
    })?;

    let highlights = highlights_res.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed computing archaeology: {}", e) })),
        )
    })?;

    Ok(Json(highlights))
}

/// GET /api/live-tail -> Server-Sent Events stream of live filesystem changes under
/// watched adapter session directories. Each event carries a `change` name; a subscriber
/// that falls too far behind the broadcast channel's buffer gets a `lagged` event instead
/// of the stream silently dropping or terminating.
async fn get_live_tail_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let receiver = state.live_tail.subscribe();

    let stream = BroadcastStream::new(receiver).map(|item| {
        let event = match item {
            Ok(live_event) => Event::default()
                .event("change")
                .json_data(&live_event)
                .unwrap_or_else(|e| {
                    tracing::warn!("live-tail: failed to serialize event: {}", e);
                    Event::default().event("change").data("{}")
                }),
            Err(BroadcastStreamRecvError::Lagged(skipped)) => {
                Event::default().event("lagged").data(skipped.to_string())
            }
        };
        Ok(event)
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
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

    let scanner = state.scanner.clone();
    let summary_res =
        tokio::task::spawn_blocking(move || scanner.run_scan(&options, |_, _| {}))
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": format!("Task joining failed: {}", e) })),
                )
            })?;

    let summary = summary_res.map_err(|e| {
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
    let scanner = state.scanner.clone();
    let id_clone = id.clone();
    let trace_res = tokio::task::spawn_blocking(move || scanner.load_trace(&id_clone))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Task joining failed: {}", e) })),
            )
        })?;

    let mut trace = trace_res.map_err(|e| {
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
