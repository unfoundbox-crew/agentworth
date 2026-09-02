//! Axum HTTP routes and handlers for the AgentWorth local API server.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agentworth_adapter_sdk::{AgentAdapter, ScanOptions};
use agentworth_core::Scanner;
use agentworth_outcomes::{OutcomeDetector, RecoveryDetector};
use agentworth_schema::{AgentWorthTrace, NormalizedEvent, OutcomeEvidence};
use agentworth_scoring::{TraceScore, TraceScorer};
use agentworth_storage::{
    BlameMatch, PacingSummary, SessionFilter, SessionOrderBy, SessionSummary, Storage,
    UsagePeriodSummary,
};
use anyhow::Result;
use axum::extract::{Path, Query, State};
use axum::http::{Request, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post, MethodRouter};
use axum::{body::Body, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast;
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};
use tokio_stream::{Stream, StreamExt};
use tower_http::compression::CompressionLayer;
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
///
/// `cost_basis`/`subscription_tier` label every `estimated_cost_usd` in `daily`/`weekly`/
/// `monthly` as an API list-price equivalent, not what the account actually paid -- see
/// `crate::cost_basis`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageResponse {
    pub daily: Vec<UsagePeriodSummary>,
    pub weekly: Vec<UsagePeriodSummary>,
    pub monthly: Vec<UsagePeriodSummary>,
    pub cost_basis: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_tier: Option<String>,
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
    /// Every raw `sessions.adapter` value counted into `sessions_count` above. Usually
    /// just `[adapter]`; an adapter that tags sessions under more than one product
    /// identity (see `AgentAdapter::identity_names`, e.g. Gemini's "gemini" and
    /// "antigravity") lists all of them here so their rows aren't silently absorbed
    /// without a trace.
    pub identities: Vec<String>,
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
    /// `trace.events` is sliced per `events_offset`/`events_limit` (see `EventsPageQuery`);
    /// every other field on `trace` (stats, provenance, metadata) is always the full,
    /// unsliced value.
    pub trace: AgentWorthTrace,
    pub score: TraceScore,
    pub outcomes: Vec<OutcomeEvidence>,
    pub recoveries: Vec<agentworth_outcomes::RecoverySignal>,
    /// Total number of events on this trace, independent of how many `trace.events` carries
    /// in this response -- lets a caller tell "sliced" from "this session genuinely has few
    /// events" and know how far there is left to page.
    pub events_total: usize,
    /// The offset actually applied (0 when the query supplied none), echoed back so a caller
    /// doesn't have to remember what it asked for.
    pub events_offset: usize,
}

/// Query parameters for paging through a trace's events, shared by `GET /api/traces/:id` (which
/// slices the embedded `trace.events`) and `GET /api/traces/:id/events` (which returns just the
/// slice). Both `offset` and `limit` are optional; omitting both reproduces the pre-pagination
/// behavior of returning every event, so no existing caller breaks.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct EventsPageQuery {
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

/// Response for `GET /api/traces/:id/events` -- just the event slice, for a dashboard to fetch
/// lazily after the initial `/api/traces/:id` load.
#[derive(Debug, Clone, Serialize)]
pub struct EventsPageResponse {
    pub events: Vec<NormalizedEvent>,
    pub events_total: usize,
    pub events_offset: usize,
}

/// Slices `events` to the requested page. `offset` defaults to 0; `limit` of `None` returns
/// everything from `offset` onward, so calling this with both params `None` is a no-op that
/// reproduces the pre-pagination behavior exactly. `Some(0)` is rejected -- a caller asking
/// for zero events almost certainly mistyped `limit`, and silently returning an empty page
/// would look like "session has no events" rather than "bad request".
///
/// Consumes `events` rather than borrowing, since every caller already owns a freshly loaded
/// `AgentWorthTrace` and slicing in place avoids cloning a payload that can run to tens of
/// thousands of entries.
pub fn paginate_events(
    events: Vec<NormalizedEvent>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<(Vec<NormalizedEvent>, usize, usize), String> {
    if limit == Some(0) {
        return Err("limit must be greater than 0".to_string());
    }

    let total = events.len();
    let offset = offset.unwrap_or(0);

    if offset >= total {
        return Ok((Vec::new(), total, offset));
    }

    let page = match limit {
        Some(l) => events.into_iter().skip(offset).take(l).collect(),
        None => events.into_iter().skip(offset).collect(),
    };

    Ok((page, total, offset))
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

/// One documented query parameter, read by `agentworth docs` (see `apps/cli/src/commands/docs.rs`).
#[derive(Debug, Clone, Copy)]
pub struct QueryParamDoc {
    pub name: &'static str,
    pub description: &'static str,
}

/// One registered API route, carrying both the live `MethodRouter` and the documentation
/// `agentworth docs` reads. `create_router` below builds the whole `/api` surface from exactly
/// this table -- there is no second, hand-copied list of paths for the two to drift apart on.
pub struct RouteEntry {
    pub method: &'static str,
    pub path: &'static str,
    pub description: &'static str,
    pub query_params: &'static [QueryParamDoc],
    pub handler: MethodRouter<AppState>,
}

/// The complete `/api/*` route table. Building a `MethodRouter` from a handler function is
/// just type erasure -- it doesn't touch `AppState` -- so this can be (and is) called from
/// `agentworth docs` for introspection without ever constructing a real server.
pub fn route_entries() -> Vec<RouteEntry> {
    const TRACES_PARAMS: &[QueryParamDoc] = &[
        QueryParamDoc { name: "adapter", description: "Filter by adapter name" },
        QueryParamDoc { name: "model", description: "Filter by model substring" },
        QueryParamDoc { name: "search", description: "Full-text search across session content" },
        QueryParamDoc { name: "outcome", description: "Filter by primary outcome kind" },
        QueryParamDoc { name: "min_tokens", description: "Minimum total token count" },
        QueryParamDoc { name: "limit", description: "Maximum number of sessions to return (default 50)" },
        QueryParamDoc { name: "offset", description: "Number of sessions to skip" },
        QueryParamDoc { name: "order_by", description: "Sort order for the result list" },
    ];
    const EVENTS_PAGE_PARAMS: &[QueryParamDoc] = &[
        QueryParamDoc { name: "offset", description: "Number of events to skip (default 0)" },
        QueryParamDoc { name: "limit", description: "Maximum number of events to return (default: all)" },
    ];
    const USAGE_PARAMS: &[QueryParamDoc] = &[
        QueryParamDoc { name: "daily_limit", description: "Maximum number of daily rollup rows" },
        QueryParamDoc { name: "weekly_limit", description: "Maximum number of weekly rollup rows" },
        QueryParamDoc { name: "monthly_limit", description: "Maximum number of monthly rollup rows" },
    ];
    const PACING_PARAMS: &[QueryParamDoc] = &[
        QueryParamDoc { name: "hours", description: "Pacing window duration in hours (default 5)" },
    ];
    const BLAME_PARAMS: &[QueryParamDoc] = &[
        QueryParamDoc { name: "file", description: "Target file path or pattern" },
        QueryParamDoc { name: "path", description: "Alias for `file`" },
    ];

    vec![
        RouteEntry {
            method: "GET",
            path: "/stats",
            description: "Machine-wide experience stats with outcome distributions and verification telemetry",
            query_params: &[],
            handler: get(get_stats_handler),
        },
        RouteEntry {
            method: "GET",
            path: "/traces",
            description: "Filtered, paginated list of indexed sessions",
            query_params: TRACES_PARAMS,
            handler: get(get_traces_handler),
        },
        RouteEntry {
            method: "GET",
            path: "/traces/:id",
            description: "Full trace details: metadata, stats, 5-factor score, outcome evidence, timeline",
            query_params: EVENTS_PAGE_PARAMS,
            handler: get(get_trace_by_id_handler),
        },
        RouteEntry {
            method: "GET",
            path: "/traces/:id/events",
            description: "Just the paginated event slice for one trace",
            query_params: EVENTS_PAGE_PARAMS,
            handler: get(get_trace_events_handler),
        },
        RouteEntry {
            method: "GET",
            path: "/usage",
            description: "Daily, weekly, and monthly token usage rollups",
            query_params: USAGE_PARAMS,
            handler: get(get_usage_handler),
        },
        RouteEntry {
            method: "GET",
            path: "/pacing",
            description: "Rolling pacing window: burn velocity and cache hit ratio",
            query_params: PACING_PARAMS,
            handler: get(get_pacing_handler),
        },
        RouteEntry {
            method: "GET",
            path: "/blame",
            description: "File change lineage matching session histories",
            query_params: BLAME_PARAMS,
            handler: get(get_blame_handler),
        },
        RouteEntry {
            method: "GET",
            path: "/matrix",
            description: "Adapter extraction coverage and capabilities matrix",
            query_params: &[],
            handler: get(get_matrix_handler),
        },
        RouteEntry {
            method: "GET",
            path: "/archaeology",
            description: "Archaeology highlights across the whole index",
            query_params: &[],
            handler: get(get_archaeology_handler),
        },
        RouteEntry {
            method: "GET",
            path: "/live-tail",
            description: "Server-Sent Events stream of live filesystem changes under watched adapter session directories",
            query_params: &[],
            handler: get(get_live_tail_handler),
        },
        RouteEntry {
            method: "POST",
            path: "/scan",
            description: "Trigger a scanner background sync (body: paths, force)",
            query_params: &[],
            handler: post(post_scan_handler),
        },
        RouteEntry {
            method: "POST",
            path: "/export/:id",
            description: "Export a trace with optional redaction, in JSON or ATIF format (body: redact, format)",
            query_params: &[],
            handler: post(post_export_handler),
        },
    ]
}

/// Builds the complete Axum router with all API routes, CORS, tracing, and static fallback.
pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let mut api_routes = Router::new();
    for entry in route_entries() {
        api_routes = api_routes.route(entry.path, entry.handler);
    }

    let dist_dir_for_fallback = state.dist_dir.clone();

    Router::new()
        .nest("/api", api_routes)
        .fallback(move |req: Request<Body>| {
            let dist_clone = dist_dir_for_fallback.clone();
            async move { serve_static_or_spa(dist_clone, req).await }
        })
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        // gzip + brotli, negotiated per-request off the client's `Accept-Encoding` -- the
        // /api/traces/:id trace payload can run tens of MB uncompressed on a large session,
        // and JSON compresses very well (repeated keys, tool-call boilerplate). Applied
        // outermost so it compresses the final response body regardless of which route
        // produced it, static SPA assets included.
        .layer(CompressionLayer::new())
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

    // `limit: None` means genuinely unlimited (see SessionFilter::limit's doc comment in
    // crates/storage/src/lib.rs). This used to be `Some(10000)`, which silently undercounted
    // outcome_distribution/average_composite_score on any index bigger than 10,000 non-stub
    // sessions (a real peer index already has 10,188) while total_sessions -- computed
    // separately via the unbounded get_aggregate_stats -- kept reporting the true count. That
    // mismatch is the same "presented as complete but silently truncated" shape already fixed
    // for compute_verdict_breakdown's old default-50 cap. There's no response-size or
    // query-shape reason to keep a cap here: this handler's own get_aggregate_stats and
    // get_top_repositories calls already do unbounded full-table scans of `sessions` on every
    // request, the response only ever carries fixed-shape aggregates (not the session list
    // itself), and the frontend fetches this endpoint once per pane-mount, not on a poll.
    // get_aggregate_stats(false): this handler's own outcome_distribution/average_composite_score
    // below are already computed from the stub-excluded `all_sessions` list, so total_sessions
    // and the other aggregates must exclude stubs too or every percentage derived from them
    // (e.g. VerdictBoard's ladder bars, dividing an outcome count by total_sessions) silently
    // corrupts against an inflated denominator. See docs/DECISION-INBOX.md,
    // stats/stub-count-mismatch entry.
    let (stats_res, top_repos_res, all_sessions_res) = tokio::task::spawn_blocking(move || {
        (
            storage.get_aggregate_stats(false),
            storage.get_top_repositories(),
            storage.list_sessions_filtered(&SessionFilter {
                limit: None,
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
///
/// `?offset=&limit=` page the embedded `trace.events` (see `paginate_events`); with neither
/// param this returns exactly what it always has -- the full event list -- so existing callers
/// are unaffected. Score, outcomes, and recoveries are always computed from the full,
/// unsliced trace first, since detection accuracy shouldn't depend on which page was requested.
async fn get_trace_by_id_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(page): Query<EventsPageQuery>,
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

    let mut trace = trace_res.map_err(|e| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("Trace session '{}' not found: {}", id, e) })),
        )
    })?;

    let scorer = TraceScorer::default();
    let score = scorer.score(&trace);
    let outcomes = OutcomeDetector::new().detect_outcomes(&trace);
    let recoveries = RecoveryDetector::new().detect_recoveries(&trace);

    let (events, events_total, events_offset) =
        paginate_events(std::mem::take(&mut trace.events), page.offset, page.limit).map_err(
            |e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))),
        )?;
    trace.events = events;

    Ok(Json(TraceDetailResponse {
        trace,
        score,
        outcomes,
        recoveries,
        events_total,
        events_offset,
    }))
}

/// GET /api/traces/:id/events -> just the paginated event slice, for a dashboard to fetch
/// lazily after the initial `/api/traces/:id` load instead of ever re-fetching the (potentially
/// huge) trace metadata/score/outcomes just to page through events.
async fn get_trace_events_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(page): Query<EventsPageQuery>,
) -> Result<Json<EventsPageResponse>, (StatusCode, Json<serde_json::Value>)> {
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

    let (events, events_total, events_offset) =
        paginate_events(trace.events, page.offset, page.limit)
            .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;

    Ok(Json(EventsPageResponse {
        events,
        events_total,
        events_offset,
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
        let cost_basis = crate::cost_basis::CostBasis::detect();
        Ok::<_, anyhow::Error>(UsageResponse {
            daily,
            weekly,
            monthly,
            cost_basis: cost_basis.cost_basis,
            subscription_tier: cost_basis.subscription_tier,
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

/// Curated display metadata for the coverage matrix: a friendlier display name, the file
/// formats a human recognizes, and hand-verified capability flags. This is presentation
/// detail that doesn't belong on `AgentAdapter` itself, so it's kept here as a lookup keyed
/// by `AgentAdapter::name()` rather than folded into the trait.
struct AdapterDisplayMeta {
    display_name: &'static str,
    formats: &'static [&'static str],
    token_accounting: bool,
    cache_breakdown: bool,
    tool_calls: bool,
    file_actions: bool,
    shell_commands: bool,
    model_switches: bool,
    thinking_blocks: bool,
    error_recovery: bool,
}

/// Metadata for an adapter this table hasn't been updated for yet. A newly-registered
/// adapter (`agentworth_adapters::all_adapters()`) still gets a row in the matrix with this
/// fallback rather than being silently absent -- that silent absence, for a hand-copied
/// adapter list instead of a lookup miss, was the root cause behind the "antigravity" rows
/// vanishing from the matrix entirely. Capability flags fall back to `capabilities()`, which
/// covers 4 of the 8 columns directly (token_accounting/tool_calls/shell_commands/
/// thinking_blocks); the remaining 4 (cache_breakdown/file_actions/model_switches/
/// error_recovery) default to the capability that most overlaps them (diffs, prompts, and
/// `true`) since there's no dedicated flag for them yet.
fn default_matrix_meta(adapter: &dyn AgentAdapter) -> AdapterDisplayMeta {
    let caps = adapter.capabilities();
    AdapterDisplayMeta {
        display_name: adapter.name(),
        formats: &[],
        token_accounting: caps.tokens,
        cache_breakdown: caps.tokens,
        tool_calls: caps.tools,
        file_actions: caps.diffs,
        shell_commands: caps.shell,
        model_switches: true,
        thinking_blocks: caps.thinking,
        error_recovery: true,
    }
}

fn adapter_display_meta(adapter: &dyn AgentAdapter) -> AdapterDisplayMeta {
    match adapter.name() {
        "claude_code" => AdapterDisplayMeta {
            display_name: "Claude Code",
            formats: &["jsonl", "json"],
            token_accounting: true,
            cache_breakdown: true,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        "codex" => AdapterDisplayMeta {
            display_name: "Codex",
            formats: &["jsonl"],
            token_accounting: true,
            cache_breakdown: false,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        "gemini" => AdapterDisplayMeta {
            display_name: "Gemini / Antigravity",
            formats: &["jsonl", "json"],
            token_accounting: true,
            cache_breakdown: true,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        "opencode" => AdapterDisplayMeta {
            display_name: "OpenCode",
            formats: &["jsonl", "db"],
            token_accounting: true,
            cache_breakdown: true,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        "cursor" => AdapterDisplayMeta {
            display_name: "Cursor Composer",
            formats: &["jsonl", "vscdb"],
            token_accounting: true,
            cache_breakdown: false,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        "cline" => AdapterDisplayMeta {
            display_name: "Cline",
            formats: &["json"],
            token_accounting: true,
            cache_breakdown: true,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        "windsurf" => AdapterDisplayMeta {
            display_name: "Windsurf Cascade",
            formats: &["json", "jsonl"],
            token_accounting: true,
            cache_breakdown: false,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        "aider" => AdapterDisplayMeta {
            display_name: "Aider",
            formats: &["chat.history.md", "jsonl"],
            token_accounting: true,
            cache_breakdown: false,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: false,
            error_recovery: true,
        },
        "deepseek" => AdapterDisplayMeta {
            display_name: "DeepSeek Coder",
            formats: &["jsonl"],
            token_accounting: true,
            cache_breakdown: true,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        "grok" => AdapterDisplayMeta {
            display_name: "Grok / xAI",
            formats: &["jsonl"],
            token_accounting: true,
            cache_breakdown: false,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        "kimi" => AdapterDisplayMeta {
            display_name: "Kimi K1.5",
            formats: &["jsonl"],
            token_accounting: true,
            cache_breakdown: true,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        "minimax" => AdapterDisplayMeta {
            display_name: "MiniMax",
            formats: &["jsonl"],
            token_accounting: true,
            cache_breakdown: false,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        "qwen" => AdapterDisplayMeta {
            display_name: "Qwen / Alibaba",
            formats: &["jsonl"],
            token_accounting: true,
            cache_breakdown: true,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        "zhipu" => AdapterDisplayMeta {
            display_name: "GLM / Zhipu",
            formats: &["jsonl"],
            token_accounting: true,
            cache_breakdown: false,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        "goose" => AdapterDisplayMeta {
            display_name: "Goose",
            formats: &["jsonl", "db"],
            token_accounting: true,
            cache_breakdown: false,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        "manus" => AdapterDisplayMeta {
            display_name: "Manus",
            formats: &["jsonl"],
            token_accounting: true,
            cache_breakdown: false,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        "herdr" => AdapterDisplayMeta {
            display_name: "Herdr",
            formats: &["jsonl"],
            token_accounting: true,
            cache_breakdown: false,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        "hermes" => AdapterDisplayMeta {
            display_name: "Hermes Agent",
            formats: &["jsonl"],
            token_accounting: true,
            cache_breakdown: false,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        "openclaw" => AdapterDisplayMeta {
            display_name: "OpenClaw",
            formats: &["jsonl"],
            token_accounting: true,
            cache_breakdown: false,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        "pi" => AdapterDisplayMeta {
            display_name: "Pi / Inflection",
            formats: &["jsonl"],
            token_accounting: true,
            cache_breakdown: false,
            tool_calls: true,
            file_actions: true,
            shell_commands: true,
            model_switches: true,
            thinking_blocks: true,
            error_recovery: true,
        },
        _ => default_matrix_meta(adapter),
    }
}

/// Compute capability and detection matrix across all registered adapters.
///
/// `pub(crate)` rather than private: `apps/cli/src/mcp/server.rs`'s `coverage_stats` tool
/// reuses this exact computation for its `include_matrix` option instead of duplicating the
/// 20-adapter definition table.
///
/// Derives its adapter list from `agentworth_adapters::all_adapters()` -- the same registry
/// `Scanner::new` uses -- instead of a hand-copied `Vec` here, so a newly-registered adapter
/// automatically gets a row. `sessions_count` sums over every name in the adapter's
/// `identity_names()`, not just its own `name()`: an adapter that files sessions under more
/// than one product identity (Gemini's "gemini" vs. "antigravity") would otherwise have most
/// of its rows silently excluded from its own matrix entry, because they're keyed in
/// storage by the identity `parse()` assigned, not by the adapter struct's `name()`. This
/// was measured against the real index: 785 sessions tagged "antigravity" were previously
/// invisible to both `agentworth matrix` and `/api/matrix`.
pub(crate) fn compute_adapter_matrix(storage: &Storage) -> AdapterMatrixResponse {
    // false: per-adapter `sessions_count` below should agree with what `/api/traces?adapter=X`
    // reports for the same adapter, which is stub-excluded by default.
    let stats = storage.get_aggregate_stats(false).unwrap_or_default();
    let scan_opts = ScanOptions::default();

    let adapter_impls = agentworth_adapters::all_adapters();

    let mut detected_count = 0;
    let mut adapters = Vec::new();

    for adapter_impl in adapter_impls {
        let meta = adapter_display_meta(adapter_impl.as_ref());

        let is_detected = adapter_impl
            .detect(&scan_opts)
            .map(|d| d.is_present)
            .unwrap_or(false);
        if is_detected {
            detected_count += 1;
        }

        let identities = adapter_impl.identity_names();
        let sess_count: usize = identities
            .iter()
            .map(|identity| stats.sessions_by_adapter.get(*identity).copied().unwrap_or(0))
            .sum();

        adapters.push(AdapterMatrixItem {
            adapter: adapter_impl.name().to_string(),
            name: meta.display_name.to_string(),
            detected: is_detected,
            sessions_count: sess_count,
            identities: identities.into_iter().map(String::from).collect(),
            formats: meta.formats.iter().map(|s| s.to_string()).collect(),
            token_accounting: meta.token_accounting,
            cache_breakdown: meta.cache_breakdown,
            tool_calls: meta.tool_calls,
            file_actions: meta.file_actions,
            shell_commands: meta.shell_commands,
            model_switches: meta.model_switches,
            thinking_blocks: meta.thinking_blocks,
            error_recovery: meta.error_recovery,
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
        ..Default::default()
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
    let content = match format.to_lowercase().as_str() {
        "atif" => agentworth_export_atif::export_to_atif(&trace, true).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("ATIF export failed: {}", e) })),
            )
        })?,
        "svg" => {
            let scorer = TraceScorer::default();
            let score = scorer.score(&trace);
            crate::commands::receipt::render_svg_receipt(&trace, &score)
        }
        "receipt" | "terminal" | "ansi" => {
            let scorer = TraceScorer::default();
            let score = scorer.score(&trace);
            crate::commands::receipt::render_terminal_receipt(&trace, &score)
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{Provenance, TokenUsage};
    use chrono::{Duration, Utc};
    use tempfile::NamedTempFile;

    /// Regression test for the old `Some(10000)` cap on get_stats_handler's
    /// `list_sessions_filtered` call. Seeds more non-stub sessions than that old cap and
    /// asserts total_sessions and the outcome_distribution buckets account for every one of
    /// them. Before the fix, outcome_distribution (and average_composite_score) silently
    /// reflected only the first 10,000 sessions while total_sessions -- computed separately via
    /// the always-unbounded get_aggregate_stats -- kept reporting the true, larger count. The
    /// fixture size (10,050) is deliberately chosen to exceed the old cap: a smaller fixture
    /// would pass on both the old and new code and wouldn't actually exercise the fix.
    #[tokio::test]
    async fn test_get_stats_handler_scans_beyond_old_10000_cap() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Storage::open_path(tmp.path()).unwrap();
        let start = Utc::now();

        const SESSION_COUNT: i64 = 10_050;
        for i in 0..SESSION_COUNT {
            let prov = Provenance::new(
                format!("/test/stats_cap_{}.jsonl", i),
                "claude_code",
                10,
                100,
                format!("fp_stats_cap_{}", i),
            );
            let mut trace = AgentWorthTrace::new(
                format!("sess-stats-cap-{}", i),
                "claude_code",
                prov,
                start + Duration::seconds(i),
            );
            // Non-stub: list_sessions_filtered's default excludes total_events <= 1 or
            // total_tokens <= 0.
            trace.stats.total_events = 2;
            trace.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
            storage.upsert_trace(&trace).unwrap();
        }

        let storage = Arc::new(storage);
        let scanner = Arc::new(Scanner::new(storage.clone()));
        let (live_tail_tx, _live_tail_rx) = broadcast::channel::<LiveTailEvent>(16);
        let state = AppState {
            storage,
            scanner,
            dist_dir: None,
            live_tail: live_tail_tx,
        };

        let response = get_stats_handler(State(state))
            .await
            .expect("get_stats_handler should succeed")
            .0;

        let total_sessions = response["total_sessions"]
            .as_u64()
            .expect("total_sessions should be present and numeric");
        assert_eq!(
            total_sessions, SESSION_COUNT as u64,
            "total_sessions should reflect every seeded session"
        );

        let outcome_dist = response["outcome_distribution"]
            .as_object()
            .expect("outcome_distribution should be a JSON object");

        // Every seeded session has no primary_outcome (upsert_trace leaves it NULL), so they
        // should all land in the "unresolved" bucket.
        let unresolved = outcome_dist
            .get("unresolved")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        assert_eq!(
            unresolved, SESSION_COUNT as u64,
            "every seeded session should land in the 'unresolved' bucket -- a lower count means \
             get_stats_handler silently dropped sessions past its old 10,000 cap"
        );

        let scanned: u64 = outcome_dist.values().filter_map(|v| v.as_u64()).sum();
        assert_eq!(
            scanned, SESSION_COUNT as u64,
            "get_stats_handler must scan every session, not silently cap at 10,000"
        );
    }

    /// Regression test for the `/api/stats` stub-count mismatch (docs/DECISION-INBOX.md,
    /// stats/stub-count-mismatch entry): `get_aggregate_stats` used to run an unconditional
    /// `COUNT(*)` over every row including stubs, while this same handler's own
    /// `outcome_distribution` (computed from a `list_sessions_filtered` call a few lines below
    /// it) already excluded them by default -- so `/api/stats` reported a `total_sessions` that
    /// disagreed with its own `outcome_distribution` bucket sum, and every percentage the
    /// frontend derives by dividing an outcome count by `total_sessions` (VerdictBoard's ladder
    /// bars) was silently corrupted against an inflated denominator. Seeds a fixture DB with a
    /// mix of stub and real sessions -- including a stub carrying a "verified" outcome, so a fix
    /// that only touches `total_sessions` and misses `verified_outcomes_count` still fails this
    /// test -- and asserts every population-sized number in the `/api/stats` response agrees
    /// with `list_sessions_filtered`'s own stub-excluded count for the same DB.
    #[tokio::test]
    async fn test_stats_handler_total_sessions_matches_non_stub_population() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Storage::open_path(tmp.path()).unwrap();
        let start = Utc::now();

        // Two stubs -- one below the event floor, one with zero tokens -- neither should count
        // toward any population-sized field in the /api/stats response.
        let stub_specs: &[(&str, i64, u64, Option<&str>)] = &[
            ("stub_low_events", 1, 500, None),
            ("stub_zero_tokens", 5, 0, Some("commit_observed")),
        ];
        for (id, events, tokens, outcome) in stub_specs.iter().copied() {
            let prov = Provenance::new(format!("/test/{id}.jsonl"), "claude_code", 10, 100, format!("fp_{id}"));
            let mut trace = AgentWorthTrace::new(id, "claude_code", prov, start);
            trace.stats.total_events = events as usize;
            trace.stats.token_usage = TokenUsage::new(tokens, 0, 0, 0);
            storage.upsert_session(&trace, outcome, outcome.map(|_| 0.9), 1).unwrap();
        }

        // Five real sessions, two of them verified.
        const REAL_COUNT: i64 = 5;
        let real_outcomes: [Option<&str>; 5] = [
            Some("ci_or_deployment_verified"),
            Some("commit_observed"),
            None,
            Some("done_claimed"),
            None,
        ];
        for i in 0..REAL_COUNT {
            let prov = Provenance::new(
                format!("/test/stats_pop_{i}.jsonl"),
                "claude_code",
                10,
                100,
                format!("fp_stats_pop_{i}"),
            );
            let mut trace = AgentWorthTrace::new(
                format!("sess-stats-pop-{i}"),
                "claude_code",
                prov,
                start + Duration::seconds(i),
            );
            trace.stats.total_events = 10;
            trace.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
            let outcome = real_outcomes[i as usize];
            storage.upsert_session(&trace, outcome, outcome.map(|_| 0.8), 1).unwrap();
        }

        let storage = Arc::new(storage);

        // The ground truth this test holds /api/stats to: whatever list_sessions_filtered's own
        // stub-excluded default reports for this exact DB (the same population /api/traces and
        // the CLI's `traces` command use).
        let non_stub_sessions = storage
            .list_sessions_filtered(&SessionFilter {
                limit: None,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            non_stub_sessions.len(),
            REAL_COUNT as usize,
            "fixture sanity check: exactly the 5 real sessions should pass the non-stub filter"
        );

        let scanner = Arc::new(Scanner::new(storage.clone()));
        let (live_tail_tx, _live_tail_rx) = broadcast::channel::<LiveTailEvent>(16);
        let state = AppState {
            storage,
            scanner,
            dist_dir: None,
            live_tail: live_tail_tx,
        };

        let response = get_stats_handler(State(state))
            .await
            .expect("get_stats_handler should succeed")
            .0;

        let total_sessions = response["total_sessions"]
            .as_u64()
            .expect("total_sessions should be present and numeric");
        assert_eq!(
            total_sessions,
            non_stub_sessions.len() as u64,
            "total_sessions must match list_sessions_filtered's default (non-stub) count, not a \
             raw unfiltered COUNT(*) that includes the 2 seeded stubs"
        );

        let outcome_dist = response["outcome_distribution"]
            .as_object()
            .expect("outcome_distribution should be a JSON object");
        let dist_sum: u64 = outcome_dist.values().filter_map(|v| v.as_u64()).sum();
        assert_eq!(
            dist_sum, total_sessions,
            "outcome_distribution's own bucket sum must equal total_sessions in the same response"
        );

        let verified_outcomes_count = response["verified_outcomes_count"]
            .as_u64()
            .expect("verified_outcomes_count should be present and numeric");
        assert_eq!(
            verified_outcomes_count, 2,
            "only the two real verified sessions (ci_or_deployment_verified, commit_observed) \
             should count -- the stub carrying commit_observed must be excluded even though it's \
             a 'verified' outcome kind, or the verified/total ratio would mix two different \
             populations"
        );
    }

    /// Regression test for the "antigravity" join bug (docs/capability-matrix.md, #71):
    /// `GeminiAdapter::parse` tags a subset of its sessions "antigravity" rather than
    /// "gemini" (see `detect_product_identity`), but the matrix used to join
    /// `stats.sessions_by_adapter` on `adapter.name()` alone ("gemini"), so every
    /// "antigravity" row was silently absent from both the matrix and its session counts.
    /// Seeds a fixture index with every adapter name the real index is known to carry,
    /// including "antigravity", and asserts every one of them is accounted for in the
    /// response: findable via some item's `identities`, and folded into that item's
    /// `sessions_count` so no session is dropped.
    #[tokio::test]
    async fn test_compute_adapter_matrix_accounts_for_every_adapter_name_in_fixture() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Storage::open_path(tmp.path()).unwrap();
        let start = Utc::now();

        // (adapter name stored on the session row, how many sessions to seed under it)
        let fixture: &[(&str, usize)] = &[
            ("claude_code", 3),
            ("codex", 2),
            ("gemini", 4),
            ("antigravity", 785), // the exact bug: this adapter name has no `AgentAdapter`
                                  // whose `name()` equals it -- only `identity_names()` on
                                  // the Gemini adapter should surface it.
        ];

        for (adapter_name, count) in fixture {
            for i in 0..*count {
                let prov = Provenance::new(
                    format!("/test/matrix_fixture/{}_{}.jsonl", adapter_name, i),
                    *adapter_name,
                    10,
                    100,
                    format!("fp_{}_{}", adapter_name, i),
                );
                let mut trace = AgentWorthTrace::new(
                    format!("sess-{}-{}", adapter_name, i),
                    *adapter_name,
                    prov,
                    start + Duration::seconds(i as i64),
                );
                trace.stats.total_events = 2;
                trace.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
                storage.upsert_trace(&trace).unwrap();
            }
        }

        let matrix = compute_adapter_matrix(&storage);

        for (adapter_name, count) in fixture {
            let owning_item = matrix
                .adapters
                .iter()
                .find(|item| item.identities.iter().any(|id| id == adapter_name))
                .unwrap_or_else(|| {
                    panic!(
                        "adapter name '{}' present in the fixture index has no matrix item \
                         claiming it in `identities` -- it would be invisible to any consumer \
                         of /api/matrix",
                        adapter_name
                    )
                });

            assert!(
                owning_item.sessions_count >= *count,
                "matrix item '{}' (identities: {:?}) has sessions_count {} but the fixture \
                 seeded {} sessions under adapter name '{}' alone -- those rows were dropped \
                 from the count",
                owning_item.adapter,
                owning_item.identities,
                owning_item.sessions_count,
                count,
                adapter_name
            );
        }

        // The defining case: "gemini" and "antigravity" are two identities of the same
        // registered adapter, so they must land on the same matrix row and its
        // sessions_count must be their sum (4 + 785), not just one or the other.
        let gemini_item = matrix
            .adapters
            .iter()
            .find(|item| item.adapter == "gemini")
            .expect("gemini adapter must have a matrix row");
        assert_eq!(
            gemini_item.sessions_count, 4 + 785,
            "gemini's matrix row must count both 'gemini' and 'antigravity' sessions"
        );
        assert!(gemini_item.identities.iter().any(|id| id == "gemini"));
        assert!(gemini_item.identities.iter().any(|id| id == "antigravity"));
    }
}
