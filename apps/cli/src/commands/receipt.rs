//! Flight Receipt generator for AgentWorth traces.
//!
//! Produces authentic ANSI ASCII box receipts for the terminal and standalone
//! 1200x630 dark-mode SVG receipt cards for sharing and social previews,
//! adhering to the SpacePilot Obsidian and Two-Tone Gold visual language.

use std::fmt::Write as FmtWrite;
use std::path::PathBuf;
use std::sync::Arc;

use agentworth_core::Scanner;
use agentworth_outcomes::{evaluate_trace_outcomes, highest_outcome, RecoveryDetector};
use agentworth_schema::{AgentWorthTrace, EventPayload, OutcomeKind};
use agentworth_scoring::{TraceScore, TraceScorer};
use agentworth_storage::{estimate_tokens_cost_usd, Storage};
use anyhow::{Context, Result};
use console::style;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Provenance categorization strictly adhering to Typed Provenance canon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypedProvenanceStatus {
    /// Measured ground-truth execution verified from source files on disk.
    Flown,
    /// Claimed or external documentation without execution verification.
    OnPaper,
    /// Synthetically generated or unproven trace.
    Unflown,
}

impl TypedProvenanceStatus {
    pub fn badge_text(&self) -> &'static str {
        match self {
            Self::Flown => "FLOWN • LOCAL GROUND TRUTH",
            Self::OnPaper => "ON PAPER • CLAIMED",
            Self::Unflown => "UNFLOWN • UNVERIFIED",
        }
    }

    pub fn short_badge(&self) -> &'static str {
        match self {
            Self::Flown => "FLOWN",
            Self::OnPaper => "ON_PAPER",
            Self::Unflown => "UNFLOWN",
        }
    }
}

/// Structured telemetry extracted from a trace for receipt rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlightReceiptData {

    pub session_id: String,
    pub short_session_id: String,
    pub adapter: String,
    pub primary_model: String,
    pub models_used: Vec<String>,
    pub started_at_str: String,
    pub ended_at_str: String,
    pub duration_str: String,
    pub duration_seconds: f64,
    pub total_events: usize,
    pub tool_calls_count: usize,
    pub top_tools: Vec<(String, usize)>,

    // Typed Provenance
    pub provenance_status: TypedProvenanceStatus,
    pub source_path: String,
    pub content_fingerprint: String,
    pub receipt_hash: String,

    // Score & Outcome
    pub composite_score: f64,
    pub outcome_score: f64,
    pub verifiability_score: f64,
    pub complexity_score: f64,
    pub recovery_score: f64,
    pub provenance_score: f64,
    pub verdict_badge: String,
    pub verdict_rung: usize,
    pub verdict_label: String,
    pub highest_outcome_summary: Option<String>,

    // Token Burn & Financials
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub spend_usd: f64,

    // Apology Tax & Remorse Audit
    pub apology_count: usize,
    pub apology_tax_usd: f64,
    pub apology_tax_tokens: u64,
    pub best_apology_quote: Option<String>,

    // Autonomous Resilience & Recovery
    pub recovery_loops_count: usize,
    pub error_count: usize,
    pub unrecovered_error_count: usize,
    pub resilience_status: String,
}

const APOLOGY_PATTERNS: &[&str] = &[
    "i apologize",
    "my apologies",
    "i am sorry",
    "i'm sorry",
    "my mistake",
    "i made an error",
    "turned my safety mechanism into a weapon",
    "lost track",
    "accidentally deleted",
    "pardon the confusion",
    "pardon my mistake",
    "sorry about that",
];

/// Extracts and normalizes flight receipt telemetry from a trace and score.
pub fn extract_flight_data(trace: &AgentWorthTrace, score: &TraceScore) -> FlightReceiptData {
    let outcomes = evaluate_trace_outcomes(trace);
    let recovery_detector = RecoveryDetector::new();
    let recoveries = recovery_detector.detect_recoveries(trace);

    // 1. Session ID & Adapter
    let session_id = trace.session_id.clone();
    let short_session_id = if session_id.len() > 16 {
        format!("{}...{}", &session_id[..8], &session_id[session_id.len() - 6..])
    } else {
        session_id.clone()
    };

    let adapter = if trace.adapter.is_empty() {
        "Unknown Agent".to_string()
    } else {
        trace.adapter.clone()
    };

    // 2. Models & Tools
    let primary_model = trace
        .stats
        .models_used
        .first()
        .cloned()
        .unwrap_or_else(|| "claude-3-7-sonnet".to_string());

    let mut top_tools: Vec<_> = trace
        .stats
        .tools_used
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    top_tools.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    // 3. Timing & Duration
    let started_at_str = trace.started_at.format("%Y-%m-%d %H:%M:%S UTC").to_string();
    let ended_at_str = trace
        .ended_at
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "-".to_string());

    let duration_seconds = trace.stats.duration_seconds.unwrap_or_else(|| {
        if let Some(ended) = trace.ended_at {
            (ended - trace.started_at).num_milliseconds().max(0) as f64 / 1000.0
        } else {
            0.0
        }
    });

    let duration_str = format_duration_compact(duration_seconds);
    let total_events = if trace.stats.total_events > 0 {
        trace.stats.total_events
    } else {
        trace.events.len()
    };

    // 4. Typed Provenance
    let is_flown = !trace.provenance.source_path.is_empty()
        && (trace.provenance.file_size_bytes > 0 || total_events > 0);
    let provenance_status = if is_flown {
        TypedProvenanceStatus::Flown
    } else if total_events > 0 {
        TypedProvenanceStatus::OnPaper
    } else {
        TypedProvenanceStatus::Unflown
    };

    let source_path = trace.provenance.source_path.clone();
    let content_fingerprint = if !trace.provenance.content_fingerprint.is_empty() {
        trace.provenance.content_fingerprint.clone()
    } else {
        "unhashed".to_string()
    };

    // Generate cryptographic receipt hash
    let mut hasher = Sha256::new();
    hasher.update(session_id.as_bytes());
    hasher.update(trace.adapter.as_bytes());
    hasher.update(started_at_str.as_bytes());
    hasher.update(content_fingerprint.as_bytes());
    let receipt_hash = hex::encode(hasher.finalize());

    // 5. Outcome & Verdict
    let highest = highest_outcome(&outcomes);
    let (verdict_badge, verdict_rung, verdict_label, highest_outcome_summary) = match highest {
        Some(ev) => {
            let (rung, label, badge) = match ev.kind {
                OutcomeKind::CiOrDeploymentVerified => (
                    5,
                    "CI / Deployment Verified",
                    "[CI_VERIFIED]".to_string(),
                ),
                OutcomeKind::CommitObserved => (
                    4,
                    "Commit Observed",
                    "[COMMITTED]".to_string(),
                ),
                OutcomeKind::TestOrBuildPassed => (
                    3,
                    "Test / Build Passed",
                    "[TEST_PASSED]".to_string(),
                ),
                OutcomeKind::ArtifactChanged => (
                    2,
                    "Artifact Changed",
                    "[ARTIFACT]".to_string(),
                ),
                OutcomeKind::DoneClaimed => (
                    1,
                    "Done Claimed",
                    "[CLAIM_ONLY]".to_string(),
                ),
            };
            (badge, rung, label.to_string(), Some(ev.summary.clone()))
        }
        None => (
            "[UNVERIFIED]".to_string(),
            0,
            "Unverified / In-Progress".to_string(),
            None,
        ),
    };

    // 6. Token Burn & Spend
    let total_tokens = trace.stats.token_usage.total();
    let input_tokens = trace.stats.token_usage.input_tokens;
    let output_tokens = trace.stats.token_usage.output_tokens;
    let cache_read_tokens = trace.stats.token_usage.cache_read_tokens;
    let cache_creation_tokens = trace.stats.token_usage.cache_creation_tokens;

    let mut spend_usd = estimate_tokens_cost_usd(
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
    );

    // Look for explicit model invocation costs if greater
    for ev in &trace.events {
        if let EventPayload::ModelInvocation {
            cost_usd: Some(c), ..
        } = &ev.payload
        {
            if *c > spend_usd {
                spend_usd = *c;
            }
        }
    }

    // 7. Apology Tax & Remorse Audit
    let mut apology_count = 0usize;
    let mut apology_tax_tokens = 0u64;
    let mut best_apology_quote = None;

    for ev in &trace.events {
        if let EventPayload::AssistantMessage { content, thinking } = &ev.payload {
            let lower_content = content.to_lowercase();
            let lower_thinking = thinking
                .as_deref()
                .map(|t| t.to_lowercase())
                .unwrap_or_default();

            let mut is_apology = false;
            for pat in APOLOGY_PATTERNS {
                if lower_content.contains(pat) || lower_thinking.contains(pat) {
                    is_apology = true;
                    if best_apology_quote.is_none() {
                        best_apology_quote = extract_remorse_sentence(content);
                    }
                    break;
                }
            }

            if is_apology {
                apology_count += 1;
                let est_tokens = (content.len() / 4) as u64;
                apology_tax_tokens += est_tokens.max(50);
            }
        }
    }

    let apology_tax_usd = if total_tokens > 0 && apology_count > 0 {
        let avg_spend_per_token = spend_usd / (total_tokens as f64);
        (apology_tax_tokens as f64 * avg_spend_per_token).max(apology_count as f64 * 0.01)
    } else {
        0.0
    };

    // 8. Autonomous Resilience & Errors
    let recovery_loops_count = recoveries.len();
    let mut error_count = 0usize;
    let mut unrecovered_error_count = 0usize;

    for ev in &trace.events {
        match &ev.payload {
            EventPayload::Error { is_recovered, .. } => {
                error_count += 1;
                if !*is_recovered {
                    unrecovered_error_count += 1;
                }
            }
            EventPayload::ToolResult(r) => {
                if r.is_error {
                    error_count += 1;
                }
            }
            EventPayload::ShellCommand(c) if c.exit_code.is_some_and(|code| code != 0) => {
                error_count += 1;
            }
            _ => {}
        }
    }

    let resilience_status = if recovery_loops_count > 0 {
        format!("RESILIENT ({} Auto-Recovery)", recovery_loops_count)
    } else if error_count == 0 {
        "CLEAN FLIGHT (0 Errors)".to_string()
    } else if unrecovered_error_count > 0 {
        format!("DEGRADED ({} Unresolved Errors)", unrecovered_error_count)
    } else {
        "NOMINAL".to_string()
    };

    FlightReceiptData {
        session_id,
        short_session_id,
        adapter,
        primary_model,
        models_used: trace.stats.models_used.clone(),
        started_at_str,
        ended_at_str,
        duration_str,
        duration_seconds,
        total_events,
        tool_calls_count: trace.stats.tool_calls_count,
        top_tools,
        provenance_status,
        source_path,
        content_fingerprint,
        receipt_hash,
        composite_score: score.composite_score * 100.0,
        outcome_score: score.outcome_score * 100.0,
        verifiability_score: score.verifiability_score * 100.0,
        complexity_score: score.complexity_score * 100.0,
        recovery_score: score.recovery_score * 100.0,
        provenance_score: score.provenance_score * 100.0,
        verdict_badge,
        verdict_rung,
        verdict_label,
        highest_outcome_summary,
        total_tokens,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
        spend_usd,
        apology_count,
        apology_tax_usd,
        apology_tax_tokens,
        best_apology_quote,
        recovery_loops_count,
        error_count,
        unrecovered_error_count,
        resilience_status,
    }
}

// -----------------------------------------------------------------------------
// Terminal Receipt Generator (ANSI / ASCII Box)
// -----------------------------------------------------------------------------

/// Renders a crisp ANSI ASCII box Flight Receipt with Two-Tone Gold and Obsidian styling.
pub fn render_terminal_receipt(trace: &AgentWorthTrace, score: &TraceScore) -> String {
    let data = extract_flight_data(trace, score);
    let mut out = String::new();

    // Box dimensions: total width = 76, interior width = 72
    let inner_width = 72;

    // Top Border
    let _ = writeln!(
        out,
        "{}",
        style("┌──────────────────────────────────────────────────────────────────────────┐")
            .bold()
            .yellow()
    );

    // Header 1: Title & Canonical Record
    let title_line = format!(
        "✈️  {}           {}",
        style("AGENTWORTH FLIGHT RECEIPT").bold().yellow(),
        style("CANONICAL RECORD").dim()
    );
    append_box_line(&mut out, &title_line, inner_width);

    // Header 2: Typed Provenance Badge
    let prov_styled = match data.provenance_status {
        TypedProvenanceStatus::Flown => {
            format!("TYPED PROVENANCE: {}", style("[FLOWN • LOCAL GROUND TRUTH]").bold().green())
        }
        TypedProvenanceStatus::OnPaper => {
            format!("TYPED PROVENANCE: {}", style("[ON PAPER • CLAIMED]").bold().yellow())
        }
        TypedProvenanceStatus::Unflown => {
            format!("TYPED PROVENANCE: {}", style("[UNFLOWN • UNVERIFIED]").dim())
        }
    };
    append_box_line(&mut out, &prov_styled, inner_width);

    // Section Divider
    append_divider(&mut out);

    // Flight Info Block
    let flight_line1 = format!(
        "SESSION ID:   {:<28} STARTED:  {:<20}",
        style(&data.short_session_id).bold().cyan(),
        style(&data.started_at_str).dim()
    );
    append_box_line(&mut out, &flight_line1, inner_width);

    let flight_line2 = format!(
        "ADAPTER:      {:<28} DURATION: {} ({} turns)",
        style(&data.adapter).bold().green(),
        style(&data.duration_str).yellow(),
        data.total_events
    );
    append_box_line(&mut out, &flight_line2, inner_width);

    let short_path = truncate_path(&data.source_path, 42);
    let flight_line3 = format!(
        "PRIMARY MODEL:{:<28} SOURCE:   {:<20}",
        style(&data.primary_model).magenta(),
        style(short_path).dim()
    );
    append_box_line(&mut out, &flight_line3, inner_width);

    // Section Divider
    append_divider(&mut out);

    // Hero Score & Verdict
    let score_str = format!("{:.1} / 100", data.composite_score);
    let styled_score = if data.composite_score >= 70.0 {
        style(score_str).bold().green()
    } else if data.composite_score >= 40.0 {
        style(score_str).bold().yellow()
    } else {
        style(score_str).bold().red()
    };

    let styled_verdict = match data.verdict_rung {
        5 => style(&data.verdict_badge).bold().green(),
        4 => style(&data.verdict_badge).green(),
        3 => style(&data.verdict_badge).bold().cyan(),
        2 => style(&data.verdict_badge).yellow(),
        1 => style(&data.verdict_badge).dim(),
        _ => style(&data.verdict_badge).dim(),
    };

    let score_line1 = format!(
        "COMPOSITE SCORE:  {:<24} VERDICT:  {}",
        styled_score, styled_verdict
    );
    append_box_line(&mut out, &score_line1, inner_width);

    // Score Gauge Bar
    let gauge = render_ascii_gauge(data.composite_score, 24);
    let score_line2 = format!(
        "{}        Rung {}: {}",
        gauge,
        data.verdict_rung,
        style(&data.verdict_label).cyan()
    );
    append_box_line(&mut out, &score_line2, inner_width);

    if let Some(ref summary) = data.highest_outcome_summary {
        let short_summary = truncate_string(summary, 66);
        let summary_line = format!("  Evidence: {}", style(short_summary).italic().dim());
        append_box_line(&mut out, &summary_line, inner_width);
    }

    append_blank_line(&mut out, inner_width);

    // 5 Dimension Breakdown
    append_box_line(&mut out, &format!("{}", style("SCORE DIMENSIONS:").bold()), inner_width);

    let dim_line1 = format!(
        "  • Outcome Quality:     {:>3.0}%   • Autonomous Recovery: {:>3.0}%",
        data.outcome_score, data.recovery_score
    );
    append_box_line(&mut out, &dim_line1, inner_width);

    let dim_line2 = format!(
        "  • Verifiability Signal:{:>3.0}%   • Typed Provenance:    {:>3.0}%",
        data.verifiability_score, data.provenance_score
    );
    append_box_line(&mut out, &dim_line2, inner_width);

    let dim_line3 = format!(
        "  • Trajectory Depth:    {:>3.0}%",
        data.complexity_score
    );
    append_box_line(&mut out, &dim_line3, inner_width);

    // Section Divider
    append_divider(&mut out);

    // Token Burn & Financials
    append_box_line(&mut out, &format!("{}", style("TOKEN USAGE & FINANCIALS:").bold()), inner_width);

    let tok_line1 = format!(
        "  Total Tokens:     {:<22} Blended Spend: {}",
        style(format!("{} tok", format_number(data.total_tokens))).bold().magenta(),
        style(format!("${:.2} USD", data.spend_usd)).bold().green()
    );
    append_box_line(&mut out, &tok_line1, inner_width);

    let tok_line2 = format!(
        "  Input / Output:   {:<22} Cache Read:    {} tok",
        format!("{}/{}", format_number(data.input_tokens), format_number(data.output_tokens)),
        format_number(data.cache_read_tokens)
    );
    append_box_line(&mut out, &tok_line2, inner_width);

    append_blank_line(&mut out, inner_width);

    // Apology Tax & Remorse Audit
    append_box_line(&mut out, &format!("{}", style("APOLOGY TAX & REMORSE AUDIT:").bold()), inner_width);

    let apology_status = if data.apology_count == 0 {
        style("0 (Zero Groveling)".to_string()).green()
    } else {
        style(format!("{} Remorse Turns", data.apology_count)).yellow()
    };

    let tax_styled = if data.apology_tax_usd > 0.0 {
        style(format!("${:.2} USD", data.apology_tax_usd)).bold().red()
    } else {
        style("$0.00 USD".to_string()).dim()
    };

    let ap_line = format!(
        "  Apology Turns:    {:<22} Apology Tax:   {}",
        apology_status, tax_styled
    );
    append_box_line(&mut out, &ap_line, inner_width);

    if let Some(ref quote) = data.best_apology_quote {
        let quote_line = format!("  Remorse: {}", style(truncate_string(quote, 62)).italic().dim());
        append_box_line(&mut out, &quote_line, inner_width);
    }

    append_blank_line(&mut out, inner_width);

    // Autonomous Resilience & Recovery
    append_box_line(&mut out, &format!("{}", style("AUTONOMOUS RESILIENCE & RECOVERY:").bold()), inner_width);

    let rec_line = format!(
        "  Recovery Loops:   {:<22} Status:        {}",
        format!("{} Loop(s)", data.recovery_loops_count),
        style(&data.resilience_status).bold().cyan()
    );
    append_box_line(&mut out, &rec_line, inner_width);

    if !data.top_tools.is_empty() {
        let tools_formatted = data
            .top_tools
            .iter()
            .take(4)
            .map(|(t, c)| format!("{}({})", t, c))
            .collect::<Vec<_>>()
            .join(", ");
        let tools_line = format!("  Active Tools:     {}", style(truncate_string(&tools_formatted, 52)).yellow());
        append_box_line(&mut out, &tools_line, inner_width);
    }

    // Section Divider
    append_divider(&mut out);

    // Cryptographic Receipt Seal & Footer
    let short_hash = if data.receipt_hash.len() > 32 {
        format!("sha256:{}...", &data.receipt_hash[..28])
    } else {
        format!("sha256:{}", data.receipt_hash)
    };
    let hash_line = format!("RECEIPT HASH: {}", style(short_hash).dim());
    append_box_line(&mut out, &hash_line, inner_width);

    let seal_line = format!(
        "{} • {}",
        style("VERIFIED BY AGENTWORTH").bold().green(),
        style("\"FLOWN BEATS ON-PAPER. ALWAYS.\"").italic().dim()
    );
    append_box_line(&mut out, &seal_line, inner_width);

    // Bottom Border
    let _ = writeln!(
        out,
        "{}",
        style("└──────────────────────────────────────────────────────────────────────────┘")
            .bold()
            .yellow()
    );

    out
}

fn append_box_line(out: &mut String, content: &str, inner_width: usize) {
    let visual_w = console::measure_text_width(content);
    if visual_w < inner_width {
        let padding = " ".repeat(inner_width - visual_w);
        let _ = writeln!(out, "│ {}{} │", content, padding);
    } else {
        let _ = writeln!(out, "│ {} │", content);
    }
}

fn append_blank_line(out: &mut String, inner_width: usize) {
    let padding = " ".repeat(inner_width);
    let _ = writeln!(out, "│ {} │", padding);
}

fn append_divider(out: &mut String) {
    let _ = writeln!(
        out,
        "{}",
        style("├──────────────────────────────────────────────────────────────────────────┤")
            .bold()
            .yellow()
    );
}

fn render_ascii_gauge(score: f64, width: usize) -> String {
    let clamped = (score / 100.0).clamp(0.0, 1.0);
    let filled = ((clamped * width as f64).round() as usize).min(width);
    let empty = width - filled;

    let fill_char = "=";
    let empty_char = "░";

    format!(
        "[{}{}]",
        style(fill_char.repeat(filled)).bold().yellow(),
        style(empty_char.repeat(empty)).dim()
    )
}

// -----------------------------------------------------------------------------
// Standalone Dark-Mode 1200x630 SVG Receipt Card Generator
// -----------------------------------------------------------------------------

/// Renders a standalone, dark-mode 1200x630 SVG Flight Receipt card.
pub fn render_svg_receipt(trace: &AgentWorthTrace, score: &TraceScore) -> String {
    let data = extract_flight_data(trace, score);

    let short_hash = if data.receipt_hash.len() > 32 {
        format!("sha256:{}...", &data.receipt_hash[..32])
    } else {
        format!("sha256:{}", data.receipt_hash)
    };

    let clean_path = escape_xml(&truncate_path(&data.source_path, 70));
    let clean_session_id = escape_xml(&data.session_id);
    let clean_adapter = escape_xml(&data.adapter);
    let clean_model = escape_xml(&data.primary_model);
    let clean_verdict = escape_xml(&data.verdict_label);
    let clean_evidence = data
        .highest_outcome_summary
        .as_ref()
        .map(|s| escape_xml(&truncate_string(s, 75)))
        .unwrap_or_default();

    let clean_apology_quote = data
        .best_apology_quote
        .as_ref()
        .map(|s| escape_xml(&truncate_string(s, 60)))
        .unwrap_or_default();

    let tools_summary = if !data.top_tools.is_empty() {
        let joined = data
            .top_tools
            .iter()
            .take(3)
            .map(|(t, c)| format!("{}({})", t, c))
            .collect::<Vec<_>>()
            .join(", ");
        escape_xml(&truncate_string(&joined, 30))
    } else {
        "None recorded".to_string()
    };

    // Dimension bar widths (max width = 160px)
    let outcome_w = ((data.outcome_score / 100.0).clamp(0.0, 1.0) * 160.0).round() as u32;
    let verif_w = ((data.verifiability_score / 100.0).clamp(0.0, 1.0) * 160.0).round() as u32;
    let compl_w = ((data.complexity_score / 100.0).clamp(0.0, 1.0) * 160.0).round() as u32;
    let recov_w = ((data.recovery_score / 100.0).clamp(0.0, 1.0) * 160.0).round() as u32;
    let prov_w = ((data.provenance_score / 100.0).clamp(0.0, 1.0) * 160.0).round() as u32;

    // Provenance badge colors
    let (badge_stroke, badge_fill, badge_text_color, badge_text) = match data.provenance_status {
        TypedProvenanceStatus::Flown => (
            "#10b981",
            "rgba(16, 185, 129, 0.12)",
            "#10b981",
            "● FLOWN (LOCAL GROUND TRUTH)",
        ),
        TypedProvenanceStatus::OnPaper => (
            "#f59e0b",
            "rgba(245, 158, 11, 0.12)",
            "#f59e0b",
            "● ON PAPER (CLAIMED)",
        ),
        TypedProvenanceStatus::Unflown => (
            "#71717a",
            "rgba(113, 113, 122, 0.12)",
            "#a1a1aa",
            "● UNFLOWN (UNVERIFIED)",
        ),
    };

    let gauge_fill_w = ((data.composite_score / 100.0).clamp(0.0, 1.0) * 360.0).round() as u32;

    format!(
        r##"<?xml version="1.0" encoding="UTF-8"?>
<svg width="1200" height="630" viewBox="0 0 1200 630" fill="none" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <!-- Background and Card Gradients -->
    <linearGradient id="bgGrad" x1="0" y1="0" x2="1200" y2="630" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#000000"/>
      <stop offset="50%" stop-color="#050506"/>
      <stop offset="100%" stop-color="#0d0d10"/>
    </linearGradient>

    <linearGradient id="cardGrad" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#141417"/>
      <stop offset="100%" stop-color="#0d0d10"/>
    </linearGradient>

    <linearGradient id="goldTextGrad" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0%" stop-color="#f6e6a8"/>
      <stop offset="60%" stop-color="#c9a227"/>
      <stop offset="100%" stop-color="#8a6a22"/>
    </linearGradient>

    <linearGradient id="goldBarGrad" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0%" stop-color="#c9a227"/>
      <stop offset="100%" stop-color="#f6e6a8"/>
    </linearGradient>

    <linearGradient id="emeraldGrad" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0%" stop-color="#10b981"/>
      <stop offset="100%" stop-color="#34d399"/>
    </linearGradient>

    <!-- Ambient Glows -->
    <radialGradient id="topGoldGlow" cx="950" cy="80" r="350" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#c9a227" stop-opacity="0.15"/>
      <stop offset="100%" stop-color="#c9a227" stop-opacity="0"/>
    </radialGradient>

    <radialGradient id="bottomEmeraldGlow" cx="150" cy="550" r="300" gradientUnits="userSpaceOnUse">
      <stop offset="0%" stop-color="#10b981" stop-opacity="0.08"/>
      <stop offset="100%" stop-color="#10b981" stop-opacity="0"/>
    </radialGradient>

    <!-- Filters -->
    <filter id="cardShadow" x="0" y="0" width="1200" height="630" filterUnits="userSpaceOnUse">
      <feDropShadow dx="0" dy="16" stdDeviation="24" flood-color="#000000" flood-opacity="0.6"/>
    </filter>
  </defs>

  <style>
    .font-sans {{ font-family: 'Geist', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; }}
    .font-mono {{ font-family: 'Geist Mono', 'SF Mono', 'Fira Code', Menlo, Consolas, monospace; }}
  </style>

  <!-- Root Canvas Background -->
  <rect width="1200" height="630" fill="url(#bgGrad)"/>
  <rect width="1200" height="630" fill="url(#topGoldGlow)"/>
  <rect width="1200" height="630" fill="url(#bottomEmeraldGlow)"/>

  <!-- Main Container Frame (Obsidian Card) -->
  <rect x="32" y="32" width="1136" height="566" rx="16" fill="url(#cardGrad)" stroke="#1e1e24" stroke-width="1.5" filter="url(#cardShadow)"/>

  <!-- Top Accent Spine (Two-Tone Gold Specular Line) -->
  <path d="M48 32 H1152" stroke="url(#goldTextGrad)" stroke-width="2" stroke-linecap="round"/>

  <!-- ================= HEADER SECTION ================= -->
  <!-- Brand Ship / Plane Icon -->
  <g transform="translate(64, 56)">
    <!-- Stealth Fighter Wedge -->
    <path d="M12 2 L22 22 L12 18 L2 22 Z" fill="#c9a227" stroke="#f6e6a8" stroke-width="1.5"/>
    <path d="M12 2 L12 18 L2 22 Z" fill="#8a6a22"/>
  </g>

  <!-- Brand Title -->
  <text x="96" y="70" class="font-sans" font-weight="800" font-size="20" letter-spacing="1.5" fill="url(#goldTextGrad)">AGENTWORTH</text>
  <text x="252" y="70" class="font-sans" font-weight="600" font-size="20" letter-spacing="1.5" fill="#cfd4dc">FLIGHT RECEIPT</text>
  <text x="96" y="90" class="font-mono" font-size="11" letter-spacing="0.5" fill="#71717a">CANONICAL AGENT EXECUTION RECORD • PROVENANCE VERIFIED</text>

  <!-- Provenance Pill Badge -->
  <g transform="translate(850, 54)">
    <rect width="280" height="34" rx="8" fill="{badge_fill}" stroke="{badge_stroke}" stroke-width="1.2"/>
    <text x="140" y="22" class="font-mono" font-weight="700" font-size="11.5" text-anchor="middle" fill="{badge_text_color}">{badge_text}</text>
  </g>

  <!-- Divider Line -->
  <line x1="64" y1="108" x2="1136" y2="108" stroke="#1e1e24" stroke-width="1"/>

  <!-- ================= FLIGHT METADATA STRIP ================= -->
  <g transform="translate(64, 128)">
    <!-- Session ID -->
    <text x="0" y="0" class="font-mono" font-size="11" fill="#71717a">SESSION ID</text>
    <text x="0" y="18" class="font-mono" font-weight="600" font-size="13" fill="#cfd4dc">{clean_session_id}</text>

    <!-- Adapter -->
    <text x="320" y="0" class="font-mono" font-size="11" fill="#71717a">ADAPTER</text>
    <text x="320" y="18" class="font-sans" font-weight="700" font-size="14" fill="#34d399">{clean_adapter}</text>

    <!-- Started At -->
    <text x="540" y="0" class="font-mono" font-size="11" fill="#71717a">STARTED AT</text>
    <text x="540" y="18" class="font-mono" font-size="13" fill="#a1a1aa">{}</text>

    <!-- Duration & Turns -->
    <text x="820" y="0" class="font-mono" font-size="11" fill="#71717a">DURATION &amp; TURNS</text>
    <text x="820" y="18" class="font-mono" font-weight="600" font-size="13" fill="#f6e6a8">{} ({} events)</text>
  </g>

  <!-- ================= HERO SCORE & VERDICT PANEL ================= -->
  <!-- Left Score Box -->
  <g transform="translate(64, 172)">
    <rect width="440" height="152" rx="12" fill="#09090b" stroke="#1e1e24" stroke-width="1"/>

    <text x="24" y="32" class="font-mono" font-size="11" font-weight="600" letter-spacing="1" fill="#71717a">COMPOSITE TRACE SCORE</text>

    <!-- Big Score Metric -->
    <text x="24" y="80" class="font-sans" font-weight="900" font-size="44" fill="url(#goldTextGrad)">{:.1}</text>
    <text x="140" y="80" class="font-sans" font-weight="600" font-size="20" fill="#71717a">/ 100</text>

    <!-- Verdict Pill Badge -->
    <rect x="220" y="52" width="196" height="30" rx="6" fill="rgba(16, 185, 129, 0.15)" stroke="#10b981" stroke-width="1"/>
    <text x="318" y="72" class="font-mono" font-weight="700" font-size="11" text-anchor="middle" fill="#10b981">{}</text>

    <!-- Score Progress Gauge Bar -->
    <rect x="24" y="104" width="392" height="8" rx="4" fill="#1e1e24"/>
    <rect x="24" y="104" width="{gauge_fill_w}" height="8" rx="4" fill="url(#goldBarGrad)"/>

    <!-- Verdict Label -->
    <text x="24" y="132" class="font-sans" font-weight="600" font-size="12" fill="#cfd4dc">Rung {}: {}</text>
  </g>

  <!-- Right Dimensions Breakdown Box -->
  <g transform="translate(528, 172)">
    <rect width="608" height="152" rx="12" fill="#09090b" stroke="#1e1e24" stroke-width="1"/>

    <text x="24" y="30" class="font-mono" font-size="11" font-weight="600" letter-spacing="1" fill="#71717a">EXPLAINABLE DIMENSIONS BREAKDOWN</text>

    <!-- Row 1: Outcome & Recovery -->
    <g transform="translate(24, 48)">
      <text x="0" y="12" class="font-sans" font-size="12" fill="#a1a1aa">Outcome Hierarchy</text>
      <rect x="130" y="3" width="120" height="10" rx="5" fill="#1e1e24"/>
      <rect x="130" y="3" width="{}" height="10" rx="5" fill="url(#goldBarGrad)"/>
      <text x="260" y="12" class="font-mono" font-weight="600" font-size="11" fill="#cfd4dc">{:.0}%</text>

      <text x="310" y="12" class="font-sans" font-size="12" fill="#a1a1aa">Recovery Resilience</text>
      <rect x="440" y="3" width="120" height="10" rx="5" fill="#1e1e24"/>
      <rect x="440" y="3" width="{}" height="10" rx="5" fill="url(#emeraldGrad)"/>
      <text x="570" y="12" class="font-mono" font-weight="600" font-size="11" fill="#10b981">{:.0}%</text>
    </g>

    <!-- Row 2: Verifiability & Provenance -->
    <g transform="translate(24, 80)">
      <text x="0" y="12" class="font-sans" font-size="12" fill="#a1a1aa">Objective Evidence</text>
      <rect x="130" y="3" width="120" height="10" rx="5" fill="#1e1e24"/>
      <rect x="130" y="3" width="{}" height="10" rx="5" fill="url(#goldBarGrad)"/>
      <text x="260" y="12" class="font-mono" font-weight="600" font-size="11" fill="#cfd4dc">{:.0}%</text>

      <text x="310" y="12" class="font-sans" font-size="12" fill="#a1a1aa">Typed Provenance</text>
      <rect x="440" y="3" width="120" height="10" rx="5" fill="#1e1e24"/>
      <rect x="440" y="3" width="{}" height="10" rx="5" fill="url(#goldBarGrad)"/>
      <text x="570" y="12" class="font-mono" font-weight="600" font-size="11" fill="#cfd4dc">{:.0}%</text>
    </g>

    <!-- Row 3: Trajectory Complexity & Supporting Evidence -->
    <g transform="translate(24, 112)">
      <text x="0" y="12" class="font-sans" font-size="12" fill="#a1a1aa">Trajectory Depth</text>
      <rect x="130" y="3" width="120" height="10" rx="5" fill="#1e1e24"/>
      <rect x="130" y="3" width="{}" height="10" rx="5" fill="url(#goldBarGrad)"/>
      <text x="260" y="12" class="font-mono" font-weight="600" font-size="11" fill="#cfd4dc">{:.0}%</text>

      <text x="310" y="12" class="font-mono" font-size="11" fill="#71717a">Evidence: <tspan fill="#cfd4dc">{clean_evidence}</tspan></text>
    </g>
  </g>

  <!-- ================= 4-CARD TELEMETRY GRID ================= -->
  <!-- Card 1: Token Burn & Spend -->
  <g transform="translate(64, 342)">
    <rect width="254" height="160" rx="10" fill="#09090b" stroke="#1e1e24" stroke-width="1"/>
    <text x="18" y="28" class="font-mono" font-weight="700" font-size="11" fill="#c9a227">TOKEN USAGE &amp; SPEND</text>
    <text x="18" y="60" class="font-mono" font-weight="800" font-size="22" fill="#f43f5e">{} tok</text>
    <text x="18" y="86" class="font-mono" font-size="12" fill="#a1a1aa">In: <tspan fill="#cfd4dc">{}</tspan> • Out: <tspan fill="#cfd4dc">{}</tspan></text>
    <text x="18" y="108" class="font-mono" font-size="12" fill="#a1a1aa">Cache Read: <tspan fill="#cfd4dc">{} tok</tspan></text>
    <text x="18" y="136" class="font-mono" font-weight="700" font-size="14" fill="#10b981">Est. Spend: ${:.2} USD</text>
  </g>

  <!-- Card 2: Apology Tax & Remorse -->
  <g transform="translate(342, 342)">
    <rect width="254" height="160" rx="10" fill="#09090b" stroke="#1e1e24" stroke-width="1"/>
    <text x="18" y="28" class="font-mono" font-weight="700" font-size="11" fill="#c9a227">APOLOGY TAX AUDIT</text>
    <text x="18" y="60" class="font-mono" font-weight="800" font-size="22" fill="{}">{}</text>
    <text x="18" y="86" class="font-mono" font-size="12" fill="#a1a1aa">Apology Tax: <tspan font-weight="700" fill="{}">${:.2} USD</tspan></text>
    <text x="18" y="108" class="font-mono" font-size="12" fill="#a1a1aa">Burned: <tspan fill="#cfd4dc">{} tok</tspan></text>
    <text x="18" y="136" class="font-sans" font-style="italic" font-size="11" fill="#71717a">"{}"</text>
  </g>

  <!-- Card 3: Autonomous Resilience & Recovery -->
  <g transform="translate(620, 342)">
    <rect width="254" height="160" rx="10" fill="#09090b" stroke="#1e1e24" stroke-width="1"/>
    <text x="18" y="28" class="font-mono" font-weight="700" font-size="11" fill="#c9a227">AUTONOMOUS RESILIENCE</text>
    <text x="18" y="60" class="font-mono" font-weight="800" font-size="22" fill="#38bdf8">{} Loops</text>
    <text x="18" y="86" class="font-mono" font-size="12" fill="#a1a1aa">Total Errors: <tspan fill="#cfd4dc">{}</tspan></text>
    <text x="18" y="108" class="font-mono" font-size="12" fill="#a1a1aa">Unresolved: <tspan fill="{}">{}</tspan></text>
    <text x="18" y="136" class="font-mono" font-weight="700" font-size="12" fill="{}">{}</text>
  </g>

  <!-- Card 4: Model & Tool Inventory -->
  <g transform="translate(898, 342)">
    <rect width="238" height="160" rx="10" fill="#09090b" stroke="#1e1e24" stroke-width="1"/>
    <text x="18" y="28" class="font-mono" font-weight="700" font-size="11" fill="#c9a227">MODEL &amp; TOOL LINEAGE</text>
    <text x="18" y="56" class="font-mono" font-size="11" fill="#71717a">PRIMARY MODEL</text>
    <text x="18" y="74" class="font-mono" font-weight="600" font-size="12" fill="#c084fc">{clean_model}</text>
    <text x="18" y="102" class="font-mono" font-size="11" fill="#71717a">TOP TOOLS</text>
    <text x="18" y="120" class="font-mono" font-size="12" fill="#fbbf24">{tools_summary}</text>
    <text x="18" y="142" class="font-mono" font-size="11" fill="#71717a">Tool Invocations: <tspan font-weight="700" fill="#cfd4dc">{}</tspan></text>
  </g>

  <!-- ================= FOOTER / PROVENANCE SEAL ================= -->
  <line x1="64" y1="520" x2="1136" y2="520" stroke="#1e1e24" stroke-width="1"/>

  <g transform="translate(64, 546)">
    <!-- Source Path -->
    <text x="0" y="0" class="font-mono" font-size="11" fill="#71717a">SOURCE: <tspan fill="#a1a1aa">{clean_path}</tspan></text>
    <!-- Receipt Hash -->
    <text x="0" y="20" class="font-mono" font-size="10.5" fill="#52525b">RECEIPT HASH: <tspan fill="#71717a">{}</tspan></text>

    <!-- Verification Seal -->
    <text x="1072" y="10" class="font-sans" font-weight="800" font-size="12" letter-spacing="1" text-anchor="end" fill="#10b981">AGENTWORTH VERIFIED</text>
    <text x="1072" y="26" class="font-mono" font-size="10" text-anchor="end" fill="#52525b">"FLOWN BEATS ON-PAPER. ALWAYS."</text>
  </g>
</svg>
"##,
        escape_xml(&data.started_at_str),
        escape_xml(&data.duration_str),
        data.total_events,
        data.composite_score,
        escape_xml(&data.verdict_badge),
        data.verdict_rung,
        clean_verdict,
        outcome_w,
        data.outcome_score,
        recov_w,
        data.recovery_score,
        verif_w,
        data.verifiability_score,
        prov_w,
        data.provenance_score,
        compl_w,
        data.complexity_score,
        format_number(data.total_tokens),
        format_number(data.input_tokens),
        format_number(data.output_tokens),
        format_number(data.cache_read_tokens),
        data.spend_usd,
        if data.apology_count == 0 { "#10b981" } else { "#f59e0b" },
        if data.apology_count == 0 { "0 Remorse".to_string() } else { format!("{} Apologies", data.apology_count) },
        if data.apology_tax_usd > 0.0 { "#ef4444" } else { "#71717a" },
        data.apology_tax_usd,
        format_number(data.apology_tax_tokens),
        if clean_apology_quote.is_empty() { "Zero groveling detected".to_string() } else { clean_apology_quote },
        data.recovery_loops_count,
        data.error_count,
        if data.unrecovered_error_count > 0 { "#ef4444" } else { "#10b981" },
        data.unrecovered_error_count,
        if data.recovery_loops_count > 0 { "#38bdf8" } else if data.error_count == 0 { "#10b981" } else { "#f59e0b" },
        escape_xml(&data.resilience_status),
        data.tool_calls_count,
        short_hash,
    )
}

// -----------------------------------------------------------------------------
// CLI Subcommand Execution
// -----------------------------------------------------------------------------

/// Executes the `agwt receipt` command to render terminal or SVG flight receipts.
pub fn run_receipt_command(
    session_id: &str,
    format: &str,
    output: Option<PathBuf>,
    db_path: Option<PathBuf>,
) -> Result<()> {
    let storage = if let Some(path) = db_path {
        Arc::new(Storage::open_path(&path)?)
    } else {
        Arc::new(Storage::open_default()?)
    };

    let scanner = Scanner::new(storage);
    let trace = scanner
        .load_trace(session_id)
        .with_context(|| format!("Failed loading trace for session '{}'", session_id))?;

    let scorer = TraceScorer::default();
    let score = scorer.score(&trace);

    let content = match format.to_lowercase().as_str() {
        "svg" => render_svg_receipt(&trace, &score),
        "json" => {
            let flight_data = extract_flight_data(&trace, &score);
            serde_json::to_string_pretty(&flight_data)?
        }
        // The named alternatives are redundant with `_` but kept as documentation of the
        // recognized `--format` values; any unrecognized value intentionally falls back to
        // the terminal receipt rather than erroring.
        #[allow(clippy::wildcard_in_or_patterns)]
        "terminal" | "ansi" | "receipt" | _ => render_terminal_receipt(&trace, &score),
    };

    if let Some(out_path) = output {
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed creating directory {:?}", parent))?;
        }
        std::fs::write(&out_path, content.as_bytes())
            .with_context(|| format!("Failed writing receipt to {:?}", out_path))?;
        eprintln!(
            "{} Flight Receipt written to {:?}",
            style("✔").green().bold(),
            out_path
        );
    } else {
        println!("{}", content);
    }

    Ok(())
}

// -----------------------------------------------------------------------------
// Utility Functions
// -----------------------------------------------------------------------------

fn format_duration_compact(seconds: f64) -> String {
    if seconds >= 3600.0 {
        let hrs = (seconds / 3600.0).floor();
        let mins = ((seconds % 3600.0) / 60.0).floor();
        format!("{:.0}h {:.0}m", hrs, mins)
    } else if seconds >= 60.0 {
        let mins = (seconds / 60.0).floor();
        let secs = (seconds % 60.0).floor();
        format!("{:.0}m {:02.0}s", mins, secs)
    } else {
        format!("{:.1}s", seconds)
    }
}

fn format_number(n: u64) -> String {
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

fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        path.to_string()
    } else {
        format!("...{}", &path[path.len() - (max_len - 3)..])
    }
}

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

fn extract_remorse_sentence(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        for pat in APOLOGY_PATTERNS {
            if lower.contains(pat) && trimmed.len() >= 8 {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{NormalizedEvent, Provenance, TokenUsage};
    use chrono::{Duration, Utc};

    fn create_test_trace() -> (AgentWorthTrace, TraceScore) {
        let start = Utc::now();
        let prov = Provenance::new(
            "/home/user/.claude/projects/repo/session.jsonl",
            "claude_code",
            1024,
            2048,
            "fp_test_12345",
        );
        let mut trace = AgentWorthTrace::new("sess_test_flight_001", "claude_code", prov, start);

        trace.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::UserMessage {
                content: "Run test suite and fix failures".to_string(),
            },
        ));

        trace.events.push(NormalizedEvent::new(
            2,
            start + Duration::seconds(2),
            EventPayload::ModelInvocation {
                model: "claude-3-7-sonnet".to_string(),
                token_usage: TokenUsage::new(5000, 800, 1200, 0),
                cost_usd: Some(0.045),
                latency_ms: Some(1500),
            },
        ));

        trace.events.push(NormalizedEvent::new(
            3,
            start + Duration::seconds(4),
            EventPayload::AssistantMessage {
                content: "I apologize for the previous oversight. I will fix src/lib.rs and run tests.".to_string(),
                thinking: Some("Running cargo test".to_string()),
            },
        ));

        trace.events.push(NormalizedEvent::new(
            4,
            start + Duration::seconds(6),
            EventPayload::ToolCall(agentworth_schema::ToolCall {
                id: Some("t1".to_string()),
                name: "Bash".to_string(),
                arguments: serde_json::json!({"command": "cargo test"}),
            }),
        ));

        trace.events.push(NormalizedEvent::new(
            5,
            start + Duration::seconds(10),
            EventPayload::ToolResult(agentworth_schema::ToolResult {
                call_id: Some("t1".to_string()),
                name: Some("Bash".to_string()),
                output: serde_json::json!("test result: ok. 14 passed; 0 failed"),
                is_error: false,
            }),
        ));


        trace.recalculate_stats();

        let scorer = TraceScorer::default();
        let score = scorer.score(&trace);

        (trace, score)
    }

    #[test]
    fn test_extract_flight_data() {
        let (trace, score) = create_test_trace();
        let data = extract_flight_data(&trace, &score);

        assert_eq!(data.session_id, "sess_test_flight_001");
        assert_eq!(data.adapter, "claude_code");
        assert_eq!(data.provenance_status, TypedProvenanceStatus::Flown);
        assert_eq!(data.apology_count, 1);
        assert!(data.best_apology_quote.is_some());
        assert!(data.total_tokens > 0);
        assert_eq!(data.top_tools[0].0, "Bash");
    }

    #[test]
    fn test_render_terminal_receipt() {
        let (trace, score) = create_test_trace();
        let receipt = render_terminal_receipt(&trace, &score);

        assert!(receipt.contains("AGENTWORTH FLIGHT RECEIPT"));
        assert!(receipt.contains("TYPED PROVENANCE:"));
        assert!(receipt.contains("FLOWN"));
        assert!(receipt.contains("COMPOSITE SCORE:"));
        assert!(receipt.contains("TOKEN USAGE & FINANCIALS:"));
        assert!(receipt.contains("APOLOGY TAX & REMORSE AUDIT"));
        assert!(receipt.contains("AUTONOMOUS RESILIENCE & RECOVERY:"));
        assert!(receipt.contains("RECEIPT HASH:"));
        assert!(receipt.contains("VERIFIED BY AGENTWORTH"));
    }

    #[test]
    fn test_render_svg_receipt() {
        let (trace, score) = create_test_trace();
        let svg = render_svg_receipt(&trace, &score);

        assert!(svg.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(svg.contains("<svg width=\"1200\" height=\"630\""));
        assert!(svg.contains("AGENTWORTH"));
        assert!(svg.contains("FLIGHT RECEIPT"));
        assert!(svg.contains("FLOWN"));
        assert!(svg.contains("TOKEN USAGE &amp; SPEND"));
        assert!(svg.contains("APOLOGY TAX AUDIT"));
        assert!(svg.contains("AUTONOMOUS RESILIENCE"));
        assert!(svg.contains("AGENTWORTH VERIFIED"));
        assert!(svg.ends_with("</svg>\n") || svg.ends_with("</svg>"));
    }
}
