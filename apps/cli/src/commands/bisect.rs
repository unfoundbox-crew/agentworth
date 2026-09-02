//! Session Bisect command for AgentWorth.
//!
//! Subcommand: `agentworth bisect <session-id> [--json]`
//! Walks a session's trajectory to pinpoint the exact turning point where the run turned negative
//! (e.g. failing build after edit, file reversion, repeated errors, apology cascade).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use agentworth_core::Scanner;
use agentworth_schema::{AgentWorthTrace, EventPayload};
use agentworth_storage::Storage;
use anyhow::{Context, Result};
use console::style;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegressionReason {
    BuildOrTestFailure,
    FileReversionOrThrashing,
    RepeatedToolError,
    ApologyRemorseTrigger,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BisectResult {
    pub session_id: String,
    pub adapter: String,
    pub total_events: usize,
    pub pivotal_turn_index: Option<usize>,
    pub pivotal_timestamp: Option<String>,
    pub reason: Option<RegressionReason>,
    pub summary: String,
    pub context_snippet: Option<String>,
}

/// Bisect a trace to locate the first inflection point where trajectory turned negative.
pub fn bisect_session_trajectory(trace: &AgentWorthTrace) -> BisectResult {
    let mut file_history: HashMap<String, usize> = HashMap::new();
    let mut had_prior_success = false;

    for event in &trace.events {
        match &event.payload {
            EventPayload::ShellCommand(cmd) => {
                let is_build_or_test = cmd.command.contains("test")
                    || cmd.command.contains("build")
                    || cmd.command.contains("cargo")
                    || cmd.command.contains("npm");

                if let Some(code) = cmd.exit_code {
                    if code == 0 {
                        had_prior_success = true;
                    } else if code != 0 && (had_prior_success || is_build_or_test) {
                        return BisectResult {
                            session_id: trace.session_id.clone(),
                            adapter: trace.adapter.clone(),
                            total_events: trace.events.len(),
                            pivotal_turn_index: Some(event.sequence as usize),
                            pivotal_timestamp: Some(event.timestamp.to_string()),
                            reason: Some(RegressionReason::BuildOrTestFailure),
                            summary: format!(
                                "Command '{}' failed with exit code {}.",
                                cmd.command, code
                            ),
                            context_snippet: cmd.output.clone(),
                        };
                    }
                }
            }
            EventPayload::FileAction { path, .. } => {
                let count = file_history.entry(path.clone()).or_insert(0);
                *count += 1;
                if *count >= 3 {
                    return BisectResult {
                        session_id: trace.session_id.clone(),
                        adapter: trace.adapter.clone(),
                        total_events: trace.events.len(),
                        pivotal_turn_index: Some(event.sequence as usize),
                        pivotal_timestamp: Some(event.timestamp.to_string()),
                        reason: Some(RegressionReason::FileReversionOrThrashing),
                        summary: format!(
                            "File '{}' modified repeatedly (iteration {}), indicating oscillation.",
                            path, count
                        ),
                        context_snippet: Some(format!("Action on {}", path)),
                    };
                }
            }
            EventPayload::ToolResult(res) => {
                if res.is_error {
                    let output_snippet = match &res.output {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    return BisectResult {
                        session_id: trace.session_id.clone(),
                        adapter: trace.adapter.clone(),
                        total_events: trace.events.len(),
                        pivotal_turn_index: Some(event.sequence as usize),
                        pivotal_timestamp: Some(event.timestamp.to_string()),
                        reason: Some(RegressionReason::RepeatedToolError),
                        summary: "Tool execution returned an unhandled error.".to_string(),
                        context_snippet: Some(output_snippet),
                    };
                }
            }
            EventPayload::AssistantMessage { content, .. } => {
                let lower = content.to_lowercase();
                if lower.contains("i apologize")
                    || lower.contains("my mistake")
                    || lower.contains("i made an error")
                    || lower.contains("抱歉")
                    || lower.contains("对不起")
                {
                    return BisectResult {
                        session_id: trace.session_id.clone(),
                        adapter: trace.adapter.clone(),
                        total_events: trace.events.len(),
                        pivotal_turn_index: Some(event.sequence as usize),
                        pivotal_timestamp: Some(event.timestamp.to_string()),
                        reason: Some(RegressionReason::ApologyRemorseTrigger),
                        summary: "Agent recognized a failure and entered an apology loop."
                            .to_string(),
                        context_snippet: Some(if content.chars().count() > 120 {
                            format!("{}...", agentworth_schema::text::truncate_chars(content, 117))
                        } else {
                            content.clone()
                        }),
                    };
                }
            }
            _ => {}
        }
    }

    BisectResult {
        session_id: trace.session_id.clone(),
        adapter: trace.adapter.clone(),
        total_events: trace.events.len(),
        pivotal_turn_index: None,
        pivotal_timestamp: None,
        reason: None,
        summary: "No regression or negative inflection detected in trajectory.".to_string(),
        context_snippet: None,
    }
}

/// Execute the `agentworth bisect` subcommand.
pub fn run_bisect_command(
    session_id: &str,
    json: bool,
    db_path: Option<PathBuf>,
) -> Result<()> {
    let storage = Arc::new(match db_path {
        Some(p) => Storage::open_path(&p)?,
        None => Storage::open_default()?,
    });

    storage
        .get_session_by_id(session_id)?
        .with_context(|| format!("Session '{}' not found in local index.", session_id))?;

    let scanner = Scanner::new(storage.clone());
    let trace = scanner.load_trace(session_id)?;
    let result = bisect_session_trajectory(&trace);

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    println!();
    println!(
        "{}",
        style("┌─ ✂️  AgentWorth Trajectory Bisect ────────────────────────┐").bold().cyan()
    );
    println!(
        "│ Session:  {:<48} │",
        style(&result.session_id).bold()
    );
    println!(
        "│ Adapter:  {:<48} │",
        style(&result.adapter).green()
    );
    println!(
        "{}",
        style("├────────────────────────────────────────────────────────────┤").bold()
    );

    if let Some(turn) = result.pivotal_turn_index {
        println!(
            "│ ⚠️  Inflection Point Detected at Event #{:<25} │",
            style(turn).bold().red()
        );
        if let Some(ts) = &result.pivotal_timestamp {
            println!("│ Timestamp: {:<47} │", style(ts).dim());
        }
        println!(
            "│ Reason:    {:<47} │",
            style(format!("{:?}", result.reason.unwrap())).yellow()
        );
        println!(
            "│ Summary:   {:<47} │",
            style(&result.summary).bold()
        );
        if let Some(ctx) = &result.context_snippet {
            println!(
                "│ Context:   {:<47} │",
                style(if ctx.chars().count() > 44 {
                    format!("{}...", agentworth_schema::text::truncate_chars(ctx, 41))
                } else {
                    ctx.clone()
                }).dim()
            );
        }
    } else {
        println!("│ ✓ Trajectory was clean (no negative turning point found).  │");
    }

    println!(
        "{}",
        style("└────────────────────────────────────────────────────────────┘").bold()
    );
    println!();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{NormalizedEvent, Provenance, ShellCommand};
    use chrono::Utc;

    #[test]
    fn test_bisect_detects_failing_test_after_success() {
        let now = Utc::now();
        let prov = Provenance::new("/tmp/test.jsonl", "claude_code", 100, 1000, "fp1");
        let mut trace = AgentWorthTrace::new("sess-bisect-1", "claude_code", prov, now);

        // Event 1: passing test
        trace.events.push(NormalizedEvent::new(
            1,
            now,
            EventPayload::ShellCommand(ShellCommand {
                command: "cargo test".to_string(),
                cwd: None,
                exit_code: Some(0),
                output: Some("test result: ok".to_string()),
            }),
        ));

        // Event 2: failing test (regression)
        trace.events.push(NormalizedEvent::new(
            2,
            now,
            EventPayload::ShellCommand(ShellCommand {
                command: "cargo test".to_string(),
                cwd: None,
                exit_code: Some(101),
                output: Some("assertion failed".to_string()),
            }),
        ));

        let res = bisect_session_trajectory(&trace);
        assert_eq!(res.pivotal_turn_index, Some(2));
        assert_eq!(res.reason, Some(RegressionReason::BuildOrTestFailure));
    }
}
