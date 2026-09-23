//! The deterministic insights core: every number this machine can honestly say about its own
//! index, computed read-only from `sessions`, `file_modifications` and `session_model_usage`.
//!
//! Ported from `tools/insights/insights_query.py` (v1.4), the reviewed Python query pass, with
//! the same population predicate everywhere: sessions must be conversations, multi-event
//! (`total_events > 1`), and carry a started_at after the 2020-01-01 clock-bug floor. Section
//! 8's vocabulary counts, the friction-trigger classification and the turn-level time-of-day
//! histogram are NOT ported: they read a separate prompt-insights turn cache, which is outside
//! the AgentWorth index and carries no schema this crate owns. They are listed under
//! [`Insights::deferred`] so no consumer mistakes their absence for a zero.
//!
//! Tool-name buckets are a display grouping over cross-adapter synonyms (`Bash`/`bash`,
//! `Edit`/`edit`), not adapter-specific branching: the map is keyed on the tool-name strings
//! every adapter already writes into `sessions.tools_used`.

use anyhow::{Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::Connection;
use serde::Serialize;

pub const INSIGHTS_SCHEMA_VERSION: u32 = 1;

/// The session population predicate shared by every metric in this module, verbatim from the
/// reference implementation: `kind='conversation' AND total_events>1 AND started_at>'2020-01-01'`.
const USABLE_PREDICATE: &str = "kind='conversation' AND total_events>1 AND started_at>'2020-01-01'";

/// The verified rungs of the evidence ladder (rung 3+): commit observed, test/build passed,
/// CI/deployment verified.
pub const VERIFIED_OUTCOMES: [&str; 3] = [
    "commit_observed",
    "test_or_build_passed",
    "ci_or_deployment_verified",
];

/// An explicit half-open time window over the usable population: `started_at >= since AND
/// started_at < until`. Boundaries are normalized UTC RFC3339 strings (millisecond precision,
/// `+00:00` offset) so the stored `started_at` strings compare lexicographically and correctly.
#[derive(Debug, Clone, PartialEq)]
pub struct InsightsTimeWindow {
    pub since: String,
    pub until: String,
}

/// Validate and normalize CLI/HTTP window flags into an [`InsightsTimeWindow`]. `None` means
/// all-time. Both flags are optional and independent until normalization, which materializes a
/// missing `until` as the call site's data horizon (see `compute_insights`).
pub fn parse_window(
    since: Option<String>,
    until: Option<String>,
) -> Result<Option<InsightsTimeWindow>> {
    if since.is_none() && until.is_none() {
        return Ok(None);
    }
    let since = match &since {
        Some(s) => normalize_rfc3339(s).with_context(|| format!("bad --since value {s:?}"))?,
        None => {
            return Err(anyhow::anyhow!(
                "--until requires --since: a window needs a start"
            ));
        }
    };
    let until = until
        .as_deref()
        .map(normalize_rfc3339)
        .transpose()
        .with_context(|| "bad --until value".to_string())?;
    if let Some(u) = &until {
        anyhow::ensure!(since < *u, "--since must be earlier than --until");
    }
    Ok(Some(InsightsTimeWindow {
        since,
        until: until.unwrap_or_default(),
    }))
}

fn normalize_rfc3339(value: &str) -> Result<String> {
    let dt = DateTime::parse_from_rfc3339(value)?;
    Ok(dt
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, false))
}

/// How many `started_at` epochs of the window belong to a real clock, per the reference
/// implementation's clock-bug receipt (the 1970 row is excluded by the date filter, not usable).
const MIN_STARTED_AT: &str = "2020-01-01";

#[derive(Debug, Clone, Serialize)]
pub struct Insights {
    pub schema_version: u32,
    pub generated_at: String,
    pub window: InsightsWindow,
    pub filter: InsightsFilter,
    pub deltas: InsightsDeltas,
    pub population: InsightsPopulation,
    pub volume: InsightsVolume,
    pub by_adapter: Vec<InsightsAdapterRow>,
    pub ladder: Vec<InsightsLadderRow>,
    pub verified: InsightsVerified,
    pub no_evidence_small_sessions: i64,
    pub calls_per_turn: InsightsCallsPerTurn,
    pub calls_per_turn_by_adapter: Vec<InsightsCallsPerTurnRow>,
    pub turn_buckets: Vec<InsightsBucketRow>,
    pub file_modifications: InsightsFileMods,
    pub top_repos: Vec<InsightsRepoRow>,
    pub top_sessions: Vec<InsightsTopSessionRow>,
    pub session_size_buckets: Vec<InsightsBucketRow>,
    pub models: Vec<InsightsModelRow>,
    pub models_totals: InsightsModelTotals,
    pub series: InsightsSeries,
    pub tool_buckets: InsightsToolBuckets,
    pub tool_buckets_detail: Vec<InsightsToolKeyValuePair>,
    pub coverage_flags: Vec<InsightsCoverageFlag>,
    pub deferred: Vec<InsightsDeferredMetric>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsWindow {
    pub sessions: InsightsSessionWindow,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsSessionWindow {
    pub min_started_at: Option<String>,
    pub max_started_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsPopulation {
    pub sessions_raw: i64,
    pub sessions_conversation_multi_event: i64,
    pub sessions_excluded_pre_2020: i64,
    pub sessions_with_tool_inventory: i64,
    pub usable_sessions: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsVolume {
    pub usable_sessions: i64,
    pub tool_calls_witnessed: i64,
    pub human_turns_index_proxy: i64,
    pub assistant_messages: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsAdapterRow {
    pub adapter: String,
    pub sessions: i64,
    pub tool_calls: i64,
    pub human_turns: i64,
    pub input_output_tokens: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsLadderRow {
    pub outcome: String,
    pub sessions: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsVerified {
    pub sessions: i64,
    pub total_tokens: i64,
    pub share_pct: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsCallsPerTurn {
    /// Ratio of sums over ALL usable sessions (the reference's `tps_strict`, bug 3 fix).
    pub strict: Option<f64>,
    /// Within-session average over only heavy sessions (>10 events). The reference's `tps_avg`.
    pub heavy_session_average: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsCallsPerTurnRow {
    pub adapter: String,
    pub calls_per_turn: f64,
    pub tool_calls: i64,
    pub human_turns: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsBucketRow {
    pub bucket: String,
    pub label: String,
    pub sessions: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsFileMods {
    pub total: i64,
    pub sessions_with_file_mods: i64,
    pub by_action: Vec<InsightsToolKeyValuePair>,
    pub worktrees: InsightsFileModsWorktrees,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsFileModsWorktrees {
    pub total: i64,
    pub sessions: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsToolKeyValuePair {
    pub key: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsRepoRow {
    pub repo: String,
    pub file_touches: i64,
    pub sessions: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsTopSessionRow {
    pub session_id: String,
    pub adapter: String,
    pub started_at: String,
    pub total_tokens: i64,
    pub total_events: i64,
    pub tool_calls: i64,
    pub human_turns: i64,
    pub duration_hours: f64,
    pub source_path: String,
    pub primary_outcome: Option<String>,
    pub repo_label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsModelRow {
    pub model: String,
    pub sessions: i64,
    pub total_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsModelTotals {
    pub total_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_read_share_pct: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsSeries {
    pub monthly_by_adapter: Vec<InsightsMonthAdapterRow>,
    pub sessions_by_month: Vec<InsightsMonthRow>,
    pub daily: Vec<InsightsDailyRow>,
    pub outcome_by_month: Vec<InsightsMonthOutcomeRow>,
    pub verified_by_month: Vec<InsightsMonthRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsMonthAdapterRow {
    pub month: String,
    pub adapter: String,
    pub sessions: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsMonthRow {
    pub month: String,
    pub sessions: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsDailyRow {
    pub date: String,
    pub sessions: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsMonthOutcomeRow {
    pub month: String,
    pub outcome: String,
    pub sessions: i64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct InsightsToolBuckets {
    pub shell_edit: i64,
    pub file_edit: i64,
    pub read_explore: i64,
    pub web_browser: i64,
    pub agent_task: i64,
    pub other: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsCoverageFlag {
    pub dimension: String,
    pub status: String,
    pub signal: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsDeferredMetric {
    pub dimension: String,
    pub reason: String,
}

/// The window flags the caller passed, echoed verbatim (`None` = all-time).
#[derive(Debug, Clone, Serialize)]
pub struct InsightsFilter {
    pub since: Option<String>,
    pub until: Option<String>,
}

/// The delta-support block: five headline KPIs, each computed over the current requested window
/// and over the immediately preceding window of equal length, so the deck can render value + Δ.
/// Absent `--since` this is all-time (`previous_window: null`, every metric single-sided).
#[derive(Debug, Clone, Serialize)]
pub struct InsightsDeltas {
    /// The current window after normalization, with a materialized `until` (the data horizon)
    /// when the caller passed `--since` without `--until`.
    pub window: InsightsDeltaWindow,
    /// The preceding window of equal length, present only when `--since` was given.
    pub previous_window: Option<InsightsDeltaWindow>,
    pub usable_sessions: InsightsDeltaMetric,
    pub verified_outcome_rate: InsightsDeltaMetric,
    pub tool_calls_per_turn_strict: InsightsDeltaMetric,
    pub token_burn: InsightsDeltaMetric,
    pub friction_rate: InsightsDeltaMetric,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsDeltaWindow {
    pub since: String,
    pub until: String,
}

/// One headline KPI over two windows. `current`/`previous` are numbers, JSON `null` when the
/// metric is undefined over that window (e.g. a ratio with a zero denominator). `delta` is
/// `current − previous`; `delta_pct` is `100·delta/previous`, `null` when the base is zero or a
/// side is undefined. `reason` explains a structurally unavailable metric (friction) and is
/// `None` for every computable one.
#[derive(Debug, Clone, Serialize)]
pub struct InsightsDeltaMetric {
    pub current: serde_json::Value,
    pub previous: serde_json::Value,
    pub delta: serde_json::Value,
    pub delta_pct: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The four headline numbers one window needs, read with one small query each.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeadlineWindow {
    pub usable_sessions: i64,
    pub verified_rate_pct: f64,
    pub strict_calls_per_turn: Option<f64>,
    pub total_tokens: i64,
}

/// Compute the whole insights payload over one connection. Reads only; writes nothing. Works on
/// any connection: the shared in-memory fixture in tests, the server's `Storage` handle, or a
/// read-only, dedicated connection the CLI opens (see `agentworth insights`).
pub fn compute_insights(
    conn: &Connection,
    window: Option<&InsightsTimeWindow>,
) -> Result<Insights> {
    // Materialize a missing `until` as the data horizon so the previous window has a length.
    let effective: Option<InsightsTimeWindow> = match window {
        None => None,
        Some(w) if !w.until.is_empty() => Some(w.clone()),
        Some(w) => {
            if w.since.is_empty() {
                Some(w.clone())
            } else {
                // The horizon defaults the open end; it is nudged 1ms past MAX(started_at) so
                // the half-open upper bound still holds the newest session.
                let horizon: Option<String> = conn
                    .query_row(
                        &format!("SELECT MAX(started_at) FROM sessions WHERE {USABLE_PREDICATE}"),
                        [],
                        |r| r.get(0),
                    )
                    .map_err(anyhow::Error::from)?;
                let until = horizon
                    .map(|h| {
                        DateTime::parse_from_rfc3339(&h)
                            .map(|dt| {
                                (dt.with_timezone(&Utc) + chrono::Duration::milliseconds(1))
                                    .to_rfc3339_opts(SecondsFormat::Millis, false)
                            })
                            .map_err(anyhow::Error::from)
                    })
                    .transpose()?
                    .unwrap_or_else(|| w.since.clone());
                Some(InsightsTimeWindow {
                    since: w.since.clone(),
                    until,
                })
            }
        }
    };
    let pred = usable_predicate(effective.as_ref());
    let headline = query_headline(conn, &pred)?;
    let deltas = query_deltas(conn, effective.as_ref(), headline)?;

    let (tool_buckets, tool_order) = query_tool_buckets(conn, &pred)?;
    Ok(Insights {
        schema_version: INSIGHTS_SCHEMA_VERSION,
        generated_at: Utc::now().to_rfc3339(),
        window: InsightsWindow {
            sessions: query_session_window(conn, &pred)?,
        },
        filter: InsightsFilter {
            since: window.map(|w| w.since.clone()),
            until: window.map(|w| w.until.clone()).filter(|u| !u.is_empty()),
        },
        deltas,
        population: query_population(conn, &pred)?,
        volume: query_volume(conn, &pred)?,
        by_adapter: query_by_adapter(conn, &pred)?,
        ladder: query_ladder(conn, &pred)?,
        verified: query_verified(conn, &pred)?,
        no_evidence_small_sessions: query_one_i64(
            conn,
            format!(
                "SELECT COUNT(*) FROM sessions WHERE (primary_outcome IS NULL OR primary_outcome='') \
                 AND total_events>1 AND started_at>{} AND tool_calls_count=0",
                quote_string(MIN_STARTED_AT)
            ),
        )?,
        calls_per_turn: query_calls_per_turn(conn, &pred)?,
        calls_per_turn_by_adapter: query_calls_per_turn_by_adapter(conn, &pred)?,
        turn_buckets: query_turn_buckets(conn, &pred)?,
        file_modifications: query_file_modifications(conn, &pred)?,
        top_repos: query_top_repos(conn, &pred)?,
        top_sessions: query_top_sessions(conn, &pred)?,
        session_size_buckets: query_session_size_buckets(conn, &pred)?,
        models: query_models(conn, &pred)?,
        models_totals: query_models_totals(conn, &pred)?,
        series: query_series(conn, &pred)?,
        tool_buckets,
        tool_buckets_detail: tool_order,
        coverage_flags: coverage_flags(),
        deferred: deferred_metrics(),
    })
}

/// The population predicate for this window: the all-time usable predicate, plus the half-open
/// `started_at` bounds when a window was requested. Literals are quoted and escaped.
fn usable_predicate(window: Option<&InsightsTimeWindow>) -> String {
    match window {
        None => USABLE_PREDICATE.to_string(),
        Some(w) => format!(
            "{USABLE_PREDICATE} AND started_at>={} AND started_at<{}",
            quote_string(&w.since),
            quote_string(&w.until)
        ),
    }
}

/// The five headline KPIs that both the delta block and the top-level payload read from.
fn query_headline(conn: &Connection, pred: &str) -> Result<HeadlineWindow> {
    let usable_sessions =
        query_one_i64(conn, format!("SELECT COUNT(*) FROM sessions WHERE {pred}"))?;
    let verified_names = VERIFIED_OUTCOMES
        .iter()
        .map(|n| format!("'{}'", n))
        .collect::<Vec<_>>()
        .join(", ");
    let verified = query_one_i64(
        conn,
        format!(
            "SELECT COUNT(*) FROM sessions WHERE {pred} AND primary_outcome IN ({verified_names})"
        ),
    )?;
    let strict_calls_per_turn = query_one_optional_f64(
        conn,
        format!(
            "SELECT SUM(tool_calls_count)*1.0/SUM(user_messages_count) FROM sessions WHERE {pred}"
        ),
    )?;
    let total_tokens = query_one_i64(
        conn,
        format!("SELECT COALESCE(SUM(total_tokens),0) FROM sessions WHERE {pred}"),
    )?;
    Ok(HeadlineWindow {
        usable_sessions,
        verified_rate_pct: share_pct(verified, usable_sessions),
        strict_calls_per_turn,
        total_tokens,
    })
}

fn query_deltas(
    conn: &Connection,
    window: Option<&InsightsTimeWindow>,
    current: HeadlineWindow,
) -> Result<InsightsDeltas> {
    let window_bounds = window.map(|w| InsightsDeltaWindow {
        since: w.since.clone(),
        until: w.until.clone(),
    });
    let previous = match window {
        None => None,
        Some(w) => {
            let since = DateTime::parse_from_rfc3339(&w.since)?.with_timezone(&Utc);
            let until = DateTime::parse_from_rfc3339(&w.until)?.with_timezone(&Utc);
            let duration = until - since;
            if duration.num_milliseconds() <= 0 {
                None // zero-width window: nothing precedes it
            } else {
                let prev_since = (since - duration).to_rfc3339_opts(SecondsFormat::Millis, false);
                let prev_pred = usable_predicate(Some(&InsightsTimeWindow {
                    since: prev_since.clone(),
                    until: w.since.clone(),
                }));
                let previous = query_headline(conn, &prev_pred)?;
                Some((
                    InsightsDeltaWindow {
                        since: prev_since.clone(),
                        until: w.since.clone(),
                    },
                    previous,
                ))
            }
        }
    };
    Ok(InsightsDeltas {
        window: window_bounds.unwrap_or(InsightsDeltaWindow {
            since: String::new(),
            until: String::new(),
        }),
        previous_window: previous.as_ref().map(|(b, _)| b.clone()),
        usable_sessions: i64_delta(
            current.usable_sessions,
            previous.as_ref().map(|(_, h)| h.usable_sessions),
        ),
        verified_outcome_rate: f64_delta(
            Some(current.verified_rate_pct),
            previous.as_ref().map(|(_, h)| Some(h.verified_rate_pct)),
        ),
        tool_calls_per_turn_strict: f64_delta(
            current.strict_calls_per_turn,
            previous.as_ref().map(|(_, h)| h.strict_calls_per_turn),
        ),
        token_burn: i64_delta(
            current.total_tokens,
            previous.as_ref().map(|(_, h)| h.total_tokens),
        ),
        friction_rate: friction_rate_delta(),
    })
}

/// Friction turns cannot come from the AgentWorth index: it stores no per-turn text, so friction
///-turn classification (the prompt-insights lane) has no deterministic input here. The metric is
/// present in the contract with an explicit reason, never a fabricated zero.
fn friction_rate_delta() -> InsightsDeltaMetric {
    InsightsDeltaMetric {
        current: serde_json::Value::Null,
        previous: serde_json::Value::Null,
        delta: serde_json::Value::Null,
        delta_pct: serde_json::Value::Null,
        reason: Some(
            "friction classification needs per-turn text, which the index does not store; see \
             deferred: friction_triggers"
                .to_string(),
        ),
    }
}

fn i64_delta(current: i64, previous: Option<i64>) -> InsightsDeltaMetric {
    let prev = match previous {
        Some(p) => p,
        None => return single_sided(serde_json::json!(current)),
    };
    let delta = current - prev;
    let delta_pct = if prev == 0 {
        serde_json::Value::Null
    } else {
        round_json(100.0 * delta as f64 / prev as f64)
    };
    InsightsDeltaMetric {
        current: serde_json::json!(current),
        previous: serde_json::json!(prev),
        delta: serde_json::json!(delta),
        delta_pct,
        reason: None,
    }
}

fn f64_delta(current: Option<f64>, previous: Option<Option<f64>>) -> InsightsDeltaMetric {
    let cur = match current {
        Some(c) => c,
        None => return single_sided(serde_json::Value::Null),
    };
    let prev = match previous {
        Some(Some(p)) => p,
        Some(None) => return single_sided(serde_json::json!(cur)),
        None => return single_sided(serde_json::json!(cur)),
    };
    let delta = cur - prev;
    let delta_pct = if prev.abs() < f64::EPSILON {
        serde_json::Value::Null
    } else {
        round_json(100.0 * delta / prev.abs())
    };
    InsightsDeltaMetric {
        current: round_json(cur),
        previous: round_json(prev),
        delta: round_json(delta),
        delta_pct,
        reason: None,
    }
}

fn single_sided(current: serde_json::Value) -> InsightsDeltaMetric {
    InsightsDeltaMetric {
        current,
        previous: serde_json::Value::Null,
        delta: serde_json::Value::Null,
        delta_pct: serde_json::Value::Null,
        reason: None,
    }
}

fn round_json(value: f64) -> serde_json::Value {
    serde_json::json!(round4(value))
}

fn query_one_optional_f64(conn: &Connection, sql: impl AsRef<str>) -> Result<Option<f64>> {
    let value: Option<f64> = conn
        .query_row(sql.as_ref(), [], |r| r.get(0))
        .map_err(anyhow::Error::from)?;
    Ok(value.map(round4))
}

/// Stream every usable session's `tools_used` inventory (a JSON object of tool name -> witnessed
/// call count through that tool) and fold it into display buckets plus the per-name detail list.
/// Row-at-a-time under a bounded result set; correctness first.
fn query_tool_buckets(
    conn: &Connection,
    pred: &str,
) -> Result<(InsightsToolBuckets, Vec<InsightsToolKeyValuePair>)> {
    let sql = format!(
        "SELECT tools_used FROM sessions WHERE {pred} AND tools_used LIKE '{{%' AND tools_used!='{{}}'"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(anyhow::Error::from)?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    let mut totals: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
    let mut buckets = InsightsToolBuckets::default();
    for row in rows.drain(..) {
        let parsed: serde_json::Map<String, serde_json::Value> = match serde_json::from_str(&row) {
            Ok(v) => v,
            Err(_) => continue, // malformed inventory degrades to a missing row, not a failure
        };
        for (name, value) in parsed.iter() {
            let n = value.as_u64().unwrap_or(0) as i64;
            if n == 0 {
                continue;
            }
            *totals.entry(name.clone()).or_insert(0) += n;
            match tool_bucket_name(name) {
                "shell_edit" => buckets.shell_edit += n,
                "file_edit" => buckets.file_edit += n,
                "read_explore" => buckets.read_explore += n,
                "web_browser" => buckets.web_browser += n,
                "agent_task" => buckets.agent_task += n,
                _ => buckets.other += n,
            }
        }
    }
    let mut detail: Vec<InsightsToolKeyValuePair> = totals
        .into_iter()
        .map(|(key, count)| InsightsToolKeyValuePair { key, count })
        .collect();
    detail.sort_by(|a, b| b.count.cmp(&a.count).then(a.key.cmp(&b.key)));
    Ok((buckets, detail))
}

fn quote_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn query_session_window(conn: &Connection, pred: &str) -> Result<InsightsSessionWindow> {
    let sql = format!("SELECT MIN(started_at), MAX(started_at) FROM sessions WHERE {pred}");
    let row = conn
        .query_row(&sql, [], |r| {
            Ok(InsightsSessionWindow {
                min_started_at: r.get::<_, Option<String>>(0)?,
                max_started_at: r.get::<_, Option<String>>(1)?,
            })
        })
        .map_err(anyhow::Error::from)?;
    Ok(row)
}

fn query_population(conn: &Connection, pred: &str) -> Result<InsightsPopulation> {
    // receipts about the index itself stay unwindowed; the usable and tool-inventory counts
    // honor the window so they agree with the delta block's current window.
    let us = query_one_i64(conn, "SELECT COUNT(*) FROM sessions")?;
    let conv = query_one_i64(
        conn,
        "SELECT COUNT(*) FROM sessions WHERE kind='conversation' AND total_events>1",
    )?;
    let excl = query_one_i64(
        conn,
        format!(
            "SELECT COUNT(*) FROM sessions WHERE kind='conversation' AND total_events>1 AND \
             started_at<={MIN_STARTED_AT_SQL}",
            MIN_STARTED_AT_SQL = quote_string(MIN_STARTED_AT)
        ),
    )?;
    let tools = query_one_i64(
        conn,
        format!(
            "SELECT COUNT(*) FROM sessions WHERE {pred} AND tools_used LIKE '{{%' AND tools_used!='{{}}'"
        ),
    )?;
    let usable = query_one_i64(conn, format!("SELECT COUNT(*) FROM sessions WHERE {pred}"))?;
    Ok(InsightsPopulation {
        sessions_raw: us,
        sessions_conversation_multi_event: conv,
        sessions_excluded_pre_2020: excl,
        sessions_with_tool_inventory: tools,
        usable_sessions: usable,
    })
}

fn query_volume(conn: &Connection, pred: &str) -> Result<InsightsVolume> {
    let sql = format!(
        "SELECT COUNT(*), COALESCE(SUM(tool_calls_count),0), COALESCE(SUM(user_messages_count),0), \
         COALESCE(SUM(assistant_messages_count),0), COALESCE(SUM(total_tokens),0) \
         FROM sessions WHERE {pred}"
    );
    let row = conn
        .query_row(&sql, [], |r| {
            Ok(InsightsVolume {
                usable_sessions: r.get(0)?,
                tool_calls_witnessed: r.get(1)?,
                human_turns_index_proxy: r.get(2)?,
                assistant_messages: r.get(3)?,
                total_tokens: r.get(4)?,
            })
        })
        .map_err(anyhow::Error::from)?;
    Ok(row)
}

fn query_by_adapter(conn: &Connection, pred: &str) -> Result<Vec<InsightsAdapterRow>> {
    let sql = format!(
        "SELECT adapter, COUNT(*), COALESCE(SUM(tool_calls_count),0), \
         COALESCE(SUM(user_messages_count),0), COALESCE(SUM(input_tokens+output_tokens),0) \
         FROM sessions WHERE {pred} GROUP BY 1 ORDER BY 2 DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(InsightsAdapterRow {
                adapter: r.get(0)?,
                sessions: r.get(1)?,
                tool_calls: r.get(2)?,
                human_turns: r.get(3)?,
                input_output_tokens: r.get(4)?,
            })
        })
        .map_err(anyhow::Error::from)?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    Ok(rows)
}

fn query_ladder(conn: &Connection, pred: &str) -> Result<Vec<InsightsLadderRow>> {
    let sql = format!(
        "SELECT CASE WHEN primary_outcome IS NULL OR primary_outcome='' \
         THEN 'no_outcome_evidence' ELSE primary_outcome END AS o, COUNT(*), \
         COALESCE(SUM(input_tokens+output_tokens+cache_read_tokens+cache_creation_tokens),0) \
         FROM sessions WHERE {pred} GROUP BY 1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt
        .query_map([], |r| {
            Ok(InsightsLadderRow {
                outcome: r.get(0)?,
                sessions: r.get(1)?,
                total_tokens: r.get(2)?,
            })
        })
        .map_err(anyhow::Error::from)?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    rows.sort_by(|a, b| {
        ladder_rank(&a.outcome)
            .cmp(&ladder_rank(&b.outcome))
            .then(b.sessions.cmp(&a.sessions))
    });
    Ok(rows)
}

/// Evidence-ladder placement for display ordering: rung 0 (no evidence) first, then rungs 1-5.
/// `verified_outcome` and any unlabelled rung names fall through to None (append at the end).
fn ladder_rank(outcome: &str) -> Option<u8> {
    match outcome {
        "no_outcome_evidence" => Some(0),
        "done_claimed" => Some(1),
        "artifact_changed" => Some(2),
        "test_or_build_passed" => Some(3),
        "commit_observed" => Some(4),
        "ci_or_deployment_verified" => Some(5),
        _ => None,
    }
}

fn query_verified(conn: &Connection, pred: &str) -> Result<InsightsVerified> {
    let ladder = query_ladder(conn, pred)?;
    let verified_sessions: i64 = ladder
        .iter()
        .filter(|row| VERIFIED_OUTCOMES.contains(&row.outcome.as_str()))
        .map(|row| row.sessions)
        .sum();
    let verified_tokens: i64 = ladder
        .iter()
        .filter(|row| VERIFIED_OUTCOMES.contains(&row.outcome.as_str()))
        .map(|row| row.total_tokens)
        .sum();
    let share_pct = share_pct(
        verified_sessions,
        ladder.iter().map(|row| row.sessions).sum(),
    );
    Ok(InsightsVerified {
        sessions: verified_sessions,
        total_tokens: verified_tokens,
        share_pct,
    })
}

fn share_pct(part: i64, whole: i64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        round4(100.0 * part as f64 / whole as f64)
    }
}

fn round4(value: f64) -> f64 {
    (value * 10000.0).round() / 10000.0
}

fn query_calls_per_turn(conn: &Connection, pred: &str) -> Result<InsightsCallsPerTurn> {
    let strict = format!(
        "SELECT SUM(tool_calls_count)*1.0/SUM(user_messages_count) FROM sessions WHERE {pred}"
    );
    let heavy = format!(
        "SELECT AVG(tool_calls_count*1.0/nullif(user_messages_count,0)) FROM sessions WHERE \
         user_messages_count>0 AND total_events>10 AND {pred}"
    );
    let strict = conn
        .query_row(&strict, [], |r| r.get::<_, Option<f64>>(0))
        .map_err(anyhow::Error::from)?
        .map(round4);
    let heavy = conn
        .query_row(&heavy, [], |r| r.get::<_, Option<f64>>(0))
        .map_err(anyhow::Error::from)?
        .map(round4);
    Ok(InsightsCallsPerTurn {
        strict,
        heavy_session_average: heavy,
    })
}

fn query_calls_per_turn_by_adapter(
    conn: &Connection,
    pred: &str,
) -> Result<Vec<InsightsCallsPerTurnRow>> {
    let sql = format!(
        "SELECT adapter, COALESCE(SUM(tool_calls_count),0), COALESCE(SUM(user_messages_count),0) \
         FROM sessions WHERE {pred} GROUP BY 1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .map_err(anyhow::Error::from)?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    let mut out: Vec<InsightsCallsPerTurnRow> = rows
        .drain(..)
        .filter(|(_, _, um)| *um > 0)
        .map(|(adapter, tc, um)| InsightsCallsPerTurnRow {
            calls_per_turn: round4(tc as f64 / um.max(1) as f64),
            adapter,
            tool_calls: tc,
            human_turns: um,
        })
        .collect();
    out.sort_by(|a, b| {
        b.calls_per_turn
            .partial_cmp(&a.calls_per_turn)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

fn query_turn_buckets(conn: &Connection, pred: &str) -> Result<Vec<InsightsBucketRow>> {
    let sql = format!(
        "SELECT CASE WHEN user_messages_count=0 THEN '0' WHEN user_messages_count=1 THEN '1' \
         WHEN user_messages_count=2 THEN '2' WHEN user_messages_count<=5 THEN '3-5' \
         WHEN user_messages_count<=20 THEN '6-20' WHEN user_messages_count<=50 THEN '21-50' \
         ELSE '>50' END AS k, COUNT(*) FROM sessions WHERE {pred} GROUP BY 1"
    );
    let counts: Vec<(String, i64)> = query_pairs(conn, &sql)?;
    let shape: [(&str, &str); 7] = [
        ("0", "0 (background)"),
        ("1", "1 (one-shot)"),
        ("2", "2"),
        ("3-5", "3-5"),
        ("6-20", "6-20"),
        ("21-50", "21-50"),
        (">50", ">50"),
    ];
    let mut out = Vec::with_capacity(shape.len());
    for (bucket, label) in shape {
        let sessions = counts
            .iter()
            .find(|(k, _)| *k == bucket)
            .map(|(_, n)| *n)
            .unwrap_or(0);
        out.push(InsightsBucketRow {
            bucket: bucket.to_string(),
            label: label.to_string(),
            sessions,
        });
    }
    Ok(out)
}

fn query_file_modifications(conn: &Connection, pred: &str) -> Result<InsightsFileMods> {
    // Windowed through the sessions join, so the file-touch story follows the same population.
    let total = query_one_i64(
        conn,
        format!(
            "SELECT COUNT(*) FROM file_modifications fm JOIN sessions s                  ON s.session_id=fm.session_id WHERE {pred}"
        ),
    )?;
    let sessions_with = query_one_i64(
        conn,
        format!(
            "SELECT COUNT(DISTINCT s.session_id) FROM file_modifications fm JOIN sessions s                  ON s.session_id=fm.session_id WHERE {pred}"
        ),
    )?;
    let by_action = query_pairs(
        conn,
        &format!(
            "SELECT fm.action, COUNT(*) FROM file_modifications fm JOIN sessions s              ON s.session_id=fm.session_id WHERE {pred} GROUP BY 1 ORDER BY 2 DESC"
        ),
    )?;
    let worktrees = conn
        .query_row(
            &format!(
                "SELECT COUNT(*), COUNT(DISTINCT s.session_id) FROM file_modifications fm                  JOIN sessions s ON s.session_id=fm.session_id                  WHERE {pred} AND fm.file_path LIKE '%worktrees%'"
            ),
            [],
            |r| {
                Ok(InsightsFileModsWorktrees {
                    total: r.get::<_, i64>(0)?,
                    sessions: r.get::<_, i64>(1)?,
                })
            },
        )
        .map_err(anyhow::Error::from)?;
    Ok(InsightsFileMods {
        total,
        sessions_with_file_mods: sessions_with,
        by_action: by_action
            .into_iter()
            .map(|(key, count)| InsightsToolKeyValuePair { key, count })
            .collect(),
        worktrees,
    })
}

fn query_top_repos(conn: &Connection, pred: &str) -> Result<Vec<InsightsRepoRow>> {
    let sql = &format!(
        "SELECT substr(fm.file_path, instr(fm.file_path, '/code/')+6, \
               instr(substr(fm.file_path, instr(fm.file_path, '/code/')+6), '/')-1) AS repo, \
               COUNT(*), COUNT(DISTINCT s.session_id) \
               FROM file_modifications fm JOIN sessions s ON s.session_id=fm.session_id \
               WHERE {pred} AND fm.file_path LIKE '%/code/%' \
               GROUP BY 1 HAVING length(repo)>2 ORDER BY 2 DESC LIMIT 12"
    );
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(InsightsRepoRow {
                repo: r.get(0)?,
                file_touches: r.get(1)?,
                sessions: r.get(2)?,
            })
        })
        .map_err(anyhow::Error::from)?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    Ok(rows)
}

fn query_top_sessions(conn: &Connection, pred: &str) -> Result<Vec<InsightsTopSessionRow>> {
    let sql = format!(
        "SELECT session_id, adapter, started_at, total_tokens, total_events, tool_calls_count, \
         user_messages_count, COALESCE(duration_seconds,0), source_path, primary_outcome \
         FROM sessions WHERE {pred} ORDER BY total_tokens DESC LIMIT 8"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |r| {
            let source_path: String = r.get(8)?;
            let duration_seconds: f64 = r.get(7)?;
            Ok(InsightsTopSessionRow {
                session_id: r.get(0)?,
                adapter: r.get(1)?,
                started_at: r.get(2)?,
                total_tokens: r.get(3)?,
                total_events: r.get(4)?,
                tool_calls: r.get(5)?,
                human_turns: r.get(6)?,
                duration_hours: round4(duration_seconds / 3600.0),
                source_path,
                primary_outcome: r.get(9)?,
                repo_label: String::new(),
            })
        })
        .map_err(anyhow::Error::from)?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    let mut rows = rows;
    for row in rows.iter_mut() {
        row.repo_label = repo_label(&row.source_path);
    }
    Ok(rows)
}

/// A display label for which workspace a session touched, derived purely from its source path.
/// Same semantics as the reference's `repo_label`, with the machine-specific dash-encoded
/// home prefix replaced by a generic search.
pub fn repo_label(source_path: &str) -> String {
    if source_path.contains(".codex-session") {
        let segs: Vec<&str> = source_path.split('/').collect();
        let pick = if segs.len() >= 3 {
            segs[segs.len() - 3]
        } else if !segs.is_empty() {
            segs[segs.len() - 1]
        } else {
            "codex"
        };
        return truncate(pick, 44);
    }
    if source_path.contains("globalStorage") {
        return "cursor (global)".to_string();
    }
    if let Some(idx) = source_path.find("/code/") {
        let rest = source_path.get(idx..).unwrap_or("").trim_start_matches("/code/");
        if let Some(first) = rest.split('/').next() {
            return truncate(first, 44);
        }
    }
    if let Some(idx) = source_path.find("-code-") {
        // Dash-encoded claude-project paths: `...-code-org-repo/sess.json` -> `org-repo`.
        let rest = source_path.get(idx..).unwrap_or("").trim_start_matches("-code-");
        if let Some(first) = rest.split('/').next() {
            return truncate(first, 44);
        }
    }
    let last = source_path.split('/').next_back().unwrap_or(source_path);
    truncate(last, 40)
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        value.chars().take(max).collect()
    }
}

fn query_session_size_buckets(conn: &Connection, pred: &str) -> Result<Vec<InsightsBucketRow>> {
    let sql = format!(
        "SELECT CASE WHEN total_tokens<10000000 THEN '<10M' WHEN total_tokens<50000000 THEN '10-50M' \
         WHEN total_tokens<200000000 THEN '50-200M' ELSE '>200M' END AS k, COUNT(*) \
         FROM sessions WHERE {pred} AND total_tokens>1000 GROUP BY 1"
    );
    let counts: Vec<(String, i64)> = query_pairs(conn, &sql)?;
    let shape: [(&str, &str); 4] = [
        ("<10M", "< 10M"),
        ("10-50M", "10-50M"),
        ("50-200M", "50-200M"),
        (">200M", "> 200M"),
    ];
    let mut out = Vec::with_capacity(shape.len());
    for (bucket, label) in shape {
        let sessions = counts
            .iter()
            .find(|(k, _)| *k == bucket)
            .map(|(_, n)| *n)
            .unwrap_or(0);
        out.push(InsightsBucketRow {
            bucket: bucket.to_string(),
            label: label.to_string(),
            sessions,
        });
    }
    Ok(out)
}

fn query_models(conn: &Connection, pred: &str) -> Result<Vec<InsightsModelRow>> {
    let sql = &format!(
        "SELECT smu.model, COUNT(*), \
               COALESCE(SUM(smu.input_tokens+smu.output_tokens+smu.cache_read_tokens+smu.cache_creation_tokens),0), \
               COALESCE(SUM(smu.cache_read_tokens),0), COALESCE(SUM(smu.cache_creation_tokens),0), \
               COALESCE(SUM(smu.input_tokens),0), COALESCE(SUM(smu.output_tokens),0) \
               FROM session_model_usage smu JOIN sessions s ON s.session_id=smu.session_id \
               WHERE {pred} GROUP BY 1 ORDER BY 2 DESC"
    );
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(InsightsModelRow {
                model: r.get(0)?,
                sessions: r.get(1)?,
                total_tokens: r.get(2)?,
                cache_read_tokens: r.get(3)?,
                cache_creation_tokens: r.get(4)?,
                input_tokens: r.get(5)?,
                output_tokens: r.get(6)?,
            })
        })
        .map_err(anyhow::Error::from)?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    Ok(rows)
}

fn query_models_totals(conn: &Connection, pred: &str) -> Result<InsightsModelTotals> {
    let row = conn
        .query_row(
            &format!(
                "SELECT COALESCE(SUM(smu.input_tokens+smu.output_tokens+smu.cache_read_tokens+smu.cache_creation_tokens),0), \
             COALESCE(SUM(smu.cache_read_tokens),0) FROM session_model_usage smu \
             JOIN sessions s ON s.session_id=smu.session_id WHERE {pred}"
            ),
            [],
            |r| {
                let total: i64 = r.get(0)?;
                let cache_read: i64 = r.get(1)?;
                Ok(InsightsModelTotals {
                    total_tokens: total,
                    cache_read_tokens: cache_read,
                    cache_read_share_pct: if total == 0 {
                        0.0
                    } else {
                        round4(100.0 * cache_read as f64 / total as f64)
                    },
                })
            },
        )
        .map_err(anyhow::Error::from)?;
    Ok(row)
}

fn query_series(conn: &Connection, pred: &str) -> Result<InsightsSeries> {
    let monthly_by_adapter = query_month_adapter(
        conn,
        format!(
            "SELECT strftime('%Y-%m', started_at), adapter, COUNT(*) FROM sessions WHERE {pred} GROUP BY 1, 2"
        ),
    )?;
    let sessions_by_month = query_pairs_i64(
        conn,
        &format!(
            "SELECT strftime('%Y-%m', started_at), COUNT(*) FROM sessions WHERE {pred} GROUP BY 1 ORDER BY 1"
        ),
    )?;
    let daily = query_pairs_i64(
        conn,
        &format!(
            "SELECT date(started_at), COUNT(*) FROM sessions WHERE {pred} GROUP BY 1 ORDER BY 1"
        ),
    )?;
    let outcome_by_month = query_month_outcome(
        conn,
        format!(
            "SELECT strftime('%Y-%m', started_at) AS m, CASE WHEN primary_outcome IS NULL OR \
                 primary_outcome='' THEN 'no_outcome_evidence' ELSE primary_outcome END AS o, \
                 COUNT(*) FROM sessions WHERE {pred} GROUP BY 1, 2 ORDER BY 1"
        ),
    )?;
    let verified_names = VERIFIED_OUTCOMES
        .iter()
        .map(|n| format!("'{}'", n))
        .collect::<Vec<_>>()
        .join(", ");
    let verified_by_month = query_pairs_i64(
        conn,
        &format!(
            "SELECT strftime('%Y-%m', started_at), COUNT(*) FROM sessions WHERE {pred} \
             AND primary_outcome IN ({verified_names}) GROUP BY 1 ORDER BY 1"
        ),
    )?;
    Ok(InsightsSeries {
        monthly_by_adapter,
        sessions_by_month: sessions_by_month
            .into_iter()
            .map(|(month, sessions)| InsightsMonthRow { month, sessions })
            .collect(),
        daily: daily
            .into_iter()
            .map(|(date, sessions)| InsightsDailyRow { date, sessions })
            .collect(),
        outcome_by_month,
        verified_by_month: verified_by_month
            .into_iter()
            .map(|(month, sessions)| InsightsMonthRow { month, sessions })
            .collect(),
    })
}

fn query_pairs_i64(conn: &Connection, sql: &str) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .map_err(anyhow::Error::from)?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    Ok(rows)
}

fn query_month_adapter(conn: &Connection, sql: String) -> Result<Vec<InsightsMonthAdapterRow>> {
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(InsightsMonthAdapterRow {
                month: r.get(0)?,
                adapter: r.get(1)?,
                sessions: r.get(2)?,
            })
        })
        .map_err(anyhow::Error::from)?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    Ok(rows)
}

fn query_month_outcome(conn: &Connection, sql: String) -> Result<Vec<InsightsMonthOutcomeRow>> {
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(InsightsMonthOutcomeRow {
                month: r.get(0)?,
                outcome: r.get(1)?,
                sessions: r.get(2)?,
            })
        })
        .map_err(anyhow::Error::from)?
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;
    Ok(rows)
}

fn query_pairs(conn: &Connection, sql: &str) -> Result<Vec<(String, i64)>> {
    query_pairs_i64(conn, sql)
}

fn query_one_i64(conn: &Connection, sql: impl AsRef<str>) -> Result<i64> {
    let value: i64 = conn
        .query_row(sql.as_ref(), [], |r| r.get(0))
        .map_err(anyhow::Error::from)?;
    Ok(value)
}

fn coverage_flags() -> Vec<InsightsCoverageFlag> {
    vec![
        InsightsCoverageFlag {
            dimension: "execution".to_string(),
            status: "real_data".to_string(),
            signal: "sessions.tool_calls_count + tools_used inventory".to_string(),
        },
        InsightsCoverageFlag {
            dimension: "steering".to_string(),
            status: "real_data".to_string(),
            signal: "sessions.user_messages_count distribution + per-adapter calls/turn"
                .to_string(),
        },
        InsightsCoverageFlag {
            dimension: "engineering_practice".to_string(),
            status: "real_data".to_string(),
            signal: "primary_outcome evidence ladder".to_string(),
        },
        InsightsCoverageFlag {
            dimension: "models".to_string(),
            status: "real_data".to_string(),
            signal: "session_model_usage token inventory".to_string(),
        },
        InsightsCoverageFlag {
            dimension: "product".to_string(),
            status: "thin_coverage".to_string(),
            signal: "turn-level intent classification is outside the index; repo prefixes and \
                     outcomes are the only proxies here"
                .to_string(),
        },
        InsightsCoverageFlag {
            dimension: "planning".to_string(),
            status: "thin_coverage".to_string(),
            signal: "plan-mode events are not indexed; no honest proxy in this payload".to_string(),
        },
    ]
}

fn deferred_metrics() -> Vec<InsightsDeferredMetric> {
    vec![
        InsightsDeferredMetricsBuild::human_turn_word_counts(),
        InsightsDeferredMetricsBuild::friction_triggers(),
        InsightsDeferredMetricsBuild::intent_categories(),
        InsightsDeferredMetricsBuild::time_of_day_histogram(),
        InsightsDeferredMetricsBuild::vocabulary_mentions(),
    ]
}

/// Named helpers keep the reasons in one place and force each deferral to be stated once.
struct InsightsDeferredMetricsBuild;

impl InsightsDeferredMetricsBuild {
    fn human_turn_word_counts() -> InsightsDeferredMetric {
        deferred(
            "human_turn_word_counts",
            "per-turn word counts live in a separate prompt-insights turn cache; the index stores no \
             per-turn text",
        )
    }
    fn friction_triggers() -> InsightsDeferredMetric {
        deferred(
            "friction_triggers",
            "friction-turn classification needs per-turn text; the index stores none",
        )
    }
    fn intent_categories() -> InsightsDeferredMetric {
        deferred(
            "intent_categories",
            "intent-category counts need per-turn text and a classifier; the index stores neither",
        )
    }
    fn time_of_day_histogram() -> InsightsDeferredMetric {
        deferred(
            "time_of_day_histogram",
            "the hour-of-day x day-of-week heatmap needs per-turn timestamps; the index stores one \
             started_at per session",
        )
    }
    fn vocabulary_mentions() -> InsightsDeferredMetric {
        deferred(
            "vocabulary_mentions",
            "vocabulary mention counts need the raw turn text, which the index intentionally does \
             not duplicate",
        )
    }
}

fn deferred(dimension: &str, reason: &str) -> InsightsDeferredMetric {
    InsightsDeferredMetric {
        dimension: dimension.to_string(),
        reason: reason.to_string(),
    }
}

/// Cross-adapter tool-name display grouping. Keyed on the exact tool-name strings the adapters
/// write into `sessions.tools_used`; a map, not per-adapter branching.
pub fn tool_bucket_name(tool_name: &str) -> &'static str {
    const SHELL_EDIT: [&str; 4] = ["Bash", "bash", "run_command", "run_terminal_command"];
    const FILE_EDIT: [&str; 8] = [
        "Edit",
        "edit",
        "Write",
        "write",
        "replace_file_content",
        "write_to_file",
        "create_file",
        "search_replace",
    ];
    const READ_EXPLORE: [&str; 12] = [
        "Read",
        "read",
        "view_file",
        "grep",
        "grep_search",
        "list_dir",
        "Glob",
        "Grep",
        "search_tool",
        "codebase_search",
        "glob_file_search",
        "list_directory",
    ];
    const WEB_BROWSER: [&str; 6] = [
        "WebFetch",
        "WebSearch",
        "mcp:Claude_Browser:computer",
        "mcp:Claude_Browser:navigate",
        "mcp:Claude_Browser:javascript_tool",
        "mcp:claude-in-chrome:computer",
    ];
    const AGENT_TASK: [&str; 7] = [
        "Agent",
        "manage_task",
        "SendMessage",
        "spawn_subagent",
        "StructuredOutput",
        "ToolSearch",
        "todo_write",
    ];
    for name in SHELL_EDIT {
        if name == tool_name {
            return "shell_edit";
        }
    }
    for name in FILE_EDIT {
        if name == tool_name {
            return "file_edit";
        }
    }
    for name in READ_EXPLORE {
        if name == tool_name {
            return "read_explore";
        }
    }
    for name in WEB_BROWSER {
        if name == tool_name {
            return "web_browser";
        }
    }
    for name in AGENT_TASK {
        if name == tool_name {
            return "agent_task";
        }
    }
    "other"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_bucket_name_matches_reference_groups() {
        assert_eq!(tool_bucket_name("Bash"), "shell_edit");
        assert_eq!(tool_bucket_name("run_terminal_command"), "shell_edit");
        assert_eq!(tool_bucket_name("Write"), "file_edit");
        assert_eq!(tool_bucket_name("Edit"), "file_edit");
        assert_eq!(tool_bucket_name("Read"), "read_explore");
        assert_eq!(tool_bucket_name("codebase_search"), "read_explore");
        assert_eq!(tool_bucket_name("WebFetch"), "web_browser");
        assert_eq!(
            tool_bucket_name("mcp:Claude_Browser:navigate"),
            "web_browser"
        );
        assert_eq!(tool_bucket_name("Agent"), "agent_task");
        assert_eq!(tool_bucket_name("todo_write"), "agent_task");
        assert_eq!(
            tool_bucket_name("mcp:definitely-not-a-real-server:tool"),
            "other"
        );
        assert_eq!(tool_bucket_name(""), "other");
    }

    #[test]
    fn ladder_rank_orders_rungs_0_through_5() {
        assert_eq!(ladder_rank("no_outcome_evidence"), Some(0));
        assert_eq!(ladder_rank("done_claimed"), Some(1));
        assert_eq!(ladder_rank("artifact_changed"), Some(2));
        assert_eq!(ladder_rank("test_or_build_passed"), Some(3));
        assert_eq!(ladder_rank("commit_observed"), Some(4));
        assert_eq!(ladder_rank("ci_or_deployment_verified"), Some(5));
        assert_eq!(ladder_rank("something_new"), None);
    }

    #[test]
    fn repo_label_handles_encoded_codex_and_paths() {
        // Dash-encoded claude-project path.
        let sp = "/x/.claude/projects/-Users-dev-code-org-repo/rollout-1.jsonl";
        assert_eq!(repo_label(sp), "org-repo");
        // Raw /code/ path.
        let sp = "/home/dev/code/myorg/myrepo/worktree/a.jsonl";
        assert_eq!(repo_label(sp), "myorg");
        // Codex session: third-from-last segment (2026/09/proj-x/.codex-session/run.jsonl).
        let sp = "/x/2026/09/proj-x/.codex-session/run.jsonl";
        assert_eq!(repo_label(sp), "proj-x");
        // Fallback: last component, truncated to 40.
        let sp = "/tmp/very-long-filename-that-goes-on-and-on-and-on.jsonl";
        let label = repo_label(sp);
        assert!(label.chars().count() <= 40, "got {label}");
        assert!(label.starts_with("very-long-filename"), "got {label}");
    }

    #[test]
    fn repo_label_codex_falls_back_when_path_shallow() {
        assert_eq!(repo_label(".codex-session"), ".codex-session");
    }

    #[test]
    fn share_pct_defines_zero_denominator_as_zero() {
        assert_eq!(share_pct(100, 0), 0.0);
        assert_eq!(share_pct(0, 0), 0.0);
        assert_eq!(share_pct(50, 100), 50.0);
    }

    // -- synthetic-fixture integration: the real compute path over a real temp index -------

    use agentworth_schema::{
        AgentWorthTrace, EventPayload, FileActionType, Provenance, TokenUsage,
    };
    use chrono::{DateTime, TimeZone, Utc};
    use tempfile::TempDir;

    type NormalizedEvent = agentworth_schema::NormalizedEvent;

    fn at(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 10, 0, 0).unwrap()
    }

    /// Build the synthetic index the golden assertions run against: one usable verified
    /// claude_code session on 2026-06-01 and one usable one-shot codex session on 2026-09-15.
    fn synthetic_fixture() -> (TempDir, crate::Storage) {
        let dir = TempDir::new().unwrap();
        let storage = crate::Storage::open_path(&dir.path().join("insights-fixture.db")).unwrap();
        let mut seq = 0u64;

        let mut a = AgentWorthTrace::new(
            "sess-a",
            "claude_code",
            Provenance::new(
                "/home/dev/code/org/alpha/log.jsonl",
                "claude_code",
                1024,
                1,
                "fp-a",
            ),
            at(2026, 6, 1),
        );
        for _ in 0..3 {
            seq += 1;
            a.events.push(NormalizedEvent::new(
                seq,
                at(2026, 6, 1),
                EventPayload::UserMessage {
                    content: "turn".into(),
                },
            ));
        }
        for name in ["Bash", "Bash", "Bash", "Read", "Read", "Write"] {
            seq += 1;
            a.events.push(NormalizedEvent::new(
                seq,
                at(2026, 6, 1),
                EventPayload::ToolCall(agentworth_schema::ToolCall {
                    id: None,
                    name: name.to_string(),
                    arguments: serde_json::json!({}),
                }),
            ));
        }
        for path in [
            "/home/dev/code/org/alpha/src.rs",
            "/home/dev/code/org/alpha/lib.rs",
        ] {
            seq += 1;
            a.events.push(NormalizedEvent::new(
                seq,
                at(2026, 6, 1),
                EventPayload::FileAction {
                    path: path.into(),
                    action: FileActionType::Write,
                    diff: None,
                    lines_changed: Some(1),
                },
            ));
        }
        seq += 1;
        a.events.push(NormalizedEvent::new(
            seq,
            at(2026, 6, 1),
            EventPayload::ModelInvocation {
                model: "model-x".into(),
                token_usage: TokenUsage::new(1_000, 100, 3_000, 200),
                cost_usd: None,
                latency_ms: None,
                effort: None,
            },
        ));
        a.recalculate_stats();
        storage
            .upsert_session(&a, Some("commit_observed"), Some(0.9), 1)
            .unwrap();

        let mut b = AgentWorthTrace::new(
            "sess-b",
            "codex",
            Provenance::new("/tmp/somewhere/b-rollout-1.jsonl", "codex", 512, 2, "fp-b"),
            at(2026, 9, 15),
        );
        seq = 0;
        seq += 1;
        b.events.push(NormalizedEvent::new(
            seq,
            at(2026, 9, 15),
            EventPayload::UserMessage {
                content: "do it".into(),
            },
        ));
        for name in ["run_touch_files", "View"] {
            seq += 1;
            b.events.push(NormalizedEvent::new(
                seq,
                at(2026, 9, 15),
                EventPayload::ToolCall(agentworth_schema::ToolCall {
                    id: None,
                    name: name.to_string(),
                    arguments: serde_json::json!({}),
                }),
            ));
        }
        b.recalculate_stats();
        storage.upsert_session(&b, None, None, 1).unwrap();
        (dir, storage)
    }

    #[test]
    fn compute_over_synthetic_index_matches_golden_counts() {
        let (_dir, storage) = synthetic_fixture();
        let insights = storage.get_insights().unwrap();

        assert_eq!(insights.population.sessions_raw, 2);
        assert_eq!(insights.population.usable_sessions, 2);
        assert_eq!(insights.population.sessions_excluded_pre_2020, 0);

        let vol = &insights.volume;
        assert_eq!(vol.tool_calls_witnessed, 8);
        assert_eq!(vol.human_turns_index_proxy, 4);

        // strict: 8 calls / 4 turns = 2.0; heavy (A only, 12 events, um>0): 6/3 = 2.0
        assert_eq!(insights.calls_per_turn.strict, Some(2.0));
        assert_eq!(insights.calls_per_turn.heavy_session_average, Some(2.0));

        let commit = insights
            .ladder
            .iter()
            .find(|r| r.outcome == "commit_observed")
            .expect("commit row");
        assert_eq!(commit.sessions, 1);
        let noe = insights
            .ladder
            .iter()
            .find(|r| r.outcome == "no_outcome_evidence")
            .expect("no evidence row");
        assert_eq!(noe.sessions, 1);
        assert_eq!(insights.verified.sessions, 1);
        assert_eq!(insights.verified.share_pct, 50.0);

        assert_eq!(
            insights
                .turn_buckets
                .iter()
                .find(|b| b.bucket == "1")
                .unwrap()
                .sessions,
            1
        );
        assert_eq!(
            insights
                .turn_buckets
                .iter()
                .find(|b| b.bucket == "3-5")
                .unwrap()
                .sessions,
            1
        );

        assert_eq!(insights.tool_buckets.shell_edit, 3);
        assert_eq!(insights.tool_buckets.read_explore, 2);
        assert_eq!(insights.tool_buckets.file_edit, 1);
        assert_eq!(insights.tool_buckets.other, 2);

        assert_eq!(insights.file_modifications.total, 2);
        assert_eq!(insights.top_repos.len(), 1);
        assert_eq!(insights.top_repos[0].repo, "org");
        assert_eq!(insights.top_repos[0].file_touches, 2);

        assert_eq!(insights.models.len(), 1);
        assert_eq!(insights.models[0].model, "model-x");
        assert_eq!(insights.models[0].total_tokens, 1_000 + 100 + 3_000 + 200);

        assert_eq!(insights.series.sessions_by_month.len(), 2);

        assert_eq!(insights.top_sessions[0].session_id, "sess-a");
        assert_eq!(insights.top_sessions[0].repo_label, "org");

        let dims: Vec<&str> = insights
            .deferred
            .iter()
            .map(|d| d.dimension.as_str())
            .collect();
        assert!(dims.contains(&"friction_triggers"));
        assert!(dims.contains(&"time_of_day_histogram"));
        assert!(dims.contains(&"vocabulary_mentions"));
    }

    #[test]
    fn parse_window_normalizes_and_validates() {
        assert!(parse_window(None, None).unwrap().is_none());
        // --until without --since is meaningless
        assert!(parse_window(None, Some("2026-09-01T00:00:00Z".into())).is_err());
        // bad input
        assert!(parse_window(Some("not-a-time".into()), None).is_err());
        // since must precede until
        assert!(
            parse_window(
                Some("2026-09-01T00:00:00Z".into()),
                Some("2026-07-01T00:00:00Z".into())
            )
            .is_err()
        );
        // Z-normalizes to +00:00 millis
        let w = parse_window(Some("2026-07-01T00:00:00Z".into()), None)
            .unwrap()
            .unwrap();
        assert_eq!(w.since, "2026-07-01T00:00:00.000+00:00");
        assert_eq!(w.until, "");
    }

    #[test]
    fn windowed_metrics_and_deltas_over_synthetic_index() {
        let (_dir, storage) = synthetic_fixture();

        // Window with both bounds: every usable session sits inside.
        let w = parse_window(
            Some("2026-05-01T00:00:00Z".into()),
            Some("2026-10-01T00:00:00Z".into()),
        )
        .unwrap()
        .unwrap();
        let insights = storage.get_insights_windowed(&w).unwrap();
        assert_eq!(insights.population.usable_sessions, 2);
        assert_eq!(
            insights.filter.since.as_deref(),
            Some("2026-05-01T00:00:00.000+00:00")
        );
        // deltas: dur = 153d; previous window [Nov 29 2025, May 1 2026) is empty.
        assert!(insights.deltas.previous_window.is_some());
        assert_eq!(insights.deltas.usable_sessions.current, 2);
        assert_eq!(insights.deltas.usable_sessions.previous, 0);
        assert_eq!(insights.deltas.usable_sessions.delta, 2);
        assert_eq!(
            insights.deltas.usable_sessions.delta_pct,
            serde_json::Value::Null
        );
        assert_eq!(insights.deltas.token_burn.previous, 0);
        // friction is structurally unavailable, never a zero
        assert!(insights.deltas.friction_rate.reason.is_some());
        assert_eq!(
            insights.deltas.friction_rate.current,
            serde_json::Value::Null
        );

        // Window [Jul 1, auto horizon): the horizon is nudged 1ms past MAX(started_at), so
        // the current window holds only sess-b, and the previous window of equal length
        // lands on sess-a (Jun 1).
        let w2 = parse_window(Some("2026-07-01T00:00:00Z".into()), None)
            .unwrap()
            .unwrap();
        let insights2 = storage.get_insights_windowed(&w2).unwrap();
        assert_eq!(insights2.population.usable_sessions, 1);
        let prev = insights2.deltas.previous_window.as_ref().unwrap();
        assert_eq!(prev.until, "2026-07-01T00:00:00.000+00:00");
        // sess-a (Jun 1) sits in the previous window
        assert_eq!(insights2.deltas.usable_sessions.current, 1);
        assert_eq!(insights2.deltas.usable_sessions.previous, 1);
        assert_eq!(insights2.deltas.usable_sessions.delta, 0);
        // sess-b is unverified: rate falls 100 -> 0 across the boundary
        assert_eq!(insights2.deltas.verified_outcome_rate.current, 0.0);
        assert_eq!(insights2.deltas.verified_outcome_rate.previous, 100.0);
        assert_eq!(insights2.deltas.verified_outcome_rate.delta, -100.0);
        assert_eq!(insights2.deltas.tool_calls_per_turn_strict.current, 2.0);
        assert_eq!(insights2.deltas.tool_calls_per_turn_strict.previous, 2.0);
        // token burn: sess-a totals 1_000+100+3_000+200; sess-b has no model usage
        assert_eq!(insights2.deltas.token_burn.current, 0);
        assert_eq!(insights2.deltas.token_burn.previous, 4_300);
        assert_eq!(insights2.deltas.token_burn.delta, -4_300);
        assert_eq!(insights2.deltas.token_burn.delta_pct, -100.0);

        // All-time: no previous window, single-sided metrics.
        let all = storage.get_insights().unwrap();
        assert!(all.deltas.previous_window.is_none());
        assert_eq!(all.deltas.usable_sessions.current, 2);
        assert_eq!(all.deltas.usable_sessions.previous, serde_json::Value::Null);
        assert_eq!(all.deltas.usable_sessions.delta, serde_json::Value::Null);
    }
}
