//! Cache Hit-Rate Doctor command for AgentWorth.
//!
//! Subcommand: `agentworth cache-doctor <session-id> [--json]`
//! Analyzes turn-by-turn prompt caching dynamics in a session, pinpointing the exact turn
//! where cache efficiency deteriorated and identifying the root cause (model switch, new tool, payload blowout).

use std::path::PathBuf;
use std::sync::Arc;
use agentworth_core::Scanner;
use agentworth_schema::{AgentWorthTrace, EventPayload};
use agentworth_storage::{calculate_cache_hit_ratio, Storage};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnCacheMetric {
    pub turn_index: usize,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_hit_ratio: f64,
    pub model: Option<String>,
    pub tool_introduced: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheDropFinding {
    pub turn_index: usize,
    pub previous_hit_ratio: f64,
    pub new_hit_ratio: f64,
    pub drop_percentage: f64,
    pub probable_cause: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheDoctorDiagnosis {
    pub session_id: String,
    pub adapter: String,
    pub total_turns: usize,
    pub average_cache_hit_ratio: f64,
    pub turn_metrics: Vec<TurnCacheMetric>,
    pub drop_findings: Vec<CacheDropFinding>,
}

/// Diagnose prompt caching behavior across a trace.
pub fn diagnose_cache_efficiency(trace: &AgentWorthTrace) -> CacheDoctorDiagnosis {
    let mut turn_metrics: Vec<TurnCacheMetric> = Vec::new();
    let mut drop_findings = Vec::new();

    let mut current_model: Option<String> = None;
    let mut known_tools = std::collections::HashSet::new();

    let mut turn_idx = 0usize;
    let mut total_hit_ratio_sum = 0.0f64;

    for event in &trace.events {
        match &event.payload {
            EventPayload::ModelInvocation { model, token_usage, .. } => {
                turn_idx += 1;
                let input = token_usage.input_tokens;
                let read = token_usage.cache_read_tokens;
                let create = token_usage.cache_creation_tokens;
                let total_in = input + read + create;

                let hit_ratio = if total_in > 0 {
                    (read as f64 / total_in as f64) * 100.0
                } else {
                    0.0
                };

                let prev_model = current_model.clone();
                current_model = Some(model.clone());

                let metric = TurnCacheMetric {
                    turn_index: turn_idx,
                    input_tokens: input,
                    cache_read_tokens: read,
                    cache_creation_tokens: create,
                    cache_hit_ratio: hit_ratio,
                    model: current_model.clone(),
                    tool_introduced: None,
                };

                total_hit_ratio_sum += hit_ratio;

                // Check for drop relative to previous turn
                if let Some(prev) = turn_metrics.last() {
                    let prev_ratio: f64 = prev.cache_hit_ratio;
                    if prev_ratio >= 30.0 && hit_ratio < (prev_ratio - 25.0) {
                        let cause = if prev_model != current_model && current_model.is_some() {
                            format!(
                                "Model switched from '{}' to '{}', invalidating prompt cache prefix.",
                                prev_model.as_deref().unwrap_or("unknown"),
                                current_model.as_deref().unwrap_or("unknown")
                            )
                        } else if create > 50_000 {
                            format!(
                                "Large prompt prefix mutation: {} tokens written to cache.",
                                create
                            )
                        } else {
                            "Prompt context divergence or prefix mutation invalidated cache.".to_string()
                        };

                        drop_findings.push(CacheDropFinding {
                            turn_index: turn_idx,
                            previous_hit_ratio: prev_ratio,
                            new_hit_ratio: hit_ratio,
                            drop_percentage: prev_ratio - hit_ratio,
                            probable_cause: cause,
                        });
                    }
                }

                turn_metrics.push(metric);
            }
            EventPayload::ToolCall(tool) if known_tools.insert(tool.name.clone()) => {
                if let Some(last) = turn_metrics.last_mut() {
                    last.tool_introduced = Some(tool.name.clone());
                }
            }
            _ => {}
        }
    }

    let avg_ratio = if !turn_metrics.is_empty() {
        total_hit_ratio_sum / (turn_metrics.len() as f64)
    } else {
        calculate_cache_hit_ratio(
            trace.stats.token_usage.input_tokens,
            trace.stats.token_usage.cache_read_tokens,
            trace.stats.token_usage.cache_creation_tokens,
        )
    };

    CacheDoctorDiagnosis {
        session_id: trace.session_id.clone(),
        adapter: trace.adapter.clone(),
        total_turns: turn_metrics.len(),
        average_cache_hit_ratio: avg_ratio,
        turn_metrics,
        drop_findings,
    }
}

/// Execute the `agentworth cache-doctor` subcommand.
pub fn run_cache_doctor_command(
    session_id: Option<String>,
    last: bool,
    current: bool,
    json: bool,
    db_path: Option<PathBuf>,
    ui: &crate::ui::Ui,
) -> Result<()> {
    let storage = Arc::new(match db_path {
        Some(p) => Storage::open_path(&p)?,
        None => Storage::open_default()?,
    });

    // Same resolution as every other show-style verb: unique prefix, `--last`/`--current`,
    // the picker on a TTY, exit 2 off one (`crate::ui::picker::resolve_or_exit`).
    let arg = crate::ui::picker::SessionArg::new(session_id, last, current);
    let session_id = crate::ui::picker::resolve_or_exit(&storage, ui, json, "session cache", &arg)?;
    let session_id = session_id.as_str();

    let scanner = Scanner::new(storage.clone());
    let trace = crate::ui::with_status(ui, "loading session", || scanner.load_trace(session_id))?;
    let diagnosis = diagnose_cache_efficiency(&trace);

    if json {
        println!("{}", serde_json::to_string_pretty(&diagnosis)?);
        return Ok(());
    }

    let view = crate::ui::views::CacheDoctorView {
        session_id: &diagnosis.session_id,
        adapter: &diagnosis.adapter,
        average_hit_ratio: diagnosis.average_cache_hit_ratio,
        drops: diagnosis
            .drop_findings
            .iter()
            .map(|d| crate::ui::views::CacheDoctorDropRow {
                turn_index: d.turn_index,
                previous_ratio: d.previous_hit_ratio,
                new_ratio: d.new_hit_ratio,
                drop_pct: d.drop_percentage,
                cause: &d.probable_cause,
            })
            .collect(),
    };
    print!("{}", crate::ui::views::cache_doctor(ui, &view));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{NormalizedEvent, Provenance, TokenUsage};
    use chrono::Utc;

    #[test]
    fn test_cache_doctor_drop_detection() {
        let now = Utc::now();
        let prov = Provenance::new("/tmp/test.jsonl", "claude_code", 100, 1000, "fp1");
        let mut trace = AgentWorthTrace::new("sess-cache-1", "claude_code", prov, now);

        // Turn 1: High cache hit ratio (90%)
        trace.events.push(NormalizedEvent::new(
            1,
            now,
            EventPayload::ModelInvocation {
                model: "claude-3-5-sonnet".to_string(),
                token_usage: TokenUsage {
                    input_tokens: 1_000,
                    output_tokens: 200,
                    cache_read_tokens: 9_000,
                    cache_creation_tokens: 0,
                },
                cost_usd: None,
                latency_ms: None,
                effort: None,
            },
        ));

        // Turn 2: Sharp drop to 10% after switching model
        trace.events.push(NormalizedEvent::new(
            2,
            now,
            EventPayload::ModelInvocation {
                model: "claude-3-opus".to_string(),
                token_usage: TokenUsage {
                    input_tokens: 9_000,
                    output_tokens: 500,
                    cache_read_tokens: 1_000,
                    cache_creation_tokens: 5_000,
                },
                cost_usd: None,
                latency_ms: None,
                effort: None,
            },
        ));

        let diag = diagnose_cache_efficiency(&trace);
        assert_eq!(diag.total_turns, 2);
        assert_eq!(diag.drop_findings.len(), 1);
        assert_eq!(diag.drop_findings[0].turn_index, 2);
        assert!(diag.drop_findings[0].probable_cause.contains("Model switched"));
    }
}
