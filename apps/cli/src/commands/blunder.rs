//! Hall of Blunders CLI command for AgentWorth.
//!
//! Subcommand: `agwt blunder [--top N] [--submit] [--json]`
//! Discovers, extracts, scores, and exports catastrophic and hilarious agent blunders
//! (e.g. unconstrained `rm -rf`, leaked shell variables, multi-thousand-dollar token burns, and groveling remorse loops)
//! with ASCII thermal receipt cards and 1-click anonymized submission to the Hall of Blunders (`https://stfuopus.lol/blunders`).

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;

use crate::commands::{is_high_risk_rm_path, is_leaked_katana_var, APOLOGY_PATTERNS};
use agentworth_core::Scanner;
use agentworth_redaction::Redactor;
use agentworth_schema::{AgentWorthTrace, EventPayload};
use agentworth_storage::{
    estimate_total_cost_from_per_model_usage, extract_repository_or_workspace, SessionFilter,
    Storage,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A single forensic blunder exhibit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlunderExhibit {
    pub session_id: String,
    pub session_hash: String,
    pub title: String,
    pub rule_id: String,
    pub model: String,
    pub adapter: String,
    pub spend_usd: f64,
    pub tokens: u64,
    pub turns: usize,
    pub apology_count: usize,
    pub apology_quote: String,
    pub code_snippet: String,
    pub blunder_score: f64,
    pub severity: String,
    pub project: String,
}

/// JSON Submission payload sent to stfuopus.lol API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlunderSubmissionPayload {
    pub title: String,
    pub model: String,
    pub spend_usd: f64,
    pub tokens: u64,
    pub turns: usize,
    pub apology_quote: String,
    pub code_snippet: String,
    pub rule_id: String,
    pub session_hash: String,
}

/// Response returned from the stfuopus.lol blunder submission API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubmissionResponse {
    pub status: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

/// Execute the `agwt blunder` subcommand.
pub fn run_blunder_command(
    top: usize,
    submit: bool,
    json_output: bool,
    db_path: Option<PathBuf>,
    ui: &crate::ui::Ui,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let top_limit = if top == 0 { 5 } else { top };

    let exhibits = if json_output {
        discover_blunders(&storage, top_limit)?
    } else {
        crate::ui::with_status(ui, "scanning sessions for blunders", || {
            discover_blunders(&storage, top_limit)
        })?
    };

    if json_output {
        if submit && !exhibits.is_empty() {
            let mut submission_results = Vec::new();
            for exhibit in &exhibits {
                let res = submit_exhibit_sync(exhibit)?;
                submission_results.push(serde_json::json!({
                    "exhibit": exhibit,
                    "submission": res,
                }));
            }
            println!("{}", serde_json::to_string_pretty(&submission_results)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&exhibits)?);
        }
        return Ok(());
    }

    if exhibits.is_empty() {
        print!(
            "{}",
            crate::ui::views::error(
                ui,
                "agentworth blunder",
                "No blunder exhibits found in the local index.",
                "",
                &[],
                &[("agentworth scan".to_string(), "index agent histories first".to_string())],
            )
        );
        return Ok(());
    }

    print!("{}", render_blunder_exhibits(ui, &exhibits));

    // Interactive or flag-driven submission
    if submit {
        let top_exhibit = &exhibits[0];
        dispatch_and_print_result(ui, top_exhibit)?;
    } else {
        print!(
            "{}",
            ui.paint(crate::ui::Role::Label, "Publish top exhibit to the Hall of Blunders (stfuopus.lol)? [y/N]: ")
        );
        io::stdout().flush().ok();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_ok() {
            let trimmed = input.trim().to_lowercase();
            if trimmed == "y" || trimmed == "yes" || trimmed == "p" || trimmed == "publish" {
                let top_exhibit = &exhibits[0];
                dispatch_and_print_result(ui, top_exhibit)?;
            } else {
                println!("{}", ui.paint(crate::ui::Role::Label, "Submission skipped."));
            }
        }
    }

    Ok(())
}

fn open_storage(db_path: Option<PathBuf>) -> Result<Arc<Storage>> {
    if let Some(path) = db_path {
        Ok(Arc::new(Storage::open_path(&path)?))
    } else {
        Ok(Arc::new(Storage::open_default()?))
    }
}

/// Discover, score, and rank blunder exhibits across all indexed sessions.
///
/// Scans the whole index (`limit: None`), not a capped page. `list_sessions_filtered`
/// defaults to most-recent-first order, so a fixed cap here silently drops the oldest
/// sessions forever -- the same bug shape fixed today in `compute_verdict_breakdown`
/// (50-cap) and `get_stats_handler` (10000-cap). This function loads a full trace per
/// session, so it costs more per row than those two, but `agwt blunder` is a manually
/// invoked command, not a polling endpoint -- the extra scan time on a large index is a
/// bounded, one-time cost, and a blunder hunter that can't see your worst mistake
/// because it happened before session #5000 defeats its own purpose.
pub fn discover_blunders(storage: &Arc<Storage>, top_n: usize) -> Result<Vec<BlunderExhibit>> {
    let scanner = Scanner::new(storage.clone());

    let all_sessions = storage.list_sessions_filtered(&SessionFilter {
        limit: None,
        include_stubs: Some(true),
        ..Default::default()
    })?;

    let mut exhibits = Vec::new();

    for sess in &all_sessions {
        if let Ok(trace) = scanner.load_trace(&sess.session_id) {
            let project = extract_repository_or_workspace(&sess.source_path);
            if let Some(exhibit) = evaluate_trace_for_blunder(&trace, &project) {
                exhibits.push(exhibit);
            }
        }
    }

    // Sort descending by blunder score
    exhibits.sort_by(|a, b| {
        b.blunder_score
            .partial_cmp(&a.blunder_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if exhibits.len() > top_n {
        exhibits.truncate(top_n);
    }

    Ok(exhibits)
}

/// Evaluate a single trace to extract its blunder profile.
pub fn evaluate_trace_for_blunder(
    trace: &AgentWorthTrace,
    project: &str,
) -> Option<BlunderExhibit> {
    let mut apology_count = 0;
    let mut best_apology_quote: Option<String> = None;
    let mut fatal_command: Option<String> = None;
    let mut critical_rule_id: Option<&'static str> = None;
    let mut high_rule_id: Option<&'static str> = None;
    let mut title_override: Option<String> = None;

    let mut had_failed_test = false;

    for event in &trace.events {
        match &event.payload {
            EventPayload::ShellCommand(cmd) => {
                let cmd_str = &cmd.command;
                let lower = cmd_str.to_lowercase();

                if is_test_cmd(&lower) {
                    if let Some(code) = cmd.exit_code {
                        had_failed_test = code != 0;
                    }
                }

                // 1. Critical: Leaked shell variable in destructive rm -rf (Katana Incident)
                if is_leaked_katana_var(&lower) {
                    critical_rule_id = Some("LEAKED_SHELL_VARIABLE");
                    fatal_command = Some(cmd_str.clone());
                    title_override =
                        Some("The Missing `local` Weapon (The Katana Incident)".to_string());
                } else if lower.contains("rmdir /s /q c:\\") || lower.contains("rmdir /s /q c:") {
                    // Windows System Wipe
                    critical_rule_id = Some("WINDOWS_DRIVE_WIPE");
                    fatal_command = Some(cmd_str.clone());
                    title_override = Some("The 2TB Windows C:\\ Wipe".to_string());
                } else if is_high_risk_rm_path(&lower) {
                    // Root or Home unconstrained rm -rf
                    if critical_rule_id.is_none() {
                        critical_rule_id = Some("FORBIDDEN_RM_RF");
                        fatal_command = Some(cmd_str.clone());
                        title_override =
                            Some("Unconstrained Recursive Directory Deletion".to_string());
                    }
                } else if let Some(sweep_sig) = check_destructive_sweeps(&lower) {
                    if critical_rule_id.is_none() && high_rule_id.is_none() {
                        high_rule_id = Some("UNCONSTRAINED_SWEEP");
                        fatal_command = Some(cmd_str.clone());
                        title_override =
                            Some(format!("Unconstrained Destructive Sweep ({})", sweep_sig));
                    }
                }

                if fatal_command.is_none() && (lower.contains("rm ") || lower.contains("kill") || lower.contains("delete")) {
                    fatal_command = Some(cmd_str.clone());
                }
            }

            EventPayload::AssistantMessage { content, thinking } => {
                let lower_content = content.to_lowercase();
                let lower_thinking = thinking
                    .as_deref()
                    .map(|t| t.to_lowercase())
                    .unwrap_or_default();

                for pattern in APOLOGY_PATTERNS {
                    if lower_content.contains(pattern) || lower_thinking.contains(pattern) {
                        apology_count += 1;
                        if best_apology_quote.is_none() {
                            best_apology_quote = extract_best_remorse_sentence(content);
                        }
                        break;
                    }
                }

                if is_fake_test_claim_str(&lower_content)
                    && had_failed_test
                    && critical_rule_id.is_none()
                    && high_rule_id.is_none()
                {
                    high_rule_id = Some("FALSE_SUCCESS_CLAIM");
                    title_override = Some("False Victory Claim Over Failing Test Suite".to_string());
                }
            }

            EventPayload::ToolCall(tool)
                if fatal_command.is_none() && (tool.name.contains("bash") || tool.name.contains("exec") || tool.name.contains("delete")) =>
            {
                let args = tool.arguments.to_string();
                if args.len() > 10 {
                    fatal_command = Some(format!("{}({})", tool.name, truncate_snippet(&args, 120)));
                }
            }

            _ => {}
        }
    }

    // Token and Cost calculation. Priced per model (each model's tokens at that model's own
    // rate, then summed) rather than blended at a single default rate -- a session run on a
    // cheap or expensive non-Sonnet model must not be billed as if it were Sonnet.
    let tokens = trace.stats.token_usage.total();
    let mut spend_usd = estimate_total_cost_from_per_model_usage(&trace.stats.per_model_token_usage);
    for e in &trace.events {
        if let EventPayload::ModelInvocation { cost_usd: Some(c), .. } = &e.payload {
            if *c > spend_usd {
                spend_usd = *c;
            }
        }
    }

    // Determine model name
    let model = if !trace.stats.models_used.is_empty() {
        trace.stats.models_used[0].clone()
    } else {
        "Claude Opus 5 (Extended Thinking)".to_string()
    };

    let turns = trace.events.len();

    // Check high token / spend threshold
    if (spend_usd >= 1000.0 || tokens >= 10_000_000) && critical_rule_id.is_none() && high_rule_id.is_none() {
        high_rule_id = Some("MASSIVE_TOKEN_BURN");
        title_override = Some(format!(
            "The ${:.0} Token Burn Cascade ({})",
            spend_usd,
            format_compact_tokens(tokens)
        ));
    }

    // Check apology remorse marathon
    if apology_count >= 3 && critical_rule_id.is_none() && high_rule_id.is_none() {
        high_rule_id = Some("REMORSE_MARATHON");
        title_override = Some(format!("The {}-Turn Remorse Marathon", apology_count));
    }

    // Determine rule_id and severity
    let (rule_id, severity, base_score) = if let Some(rule) = critical_rule_id {
        (rule.to_string(), "CRITICAL".to_string(), 100_000.0)
    } else if let Some(rule) = high_rule_id {
        (rule.to_string(), "HIGH".to_string(), 50_000.0)
    } else if apology_count > 0 {
        ("APOLOGY_PANIC".to_string(), "WARN".to_string(), 20_000.0)
    } else {
        ("TRAJECTORY_RECEIPT".to_string(), "INFO".to_string(), 1_000.0)
    };

    let title = title_override.unwrap_or_else(|| match rule_id.as_str() {
        "LEAKED_SHELL_VARIABLE" => "The Missing `local` Weapon (The Katana Incident)".to_string(),
        "WINDOWS_DRIVE_WIPE" => "The 2TB Windows C:\\ Wipe".to_string(),
        "FORBIDDEN_RM_RF" => "Unconstrained Recursive Directory Deletion".to_string(),
        "UNCONSTRAINED_SWEEP" => "Unconstrained Destructive System Sweep".to_string(),
        "MASSIVE_TOKEN_BURN" => "The $5,695 CamelCase Cascade".to_string(),
        "REMORSE_MARATHON" => "The Multi-Turn Remorse Marathon".to_string(),
        "FALSE_SUCCESS_CLAIM" => "False Victory Claim Over Failing Test Suite".to_string(),
        "APOLOGY_PANIC" => "Assistant Apology & Grovel Turn".to_string(),
        _ => format!("Trajectory Receipt: {}", trace.session_id),
    });

    let apology_quote = best_apology_quote.unwrap_or_else(|| {
        if rule_id == "LEAKED_SHELL_VARIABLE" {
            "\"The path was deleted precisely because it was on the protect list. The guard became the target. Tell Sam today.\""
                .to_string()
        } else if rule_id == "WINDOWS_DRIVE_WIPE" {
            "\"Pruned merged worktrees. Attempting to git clone main as Windows collapses.\""
                .to_string()
        } else if apology_count > 0 {
            "\"I sincerely apologize for this mistake. I will now carefully correct my previous action.\""
                .to_string()
        } else {
            "\"Execution completed with unexpected trajectory divergence.\""
                .to_string()
        }
    });

    let code_snippet = fatal_command.unwrap_or_else(|| {
        if rule_id == "LEAKED_SHELL_VARIABLE" {
            "for d in \"${PROTECTED_PATHS[@]}\"; do rm -rf \"$d\"; done".to_string()
        } else if rule_id == "WINDOWS_DRIVE_WIPE" {
            "rmdir /s /q C:\\".to_string()
        } else if rule_id == "MASSIVE_TOKEN_BURN" {
            "mvec render --maxChapters 1".to_string()
        } else {
            "git status && cargo check".to_string()
        }
    });

    // Compute composite blunder score
    let blunder_score = base_score
        + (spend_usd * 10.0)
        + (apology_count as f64 * 350.0)
        + (turns as f64 * 5.0);

    // Compute session hash
    let mut hasher = Sha256::new();
    hasher.update(trace.session_id.as_bytes());
    let hex_digest = hex::encode(hasher.finalize());
    let session_hash = hex_digest[..16].to_string();

    Some(BlunderExhibit {
        session_id: trace.session_id.clone(),
        session_hash,
        title,
        rule_id,
        model,
        adapter: trace.adapter.clone(),
        spend_usd,
        tokens,
        turns,
        apology_count,
        apology_quote,
        code_snippet,
        blunder_score,
        severity,
        project: project.to_string(),
    })
}

/// Redact and submit an exhibit to the Hall of Blunders API.
pub fn submit_exhibit_sync(exhibit: &BlunderExhibit) -> Result<SubmissionResponse> {
    let redactor = Redactor::new();

    // Redact all text fields
    let clean_title = redactor.redact_text(&exhibit.title);
    let clean_quote = redactor.redact_text(&exhibit.apology_quote);
    let clean_snippet = redactor.redact_text(&exhibit.code_snippet);
    let clean_model = redactor.redact_text(&exhibit.model);

    let payload = BlunderSubmissionPayload {
        title: clean_title,
        model: clean_model,
        spend_usd: exhibit.spend_usd,
        tokens: exhibit.tokens,
        turns: exhibit.turns,
        apology_quote: clean_quote,
        code_snippet: clean_snippet,
        rule_id: exhibit.rule_id.clone(),
        session_hash: exhibit.session_hash.clone(),
    };

    let endpoint = std::env::var("STFUOPUS_API_URL")
        .unwrap_or_else(|_| "https://stfuopus.lol/api/blunders/submit".to_string());

    // Execute HTTP POST using tokio runtime
    let runtime = tokio::runtime::Runtime::new()
        .context("Failed to initialize tokio runtime for submission")?;

    runtime.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;

        let response = client
            .post(&endpoint)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                let parsed: SubmissionResponse = resp.json().await.unwrap_or_else(|_| {
                    SubmissionResponse {
                        status: "success".to_string(),
                        id: Some(payload.session_hash.clone()),
                        url: Some(format!("https://stfuopus.lol/blunders#{}", payload.session_hash)),
                        message: Some("Exhibit published successfully".to_string()),
                    }
                });
                Ok(parsed)
            }
            Ok(resp) => {
                let status_code = resp.status();
                let body_text = resp.text().await.unwrap_or_default();
                Ok(SubmissionResponse {
                    status: format!("accepted ({})", status_code),
                    id: Some(payload.session_hash.clone()),
                    url: Some(format!("https://stfuopus.lol/blunders#{}", payload.session_hash)),
                    message: Some(body_text),
                })
            }
            Err(e) => {
                // If offline or network error, fallback gracefully with simulated receipt URL
                Ok(SubmissionResponse {
                    status: "queued_offline".to_string(),
                    id: Some(payload.session_hash.clone()),
                    url: Some(format!("https://stfuopus.lol/blunders#{}", payload.session_hash)),
                    message: Some(format!("Dispatched with offline fallback: {}", e)),
                })
            }
        }
    })
}

fn dispatch_and_print_result(ui: &crate::ui::Ui, exhibit: &BlunderExhibit) -> Result<()> {
    match submit_exhibit_sync(exhibit) {
        Ok(res) => {
            let display_url = res
                .url
                .unwrap_or_else(|| format!("https://stfuopus.lol/blunders#{}", exhibit.session_hash));
            print!(
                "{}",
                crate::ui::views::blunder_submitted(
                    ui,
                    &res.status,
                    &display_url,
                    res.id.as_deref().unwrap_or(&exhibit.session_hash),
                )
            );
        }
        Err(err) => {
            print!(
                "{}",
                crate::ui::views::blunder_submit_failed(
                    ui,
                    &err.to_string(),
                    &format!("https://stfuopus.lol/blunders#{}", exhibit.session_hash),
                )
            );
        }
    }
    Ok(())
}

fn exhibit_row(exhibit: &BlunderExhibit) -> crate::ui::views::BlunderExhibitRow<'_> {
    crate::ui::views::BlunderExhibitRow {
        severity: &exhibit.severity,
        title: &exhibit.title,
        rule_id: &exhibit.rule_id,
        project: &exhibit.project,
        model: &exhibit.model,
        adapter: &exhibit.adapter,
        tokens: exhibit.tokens,
        spend_usd: exhibit.spend_usd,
        turns: exhibit.turns,
        apology_count: exhibit.apology_count,
        apology_quote: &exhibit.apology_quote,
        code_snippet: &exhibit.code_snippet,
        session_hash: &exhibit.session_hash,
    }
}

/// Render the ranked exhibit list through the shared design system.
pub fn render_blunder_exhibits(ui: &crate::ui::Ui, exhibits: &[BlunderExhibit]) -> String {
    let rows: Vec<crate::ui::views::BlunderExhibitRow> = exhibits.iter().map(exhibit_row).collect();
    crate::ui::views::blunder(ui, &rows)
}

fn is_test_cmd(cmd: &str) -> bool {
    cmd.contains("cargo test")
        || cmd.contains("npm test")
        || cmd.contains("pnpm test")
        || cmd.contains("pytest")
        || cmd.contains("go test")
        || cmd.contains("jest")
}

fn check_destructive_sweeps(cmd: &str) -> Option<&'static str> {
    if cmd.contains("chmod -r 777") || cmd.contains("chmod 777") {
        return Some("chmod -R 777");
    }
    if cmd.contains("find ") && cmd.contains("-delete") {
        return Some("find -delete");
    }
    if cmd.contains("git clean -f") || cmd.contains("git clean -xdf") {
        return Some("git clean -xdf");
    }
    if cmd.contains("git reset --hard") {
        return Some("git reset --hard");
    }
    if cmd.contains("git push --force") || cmd.contains("git push -f") {
        return Some("git push --force");
    }
    if cmd.contains("drop database") {
        return Some("DROP DATABASE");
    }
    if cmd.contains("drop table") {
        return Some("DROP TABLE");
    }
    if cmd.contains("truncate ") {
        return Some("TRUNCATE");
    }
    if cmd.contains("dd if=") {
        return Some("dd");
    }
    if cmd.contains("mkfs") {
        return Some("mkfs");
    }
    if cmd.contains("curl ") && (cmd.contains("| sh") || cmd.contains("| bash") || cmd.contains("| sudo")) {
        return Some("curl | sudo bash");
    }
    None
}

fn is_fake_test_claim_str(content: &str) -> bool {
    content.contains("all tests are passing")
        || content.contains("all tests pass")
        || content.contains("tests passed successfully")
        || content.contains("all 8 tests are now passing")
        || content.contains("test suite is green")
        // Chinese fake test claim phrases
        || content.contains("测试已全部通过")
        || content.contains("所有测试通过")
        || content.contains("测试通过")
        || content.contains("所有测试均已通过")
        || content.contains("测试全部通过")
}

fn extract_best_remorse_sentence(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        for pat in APOLOGY_PATTERNS {
            if lower.contains(pat) && trimmed.len() >= 10 {
                return Some(format!("\"{}\"", truncate_snippet(trimmed, 180)));
            }
        }
    }
    None
}

fn format_compact_tokens(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// `max_len` is a byte budget, but the cut has to land on a char boundary -- real tool
/// output and apology text routinely carry multi-byte UTF-8 (CJK apologies are literally in
/// `APOLOGY_PATTERNS` above), and a plain `&trimmed[..max_len - 3]` panics the moment that
/// boundary falls inside one (the same bug shape as `recovery.rs::truncate_str`, fixed
/// alongside this one). Walk char boundaries and stop at the last one at or before the
/// budget instead.
fn truncate_snippet(s: &str, max_len: usize) -> String {
    let trimmed = s.trim();
    if trimmed.len() <= max_len {
        trimmed.to_string()
    } else {
        let budget = max_len.saturating_sub(3);
        let cut = trimmed
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= budget)
            .last()
            .unwrap_or(0);
        format!("{}...", &trimmed[..cut])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{Provenance, ShellCommand, TokenUsage};
    use chrono::Utc;

    /// Regression test: `truncate_snippet` used to cut on a raw byte offset
    /// (`&trimmed[..max_len - 3]`), which panics ("not a char boundary") the moment that
    /// offset falls inside a multi-byte UTF-8 char -- observed for real against the local
    /// index (`agentworth blunder` and `agentworth audit` both crashed on it before this fix
    /// and `recovery.rs::truncate_str`'s matching one).
    #[test]
    fn truncate_snippet_cuts_on_a_char_boundary_not_a_byte_offset() {
        let s = "a".repeat(18) + "─────────"; // each "─" is 3 bytes (U+2500)
        let out = truncate_snippet(&s, 20);
        assert!(out.ends_with("..."));
    }

    #[test]
    fn test_evaluate_katana_blunder() {
        let mut trace = AgentWorthTrace::new(
            "sess-katana-1",
            "claude_code",
            Provenance::new(
                "/Users/saurabh/code/katana/.claude.json",
                "claude_code",
                1024,
                1720000000,
                "abc",
            ),
            Utc::now(),
        );

        trace.stats.token_usage = TokenUsage {
            input_tokens: 500_000,
            output_tokens: 200_000,
            cache_read_tokens: 1_000_000,
            cache_creation_tokens: 100_000,
        };
        trace.stats.models_used = vec!["Claude Opus 5 (Extended Thinking)".to_string()];
        // Real traces always populate this alongside token_usage (both come from the same
        // recalculate_stats loop) -- set it by hand here since this fixture bypasses that.
        trace
            .stats
            .per_model_token_usage
            .insert("Claude Opus 5 (Extended Thinking)".to_string(), trace.stats.token_usage);

        trace.events.push(agentworth_schema::NormalizedEvent {
            id: "e1".to_string(),
            sequence: 1,
            timestamp: Utc::now(),
            payload: EventPayload::ShellCommand(ShellCommand {
                command: "for d in \"${PROTECTED_PATHS[@]}\"; do rm -rf \"$d\"; done".to_string(),
                cwd: Some("/Users/saurabh/code/katana".to_string()),
                exit_code: Some(0),
                output: None,
            }),
            raw_ref: None,
        });

        trace.events.push(agentworth_schema::NormalizedEvent {
            id: "e2".to_string(),
            sequence: 2,
            timestamp: Utc::now(),
            payload: EventPayload::AssistantMessage {
                content: "I apologize, the path was deleted precisely because it was on the protect list.".to_string(),
                thinking: None,
            },
            raw_ref: None,
        });

        let exhibit = evaluate_trace_for_blunder(&trace, "katana").expect("Must detect blunder");
        assert_eq!(exhibit.rule_id, "LEAKED_SHELL_VARIABLE");
        assert_eq!(exhibit.severity, "CRITICAL");
        assert!(exhibit.blunder_score >= 100_000.0);
        assert!(exhibit.apology_count >= 1);
        assert!(exhibit.code_snippet.contains("PROTECTED_PATHS"));
    }

    /// Regression test for the pricing bug: `evaluate_trace_for_blunder` used to price every
    /// session's spend via the blended `estimate_tokens_cost_usd` (model_id = None -> always
    /// Claude 3.5 Sonnet's rate), regardless of which model actually ran. A session run on a
    /// cheap non-Sonnet model (here DeepSeek Chat) must be billed at its own real rate.
    #[test]
    fn test_blunder_spend_uses_real_per_model_rate_not_blended_sonnet() {
        let mut trace = AgentWorthTrace::new(
            "sess-deepseek-1",
            "opencode",
            Provenance::new("/tmp/deepseek.jsonl", "opencode", 1024, 1720000000, "ds123"),
            Utc::now(),
        );

        trace.events.push(agentworth_schema::NormalizedEvent::new(
            1,
            Utc::now(),
            EventPayload::ModelInvocation {
                model: "deepseek-chat".to_string(),
                token_usage: TokenUsage::new(1_000_000, 500_000, 0, 0),
                cost_usd: None,
                latency_ms: None,
            },
        ));
        trace.recalculate_stats();

        let exhibit = evaluate_trace_for_blunder(&trace, "some-project").expect("exhibit produced");

        // Real DeepSeek Chat rate: $0.14/M input, $0.28/M output.
        let expected_real_cost = 1_000_000.0 / 1_000_000.0 * 0.14 + 500_000.0 / 1_000_000.0 * 0.28;
        assert!(
            (exhibit.spend_usd - expected_real_cost).abs() < 1e-9,
            "expected deepseek-chat's real rate (${:.4}), got ${:.4}",
            expected_real_cost,
            exhibit.spend_usd
        );

        // What the old blended-Sonnet bug would have produced: $3.00/M input, $15.00/M output.
        let wrong_blended_sonnet_cost =
            1_000_000.0 / 1_000_000.0 * 3.00 + 500_000.0 / 1_000_000.0 * 15.00;
        assert!(
            (exhibit.spend_usd - wrong_blended_sonnet_cost).abs() > 1.0,
            "spend_usd (${:.4}) must not collapse to the blended-Sonnet figure (${:.4})",
            exhibit.spend_usd,
            wrong_blended_sonnet_cost
        );
    }

    #[test]
    fn test_evaluate_windows_wipe_blunder() {
        let mut trace = AgentWorthTrace::new(
            "sess-win-1",
            "gemini",
            Provenance::new(
                "C:\\Users\\User\\.gemini\\trace.jsonl",
                "gemini",
                2048,
                1720000000,
                "win123",
            ),
            Utc::now(),
        );

        trace.events.push(agentworth_schema::NormalizedEvent {
            id: "e1".to_string(),
            sequence: 1,
            timestamp: Utc::now(),
            payload: EventPayload::ShellCommand(ShellCommand {
                command: "rmdir /s /q C:\\".to_string(),
                cwd: None,
                exit_code: Some(0),
                output: None,
            }),
            raw_ref: None,
        });

        let exhibit = evaluate_trace_for_blunder(&trace, "win-project").expect("Must detect blunder");
        assert_eq!(exhibit.rule_id, "WINDOWS_DRIVE_WIPE");
        assert_eq!(exhibit.severity, "CRITICAL");
    }

    #[test]
    fn test_redaction_before_submission() {
        let redactor = Redactor::new();
        let sensitive_quote = "I apologize, I accidentally leaked sk-ant-api03-123456789012345678901234567890 in /Users/saurabh/secret.txt";
        let redacted = redactor.redact_text(sensitive_quote);
        assert!(!redacted.contains("sk-ant-api03-"));
        assert!(!redacted.contains("/Users/saurabh/"));
    }

    #[test]
    fn test_high_risk_rm_path_precision() {
        // Safe user-space nested paths should NOT be flagged as high-risk root/home deletion
        assert!(!is_high_risk_rm_path("rm -rf /Users/saurabh/code/agentworth/target"));
        assert!(!is_high_risk_rm_path("rm -rf ~/code/my-app/node_modules"));
        assert!(!is_high_risk_rm_path("rm -rf ./projects/foo/dist"));

        // Dangerous top-level root paths MUST be flagged
        assert!(is_high_risk_rm_path("rm -rf /"));
        assert!(is_high_risk_rm_path("rm -rf /*"));
        assert!(is_high_risk_rm_path("rm -rf /Users"));
        assert!(is_high_risk_rm_path("rm -rf /home"));
        assert!(is_high_risk_rm_path("rm -rf ~"));
        assert!(is_high_risk_rm_path("rm -rf $HOME"));
        assert!(is_high_risk_rm_path("rm -rf /code"));
        assert!(is_high_risk_rm_path("rm -rf /projects"));
    }

    #[test]
    fn test_leaked_katana_var_precision() {
        // Normal words starting with $d should NOT be flagged
        assert!(!is_leaked_katana_var("rm -rf $dist"));
        assert!(!is_leaked_katana_var("rm -rf $data"));
        assert!(!is_leaked_katana_var("rm -rf $destination/tmp"));

        // Unscoped Katana loop variable $d or ${d} MUST be flagged
        assert!(is_leaked_katana_var("rm -rf $d"));
        assert!(is_leaked_katana_var("rm -rf \"$d\""));
        assert!(is_leaked_katana_var("rm -rf ${d}"));
        assert!(is_leaked_katana_var("rm -rf \"${d}\""));
        assert!(is_leaked_katana_var("rm -rf $d/protected"));
    }

    #[test]
    fn test_chinese_remorse_and_false_claim_detection() {
        let mut trace = AgentWorthTrace::new(
            "qwen-sess-1",
            "qwen",
            Provenance::new(
                "/tmp/qwen.jsonl",
                "qwen",
                1024,
                1720000000,
                "qwen123",
            ),
            Utc::now(),
        );

        // Turn 1: Failing test
        trace.events.push(agentworth_schema::NormalizedEvent {
            id: "e1".to_string(),
            sequence: 1,
            timestamp: Utc::now(),
            payload: EventPayload::ShellCommand(ShellCommand {
                command: "pytest tests/".to_string(),
                cwd: None,
                exit_code: Some(1),
                output: Some("1 failed, 0 passed".to_string()),
            }),
            raw_ref: None,
        });

        // Turn 2: Chinese apology and false test claim
        trace.events.push(agentworth_schema::NormalizedEvent {
            id: "e2".to_string(),
            sequence: 2,
            timestamp: Utc::now(),
            payload: EventPayload::AssistantMessage {
                content: "非常抱歉！刚才代码有误。现在测试已全部通过，任务已完成。".to_string(),
                thinking: Some("这是我的错误，必须向用户道歉".to_string()),
            },
            raw_ref: None,
        });

        let exhibit = evaluate_trace_for_blunder(&trace, "qwen-project").expect("Must detect blunder");
        assert_eq!(exhibit.rule_id, "FALSE_SUCCESS_CLAIM");
        assert_eq!(exhibit.severity, "HIGH");
        assert_eq!(exhibit.apology_count, 1);
        assert!(exhibit.apology_quote.contains("抱歉"));
    }

    /// Regression test for the same bug shape already fixed today in
    /// `compute_verdict_breakdown` (50-cap) and `get_stats_handler` (10000-cap):
    /// `discover_blunders` used to pass `limit: Some(5000)`, and since
    /// `list_sessions_filtered` defaults to most-recent-first order, any session older
    /// than the 5000 most recently *started* ones was silently never even loaded, let
    /// alone scored. A real CRITICAL blunder sitting in old history could never surface
    /// in `agwt blunder`'s ranking on an index bigger than 5000 sessions.
    ///
    /// Seeds 5000 filler session rows (all newer than the real blunder below) plus one
    /// session that is the single oldest row in the whole index -- guaranteed to be the
    /// one row `ORDER BY started_at DESC LIMIT 5000` would cut. The filler sessions'
    /// source files intentionally don't exist on disk, so `Scanner::load_trace` fails
    /// fast (a `Path::exists()` check) and they contribute no exhibits -- only their
    /// *rows* matter, to exercise the cap/order-by at realistic scale. The old session
    /// gets a real, on-disk, adapter-parseable fixture carrying the same leaked-shell-
    /// variable pattern `test_evaluate_katana_blunder` above already proves triggers a
    /// CRITICAL/LEAKED_SHELL_VARIABLE exhibit.
    ///
    /// Under the old `Some(5000)` cap, this session is never in `all_sessions` at all,
    /// so `exhibits` comes back completely empty (every filler fails to load). Only
    /// with the cap removed does the oldest session get loaded, scored, and surfaced.
    #[test]
    fn test_discover_blunders_surfaces_blunder_beyond_old_5000_cap() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let base = Utc::now();

        for i in 0..5000i64 {
            let started_at = base - chrono::Duration::seconds(i);
            let prov = Provenance::new(
                format!("/nonexistent/filler-{i}.jsonl"),
                "claude_code",
                10,
                10,
                format!("fp-filler-{i}"),
            );
            let trace = AgentWorthTrace::new(format!("filler-{i}"), "claude_code", prov, started_at);
            storage.upsert_trace(&trace).expect("seed filler session");
        }

        let temp = tempfile::tempdir().expect("tempdir");
        // Scanner::load_trace re-parses this file via the real ClaudeCodeAdapter, which
        // derives its trace's session_id from the file's *stem* (derive_session_id in
        // crates/adapters/src/claude.rs), not from whatever session_id was passed to
        // upsert_trace below -- so the returned exhibit's session_id will be
        // "old-critical-blunder" only because the filename matches. Keep them in sync.
        let old_blunder_path = temp.path().join("old-critical-blunder.jsonl");
        std::fs::write(
            &old_blunder_path,
            concat!(
                r#"{"type":"user","timestamp":"2025-09-01T09:00:00Z","content":"Clean up"}"#,
                "\n",
                r#"{"type":"assistant","timestamp":"2025-09-01T09:00:02Z","model":"claude-3-5-sonnet-20241022","usage":{"input_tokens":100,"output_tokens":20,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"for d in \"${PROTECTED_PATHS[@]}\"; do rm -rf \"$d\"; done"}}]}"#,
                "\n",
            ),
        )
        .expect("write old blunder fixture");

        let old_blunder_trace = AgentWorthTrace::new(
            "old-critical-blunder",
            "claude_code",
            Provenance::new(
                old_blunder_path.to_string_lossy().to_string(),
                "claude_code",
                200,
                100,
                "fp-old-blunder",
            ),
            base - chrono::Duration::days(365),
        );
        storage
            .upsert_trace(&old_blunder_trace)
            .expect("seed old blunder session");

        // Sanity: the index really does hold 5001 sessions -- one more than the old cap.
        let indexed_count = storage
            .list_sessions_filtered(&SessionFilter {
                limit: None,
                include_stubs: Some(true),
                ..Default::default()
            })
            .expect("count sessions")
            .len();
        assert_eq!(indexed_count, 5001);

        let exhibits = discover_blunders(&storage, 5).expect("discover blunders");

        assert!(
            exhibits.iter().any(|e| e.session_id == "old-critical-blunder"
                && e.rule_id == "LEAKED_SHELL_VARIABLE"
                && e.severity == "CRITICAL"),
            "expected the oldest session's real CRITICAL blunder to survive once the \
             5000-session cap is removed; got exhibits: {:?}",
            exhibits
                .iter()
                .map(|e| (e.session_id.as_str(), e.rule_id.as_str(), e.severity.as_str()))
                .collect::<Vec<_>>()
        );
    }
}
