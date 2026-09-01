//! Threat Digest command for AgentWorth.
//!
//! Subcommand: `agentworth threat-digest [--limit N] [--min-severity <low|medium|high|critical>] [--json]`
//! Surfaces real secret/credential exposure risk across indexed sessions: which sessions
//! leaked what, broken down by category and severity, so a human can prioritize rotating a
//! key that actually leaked over ignoring routine noise (home paths, emails, private IPs).
//!
//! ## Design decision: live scan, not stored counts
//!
//! This was an open design call (see `docs/DECISION-INBOX.md`). The `sessions` table in
//! `agentworth_storage` (see `crates/storage/src/lib.rs::initialize_schema`) stores only
//! aggregate counters (event/message/tool counts, token usage, `primary_outcome`,
//! `composite_score`) plus a separate `file_modifications` table -- there is no
//! `redaction_report` column, and no table anywhere persists a session's full event stream.
//! The only place full event content exists is the original session file on disk, lazily
//! re-parsed on demand via `Scanner::load_trace` -- the exact same pattern every sibling
//! report command already uses for anything that needs event-level detail instead of the
//! summary row (`cache_doctor::run_cache_doctor_command`, `blind_spots::generate_blind_spots_report`,
//! `autopsy::perform_prompt_autopsy`, `audit::run_audit_command`).
//!
//! So "stored counts" was never actually available to choose -- there is nothing stored to
//! read. This command re-parses each indexed session's source file and runs
//! `agentworth_redaction::Redactor` (the canonical default rule set, including the
//! high-entropy fallback) over the freshly parsed trace, mirroring the lazy-load pattern
//! above rather than introducing a new one. The alternative (add a `redaction_report` column,
//! populate it eagerly in `Scanner::run_scan`) would touch the hot scan path every other
//! command depends on, require a migration, and leave every already-indexed session reporting
//! zero exposure until rescanned -- a real footgun for a security report. If eager
//! precomputation is ever wanted (e.g. once this becomes a common enough query to want to
//! avoid re-parsing on every run), it belongs as a follow-up that adds storage, not as part of
//! standing this report up.
//!
//! ## Design decision: no secret snippets in the report
//!
//! The report shows counts, per-category breakdowns, and *locations* (which event, by
//! sequence number and kind -- e.g. "ShellCommand", "UserMessage" -- never the matched text)
//! for every finding. It deliberately never reproduces the matched secret or even a redacted
//! surrounding snippet: a location pointer is enough to go find and rotate the real thing in
//! the original tool (or re-run `agentworth export --redact` on that session), and it removes
//! any risk of a redaction-pipeline edge case leaking a fragment through the digest itself.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use agentworth_core::Scanner;
use agentworth_redaction::{RedactionCategory, RedactionReport, Redactor};
use agentworth_schema::AgentWorthTrace;
use agentworth_storage::{SessionFilter, Storage};
use anyhow::Result;
use chrono::{DateTime, Utc};
use console::style;
use serde::{Deserialize, Serialize};

/// Cap on findings kept per session so one extremely noisy session (e.g. hundreds of
/// routine home-path redactions) can't blow up the report -- a digest should stay
/// digestible. Sessions with more findings than this still contribute their full counts to
/// `report`/`risk_score`; only the location list is truncated.
const MAX_FINDINGS_PER_SESSION: usize = 20;

/// Synthetic location label for a hit in the trace's provenance (source file path), which
/// `Redactor::redact_trace`/`preview_redactions` scrub alongside events but which isn't
/// itself a `NormalizedEvent`.
const LOCATION_PROVENANCE: &str = "provenance.source_path";
/// Synthetic location label for a hit in the trace's freeform adapter metadata JSON blob.
const LOCATION_METADATA: &str = "metadata";

/// Coarse severity tier for a redaction category. Declared weakest-to-strongest (like
/// `agentworth_schema::OutcomeKind`, not like this crate's own `audit::SafetySeverity`, which
/// sorts the opposite way) so `#[derive(Ord)]` gives the natural reading: `Critical` is the
/// max element, `.max()` over a set of findings gives you the ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ThreatSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl ThreatSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
            Self::Critical => "CRITICAL",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "critical" => Ok(Self::Critical),
            other => anyhow::bail!(
                "invalid severity {:?}: expected one of low, medium, high, critical",
                other
            ),
        }
    }
}

/// Maps a redaction category to its threat severity tier. Categories that are literal,
/// directly-usable credential material (a key, a token, a private key, a `user:pass@host`
/// URL) are Critical or High; structural/PII categories that show up in almost every real
/// session (a home directory path, an email address, a private IP) are Low/Medium -- present
/// constantly and not, on their own, something to rotate. This is the mechanism that lets the
/// digest separate "a key actually leaked" from "routine noise."
fn category_severity(category: RedactionCategory) -> ThreatSeverity {
    match category {
        RedactionCategory::PrivateKey | RedactionCategory::ApiKey | RedactionCategory::Credential => {
            ThreatSeverity::Critical
        }
        RedactionCategory::JwtToken
        | RedactionCategory::HighEntropySecret
        | RedactionCategory::EnvVar
        | RedactionCategory::Custom => ThreatSeverity::High,
        RedactionCategory::IpAddress => ThreatSeverity::Medium,
        RedactionCategory::FilePath | RedactionCategory::Email => ThreatSeverity::Low,
    }
}

/// Weight per severity tier for a session's aggregate risk score. Deliberately convex
/// (10/6/2/1, not linear) so a handful of Critical hits dominates the ranking over a pile of
/// Low-severity noise -- a session with one leaked private key must outrank a session with
/// two hundred redacted home-directory paths.
fn severity_weight(severity: ThreatSeverity) -> u64 {
    match severity {
        ThreatSeverity::Critical => 10,
        ThreatSeverity::High => 6,
        ThreatSeverity::Medium => 2,
        ThreatSeverity::Low => 1,
    }
}

/// Every named category paired with its count from a `RedactionReport`. Manual list (mirrors
/// `RedactionReport::add`/`merge`, which are themselves manual per-field) rather than derived,
/// so `category_severity`'s exhaustive match is what forces a compile error if a category is
/// ever added and forgotten here.
fn category_counts(report: &RedactionReport) -> [(RedactionCategory, usize); 10] {
    [
        (RedactionCategory::ApiKey, report.api_keys_count),
        (RedactionCategory::EnvVar, report.env_vars_count),
        (RedactionCategory::FilePath, report.paths_count),
        (RedactionCategory::Email, report.emails_count),
        (RedactionCategory::Credential, report.credentials_count),
        (RedactionCategory::JwtToken, report.jwt_tokens_count),
        (RedactionCategory::IpAddress, report.ip_addresses_count),
        (RedactionCategory::PrivateKey, report.private_keys_count),
        (RedactionCategory::HighEntropySecret, report.high_entropy_secrets_count),
        (RedactionCategory::Custom, report.custom_count),
    ]
}

/// Aggregate risk score for a session: sum of `count * severity_weight` across every
/// category present. Used purely for ranking -- not displayed as a probability or percentage.
fn compute_risk_score(report: &RedactionReport) -> u64 {
    category_counts(report)
        .into_iter()
        .map(|(cat, count)| count as u64 * severity_weight(category_severity(cat)))
        .sum()
}

/// The single worst severity tier present in a report, or `None` for a clean report.
fn highest_severity_present(report: &RedactionReport) -> Option<ThreatSeverity> {
    category_counts(report)
        .into_iter()
        .filter(|&(_, count)| count > 0)
        .map(|(cat, _)| category_severity(cat))
        .max()
}

/// A single location where exposure was found within a session: where (event sequence +
/// kind, or a synthetic provenance/metadata label) and what categories/counts -- never the
/// matched text itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreatFinding {
    /// Sequence number of the source event, or `None` for a provenance/metadata hit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_sequence: Option<u64>,
    pub timestamp: DateTime<Utc>,
    /// Where this was found: an event kind label (e.g. "ShellCommand", "ToolResult",
    /// "UserMessage") or `LOCATION_PROVENANCE`/`LOCATION_METADATA`. Never the matched text.
    pub location: String,
    pub categories: BTreeMap<String, usize>,
    pub total: usize,
}

/// One session's exposure summary: risk score, worst severity, full category breakdown, and
/// a bounded list of findings pointing at where each hit was.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionThreatEntry {
    pub session_id: String,
    pub adapter: String,
    pub source_path: String,
    pub started_at: DateTime<Utc>,
    pub risk_score: u64,
    pub highest_severity: ThreatSeverity,
    pub report: RedactionReport,
    pub findings: Vec<ThreatFinding>,
    pub findings_truncated: bool,
}

/// Full Threat Digest report across the indexed corpus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreatDigestReport {
    pub sessions_scanned: usize,
    /// Source file no longer readable from disk (moved/deleted since indexing) -- skipped,
    /// not a hard failure, same tolerance `autopsy`/`blind-spots` already apply.
    pub sessions_unreadable: usize,
    pub sessions_clean: usize,
    /// Had exposure, but its peak severity fell below `--min-severity` -- not zero, but not
    /// surfaced in `top_sessions` either.
    pub sessions_below_min_severity: usize,
    /// Sessions whose peak severity meets `--min-severity` (default: any exposure at all).
    /// This is the pre-`--limit`-truncation count; `top_sessions` may be shorter.
    pub sessions_with_exposure: usize,
    pub sessions_by_highest_severity: BTreeMap<String, usize>,
    /// Aggregate category breakdown across every qualifying session (before `--limit`
    /// truncates the displayed list).
    pub totals: RedactionReport,
    pub top_sessions: Vec<SessionThreatEntry>,
}

/// Redaction-scan a single trace, returning its aggregate report plus a location for every
/// event (or provenance/metadata field) that contributed at least one redaction.
///
/// Walking events one at a time (rather than calling `Redactor::preview_redactions` once for
/// the whole trace) costs no more work -- same total text scanned, same rules applied -- but
/// additionally reveals per-event/location detail that `preview_redactions` collapses away.
/// Provenance and metadata are included too, matching exactly what
/// `Redactor::redact_trace`/`preview_redactions` themselves cover, so `session_report` here is
/// never a subset of what `agentworth export --redact` would actually strip from this trace.
pub fn scan_trace_for_threats(
    redactor: &Redactor,
    trace: &AgentWorthTrace,
) -> (RedactionReport, Vec<ThreatFinding>) {
    let mut session_report = RedactionReport::new();
    let mut findings = Vec::new();

    let mut prov_report = RedactionReport::new();
    let _ = redactor.redact_text_with_counts(&trace.provenance.source_path, &mut prov_report);
    if prov_report.total() > 0 {
        findings.push(ThreatFinding {
            event_sequence: None,
            timestamp: trace.started_at,
            location: LOCATION_PROVENANCE.to_string(),
            categories: prov_report.breakdown_by_category.clone(),
            total: prov_report.total(),
        });
        session_report.merge(&prov_report);
    }

    let mut meta_report = RedactionReport::new();
    let _ = redactor.redact_json_with_counts(&trace.metadata, &mut meta_report);
    if meta_report.total() > 0 {
        findings.push(ThreatFinding {
            event_sequence: None,
            timestamp: trace.started_at,
            location: LOCATION_METADATA.to_string(),
            categories: meta_report.breakdown_by_category.clone(),
            total: meta_report.total(),
        });
        session_report.merge(&meta_report);
    }

    for event in &trace.events {
        let mut event_report = RedactionReport::new();
        let _ = redactor.redact_event_with_counts(event, &mut event_report);
        if event_report.total() > 0 {
            findings.push(ThreatFinding {
                event_sequence: Some(event.sequence),
                timestamp: event.timestamp,
                location: format!("{:?}", event.payload.event_type()),
                categories: event_report.breakdown_by_category.clone(),
                total: event_report.total(),
            });
            session_report.merge(&event_report);
        }
    }

    (session_report, findings)
}

/// Scan every indexed session and build the ranked Threat Digest report.
pub fn generate_threat_digest(
    storage: &Arc<Storage>,
    limit: Option<usize>,
    min_severity: ThreatSeverity,
) -> Result<ThreatDigestReport> {
    let all_sessions = storage.list_sessions_filtered(&SessionFilter {
        limit: Some(10000),
        include_stubs: Some(true),
        ..Default::default()
    })?;

    let scanner = Scanner::new(storage.clone());
    let redactor = Redactor::new();

    let mut sessions_unreadable = 0usize;
    let mut sessions_clean = 0usize;
    let mut sessions_below_min_severity = 0usize;
    let mut totals = RedactionReport::new();
    let mut sessions_by_highest_severity: BTreeMap<String, usize> = BTreeMap::new();
    let mut entries: Vec<SessionThreatEntry> = Vec::new();

    for summary in &all_sessions {
        let trace = match scanner.load_trace(&summary.session_id) {
            Ok(t) => t,
            Err(_) => {
                sessions_unreadable += 1;
                continue;
            }
        };

        let (session_report, mut findings) = scan_trace_for_threats(&redactor, &trace);

        let highest = match highest_severity_present(&session_report) {
            Some(h) => h,
            None => {
                sessions_clean += 1;
                continue;
            }
        };

        if highest < min_severity {
            sessions_below_min_severity += 1;
            continue;
        }

        totals.merge(&session_report);
        *sessions_by_highest_severity
            .entry(highest.as_str().to_string())
            .or_insert(0) += 1;

        findings.sort_by(|a, b| b.total.cmp(&a.total));
        let findings_truncated = findings.len() > MAX_FINDINGS_PER_SESSION;
        findings.truncate(MAX_FINDINGS_PER_SESSION);

        entries.push(SessionThreatEntry {
            session_id: summary.session_id.clone(),
            adapter: summary.adapter.clone(),
            source_path: summary.source_path.clone(),
            started_at: summary.started_at,
            risk_score: compute_risk_score(&session_report),
            highest_severity: highest,
            report: session_report,
            findings,
            findings_truncated,
        });
    }

    let sessions_with_exposure = entries.len();
    entries.sort_by(|a, b| {
        b.risk_score
            .cmp(&a.risk_score)
            .then_with(|| b.started_at.cmp(&a.started_at))
    });
    if let Some(lim) = limit {
        entries.truncate(lim);
    }

    Ok(ThreatDigestReport {
        sessions_scanned: all_sessions.len(),
        sessions_unreadable,
        sessions_clean,
        sessions_below_min_severity,
        sessions_with_exposure,
        sessions_by_highest_severity,
        totals,
        top_sessions: entries,
    })
}

/// Execute the `agentworth threat-digest` subcommand.
pub fn run_threat_digest_command(
    limit: usize,
    min_severity: &str,
    json: bool,
    db_path: Option<PathBuf>,
) -> Result<()> {
    let storage = Arc::new(match db_path {
        Some(p) => Storage::open_path(&p)?,
        None => Storage::open_default()?,
    });

    let min_severity = ThreatSeverity::parse(min_severity)?;
    let report = generate_threat_digest(&storage, Some(limit), min_severity)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    render_ascii_threat_digest(&report);
    Ok(())
}

fn render_ascii_threat_digest(report: &ThreatDigestReport) {
    println!();
    println!(
        "{}",
        style("┌─ 🔐 AgentWorth Threat Digest: Secret Exposure Report ───────┐")
            .bold()
            .red()
    );
    println!(
        "│ Sessions Scanned:  {:<42} │",
        style(report.sessions_scanned).bold()
    );
    println!(
        "│ Exposure Found In: {:<42} │",
        style(format!("{} sessions", report.sessions_with_exposure))
            .bold()
            .yellow()
    );
    println!(
        "│ Clean Sessions:    {:<42} │",
        style(report.sessions_clean).green()
    );
    if report.sessions_unreadable > 0 {
        println!(
            "│ Unreadable:        {:<42} │",
            style(format!(
                "{} (source file moved/removed)",
                report.sessions_unreadable
            ))
            .dim()
        );
    }
    println!(
        "{}",
        style("├────────────────────────────────────────────────────────────┤").bold()
    );

    let crit = *report
        .sessions_by_highest_severity
        .get("CRITICAL")
        .unwrap_or(&0);
    let high = *report.sessions_by_highest_severity.get("HIGH").unwrap_or(&0);
    let med = *report
        .sessions_by_highest_severity
        .get("MEDIUM")
        .unwrap_or(&0);
    let low = *report.sessions_by_highest_severity.get("LOW").unwrap_or(&0);
    println!(
        "│ By Peak Severity:  {} Critical  {} High  {} Medium  {} Low{:>7} │",
        style(crit).bold().red(),
        style(high).bold().yellow(),
        style(med).bold().cyan(),
        style(low).dim(),
        ""
    );

    if !report.totals.breakdown_by_category.is_empty() {
        println!(
            "{}",
            style("├────────────────────────────────────────────────────────────┤").bold()
        );
        println!("│ Category Breakdown (all qualifying sessions):               │");
        for (category, count) in &report.totals.breakdown_by_category {
            println!("│   {:<44} {:>16} │", category, count);
        }
    }

    println!(
        "{}",
        style("├────────────────────────────────────────────────────────────┤").bold()
    );

    if report.top_sessions.is_empty() {
        println!("│ ✓ No exposure at or above the requested severity threshold.│");
    } else {
        println!("│ Top Sessions by Risk (rotate these first):                 │");
        for (i, s) in report.top_sessions.iter().enumerate() {
            let sev_styled = match s.highest_severity {
                ThreatSeverity::Critical => style("[CRITICAL]").bold().red(),
                ThreatSeverity::High => style("[HIGH]").bold().yellow(),
                ThreatSeverity::Medium => style("[MEDIUM]").bold().cyan(),
                ThreatSeverity::Low => style("[LOW]").dim(),
            };
            println!(
                "│ [{:02}] {:<11} {:<24} Risk:{:>6} │",
                i + 1,
                sev_styled,
                style(&s.session_id).bold(),
                s.risk_score
            );
            println!(
                "│      Adapter: {:<15} Findings: {:<24} │",
                style(&s.adapter).green(),
                s.report.total()
            );
            let cats: Vec<String> = s
                .report
                .breakdown_by_category
                .iter()
                .map(|(k, v)| format!("{}:{}", k, v))
                .collect();
            println!("│      {:<56} │", style(cats.join(", ")).dim());
        }
    }

    println!(
        "{}",
        style("└────────────────────────────────────────────────────────────┘").bold()
    );
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_adapters::ClaudeCodeAdapter;
    use agentworth_adapter_sdk::ScanOptions;
    use agentworth_schema::{
        EventPayload, FileActionType, NormalizedEvent, Provenance, ShellCommand,
    };
    use serde_json::json;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // --- Pure classification/scoring functions -------------------------------------

    #[test]
    fn test_category_severity_mapping() {
        assert_eq!(
            category_severity(RedactionCategory::PrivateKey),
            ThreatSeverity::Critical
        );
        assert_eq!(
            category_severity(RedactionCategory::ApiKey),
            ThreatSeverity::Critical
        );
        assert_eq!(
            category_severity(RedactionCategory::Credential),
            ThreatSeverity::Critical
        );
        assert_eq!(
            category_severity(RedactionCategory::JwtToken),
            ThreatSeverity::High
        );
        assert_eq!(
            category_severity(RedactionCategory::HighEntropySecret),
            ThreatSeverity::High
        );
        assert_eq!(
            category_severity(RedactionCategory::EnvVar),
            ThreatSeverity::High
        );
        assert_eq!(
            category_severity(RedactionCategory::Custom),
            ThreatSeverity::High
        );
        assert_eq!(
            category_severity(RedactionCategory::IpAddress),
            ThreatSeverity::Medium
        );
        assert_eq!(
            category_severity(RedactionCategory::Email),
            ThreatSeverity::Low
        );
        assert_eq!(
            category_severity(RedactionCategory::FilePath),
            ThreatSeverity::Low
        );
    }

    #[test]
    fn test_severity_ordering_critical_is_max() {
        assert!(ThreatSeverity::Critical > ThreatSeverity::High);
        assert!(ThreatSeverity::High > ThreatSeverity::Medium);
        assert!(ThreatSeverity::Medium > ThreatSeverity::Low);
        let all = [
            ThreatSeverity::Low,
            ThreatSeverity::Critical,
            ThreatSeverity::Medium,
        ];
        assert_eq!(all.into_iter().max(), Some(ThreatSeverity::Critical));
    }

    #[test]
    fn test_parse_severity_accepts_known_values_case_insensitively() {
        assert_eq!(ThreatSeverity::parse("low").unwrap(), ThreatSeverity::Low);
        assert_eq!(
            ThreatSeverity::parse("CRITICAL").unwrap(),
            ThreatSeverity::Critical
        );
        assert!(ThreatSeverity::parse("extreme").is_err());
    }

    #[test]
    fn test_compute_risk_score_weights_critical_over_pile_of_noise() {
        let mut noisy_but_low = RedactionReport::new();
        noisy_but_low.add(RedactionCategory::FilePath, 200);
        noisy_but_low.add(RedactionCategory::Email, 50);

        let mut single_leaked_key = RedactionReport::new();
        single_leaked_key.add(RedactionCategory::ApiKey, 1);

        assert!(
            compute_risk_score(&single_leaked_key) > compute_risk_score(&noisy_but_low),
            "one leaked API key (score {}) must outrank 250 routine redactions (score {})",
            compute_risk_score(&single_leaked_key),
            compute_risk_score(&noisy_but_low)
        );
    }

    #[test]
    fn test_highest_severity_present() {
        assert_eq!(highest_severity_present(&RedactionReport::new()), None);

        let mut low_only = RedactionReport::new();
        low_only.add(RedactionCategory::Email, 3);
        assert_eq!(highest_severity_present(&low_only), Some(ThreatSeverity::Low));

        let mut mixed = RedactionReport::new();
        mixed.add(RedactionCategory::Email, 3);
        mixed.add(RedactionCategory::ApiKey, 1);
        mixed.add(RedactionCategory::IpAddress, 2);
        assert_eq!(highest_severity_present(&mixed), Some(ThreatSeverity::Critical));
    }

    // --- scan_trace_for_threats: in-memory trace, no storage/adapter involved ------

    #[test]
    fn test_scan_trace_for_threats_finds_categories_and_locations_without_leaking_secret() {
        let redactor = Redactor::new();
        let start = Utc::now();

        let leaked_key = "sk-proj-1234567890abcdef1234567890abcdef12";
        let leaked_ip = "192.168.50.7";
        let leaked_email = "leaker@example.com";
        let home_path = "/Users/leaker/secrets";

        let prov = Provenance::new(
            format!("{}/session.jsonl", home_path),
            "claude_code",
            10,
            10,
            "fp-threat-1",
        );
        let mut trace = AgentWorthTrace::new("sess-threat-1", "claude_code", prov, start);
        trace.metadata = json!({ "reporter_email": leaked_email });

        trace.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::UserMessage {
                content: format!("Here is my key: {leaked_key}"),
            },
        ));
        trace.events.push(NormalizedEvent::new(
            2,
            start,
            EventPayload::ShellCommand(ShellCommand {
                command: "curl internal".to_string(),
                cwd: None,
                exit_code: Some(0),
                output: Some(format!("connected to {leaked_ip}")),
            }),
        ));
        trace.events.push(NormalizedEvent::new(
            3,
            start,
            EventPayload::AssistantMessage {
                content: "Nothing sensitive in this turn.".to_string(),
                thinking: None,
            },
        ));

        let (session_report, findings) = scan_trace_for_threats(&redactor, &trace);

        // Categories found: ApiKey (user message), IpAddress (shell output), Email (metadata),
        // FilePath (the home-directory prefix baked into provenance.source_path above).
        assert_eq!(session_report.api_keys_count, 1);
        assert_eq!(session_report.ip_addresses_count, 1);
        assert_eq!(session_report.emails_count, 1);
        assert!(session_report.paths_count >= 1);
        assert!(!session_report.is_clean());

        // Invariant: every redaction the aggregate report counted is accounted for by exactly
        // one location in `findings` (nothing silently dropped, nothing double-counted).
        let findings_total: usize = findings.iter().map(|f| f.total).sum();
        assert_eq!(findings_total, session_report.total());

        // Locations are meaningful and distinguish event vs. provenance vs. metadata.
        let locations: Vec<&str> = findings.iter().map(|f| f.location.as_str()).collect();
        assert!(locations.contains(&"UserMessage"));
        assert!(locations.contains(&"ShellCommand"));
        assert!(locations.contains(&LOCATION_METADATA));
        assert!(locations.contains(&LOCATION_PROVENANCE));
        // The clean AssistantMessage (sequence 3) must not produce a finding at all.
        assert!(!findings.iter().any(|f| f.event_sequence == Some(3)));

        // The report itself must never reproduce the actual secret values anywhere --
        // categories/locations are strings too, so prove they're clean by serializing.
        let dump = serde_json::to_string(&findings).unwrap();
        assert!(!dump.contains(leaked_key));
        assert!(!dump.contains(leaked_ip));
        assert!(!dump.contains(leaked_email));
        assert!(!dump.contains("/Users/leaker"));
    }

    #[test]
    fn test_scan_trace_for_threats_reports_clean_trace_as_empty() {
        let redactor = Redactor::new();
        let start = Utc::now();
        let prov = Provenance::new("session.jsonl", "claude_code", 10, 10, "fp-clean");
        let mut trace = AgentWorthTrace::new("sess-clean", "claude_code", prov, start);
        trace.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::UserMessage {
                content: "Please refactor the parser module.".to_string(),
            },
        ));

        let (session_report, findings) = scan_trace_for_threats(&redactor, &trace);
        assert!(session_report.is_clean());
        assert!(findings.is_empty());
        assert_eq!(highest_severity_present(&session_report), None);
    }

    #[test]
    fn test_scan_trace_for_threats_truncation_preserves_totals() {
        // A session with dozens of file-action diffs each containing a distinct home path --
        // proves per-session finding truncation in generate_threat_digest (tested below)
        // doesn't need to touch the underlying report/risk math, only the displayed list.
        let redactor = Redactor::new();
        let start = Utc::now();
        let prov = Provenance::new("many.jsonl", "claude_code", 10, 10, "fp-many");
        let mut trace = AgentWorthTrace::new("sess-many", "claude_code", prov, start);
        for i in 0..30u64 {
            trace.events.push(NormalizedEvent::new(
                i + 1,
                start,
                EventPayload::FileAction {
                    path: format!("/Users/dev{i}/file.rs"),
                    action: FileActionType::Edit,
                    diff: None,
                    lines_changed: None,
                },
            ));
        }

        let (session_report, findings) = scan_trace_for_threats(&redactor, &trace);
        assert_eq!(session_report.paths_count, 30);
        assert_eq!(findings.len(), 30, "scan itself must not truncate");
    }

    // --- generate_threat_digest: real Storage + Scanner + on-disk fixtures ----------

    /// Writes a minimal but real Claude Code JSONL session file and returns its derived
    /// session_id (the file stem), matching the shape `crates/core/src/lib.rs`'s own tests
    /// use -- deliberately going through the real adapter parse path rather than constructing
    /// an `AgentWorthTrace` by hand, since the property under test is specifically "does a
    /// live re-parse of a real file surface what's actually in it."
    fn write_claude_session(user_content: &str, assistant_content: &str) -> NamedTempFile {
        let mut temp = tempfile::Builder::new()
            .suffix(".jsonl")
            .tempfile()
            .unwrap();
        let line1 = json!({
            "type": "user",
            "timestamp": "2026-08-29T10:00:00Z",
            "content": user_content,
        });
        let line2 = json!({
            "type": "assistant",
            "timestamp": "2026-08-29T10:00:05Z",
            "model": "claude-3-5-sonnet",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_read_input_tokens": 0,
                "cache_creation_input_tokens": 0
            },
            "content": [{"type": "text", "text": assistant_content}],
        });
        writeln!(temp, "{}", line1).unwrap();
        writeln!(temp, "{}", line2).unwrap();
        temp
    }

    fn session_id_of(temp: &NamedTempFile) -> String {
        temp.path().file_stem().unwrap().to_string_lossy().to_string()
    }

    #[test]
    fn test_generate_threat_digest_ranks_and_filters_by_severity() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner =
            Scanner::with_adapters(vec![Box::new(ClaudeCodeAdapter::new())], storage.clone());

        // Session A: a real-shaped OpenAI key (Critical) plus a private IP (Medium).
        let leaked_key = "sk-proj-1234567890abcdef1234567890abcdef12";
        let session_a = write_claude_session(
            &format!("use this key please: {leaked_key}"),
            "noted, will use 192.168.50.7 for the internal endpoint",
        );

        // Session B: routine noise only -- an email and nothing else sensitive.
        let session_b = write_claude_session(
            "reach me at developer@example.com if this breaks",
            "will do, no other action needed",
        );

        // Session C: entirely clean.
        let session_c = write_claude_session(
            "please add a unit test for the parser",
            "done, added tests/parser_test.rs",
        );

        for temp in [&session_a, &session_b, &session_c] {
            let options = ScanOptions {
                custom_paths: vec![temp.path().to_path_buf()],
                force: true,
            };
            let summary = scanner.run_scan(&options, |_, _| {}).expect("scan run");
            assert_eq!(summary.scanned_sessions, 1);
        }

        let id_a = session_id_of(&session_a);
        let id_b = session_id_of(&session_b);

        // Default threshold (Low): both A and B qualify. (Session C's exact bucket isn't
        // asserted here: whether its temp-file provenance path incidentally matches the
        // home-path rule depends on the build box's tmp dir, which this test doesn't control
        // -- see the fully-controlled in-memory `scan_trace_for_threats` tests above for a
        // strict "clean trace stays clean" assertion. FilePath is Low severity either way, so
        // it can never change which session ranks first or promote C above B.)
        let report =
            generate_threat_digest(&storage, None, ThreatSeverity::Low).expect("digest");
        assert_eq!(report.sessions_scanned, 3);
        assert_eq!(report.sessions_unreadable, 0);
        assert!(report.sessions_with_exposure >= 2);

        // A must outrank B: a leaked API key beats an email address every time.
        assert_eq!(report.top_sessions[0].session_id, id_a);
        assert_eq!(report.top_sessions[0].highest_severity, ThreatSeverity::Critical);
        let entry_b = report
            .top_sessions
            .iter()
            .find(|s| s.session_id == id_b)
            .expect("session B present with at least Low-severity exposure");
        assert_eq!(entry_b.highest_severity, ThreatSeverity::Low);
        assert!(report.top_sessions[0].risk_score > entry_b.risk_score);

        assert_eq!(report.totals.api_keys_count, 1);
        assert_eq!(report.totals.ip_addresses_count, 1);
        assert_eq!(report.totals.emails_count, 1);

        // The digest itself must never contain the raw leaked key.
        let dump = serde_json::to_string(&report).unwrap();
        assert!(!dump.contains(leaked_key));

        // Raising the floor to High: only A (Critical) qualifies. B and C are both Low-severity
        // ceiling at most (an email or a redacted home path, never anything higher), so neither
        // can ever clear a High floor -- this holds regardless of the provenance-path question
        // above.
        let strict =
            generate_threat_digest(&storage, None, ThreatSeverity::High).expect("digest strict");
        assert_eq!(strict.sessions_with_exposure, 1);
        assert_eq!(strict.top_sessions.len(), 1);
        assert_eq!(strict.top_sessions[0].session_id, id_a);

        // --limit truncates the displayed list but not the accounting counts.
        let limited =
            generate_threat_digest(&storage, Some(1), ThreatSeverity::Low).expect("digest limit");
        assert!(limited.sessions_with_exposure >= 2);
        assert_eq!(limited.top_sessions.len(), 1);
        assert_eq!(limited.top_sessions[0].session_id, id_a);
    }

    #[test]
    fn test_generate_threat_digest_counts_missing_source_file_as_unreadable() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner =
            Scanner::with_adapters(vec![Box::new(ClaudeCodeAdapter::new())], storage.clone());

        let temp = write_claude_session("hello", "hi there");
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: true,
        };
        scanner.run_scan(&options, |_, _| {}).expect("scan run");

        // The session is indexed in SQLite, but its backing file is now gone.
        temp.close().expect("remove backing file");

        let report =
            generate_threat_digest(&storage, None, ThreatSeverity::Low).expect("digest");
        assert_eq!(report.sessions_scanned, 1);
        assert_eq!(report.sessions_unreadable, 1);
        assert_eq!(report.sessions_clean, 0);
        assert_eq!(report.sessions_with_exposure, 0);
        assert!(report.top_sessions.is_empty());
    }

    #[test]
    fn test_generate_threat_digest_accounting_invariant() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner =
            Scanner::with_adapters(vec![Box::new(ClaudeCodeAdapter::new())], storage.clone());

        let leaked_key = "sk-proj-abcdefabcdefabcdefabcdefabcdefabcd";
        let sessions = [
            write_claude_session(&format!("key: {leaked_key}"), "ok"),
            write_claude_session("email me at a@example.com", "ok"),
            write_claude_session("nothing sensitive here", "all clear"),
        ];
        for temp in &sessions {
            let options = ScanOptions {
                custom_paths: vec![temp.path().to_path_buf()],
                force: true,
            };
            scanner.run_scan(&options, |_, _| {}).expect("scan run");
        }

        let report =
            generate_threat_digest(&storage, None, ThreatSeverity::Medium).expect("digest");
        // Every scanned session lands in exactly one bucket.
        assert_eq!(
            report.sessions_scanned,
            report.sessions_unreadable
                + report.sessions_clean
                + report.sessions_below_min_severity
                + report.sessions_with_exposure
        );
    }
}
