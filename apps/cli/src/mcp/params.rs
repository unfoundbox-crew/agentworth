//! Typed parameter structs for every `agentworth mcp` tool. `schemars::JsonSchema` drives the
//! JSON Schema `rmcp`'s `#[tool]` macro publishes to MCP clients; `serde::Deserialize` drives
//! decoding the client's actual call arguments.

use agentworth_storage::SessionOrderBy;
use chrono::{DateTime, Utc};
use serde::Deserialize;

/// Mirrors `agentworth_storage::SessionOrderBy` with the same snake_case wire values. A local
/// copy rather than deriving `schemars::JsonSchema` on the storage crate's own enum, since
/// `agentworth-storage` has no reason to take on a `schemars` dependency for one MCP-only need.
#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionsOrderBy {
    StartedAtDesc,
    StartedAtAsc,
    TokensDesc,
    TokensAsc,
    EventsDesc,
    EventsAsc,
    DurationDesc,
    ScoreDesc,
    ScoreAsc,
}

impl From<SessionsOrderBy> for SessionOrderBy {
    fn from(value: SessionsOrderBy) -> Self {
        match value {
            SessionsOrderBy::StartedAtDesc => SessionOrderBy::StartedAtDesc,
            SessionsOrderBy::StartedAtAsc => SessionOrderBy::StartedAtAsc,
            SessionsOrderBy::TokensDesc => SessionOrderBy::TokensDesc,
            SessionsOrderBy::TokensAsc => SessionOrderBy::TokensAsc,
            SessionsOrderBy::EventsDesc => SessionOrderBy::EventsDesc,
            SessionsOrderBy::EventsAsc => SessionOrderBy::EventsAsc,
            SessionsOrderBy::DurationDesc => SessionOrderBy::DurationDesc,
            SessionsOrderBy::ScoreDesc => SessionOrderBy::ScoreDesc,
            SessionsOrderBy::ScoreAsc => SessionOrderBy::ScoreAsc,
        }
    }
}

/// Parameters for the `sessions_find` tool. `limit` is required with no default (see
/// `docs/specs/mcp-server.md`'s "limit default trap" note) and is checked at call time
/// against `SESSIONS_FIND_LIMIT_CEILING`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SessionsFindParams {
    /// Filter to sessions whose derived repository/workspace name matches exactly. Not a
    /// stored column -- computed per-row from `source_path` and post-filtered client-side,
    /// so combining this with `limit` may require over-fetching (see `truncated` in the
    /// response).
    pub repo: Option<String>,
    /// Exact adapter name match (e.g. `claude_code`, `codex`, `gemini`).
    pub adapter: Option<String>,
    /// Substring match against the session's recorded models.
    pub model: Option<String>,
    /// Exact match against the stored primary outcome, snake_case (e.g. `commit_observed`).
    pub outcome: Option<String>,
    /// Substring match across session ID, source path, models, and adapter.
    pub search: Option<String>,
    /// RFC 3339 timestamp; only sessions started at or after this instant.
    pub start_date: Option<String>,
    /// RFC 3339 timestamp; only sessions started at or before this instant.
    pub end_date: Option<String>,
    /// Only sessions with at least this many total tokens.
    pub min_tokens: Option<u64>,
    /// Sort order; defaults to `started_at_desc`.
    pub order_by: Option<SessionsOrderBy>,
    /// Maximum rows to return. Required -- there is no silent default -- and capped at
    /// `SESSIONS_FIND_LIMIT_CEILING` (200).
    pub limit: usize,
    pub offset: Option<usize>,
    /// Include near-empty session stubs (defaults to excluding them).
    #[serde(default)]
    pub include_stubs: Option<bool>,
}

pub(super) fn parse_rfc3339_opt(value: Option<&str>) -> Result<Option<DateTime<Utc>>, String> {
    match value {
        None => Ok(None),
        Some(s) => DateTime::parse_from_rfc3339(s)
            .map(|dt| Some(dt.with_timezone(&Utc)))
            .map_err(|e| format!("'{s}' is not a valid RFC 3339 timestamp: {e}")),
    }
}

/// Default cap on how many of a trace's events `session_get` returns when the caller doesn't
/// say otherwise. A large real session's event list can run to tens of MB of JSON; without a
/// default cap, a remote model asking for "the session" gets that in full by accident. 500 is
/// generous enough to cover most sessions outright while forcing an explicit `events_limit` for
/// the rest -- the same "no silent unbounded default" principle `SessionsFindParams::limit`
/// already enforces, just with a permissive rather than a required value here since
/// `session_get` (unlike `sessions_find`) is about one specific session the caller already
/// knows they want.
pub const SESSION_GET_DEFAULT_EVENTS_LIMIT: usize = 500;

/// Parameters for the `session_get` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SessionGetParams {
    pub session_id: String,
    /// Return the unredacted trace, outcomes, and recoveries. Defaults to false -- redacted
    /// output is the default for every tool that can carry event or file content (see
    /// docs/specs/mcp-server.md, "What it must not expose").
    #[serde(default)]
    pub include_raw: bool,
    /// Zero-based offset into the trace's events. Defaults to 0.
    #[serde(default)]
    pub events_offset: Option<usize>,
    /// Max number of events to return. Defaults to `SESSION_GET_DEFAULT_EVENTS_LIMIT` (500) so
    /// a call can never receive a session's full event list by accident; pass an explicit,
    /// larger value to see more. Must be greater than 0.
    #[serde(default)]
    pub events_limit: Option<usize>,
}

/// Parameters for the `blame_find` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BlameFindParams {
    /// Substring pattern matched against recorded file-modification paths.
    pub file_path: String,
}

/// Rollup period for the `usage_summary` tool, mirroring `Storage::get_daily_usage` /
/// `get_weekly_usage` / `get_monthly_usage`.
#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UsagePeriodParam {
    Day,
    Week,
    Month,
}

/// Parameters for the `usage_summary` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UsageSummaryParams {
    pub period: UsagePeriodParam,
    /// Row cap; defaults match the HTTP route's own per-period defaults (30 / 20 / 12).
    pub limit: Option<usize>,
}

/// Parameters for the `pacing_window` tool.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct PacingWindowParams {
    /// Rolling window size in hours; defaults to 5.
    pub hours: Option<i64>,
}

/// Parameters for the `coverage_stats` tool.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct CoverageStatsParams {
    /// Also compute and include the per-adapter detection/capability matrix
    /// (`/api/matrix`'s equivalent). Defaults to false.
    #[serde(default)]
    pub include_matrix: bool,
}

/// Mirrors `agentworth_storage::OutcomeRateGroupBy` with the same snake_case wire values -- a
/// local copy for the same reason `SessionsOrderBy` is one (see its doc comment above).
#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeRateGroupByParam {
    Model,
    Adapter,
    Repo,
}

impl From<OutcomeRateGroupByParam> for agentworth_storage::OutcomeRateGroupBy {
    fn from(value: OutcomeRateGroupByParam) -> Self {
        match value {
            OutcomeRateGroupByParam::Model => agentworth_storage::OutcomeRateGroupBy::Model,
            OutcomeRateGroupByParam::Adapter => agentworth_storage::OutcomeRateGroupBy::Adapter,
            OutcomeRateGroupByParam::Repo => agentworth_storage::OutcomeRateGroupBy::Repo,
        }
    }
}

/// Parameters for the `outcome_rate` tool. See `docs/specs/verified-outcome-rate.md`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OutcomeRateParams {
    pub group_by: OutcomeRateGroupByParam,
    /// RFC 3339 timestamp; only sessions started at or after this instant.
    pub since: Option<String>,
    /// RFC 3339 timestamp; only sessions started at or before this instant.
    pub until: Option<String>,
    /// Groups with fewer than this many claimed sessions are suppressed (counted in
    /// `suppressed_groups`) rather than returned as a row. Defaults to 20.
    pub min_n: Option<usize>,
    /// Include near-empty session stubs in the population. Defaults to false.
    #[serde(default)]
    pub include_stubs: Option<bool>,
}
