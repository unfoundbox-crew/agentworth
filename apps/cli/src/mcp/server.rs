//! `archie mcp`: the read-only MCP tool surface over stdio, sharing the same `Storage` and
//! `Scanner` wiring `apps/cli/src/server/routes.rs` builds for the HTTP API (see `AppState`
//! there). Nothing here writes to the index or to original session logs -- every tool is a
//! read-only wrapper around a `Storage`/`Scanner` call the HTTP surface already exposes.
//!
//! Redaction policy (`docs/specs/mcp-server.md`, "What it must not expose"): redacted is the
//! default for every tool that can carry event, file, or path content; raw is an explicit
//! per-call opt-in (`session_show`'s `include_raw`), never a server-wide switch.

use std::sync::Arc;

use agentworth_core::Scanner;
use agentworth_outcomes::{OutcomeDetector, RecoveryDetector};
use agentworth_redaction::{repository_identity_rule, Redactor};
use agentworth_schema::extract_repository_or_workspace;
use agentworth_scoring::TraceScorer;
// `OUTCOME_RATE_DEFAULT_MIN_N` is defined once in storage, beside the query that applies it,
// so the CLI, `stats_outcomes` and `stats_ladder` cannot drift apart on the sample floor.
use agentworth_storage::{SessionFilter, Storage, OUTCOME_RATE_DEFAULT_MIN_N};

use crate::commands::suspect;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::transport::stdio;
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt};
use serde_json::json;

use super::params::{
    parse_rfc3339_opt, BlameFindParams, CarryForwardParams, CoverageStatsParams,
    ForgottenContextParams, LadderParams, OutcomeRateParams, PacingWindowParams, SessionAsksParams,
    SessionGetParams, SessionHandoffParams, SessionsFindParams, SuspectCommitsParams,
    UsagePeriodParam, UsageSummaryParams,
};
use crate::asks::{self, AsksOptions, AsksReport};
use crate::forgotten::{self, ForgottenOptions, ForgottenReport};
use crate::handoff::{
    self, render_markdown, HandoffOptions, HandoffReport, DEFAULT_MAX_LINES, MAX_LINES_CEILING,
};

/// Hard ceiling on `session_list`'s `limit`, so a remote model is forced to state how much
/// it's asking for instead of getting a silently truncated "complete-looking" answer -- the
/// exact trap `/api/traces`'s default-50 behavior already shipped once (`AGENTS.md`, "Things
/// you cannot learn from the code," item 3).
const SESSIONS_FIND_LIMIT_CEILING: usize = 200;

/// How much further than `limit` to over-fetch when `repo` is set, since `repo` isn't a stored
/// column and has to be post-filtered client-side after a bounded fetch (see
/// `docs/specs/mcp-server.md`'s `session_list` "repo is not a stored column" note).
const REPO_OVERFETCH_MULTIPLIER: usize = 4;

/// Hard ceiling on `session_carry_forward`'s `n`, matching `docs/specs/handoff.md`. Ten handoffs is
/// already more history than a session opening turn can use; past that the caller wants
/// `session_list`.
const CARRY_FORWARD_CEILING: usize = 10;

/// The `archie mcp` tool server. Cheap to construct -- `Scanner::new` only builds the
/// adapter list, it does no I/O -- so a fresh instance per `run_mcp_server` call is fine.
#[derive(Clone)]
pub struct AgentWorthMcpServer {
    storage: Arc<Storage>,
    scanner: Arc<Scanner>,
    // Read only by the dispatch code `#[tool_handler]` generates on the `ServerHandler` impl
    // below, which rustc's dead_code analysis doesn't trace through -- the upstream rmcp
    // examples hit the same false positive (see `counter.rs`'s file-level `#![allow(dead_code)]`
    // in the official SDK repo). Scoped to just this field rather than the whole module.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl AgentWorthMcpServer {
    pub fn new(storage: Arc<Storage>) -> Self {
        let scanner = Arc::new(Scanner::new(storage.clone()));
        Self {
            storage,
            scanner,
            tool_router: Self::tool_router(),
        }
    }

    /// Redacts a bare path string using the same repository-identity-augmented rule set
    /// `Redactor::redact_trace` applies to `trace.provenance.source_path` -- built directly via
    /// `repository_identity_rule` (rather than `Redactor::for_trace`, which needs a whole
    /// `AgentWorthTrace`) since `session_list`/`repo_blame` only ever see bare path strings
    /// spanning many different sessions, not a single trace.
    fn redact_path(path: &str) -> String {
        let mut redactor = Redactor::new();
        let identity = extract_repository_or_workspace(path);
        if let Some(rule) = repository_identity_rule(&identity) {
            redactor.add_rule(rule);
        }
        redactor.redact_text(path)
    }

    fn json_result<T: serde::Serialize>(value: &T) -> Result<CallToolResult, McpError> {
        let text = serde_json::to_string_pretty(value).map_err(|e| {
            McpError::internal_error(format!("failed to serialize tool result: {e}"), None)
        })?;
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
    }

    fn join_error(e: tokio::task::JoinError) -> McpError {
        McpError::internal_error(format!("tool task panicked or was cancelled: {e}"), None)
    }

    /// Loads and renders one session's handoff, applying the redaction default.
    ///
    /// Shared by `session_handoff` and `session_carry_forward` so the two can't drift on the thing
    /// that matters most here: `include_raw: false` builds one `Redactor::for_trace` instance
    /// from the session's own trace and runs every field of the report through that same
    /// instance, which is what makes the session's repository identity get masked across
    /// paths, commands and quoted sentences rather than only one of them.
    fn render_one(
        storage: &Storage,
        scanner: &Scanner,
        session_id: &str,
        max_lines: usize,
        options: HandoffOptions,
        include_raw: bool,
    ) -> anyhow::Result<serde_json::Value> {
        let (report, trace) = handoff::load_handoff(storage, scanner, session_id, options)?;
        let report: HandoffReport = if include_raw {
            report
        } else {
            report.redacted(&Redactor::new().for_trace(&trace))
        };

        Ok(json!({
            "markdown": render_markdown(&report, max_lines),
            "receipt": report.receipt,
            "gaps": report.gaps,
        }))
    }

    /// Validates a caller-supplied line budget rather than clamping it.
    ///
    /// Out of range is an error, not a silent clamp -- the same choice `session_list`'s
    /// `limit` already makes, and for the same reason: a clamped answer looks complete.
    fn validated_max_lines(max_lines: Option<usize>) -> Result<usize, McpError> {
        match max_lines {
            None => Ok(DEFAULT_MAX_LINES),
            Some(n) if n == 0 || n > MAX_LINES_CEILING => Err(McpError::invalid_params(
                format!("max_lines must be between 1 and {MAX_LINES_CEILING} (got {n})"),
                None,
            )),
            Some(n) => Ok(n),
        }
    }

    /// The repository this server process is running in, used when `session_handoff` is called
    /// with no `session_id`. Derived the same way every other repo key in the product is, so a
    /// worktree resolves to the repo it belongs to rather than to itself.
    fn cwd_repo() -> Option<String> {
        let cwd = std::env::current_dir().ok()?;
        Some(extract_repository_or_workspace(&cwd.to_string_lossy()))
    }

    /// The session a tool means when the caller names none: the newest one indexed for the repo
    /// this server runs in. Shared by `session_handoff` and `session_forgotten` so both fail
    /// with the same sentence when there is nothing to fall back to.
    fn newest_session_for_cwd(storage: &Storage) -> anyhow::Result<String> {
        let repo = Self::cwd_repo().ok_or_else(|| {
            anyhow::anyhow!(
                "no session_id given and this server's working directory could not be read; \
                 pass session_id explicitly"
            )
        })?;
        storage
            .list_sessions_for_repo(&repo, 1, false)?
            .sessions
            .into_iter()
            .next()
            .map(|s| s.session_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no indexed session for repo '{repo}' (this server's working directory); \
                     pass session_id explicitly, or run `archie scan`"
                )
            })
    }
}

// `vis = "pub(crate)"`: `archie docs` (apps/cli/src/commands/docs.rs) calls
// `Self::tool_router().list_all()` from another module to read every tool's name,
// description, and JSON schema without constructing a live server instance -- building a
// `ToolRouter` doesn't touch `Storage`/`Scanner` at all, it only type-erases the handlers.
#[tool_router(vis = "pub(crate)")]
impl AgentWorthMcpServer {
    #[tool(
        description = "Find sessions by adapter, model, outcome, search text, date range, token \
                        floor, or derived repo/workspace name. `limit` is required with a hard \
                        ceiling of 200 -- there is no silent default, so state how many results \
                        you want. Returns summaries only (no event content); `source_path` is \
                        redacted."
    )]
    pub(crate) async fn session_list(
        &self,
        Parameters(params): Parameters<SessionsFindParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.limit == 0 || params.limit > SESSIONS_FIND_LIMIT_CEILING {
            return Err(McpError::invalid_params(
                format!(
                    "limit must be between 1 and {SESSIONS_FIND_LIMIT_CEILING} (got {})",
                    params.limit
                ),
                None,
            ));
        }

        let start_date = parse_rfc3339_opt(params.start_date.as_deref())
            .map_err(|e| McpError::invalid_params(e, None))?;
        let end_date = parse_rfc3339_opt(params.end_date.as_deref())
            .map_err(|e| McpError::invalid_params(e, None))?;

        let repo = params.repo.clone();
        let fetch_limit = if repo.is_some() {
            params
                .limit
                .saturating_mul(REPO_OVERFETCH_MULTIPLIER)
                .min(SESSIONS_FIND_LIMIT_CEILING * REPO_OVERFETCH_MULTIPLIER)
        } else {
            params.limit
        };

        let filter = SessionFilter {
            adapter: params.adapter.clone(),
            model: params.model.clone(),
            search: params.search.clone(),
            start_date,
            end_date,
            min_tokens: params.min_tokens,
            limit: Some(fetch_limit),
            offset: params.offset,
            order_by: Some(params.order_by.map(Into::into).unwrap_or_default()),
            include_stubs: params.include_stubs,
            outcome: params.outcome.clone(),
        };

        let storage = self.storage.clone();
        let sessions = tokio::task::spawn_blocking(move || storage.list_sessions_filtered(&filter))
            .await
            .map_err(Self::join_error)?
            .map_err(|e| McpError::internal_error(format!("session_list query failed: {e}"), None))?;

        let fetched_len = sessions.len();
        let mut truncated = repo.is_some() && fetched_len >= fetch_limit;

        let mut filtered: Vec<_> = match &repo {
            Some(r) => sessions
                .into_iter()
                .filter(|s| extract_repository_or_workspace(&s.source_path) == *r)
                .collect(),
            None => sessions,
        };

        if filtered.len() > params.limit {
            filtered.truncate(params.limit);
            truncated = true;
        }

        for s in &mut filtered {
            s.source_path = Self::redact_path(&s.source_path);
        }

        Self::json_result(&json!({
            "sessions": filtered,
            "truncated": truncated,
        }))
    }

    #[tool(
        description = "Get full detail for one session by ID: the trace, its 5-component \
                        TraceScore, outcome evidence, and recovery signals -- the same shape \
                        /api/traces/:id returns. Redacted by default (trace events, outcome \
                        summaries, and recovery summaries all pass through the redaction \
                        engine); pass include_raw=true for the unredacted trace. \
                        `trace.events` is paginated: events_offset (default 0) and \
                        events_limit (default 500, must be > 0) page through it, and the \
                        response's events_total says how many events the session actually has, \
                        so a large session is never returned in full by accident."
    )]
    pub(crate) async fn session_show(
        &self,
        Parameters(params): Parameters<SessionGetParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.events_limit == Some(0) {
            return Err(McpError::invalid_params(
                "events_limit must be greater than 0".to_string(),
                None,
            ));
        }

        let scanner = self.scanner.clone();
        let session_id_for_task = params.session_id.clone();
        let mut trace = tokio::task::spawn_blocking(move || scanner.load_trace(&session_id_for_task))
            .await
            .map_err(Self::join_error)?
            .map_err(|e| {
                McpError::resource_not_found(
                    format!("session '{}' not found: {e}", params.session_id),
                    None,
                )
            })?;

        let scorer = TraceScorer::default();
        let score = scorer.score(&trace);
        let outcomes = OutcomeDetector::new().detect_outcomes(&trace);
        let recoveries = RecoveryDetector::new().detect_recoveries(&trace);

        let events_limit = Some(
            params
                .events_limit
                .unwrap_or(super::params::SESSION_GET_DEFAULT_EVENTS_LIMIT),
        );
        let (events, events_total, events_offset) = crate::server::routes::paginate_events(
            std::mem::take(&mut trace.events),
            params.events_offset,
            events_limit,
        )
        .map_err(|e| McpError::invalid_params(e, None))?;
        trace.events = events;

        let (trace, outcomes, recoveries) = if params.include_raw {
            (trace, outcomes, recoveries)
        } else {
            // Same (for_trace-augmented) redactor instance for all three, so the trace's own
            // repository/workspace identity gets masked consistently across trace, outcomes,
            // and recoveries rather than only on the trace object. See
            // docs/specs/mcp-server.md's session_show redaction note and
            // Redactor::for_trace's own doc comment.
            let redactor = Redactor::new().for_trace(&trace);
            (
                redactor.redact_trace(&trace),
                redactor.redact_outcome_evidence(&outcomes),
                redactor.redact_recovery_signal(&recoveries),
            )
        };

        Self::json_result(&json!({
            "trace": trace,
            "score": score,
            "outcomes": outcomes,
            "recoveries": recoveries,
            "events_total": events_total,
            "events_offset": events_offset,
        }))
    }

    #[tool(
        description = "Find sessions whose recorded file modifications match a substring of \
                        file_path -- AI Code Blame, the same query /api/blame makes. Returned \
                        paths are redacted."
    )]
    pub(crate) async fn repo_blame(
        &self,
        Parameters(params): Parameters<BlameFindParams>,
    ) -> Result<CallToolResult, McpError> {
        let storage = self.storage.clone();
        let pattern = params.file_path.clone();
        let mut matches = tokio::task::spawn_blocking(move || storage.find_sessions_for_blame(&pattern))
            .await
            .map_err(Self::join_error)?
            .map_err(|e| McpError::internal_error(format!("repo_blame query failed: {e}"), None))?;

        for m in &mut matches {
            m.source_path = Self::redact_path(&m.source_path);
            m.file_path = Self::redact_path(&m.file_path);
        }

        Self::json_result(&matches)
    }

    #[tool(
        description = "Daily, weekly, or monthly usage rollups: session counts, token \
                        breakdown, estimated cost, and cache hit ratio, grouped by adapter -- \
                        the same rollups /api/usage returns for one period at a time. Every \
                        estimated_cost_usd is an API list-price equivalent, not what the \
                        account actually paid -- see cost_basis/subscription_tier."
    )]
    pub(crate) async fn stats_usage(
        &self,
        Parameters(params): Parameters<UsageSummaryParams>,
    ) -> Result<CallToolResult, McpError> {
        let storage = self.storage.clone();
        let period = params.period;
        let limit = params.limit;
        let rows = tokio::task::spawn_blocking(move || match period {
            UsagePeriodParam::Day => storage.get_daily_usage(limit),
            UsagePeriodParam::Week => storage.get_weekly_usage(limit),
            UsagePeriodParam::Month => storage.get_monthly_usage(limit),
        })
        .await
        .map_err(Self::join_error)?
        .map_err(|e| McpError::internal_error(format!("stats_usage query failed: {e}"), None))?;

        let cost_basis = crate::cost_basis::CostBasis::detect();
        Self::json_result(&serde_json::json!({
            "rows": rows,
            "cost_basis": cost_basis.cost_basis,
            "subscription_tier": cost_basis.subscription_tier,
        }))
    }

    #[tool(
        description = "Rolling burn-rate window (default 5 hours): tokens/hour, active \
                        adapters and models, estimated cost, and cache hit ratio -- the same \
                        window /api/pacing computes. Answers \"what am I burning right now\"."
    )]
    pub(crate) async fn window_show(
        &self,
        Parameters(params): Parameters<PacingWindowParams>,
    ) -> Result<CallToolResult, McpError> {
        let storage = self.storage.clone();
        let hours = params.hours.unwrap_or(5);
        let pacing = tokio::task::spawn_blocking(move || storage.get_pacing_window(hours))
            .await
            .map_err(Self::join_error)?
            .map_err(|e| McpError::internal_error(format!("window_show query failed: {e}"), None))?;

        Self::json_result(&pacing)
    }

    #[tool(
        description = "Machine-wide aggregate stats: total sessions/events, token usage, \
                        sessions by adapter, model/tool usage counts, and verified-outcome \
                        count -- the same population /api/stats reports. Pass \
                        include_matrix=true to also get the per-adapter detection/capability \
                        matrix (/api/matrix's equivalent), answering \"what does this machine \
                        even have\" without opening the dashboard."
    )]
    pub(crate) async fn agent_list(
        &self,
        Parameters(params): Parameters<CoverageStatsParams>,
    ) -> Result<CallToolResult, McpError> {
        let storage = self.storage.clone();
        let include_matrix = params.include_matrix;
        let (stats_res, matrix) = tokio::task::spawn_blocking(move || {
            let stats = storage.get_aggregate_stats(false);
            let matrix = if include_matrix {
                Some(crate::server::routes::compute_adapter_matrix(&storage))
            } else {
                None
            };
            (stats, matrix)
        })
        .await
        .map_err(Self::join_error)?;

        let stats = stats_res
            .map_err(|e| McpError::internal_error(format!("agent_list query failed: {e}"), None))?;

        let mut value = serde_json::to_value(&stats).map_err(|e| {
            McpError::internal_error(format!("failed to serialize agent_list: {e}"), None)
        })?;
        if let Some(matrix) = matrix {
            if let serde_json::Value::Object(ref mut map) = value {
                let matrix_value = serde_json::to_value(&matrix).map_err(|e| {
                    McpError::internal_error(format!("failed to serialize matrix: {e}"), None)
                })?;
                map.insert("matrix".to_string(), matrix_value);
            }
        }

        let text = serde_json::to_string_pretty(&value).map_err(|e| {
            McpError::internal_error(format!("failed to serialize agent_list: {e}"), None)
        })?;
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
    }

    #[tool(
        description = "Verified-outcome rate by model, adapter, or repo: of the sessions that \
                        claimed done, what share left evidence (a passed test/build or \
                        stronger), with the sample size next to every row. Groups under min_n \
                        (default 20) are suppressed and counted in suppressed_groups rather \
                        than shown; a group with sessions but zero detected outcomes comes back \
                        as rate: null with reason: \"no_outcome_detection\" instead of being \
                        suppressed -- those are different claims. Includes a receipt \
                        (db_path, counted_at, index_last_session_at, the non-stub predicate, and \
                        the session IDs behind each row) so the answer is checkable."
    )]
    pub(crate) async fn stats_outcomes(
        &self,
        Parameters(params): Parameters<OutcomeRateParams>,
    ) -> Result<CallToolResult, McpError> {
        let since = parse_rfc3339_opt(params.since.as_deref())
            .map_err(|e| McpError::invalid_params(e, None))?;
        let until = parse_rfc3339_opt(params.until.as_deref())
            .map_err(|e| McpError::invalid_params(e, None))?;
        let min_n = params.min_n.unwrap_or(OUTCOME_RATE_DEFAULT_MIN_N);
        let include_stubs = params.include_stubs.unwrap_or(false);
        let group_by = params.group_by.into();

        let storage = self.storage.clone();
        let result = tokio::task::spawn_blocking(move || {
            storage.get_outcome_rate(group_by, since, until, min_n, include_stubs)
        })
        .await
        .map_err(Self::join_error)?
        .map_err(|e| McpError::internal_error(format!("stats_outcomes query failed: {e}"), None))?;

        Self::json_result(&result)
    }

    #[tool(
        description = "The evidence ladder over a window, in three blocks: how many sessions \
                        reached each rung (rung 0 is unflown -- no outcome evidence at all, \
                        which is not a failure) with the API-equivalent spend that sits below \
                        the evidence line; what a verified outcome costs per model, repo, \
                        adapter or effort; and the newest sessions that got past the line. \
                        A group under the sample floor returns rate: null and \
                        cost_per_verified_usd: null rather than a number nothing supports. \
                        Every dollar is API-equivalent at list prices (cost_basis says so), \
                        never what the account was billed. Reads the index only -- no \
                        transcript is reparsed. See docs/specs/archie-bench.md."
    )]
    pub(crate) async fn stats_ladder(
        &self,
        Parameters(params): Parameters<LadderParams>,
    ) -> Result<CallToolResult, McpError> {
        let period = params.period.as_deref().unwrap_or("month");
        let period = crate::app::config::normalize_period(period).ok_or_else(|| {
            McpError::invalid_params(
                format!("period must be one of day, week, month, year, all (got {period:?})"),
                None,
            )
        })?;
        let mut query = crate::commands::ladder::ladder_query(
            period,
            "model",
            params.repo,
            params.adapter,
            params.model,
            params.min_n,
            params.include_stubs.unwrap_or(false),
        );
        query.group_by = params.by.into();

        let storage = self.storage.clone();
        let result = tokio::task::spawn_blocking(move || storage.get_ladder(&query))
            .await
            .map_err(Self::join_error)?
            .map_err(|e| McpError::internal_error(format!("stats_ladder query failed: {e}"), None))?;

        let payload = crate::commands::ladder::ladder_json(&result, period)
            .map_err(|e| McpError::internal_error(format!("stats_ladder failed: {e}"), None))?;
        Self::json_result(&payload)
    }

    #[tool(
        description = "The handoff for one session, written from rows rather than by a model: \
                        what it said it would do and never did, what it said it decided, which \
                        files changed, which commands ran and with what exit code, the outcome \
                        rung reached, and how often the context was compacted. Returns \
                        markdown under a line budget (max_lines, default 60, ceiling 120), the \
                        receipt every claim traces back to, and `gaps` -- the machine-readable \
                        list of what this session could not answer, which is never padded over. \
                        Open decisions, PR/CI state and environment traps are NOT in the index \
                        and the output says so. Defaults to the newest session for the repo \
                        this server runs in. Redacted by default; include_raw=true opts out, \
                        per call."
    )]
    pub(crate) async fn session_handoff(
        &self,
        Parameters(params): Parameters<SessionHandoffParams>,
    ) -> Result<CallToolResult, McpError> {
        let max_lines = Self::validated_max_lines(params.max_lines)?;
        let options = HandoffOptions {
            include_loose_ends: params.include_loose_ends.unwrap_or(true),
            ..HandoffOptions::default()
        };

        let storage = self.storage.clone();
        let scanner = self.scanner.clone();
        let explicit_id = params.session_id.clone();
        let include_raw = params.include_raw;

        let value = tokio::task::spawn_blocking(move || -> anyhow::Result<serde_json::Value> {
            let session_id = match explicit_id {
                Some(id) => id,
                None => Self::newest_session_for_cwd(&storage)?,
            };
            Self::render_one(&storage, &scanner, &session_id, max_lines, options, include_raw)
        })
        .await
        .map_err(Self::join_error)?
        .map_err(|e| McpError::resource_not_found(format!("{e:#}"), None))?;

        Self::json_result(&value)
    }

    #[tool(
        description = "The last N handoffs for one repository, newest first, so a session's \
                        first tool call can be \"what happened here recently\" and the answer \
                        is structured rather than a file it has to find and parse. `repo` is \
                        the same key session_handoff's receipt reports (e.g. \
                        `unfoundbox/agentworth`); a repo's worktrees all answer to one value. \
                        n defaults to 3, ceiling 10. Handoffs are listed, never merged -- \
                        merging two contradictory facts needs judgment about which is current, \
                        and that is not in the index. Subagent transcripts are excluded unless \
                        include_subagents is true. Redacted by default."
    )]
    pub(crate) async fn session_carry_forward(
        &self,
        Parameters(params): Parameters<CarryForwardParams>,
    ) -> Result<CallToolResult, McpError> {
        let max_lines = Self::validated_max_lines(params.max_lines)?;
        let n = params.n.unwrap_or(3);
        if n == 0 || n > CARRY_FORWARD_CEILING {
            return Err(McpError::invalid_params(
                format!("n must be between 1 and {CARRY_FORWARD_CEILING} (got {n})"),
                None,
            ));
        }
        let since = parse_rfc3339_opt(params.since.as_deref())
            .map_err(|e| McpError::invalid_params(e, None))?;

        let storage = self.storage.clone();
        let scanner = self.scanner.clone();
        let repo = params.repo.clone();
        let include_raw = params.include_raw;
        let include_subagents = params.include_subagents;

        let value = tokio::task::spawn_blocking(move || -> anyhow::Result<serde_json::Value> {
            let page = storage.list_sessions_for_repo(&repo, n, include_subagents)?;
            let mut handoffs = Vec::new();
            let mut unreadable = Vec::new();

            for summary in page.sessions {
                if since.is_some_and(|s| summary.started_at < s) {
                    continue;
                }
                match Self::render_one(
                    &storage,
                    &scanner,
                    &summary.session_id,
                    max_lines,
                    HandoffOptions::default(),
                    include_raw,
                ) {
                    Ok(value) => handoffs.push(value),
                    // A session whose source file has been deleted or rotated away can't be
                    // re-parsed. Name it rather than dropping it: "three sessions, one of them
                    // unreadable" is a different answer from "two sessions".
                    Err(e) => unreadable.push(json!({
                        "session_id": summary.session_id,
                        "reason": format!("{e:#}"),
                    })),
                }
            }

            Ok(json!({
                "repo": repo,
                "handoffs": handoffs,
                "unreadable": unreadable,
                "scan_exhausted": page.scan_exhausted,
            }))
        })
        .await
        .map_err(Self::join_error)?
        .map_err(|e| McpError::internal_error(format!("session_carry_forward failed: {e:#}"), None))?;

        Self::json_result(&value)
    }

    #[tool(
        description = "The decisions this session's compaction rounds threw away, quoted \
                        verbatim with a receipt on each. Compaction replaces the conversation \
                        with a summary in the model's view while the full transcript stays on \
                        disk, so the dropped span and the summary that replaced it both exist \
                        and can be diffed. Measured on one real 8-round session \
                        (docs/specs/compaction-diff.md): 402 decision-shaped sentences went in \
                        and 28 came out -- conclusions survive at 15%, reasons at 1.7%, which \
                        is the shape that makes a session re-propose something it already \
                        rejected. Filter with round (1-based) and classes (decision, rejected, \
                        reason); limit defaults to 20, ceiling 200, and the totals describe the \
                        whole session regardless of it. Every statement carries its round, \
                        source sequence, and what the session did in the next few events, so a \
                        stated decision that was acted on can be told from one that was only \
                        claimed. Three answers are kept distinct and none is padded: never \
                        compacted, compacted with nothing decision-shaped dropped, and a real \
                        list. No model is involved -- three regexes return the sentence \
                        verbatim, because a paraphrase would make this a second summariser. \
                        Refuses if the raw session file is gone. Redacted by default; \
                        include_raw=true opts out, per call."
    )]
    pub(crate) async fn session_forgotten(
        &self,
        Parameters(params): Parameters<ForgottenContextParams>,
    ) -> Result<CallToolResult, McpError> {
        let limit = params.limit.unwrap_or(forgotten::DEFAULT_LIMIT);
        if limit == 0 || limit > forgotten::LIMIT_CEILING {
            return Err(McpError::invalid_params(
                format!(
                    "limit must be between 1 and {} (got {limit})",
                    forgotten::LIMIT_CEILING
                ),
                None,
            ));
        }
        let classes = forgotten::parse_classes(params.classes.as_deref().unwrap_or(&[]))
            .map_err(|e| McpError::invalid_params(format!("{e:#}"), None))?;
        let options = ForgottenOptions {
            round: params.round,
            classes,
            limit,
        };

        let storage = self.storage.clone();
        let scanner = self.scanner.clone();
        let explicit_id = params.session_id.clone();
        let include_raw = params.include_raw;

        let report = tokio::task::spawn_blocking(move || -> anyhow::Result<ForgottenReport> {
            let session_id = match explicit_id {
                Some(id) => id,
                None => Self::newest_session_for_cwd(&storage)?,
            };
            let (report, trace) =
                forgotten::load_forgotten(&storage, &scanner, &session_id, &options)?;
            Ok(if include_raw {
                report
            } else {
                report.redacted(&Redactor::new().for_trace(&trace))
            })
        })
        .await
        .map_err(Self::join_error)?
        .map_err(|e| McpError::resource_not_found(format!("{e:#}"), None))?;

        Self::json_result(&report)
    }

    #[tool(
        description = "The questions-to-answers index for one session: every question asked \
                        (a `?` sentence in a user turn, or a flag-prefixed `⚑`/`🚩` line in an \
                        assistant turn asking the user something) matched to the first \
                        substantive assistant text that follows it, before the next user turn. \
                        Exists so an agent can be told where an answer already landed instead \
                        of the user re-scrolling or re-asking. Each result carries the question \
                        (trimmed to 120 chars), a status (answered, flagged_back_to_user -- \
                        either a flag line or a reply that was itself a question, or \
                        no_reply_yet), an answer excerpt when one was found (trimmed to 200 \
                        chars), and a pointer (event sequence and timestamp) to jump to: the \
                        answer's location when there is one, otherwise the question's own. \
                        Filter with since (RFC 3339) and unanswered_only; limit defaults to 50, \
                        ceiling 500, and the totals describe the whole session regardless of \
                        it. No model is involved -- three deterministic patterns, same \
                        `regex_v1` method `session_forgotten` uses. Defaults to the newest \
                        session for the repo this server runs in. Redacted by default; \
                        include_raw=true opts out, per call."
    )]
    pub(crate) async fn session_asks(
        &self,
        Parameters(params): Parameters<SessionAsksParams>,
    ) -> Result<CallToolResult, McpError> {
        let limit = params.limit.unwrap_or(asks::DEFAULT_LIMIT);
        if limit == 0 || limit > asks::LIMIT_CEILING {
            return Err(McpError::invalid_params(
                format!(
                    "limit must be between 1 and {} (got {limit})",
                    asks::LIMIT_CEILING
                ),
                None,
            ));
        }
        let since = parse_rfc3339_opt(params.since.as_deref())
            .map_err(|e| McpError::invalid_params(e, None))?;
        let options = AsksOptions {
            since,
            unanswered_only: params.unanswered_only,
            limit,
        };

        let storage = self.storage.clone();
        let scanner = self.scanner.clone();
        let explicit_id = params.session_id.clone();
        let include_raw = params.include_raw;

        let report = tokio::task::spawn_blocking(move || -> anyhow::Result<AsksReport> {
            let session_id = match explicit_id {
                Some(id) => id,
                None => Self::newest_session_for_cwd(&storage)?,
            };
            let (report, trace) = asks::load_asks(&storage, &scanner, &session_id, &options)?;
            Ok(if include_raw {
                report
            } else {
                report.redacted(&Redactor::new().for_trace(&trace))
            })
        })
        .await
        .map_err(Self::join_error)?
        .map_err(|e| McpError::resource_not_found(format!("{e:#}"), None))?;

        Self::json_result(&report)
    }

    #[tool(
        description = "Which commits on this branch came out of a session that never proved \
                        anything. Walks `git log` over the range, joins each commit's changed \
                        paths to indexed sessions that touched them within window_hours before \
                        it, and reports each session's risk signals: no_test_run (the session \
                        never got past artifact_changed), no_outcome_detected (the adapter \
                        extracted no outcome at all -- weaker), demoted_claim (verification \
                        contradicted a claim, with the event sequence), and loop (the sentinel \
                        caught repetition). Returns a list and a copyable prompt -- never a \
                        patch, a diff, or a PR: a trajectory says the session was going badly, \
                        not what the code does wrong. Two counts are load-bearing and must be \
                        reported to the user, not dropped: `unattributed` commits had no \
                        indexed session at all (unknown, not clean), and `unanchored_blame_rows` \
                        is evidence that could not be placed in any repository. Paths and \
                        session source paths are redacted."
    )]
    pub(crate) async fn repo_suspect(
        &self,
        Parameters(params): Parameters<SuspectCommitsParams>,
    ) -> Result<CallToolResult, McpError> {
        let since = parse_rfc3339_opt(params.since.as_deref())
            .map_err(|e| McpError::invalid_params(e, None))?;
        let window_hours = params.window_hours.unwrap_or(suspect::DEFAULT_WINDOW_HOURS);
        if window_hours < 0 {
            return Err(McpError::invalid_params(
                "window_hours must not be negative".to_string(),
                None,
            ));
        }
        let max_commits = params
            .max_commits
            .unwrap_or(suspect::DEFAULT_MAX_COMMITS)
            .clamp(1, suspect::MAX_COMMITS_CEILING);

        let query = suspect::SuspectQuery {
            repo: std::path::PathBuf::from(&params.repo),
            branch: params.branch.clone(),
            base: params.base.clone(),
            since,
            window_hours,
            max_commits,
        };

        let storage = self.storage.clone();
        let mut report = tokio::task::spawn_blocking(move || {
            suspect::compute_suspect_commits(&storage, &query)
        })
        .await
        .map_err(Self::join_error)?
        // A bad repo path or an unresolvable ref is the caller's mistake, not a server fault,
        // and the message names the failing noun -- so it comes back as invalid_params.
        .map_err(|e| McpError::invalid_params(format!("repo_suspect failed: {e}"), None))?;

        report.repo = Self::redact_path(&report.repo);
        for commit in &mut report.suspect {
            for session in &mut commit.sessions {
                session.evidence_path = Self::redact_path(&session.evidence_path);
            }
        }

        Self::json_result(&report)
    }
    // -------------------------------------------------------------------------
    // Pre-0.1.16 tool names, kept registered so a client that was configured against
    // them keeps working. Each one is a two-line forward to the renamed handler, so the
    // two names can never answer differently. Removed in v0.1.20, along with the CLI
    // aliases (`crate::app::HIDDEN_ALIASES_REMOVED_IN`).
    //
    // They are listed rather than hidden: MCP has no unlisted-but-callable tool, and
    // rmcp's own `ToolRouter::disable_route` takes a tool out of `call` as well as out of
    // `list_all`. Naming the replacement in the first sentence of the description is what
    // is available; `archie docs` leaves them out of the generated reference.
    // -------------------------------------------------------------------------

    #[tool(
        name = "sessions_find",
        description = "Deprecated alias of `session_list`, removed in v0.1.20. Same parameters, \
                       same handler, same result -- use `session_list`."
    )]
    pub(crate) async fn sessions_find_alias(
        &self,
        params: Parameters<SessionsFindParams>,
    ) -> Result<CallToolResult, McpError> {
        self.session_list(params).await
    }

    #[tool(
        name = "session_get",
        description = "Deprecated alias of `session_show`, removed in v0.1.20. Same parameters, \
                       same handler, same result -- use `session_show`."
    )]
    pub(crate) async fn session_get_alias(
        &self,
        params: Parameters<SessionGetParams>,
    ) -> Result<CallToolResult, McpError> {
        self.session_show(params).await
    }

    #[tool(
        name = "blame_find",
        description = "Deprecated alias of `repo_blame`, removed in v0.1.20. Same parameters, \
                       same handler, same result -- use `repo_blame`."
    )]
    pub(crate) async fn blame_find_alias(
        &self,
        params: Parameters<BlameFindParams>,
    ) -> Result<CallToolResult, McpError> {
        self.repo_blame(params).await
    }

    #[tool(
        name = "usage_summary",
        description = "Deprecated alias of `stats_usage`, removed in v0.1.20. Same parameters, \
                       same handler, same result -- use `stats_usage`."
    )]
    pub(crate) async fn usage_summary_alias(
        &self,
        params: Parameters<UsageSummaryParams>,
    ) -> Result<CallToolResult, McpError> {
        self.stats_usage(params).await
    }

    #[tool(
        name = "pacing_window",
        description = "Deprecated alias of `window_show`, removed in v0.1.20. Same parameters, \
                       same handler, same result -- use `window_show`."
    )]
    pub(crate) async fn pacing_window_alias(
        &self,
        params: Parameters<PacingWindowParams>,
    ) -> Result<CallToolResult, McpError> {
        self.window_show(params).await
    }

    #[tool(
        name = "coverage_stats",
        description = "Deprecated alias of `agent_list`, removed in v0.1.20. Same parameters, \
                       same handler, same result -- use `agent_list`."
    )]
    pub(crate) async fn coverage_stats_alias(
        &self,
        params: Parameters<CoverageStatsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.agent_list(params).await
    }

    #[tool(
        name = "outcome_rate",
        description = "Deprecated alias of `stats_outcomes`, removed in v0.1.20. Same parameters, \
                       same handler, same result -- use `stats_outcomes`."
    )]
    pub(crate) async fn outcome_rate_alias(
        &self,
        params: Parameters<OutcomeRateParams>,
    ) -> Result<CallToolResult, McpError> {
        self.stats_outcomes(params).await
    }

    #[tool(
        name = "carry_forward",
        description = "Deprecated alias of `session_carry_forward`, removed in v0.1.20. Same parameters, \
                       same handler, same result -- use `session_carry_forward`."
    )]
    pub(crate) async fn carry_forward_alias(
        &self,
        params: Parameters<CarryForwardParams>,
    ) -> Result<CallToolResult, McpError> {
        self.session_carry_forward(params).await
    }

    #[tool(
        name = "forgotten_context",
        description = "Deprecated alias of `session_forgotten`, removed in v0.1.20. Same parameters, \
                       same handler, same result -- use `session_forgotten`."
    )]
    pub(crate) async fn forgotten_context_alias(
        &self,
        params: Parameters<ForgottenContextParams>,
    ) -> Result<CallToolResult, McpError> {
        self.session_forgotten(params).await
    }

    #[tool(
        name = "suspect_commits",
        description = "Deprecated alias of `repo_suspect`, removed in v0.1.20. Same parameters, \
                       same handler, same result -- use `repo_suspect`."
    )]
    pub(crate) async fn suspect_commits_alias(
        &self,
        params: Parameters<SuspectCommitsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.repo_suspect(params).await
    }

}

#[tool_handler]
impl ServerHandler for AgentWorthMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions(
                "Read-only local index of AI-agent session histories on this machine. Tools: \
                 session_list, session_show, repo_blame, stats_usage, window_show, \
                 agent_list, stats_outcomes, stats_ladder, session_handoff, session_carry_forward, \
                 session_forgotten, session_asks, repo_suspect. Start a \
                 session in a repo with session_carry_forward to read what recent sessions there \
                 actually did; end one with session_handoff. If a session has compacted, \
                 session_forgotten returns the decisions its own summaries dropped. \
                 session_asks finds where a question's answer already landed, so it never \
                 needs re-asking. Before \
                 pushing, repo_suspect names the commits whose authoring session never \
                 proved anything -- a list and a prompt, never a patch. Redacted \
                 output is the default \
                 everywhere event or file content is returned; include_raw is the only opt-in \
                 to raw content, and it is per-call, never global. Run `archie scan` first \
                 if the index looks stale -- this server never scans on its own."
                    .to_string(),
            )
    }
}

/// Runs the `archie mcp` stdio server until the parent process closes the pipe -- the same
/// lifecycle every other stdio MCP server has. Logging must go to stderr, never stdout: stdout
/// is the JSON-RPC wire, and even one stray line on it would corrupt the protocol stream for
/// whatever client spawned this process. `main()` is responsible for pointing the global
/// tracing subscriber at stderr before calling this for the `mcp` subcommand specifically.
pub async fn run_mcp_server(storage: Arc<Storage>) -> anyhow::Result<()> {
    let server = AgentWorthMcpServer::new(storage);
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
