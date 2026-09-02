//! Typed parameter structs for every `archie mcp` tool. `schemars::JsonSchema` drives the
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

/// Parameters for the `session_list` tool. `limit` is required with no default (see
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

/// Default cap on how many of a trace's events `session_show` returns when the caller doesn't
/// say otherwise. A large real session's event list can run to tens of MB of JSON; without a
/// default cap, a remote model asking for "the session" gets that in full by accident. 500 is
/// generous enough to cover most sessions outright while forcing an explicit `events_limit` for
/// the rest -- the same "no silent unbounded default" principle `SessionsFindParams::limit`
/// already enforces, just with a permissive rather than a required value here since
/// `session_show` (unlike `session_list`) is about one specific session the caller already
/// knows they want.
pub const SESSION_GET_DEFAULT_EVENTS_LIMIT: usize = 500;

/// Parameters for the `session_show` tool.
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

/// Parameters for the `repo_blame` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BlameFindParams {
    /// Substring pattern matched against recorded file-modification paths.
    pub file_path: String,
}

/// Rollup period for the `stats_usage` tool, mirroring `Storage::get_daily_usage` /
/// `get_weekly_usage` / `get_monthly_usage`.
#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UsagePeriodParam {
    Day,
    Week,
    Month,
}

/// Parameters for the `stats_usage` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UsageSummaryParams {
    pub period: UsagePeriodParam,
    /// Row cap; defaults match the HTTP route's own per-period defaults (30 / 20 / 12).
    pub limit: Option<usize>,
}

/// Parameters for the `window_show` tool.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct PacingWindowParams {
    /// Rolling window size in hours; defaults to 5.
    pub hours: Option<i64>,
}

/// Parameters for the `agent_list` tool.
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

/// Parameters for the `repo_suspect` tool. See `docs/specs/suspect-commits.md`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SuspectCommitsParams {
    /// Absolute path to a git checkout on this machine. Anything inside it works — the
    /// repository root is resolved with `git rev-parse --show-toplevel`.
    pub repo: String,
    /// Branch to walk. Defaults to `HEAD`.
    pub branch: Option<String>,
    /// Ref to diff against. Defaults to the branch's own upstream, then `origin/main`, then
    /// `origin/master`, then the most recent `max_commits` commits.
    pub base: Option<String>,
    /// RFC 3339 timestamp. Only consulted when `base` is absent.
    pub since: Option<String>,
    /// How long before a commit a session's file touch still counts as having authored it.
    /// Defaults to 24.
    pub window_hours: Option<i64>,
    /// Ceiling on commits walked. Defaults to 200, hard-capped at 1000.
    pub max_commits: Option<usize>,
}

/// Parameters for the `stats_outcomes` tool. See `docs/specs/verified-outcome-rate.md`.
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

/// Mirrors `agentworth_storage::LadderGroupBy` with the same snake_case wire values -- a
/// local copy for the same reason `OutcomeRateGroupByParam` is one.
#[derive(Debug, Clone, Copy, Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LadderGroupByParam {
    #[default]
    Model,
    Repo,
    Adapter,
    /// Only Codex records an effort today; sessions without one are counted and named, not
    /// given a made-up value.
    Effort,
}

impl From<LadderGroupByParam> for agentworth_storage::LadderGroupBy {
    fn from(value: LadderGroupByParam) -> Self {
        match value {
            LadderGroupByParam::Model => agentworth_storage::LadderGroupBy::Model,
            LadderGroupByParam::Repo => agentworth_storage::LadderGroupBy::Repo,
            LadderGroupByParam::Adapter => agentworth_storage::LadderGroupBy::Adapter,
            LadderGroupByParam::Effort => agentworth_storage::LadderGroupBy::Effort,
        }
    }
}

/// Parameters for the `stats_ladder` tool -- the same set `archie stats ladder` takes.
/// See `docs/specs/archie-bench.md`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct LadderParams {
    /// Lookback window: `day`, `week`, `month`, `year` or `all`. Defaults to `month`
    /// (30 days). This is how far back the window reaches, not a rollup granularity.
    pub period: Option<String>,
    /// Axis for the cost-per-verified-outcome table. Defaults to `model`.
    #[serde(default)]
    pub by: LadderGroupByParam,
    /// Repository or workspace substring, as `session_list`'s `repo` reports it.
    pub repo: Option<String>,
    /// Exact adapter name (`claude_code`, `codex`, `opencode`, ...).
    pub adapter: Option<String>,
    /// Model substring (`sonnet`, `gpt-4o`, ...).
    pub model: Option<String>,
    /// A group with fewer than this many claimed sessions returns a null rate and a null
    /// cost rather than a number nothing supports. Defaults to 20.
    pub min_n: Option<usize>,
    /// Include near-empty session stubs in the population. Defaults to false.
    #[serde(default)]
    pub include_stubs: Option<bool>,
}

/// Parameters for the `session_handoff` tool (`docs/specs/handoff.md`).
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct SessionHandoffParams {
    /// Session to hand over. Defaults to the most recent indexed session for the repository
    /// this server process is running in, which is what an agent asking "what did I just do
    /// here" means; pass one explicitly to reach any other session or repo.
    pub session_id: Option<String>,
    /// Line budget for the rendered markdown. Defaults to 60, hard ceiling 120.
    pub max_lines: Option<usize>,
    /// Include the "said it would, no evidence it did" section. Defaults to true.
    pub include_loose_ends: Option<bool>,
    /// Return unredacted paths, commands and quoted sentences. Defaults to false -- redacted
    /// is the default for every tool that can carry event or file content
    /// (docs/specs/mcp-server.md, "What it must not expose").
    #[serde(default)]
    pub include_raw: bool,
}

/// Parameters for the `session_forgotten` tool.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct ForgottenContextParams {
    /// Session to diff. Defaults to the most recent indexed session for the repository this
    /// server process is running in, which is what an agent asking "what did I forget here"
    /// means; pass one explicitly to reach any other session.
    pub session_id: Option<String>,
    /// One 1-based compaction round. Defaults to every round.
    pub round: Option<u32>,
    /// Any of `decision`, `rejected`, `reason`. Defaults to all three. An unknown name is an
    /// error, not an ignored filter.
    pub classes: Option<Vec<String>>,
    /// How many statements to return, newest first. Defaults to 20, hard ceiling 200. The
    /// totals in the response describe the whole session regardless of this.
    pub limit: Option<usize>,
    /// Return unredacted sentences, paths and evidence labels. Defaults to false -- everything
    /// this tool returns is transcript text (docs/specs/mcp-server.md, "What it must not
    /// expose").
    #[serde(default)]
    pub include_raw: bool,
}

/// Parameters for the `session_carry_forward` tool.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct CarryForwardParams {
    /// Repository/workspace key, as `session_list`'s `repo` and the handoff receipt report
    /// it (e.g. `unfoundbox/agentworth`). A repo's worktrees all answer to one value.
    pub repo: String,
    /// How many handoffs to return, newest first. Defaults to 3, ceiling 10.
    pub n: Option<usize>,
    /// RFC 3339 timestamp; only sessions started at or after this instant.
    pub since: Option<String>,
    /// Line budget for each rendered handoff. Defaults to 60, hard ceiling 120.
    pub max_lines: Option<usize>,
    /// Same per-call raw opt-in `session_handoff` has, applied to every handoff returned.
    #[serde(default)]
    pub include_raw: bool,
}

/// Parameters for the `session_asks` tool (`docs/specs/asks.md`).
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct SessionAsksParams {
    /// Session to index. Defaults to the most recent indexed session for the repository this
    /// server process is running in, same default `session_handoff` uses.
    pub session_id: Option<String>,
    /// RFC 3339 timestamp; only questions asked at or after this instant.
    pub since: Option<String>,
    /// Only questions that are not `answered` -- still open, or handed back to the user.
    /// Defaults to false.
    #[serde(default)]
    pub unanswered_only: bool,
    /// How many questions to return, newest first. Defaults to 50, hard ceiling 500. The
    /// totals in the response describe the whole session regardless of this.
    pub limit: Option<usize>,
    /// Return unredacted questions and answer excerpts. Defaults to false -- everything this
    /// tool returns is transcript text (docs/specs/mcp-server.md, "What it must not expose").
    #[serde(default)]
    pub include_raw: bool,
}
