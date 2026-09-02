//! `archie stats ladder`: the evidence ladder, what a verified outcome costs, and the newest
//! sessions that left evidence -- one screen, read entirely from the index.
//!
//! The question it answers is the one `docs/specs/archie-bench.md` set out to answer locally:
//! of everything spent in this window, how much of it bought something a test, a commit or a
//! CI run can be pointed at. Every dollar figure is API-equivalent at list prices
//! (`crate::cost_basis`), never what the account was billed.

use std::path::PathBuf;

use agentworth_storage::{
    LadderGroupBy, LadderQuery, LadderResult, Storage, EVIDENCE_FLOOR,
};
use anyhow::{Context, Result};
use serde_json::json;

/// How many group rows the table prints before it starts counting the rest. A real index has
/// forty-odd models and repos; the screen is a screen, not a dump, and the note under it says
/// how many it left out.
const GROUP_ROW_CAP: usize = 8;

/// What `--period` means as a lookback window. `stats usage` uses the same words for a rollup
/// granularity; here they name how far back the window reaches, which is what a ladder over
/// "this period" has to mean.
pub fn period_days(period: &str) -> Option<i64> {
    match period {
        "day" => Some(1),
        "week" => Some(7),
        "month" => Some(30),
        "year" => Some(365),
        _ => None,
    }
}

/// The human label for the resolved window: `30 days`, `all time`.
pub fn period_label(period: &str) -> String {
    match period_days(period) {
        Some(1) => "24 hours".to_string(),
        Some(days) => format!("{days} days"),
        None => "all time".to_string(),
    }
}

pub struct LadderArgs {
    pub period: String,
    pub by: String,
    pub repo: Option<String>,
    pub adapter: Option<String>,
    pub model: Option<String>,
    pub min_n: Option<usize>,
    pub include_stubs: bool,
    pub json: bool,
    pub db_path: Option<PathBuf>,
}

/// Build the query one place, so the CLI and the `stats_ladder` MCP tool ask the index the
/// same question and can never drift on what a period or a filter means.
pub fn ladder_query(
    period: &str,
    by: &str,
    repo: Option<String>,
    adapter: Option<String>,
    model: Option<String>,
    min_n: Option<usize>,
    include_stubs: bool,
) -> LadderQuery {
    LadderQuery {
        since: period_days(period).map(|d| chrono::Utc::now() - chrono::Duration::days(d)),
        until: None,
        repo,
        adapter,
        model,
        group_by: LadderGroupBy::parse(by).unwrap_or(LadderGroupBy::Model),
        min_n: min_n.unwrap_or(agentworth_storage::OUTCOME_RATE_DEFAULT_MIN_N),
        include_stubs,
        recent_limit: 5,
    }
}

/// The JSON body both surfaces return: the three blocks as data, plus the cost-basis label
/// that says what every dollar in them is.
pub fn ladder_json(result: &LadderResult, period: &str) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(result).context("serializing the ladder")?;
    let basis = crate::cost_basis::CostBasis::detect();
    if let Some(obj) = value.as_object_mut() {
        obj.insert("period".to_string(), json!(period));
        obj.insert("cost_basis".to_string(), json!(basis.cost_basis));
        if let Some(tier) = &basis.subscription_tier {
            obj.insert("subscription_tier".to_string(), json!(tier));
        }
    }
    Ok(value)
}

pub fn run_ladder_command(args: LadderArgs, ui: &crate::ui::Ui) -> Result<()> {
    let storage = match &args.db_path {
        Some(path) => Storage::open_path(path)?,
        None => Storage::open_default()?,
    };

    let query = ladder_query(
        &args.period,
        &args.by,
        args.repo.clone(),
        args.adapter.clone(),
        args.model.clone(),
        args.min_n,
        args.include_stubs,
    );
    let result = storage.get_ladder(&query)?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ladder_json(&result, &args.period)?)?
        );
        return Ok(());
    }

    print!("{}", render(ui, &result, &args));
    Ok(())
}

fn render(ui: &crate::ui::Ui, result: &LadderResult, args: &LadderArgs) -> String {
    use crate::ui::views;

    let rungs: Vec<views::LadderRungRowView<'_>> = result
        .rungs
        .iter()
        .map(|r| views::LadderRungRowView {
            rung: r.rung as usize,
            label: views::RUNG_LABELS[r.rung as usize],
            sessions: r.sessions,
            share: r.share,
            median_tokens: r.median_tokens,
            median_cost_usd: r.median_cost_usd,
            spend_usd: r.cost_usd,
            spend_share: r.cost_share,
        })
        .collect();
    let below_sessions: usize = result
        .rungs
        .iter()
        .filter(|r| (r.rung as usize) < EVIDENCE_FLOOR)
        .map(|r| r.sessions)
        .sum();

    let groups: Vec<views::LadderGroupRowView<'_>> = result
        .groups
        .iter()
        .take(GROUP_ROW_CAP)
        .map(|g| views::LadderGroupRowView {
            key: &g.key,
            n: g.n,
            rate: g.rate,
            median_tokens: g.median_tokens,
            median_steps: g.median_steps,
            cost_per_verified_usd: g.cost_per_verified_usd,
        })
        .collect();

    let recent_when: Vec<String> = result
        .recent_verified
        .iter()
        .map(|s| s.started_at.format("%m-%d %H:%M").to_string())
        .collect();
    let recent: Vec<views::LadderSessionRowView<'_>> = result
        .recent_verified
        .iter()
        .zip(recent_when.iter())
        .map(|(s, when)| views::LadderSessionRowView {
            when,
            repo: &s.repo,
            rung: s.rung as usize,
            model: &s.model,
            tokens: s.total_tokens,
            cost_usd: s.cost_usd,
        })
        .collect();

    let filters = [args.repo.as_deref(), args.adapter.as_deref(), args.model.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(&format!(" {} ", ui.dot()));

    let cost_note = crate::cost_basis::CostBasis::detect().label_long();
    let period_label = period_label(&args.period);

    views::ladder(
        ui,
        &views::LadderView {
            period_label: &period_label,
            filters: &filters,
            total_sessions: result.total_sessions,
            group_by: result.group_by.as_str(),
            min_n: result.min_n,
            rungs: &rungs,
            below_sessions,
            below_share: if result.total_sessions > 0 {
                below_sessions as f64 / result.total_sessions as f64
            } else {
                0.0
            },
            below_spend_usd: result.below_line_cost_usd,
            below_spend_share: result.below_line_cost_share,
            groups: &groups,
            groups_hidden: result.groups.len().saturating_sub(GROUP_ROW_CAP),
            sessions_without_effort: if result.group_by == LadderGroupBy::Effort {
                result.sessions_without_effort
            } else {
                0
            },
            recent: &recent,
            cost_note: &cost_note,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_period_is_a_lookback_window_not_a_rollup_bucket() {
        assert_eq!(period_days("day"), Some(1));
        assert_eq!(period_days("month"), Some(30));
        assert_eq!(period_days("all"), None);
        assert_eq!(period_label("month"), "30 days");
        assert_eq!(period_label("all"), "all time");
    }

    /// The floor is the one `stats outcomes` uses, not a second number invented here.
    #[test]
    fn the_default_floor_is_the_shared_one() {
        let q = ladder_query("month", "model", None, None, None, None, false);
        assert_eq!(q.min_n, agentworth_storage::OUTCOME_RATE_DEFAULT_MIN_N);
        assert_eq!(q.group_by, LadderGroupBy::Model);
        assert!(q.since.is_some(), "a 30-day period has a lower bound");

        let all = ladder_query("all", "effort", None, None, None, Some(3), false);
        assert!(all.since.is_none(), "all time has no lower bound");
        assert_eq!(all.min_n, 3);
        assert_eq!(all.group_by, LadderGroupBy::Effort);
    }
}
