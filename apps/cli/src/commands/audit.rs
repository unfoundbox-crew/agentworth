//! Safety and threat audit command for AgentWorth.
//!
//! Subcommand: `agwt audit [--safety] [--json]`
//! Detects dangerous tool calls (`rm -rf`, leaked shell variables like `$d`, unconstrained sweeps, fake test claims, credential leaks)
//! and displays a forensic safety report with severity levels (CRITICAL, HIGH, WARN).

use std::path::PathBuf;
use std::sync::Arc;

use crate::commands::{is_high_risk_rm_path, is_leaked_katana_var, APOLOGY_PATTERNS};
use agentworth_core::Scanner;
use agentworth_redaction::{RedactionCategory, RedactionReport, Redactor};
use agentworth_schema::{AgentWorthTrace, EventPayload, NormalizedEvent};
use agentworth_storage::{extract_repository_or_workspace, SessionFilter, Storage};
use anyhow::Result;
use console::style;
use serde::{Deserialize, Serialize};

/// Severity levels for forensic safety findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum SafetySeverity {
    Critical,
    High,
    Warn,
}

impl SafetySeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Critical => "CRITICAL",
            Self::High => "HIGH",
            Self::Warn => "WARN",
        }
    }
}

/// A single detected forensic safety issue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SafetyFinding {
    pub severity: SafetySeverity,
    pub session_id: String,
    pub adapter: String,
    pub timestamp: String,
    pub rule_id: String,
    pub title: String,
    pub description: String,
    pub offending_snippet: String,
    pub turn_index: usize,
    pub project: String,
}

/// Aggregated Safety Audit Report.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SafetyAuditReport {
    pub total_sessions_audited: usize,
    pub critical_count: usize,
    pub high_count: usize,
    pub warn_count: usize,
    pub findings: Vec<SafetyFinding>,
}

/// Execute the `agwt audit` command.
pub fn run_audit_command(
    safety_only: bool,
    json_output: bool,
    db_path: Option<PathBuf>,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let scanner = Scanner::new(storage.clone());

    let all_sessions = storage.list_sessions_filtered(&SessionFilter {
        limit: Some(5000),
        include_stubs: Some(true),
        ..Default::default()
    })?;

    let mut report = SafetyAuditReport {
        total_sessions_audited: all_sessions.len(),
        critical_count: 0,
        high_count: 0,
        warn_count: 0,
        findings: Vec::new(),
    };

    // Shared with `agentworth threat-digest` and `agentworth export --redact` -- using the
    // same `Redactor` here means `agwt audit --safety` can no longer disagree with either of
    // them about what counts as a leaked secret. See `detect_credential_leak` below.
    let redactor = Redactor::new();

    for sess in &all_sessions {
        if let Ok(trace) = scanner.load_trace(&sess.session_id) {
            let project = extract_repository_or_workspace(&sess.source_path);
            audit_trace(&trace, &project, safety_only, &redactor, &mut report);
        }
    }

    // Sort findings: Critical first, then High, then Warn, then by timestamp desc
    report.findings.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then_with(|| b.timestamp.cmp(&a.timestamp))
    });

    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    render_ascii_safety_report(&report);

    Ok(())
}

fn open_storage(db_path: Option<PathBuf>) -> Result<Arc<Storage>> {
    if let Some(path) = db_path {
        Ok(Arc::new(Storage::open_path(&path)?))
    } else {
        Ok(Arc::new(Storage::open_default()?))
    }
}

/// Audit a single trace for safety and threat vectors.
fn audit_trace(
    trace: &AgentWorthTrace,
    project: &str,
    safety_only: bool,
    redactor: &Redactor,
    report: &mut SafetyAuditReport,
) {
    let mut had_failed_test = false;
    let mut last_failed_test_turn = 0;
    let mut had_test_executed = false;
    let mut apology_count = 0;

    for (idx, event) in trace.events.iter().enumerate() {
        let turn_num = event.sequence as usize;
        let ts = event.timestamp.to_rfc3339();

        // Runs for every event kind, unconditionally (a leaked secret is a safety concern
        // whether or not `--safety` was passed -- matches how the credential checks below used
        // to run unconditionally too, before this call replaced them). See
        // `detect_credential_leak` for why this now covers every event kind instead of just
        // ShellCommand/ToolCall/UserMessage.
        if let Some(finding) = detect_credential_leak(event, redactor, trace, project, turn_num, &ts)
        {
            report.high_count += 1;
            report.findings.push(finding);
        }

        match &event.payload {
            // 1. Shell Command Checks
            EventPayload::ShellCommand(cmd) => {
                let cmd_str = &cmd.command;
                let lower_cmd = cmd_str.to_lowercase();

                if is_test_command(&lower_cmd) {
                    had_test_executed = true;
                    if let Some(code) = cmd.exit_code {
                        if code != 0 {
                            had_failed_test = true;
                            last_failed_test_turn = turn_num;
                        } else {
                            had_failed_test = false;
                        }
                    }
                }

                // Check Critical: rm -rf on protected paths or with leaked variables. Gated on
                // `is_recursive_delete_command` first -- without that guard, every ShellCommand
                // event (ls, git status, echo, cargo build, ...) fell through to the `else` arm
                // below and was mislabeled as "recursive file removal." See
                // docs/DECISION-INBOX.md for how this was found (while unifying credential
                // detection in this same file, unrelated to that change).
                if is_recursive_delete_command(&lower_cmd) {
                    if is_leaked_katana_var(&lower_cmd) {
                        report.critical_count += 1;
                        report.findings.push(SafetyFinding {
                            severity: SafetySeverity::Critical,
                            session_id: trace.session_id.clone(),
                            adapter: trace.adapter.clone(),
                            timestamp: ts.clone(),
                            rule_id: "LEAKED_SHELL_VARIABLE".to_string(),
                            title: "Leaked Shell Variable in Destructive Deletion".to_string(),
                            description: "Command executes 'rm -rf' with an unconstrained/leaked shell variable ($d), matching the Katana disaster signature where a missing 'local d' deleted protected repositories.".to_string(),
                            offending_snippet: cmd_str.clone(),
                            turn_index: turn_num,
                            project: project.to_string(),
                        });
                    } else if is_windows_drive_wipe(&lower_cmd) {
                        report.critical_count += 1;
                        report.findings.push(SafetyFinding {
                            severity: SafetySeverity::Critical,
                            session_id: trace.session_id.clone(),
                            adapter: trace.adapter.clone(),
                            timestamp: ts.clone(),
                            rule_id: "WINDOWS_DRIVE_WIPE".to_string(),
                            title: "Unconstrained Windows Drive Wipe".to_string(),
                            description: "Agent executed 'rmdir /s /q' targeting the C: drive without strict sandbox containment.".to_string(),
                            offending_snippet: cmd_str.clone(),
                            turn_index: turn_num,
                            project: project.to_string(),
                        });
                    } else if is_high_risk_rm_path(&lower_cmd) {
                        report.critical_count += 1;
                        report.findings.push(SafetyFinding {
                            severity: SafetySeverity::Critical,
                            session_id: trace.session_id.clone(),
                            adapter: trace.adapter.clone(),
                            timestamp: ts.clone(),
                            rule_id: "FORBIDDEN_RM_RF".to_string(),
                            title: "Unconstrained Recursive Directory Deletion".to_string(),
                            description: "Agent executed 'rm -rf' targeting top-level project, root, or home workspace directory without strict sandbox containment.".to_string(),
                            offending_snippet: cmd_str.clone(),
                            turn_index: turn_num,
                            project: project.to_string(),
                        });
                    } else {
                        report.high_count += 1;
                        report.findings.push(SafetyFinding {
                            severity: SafetySeverity::High,
                            session_id: trace.session_id.clone(),
                            adapter: trace.adapter.clone(),
                            timestamp: ts.clone(),
                            rule_id: "RECURSIVE_DELETION".to_string(),
                            title: "Recursive Directory Deletion".to_string(),
                            description: "Agent invoked recursive file removal ('rm -rf').".to_string(),
                            offending_snippet: cmd_str.clone(),
                            turn_index: turn_num,
                            project: project.to_string(),
                        });
                    }
                }

                // Check High: Unconstrained sweeps
                if let Some(sweep_sig) = find_destructive_sweep_signature(&lower_cmd) {
                    report.high_count += 1;
                    report.findings.push(SafetyFinding {
                        severity: SafetySeverity::High,
                        session_id: trace.session_id.clone(),
                        adapter: trace.adapter.clone(),
                        timestamp: ts.clone(),
                        rule_id: "UNCONSTRAINED_SWEEP".to_string(),
                        title: format!("Unconstrained Destructive Sweep ({})", sweep_sig),
                        description: format!(
                            "Command invoked potentially hazardous system sweep operation: '{}'.",
                            sweep_sig
                        ),
                        offending_snippet: cmd_str.clone(),
                        turn_index: turn_num,
                        project: project.to_string(),
                    });
                }
            }

            // 2. Assistant Message Checks (Fake Claims & Apology Cascades - only when not safety_only)
            EventPayload::AssistantMessage { content, thinking } => {
                if !safety_only {
                    let lower_content = content.to_lowercase();
                    let lower_thinking = thinking.as_deref().map(|t| t.to_lowercase()).unwrap_or_default();

                    if is_fake_test_claim(&lower_content) {
                        if had_failed_test {
                            report.warn_count += 1;
                            report.findings.push(SafetyFinding {
                                severity: SafetySeverity::Warn,
                                session_id: trace.session_id.clone(),
                                adapter: trace.adapter.clone(),
                                timestamp: ts.clone(),
                                rule_id: "FAKE_TEST_CLAIM".to_string(),
                                title: "False Success Claim on Failing Tests".to_string(),
                                description: format!(
                                    "Assistant claimed tests passed or succeeded, but command at turn #{} previously failed with non-zero exit code.",
                                    last_failed_test_turn
                                ),
                                offending_snippet: truncate_str(content, 200),
                                turn_index: turn_num,
                                project: project.to_string(),
                            });
                        } else if !had_test_executed && idx > 2 {
                            report.warn_count += 1;
                            report.findings.push(SafetyFinding {
                                severity: SafetySeverity::Warn,
                                session_id: trace.session_id.clone(),
                                adapter: trace.adapter.clone(),
                                timestamp: ts.clone(),
                                rule_id: "UNVERIFIED_COMPLETION_CLAIM".to_string(),
                                title: "Unverified Done Claim without Test Evidence".to_string(),
                                description: "Assistant claimed test completion without any verified test execution receipts in trace.".to_string(),
                                offending_snippet: truncate_str(content, 200),
                                turn_index: turn_num,
                                project: project.to_string(),
                            });
                        }
                    }

                    // Check for apology panic signatures
                    for p in APOLOGY_PATTERNS {
                        if lower_content.contains(p) || lower_thinking.contains(p) {
                            apology_count += 1;
                            break;
                        }
                    }
                }
            }

            _ => {}
        }
    }

    if !safety_only && apology_count >= 3 {
        report.warn_count += 1;
        report.findings.push(SafetyFinding {
            severity: SafetySeverity::Warn,
            session_id: trace.session_id.clone(),
            adapter: trace.adapter.clone(),
            timestamp: trace.started_at.to_rfc3339(),
            rule_id: "APOLOGY_PANIC_CASCADE".to_string(),
            title: "Excessive Panic & Grovel Loop".to_string(),
            description: format!(
                "Assistant entered a multi-turn apology loop ({} distinct apology/panic turns), wasting developer tokens.",
                apology_count
            ),
            offending_snippet: format!("Observed {} apology turns across session.", apology_count),
            turn_index: 0,
            project: project.to_string(),
        });
    }
}

fn find_destructive_sweep_signature(cmd: &str) -> Option<&'static str> {
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

/// True if `cmd` (already lowercased) actually looks like a recursive and/or bulk directory
/// deletion. Gates the Critical/High rm classification above -- without this guard, *every*
/// `ShellCommand` (`ls`, `git status`, `echo`, `cargo build`, ...) fell into that chain's `else`
/// arm and was mislabeled "recursive file removal." Deliberately broader than
/// `is_leaked_katana_var`'s own internal substring check (`rm -rf`/`rm -fr`/`rm -r -f` only): a
/// bare `rm -r` (recursive, no explicit `-f`) is still a real recursive deletion in a
/// non-interactive agent shell (nothing prompts for confirmation), so it's still worth surfacing
/// as at least `RECURSIVE_DELETION` -- it just can't be the *Katana* signature specifically,
/// which needs an actual leaked variable in an `-rf`-shaped command. A non-recursive `rm -f
/// somefile.txt` (single-file force delete, no `-r` anywhere) is deliberately excluded: it isn't
/// a "recursive directory deletion" and doesn't belong in this rule at all -- see
/// `find_destructive_sweep_signature` for other single-purpose destructive commands.
fn is_recursive_delete_command(cmd: &str) -> bool {
    cmd.contains("rm -rf")
        || cmd.contains("rm -fr")
        || cmd.contains("rm -r -f")
        || cmd.contains("rm -f -r")
        || cmd.contains("rm -r ")
        || cmd.ends_with("rm -r")
        || cmd.contains("rm --recursive")
        || is_windows_drive_wipe(cmd)
}

/// True if `cmd` (already lowercased) matches the Windows `rmdir /s /q C:\` drive-wipe
/// signature. Mirrors `blunder.rs`'s identical check exactly (kept local here rather than
/// shared, since it's a one-line condition used by exactly two call sites with otherwise
/// unrelated surrounding logic).
fn is_windows_drive_wipe(cmd: &str) -> bool {
    cmd.contains("rmdir /s /q c:\\") || cmd.contains("rmdir /s /q c:")
}

fn is_test_command(cmd: &str) -> bool {
    cmd.contains("cargo test")
        || cmd.contains("npm test")
        || cmd.contains("pnpm test")
        || cmd.contains("pytest")
        || cmd.contains("go test")
        || cmd.contains("jest")
}

fn is_fake_test_claim(content: &str) -> bool {
    content.contains("all tests are passing")
        || content.contains("all tests pass")
        || content.contains("tests passed successfully")
        || content.contains("all 8 tests are now passing")
        || content.contains("test suite is green")
        || content.contains("fixed the bug and tests pass")
        // Chinese fake test claim phrases
        || content.contains("测试已全部通过")
        || content.contains("所有测试通过")
        || content.contains("测试通过")
        || content.contains("所有测试均已通过")
        || content.contains("测试全部通过")
}

/// Redaction categories this audit treats as an actual leaked credential/secret: a named
/// vendor key, a private key blob, a URL with embedded credentials, a bearer/JWT, a
/// `KEY=`-shaped env var, a user-defined custom rule, or the high-entropy fallback for a
/// novel/unrecognized secret format. This is the same Critical/High tier that
/// `threat_digest::category_severity` scores a session by -- `FilePath`, `Email`, and
/// `IpAddress` are deliberately excluded (threat_digest's Medium/Low tier): routine noise
/// present in nearly every real session, not something to rotate. Without this filter, running
/// the full `Redactor` over every event would turn "credential leak audit" into "flag every
/// home directory path and email address," which is not what `agwt audit --safety` is for.
const CREDENTIAL_CATEGORIES: [RedactionCategory; 7] = [
    RedactionCategory::ApiKey,
    RedactionCategory::PrivateKey,
    RedactionCategory::Credential,
    RedactionCategory::JwtToken,
    RedactionCategory::EnvVar,
    RedactionCategory::HighEntropySecret,
    RedactionCategory::Custom,
];

/// Reads a single category's count out of a `RedactionReport`. Exhaustive match (mirrors
/// `RedactionReport::add`'s own match over the same fields, in `crates/redaction/src/report.rs`)
/// so a new `RedactionCategory` variant fails to compile here instead of silently reading zero.
fn category_count(report: &RedactionReport, category: RedactionCategory) -> usize {
    match category {
        RedactionCategory::ApiKey => report.api_keys_count,
        RedactionCategory::EnvVar => report.env_vars_count,
        RedactionCategory::FilePath => report.paths_count,
        RedactionCategory::Email => report.emails_count,
        RedactionCategory::Credential => report.credentials_count,
        RedactionCategory::JwtToken => report.jwt_tokens_count,
        RedactionCategory::IpAddress => report.ip_addresses_count,
        RedactionCategory::PrivateKey => report.private_keys_count,
        RedactionCategory::HighEntropySecret => report.high_entropy_secrets_count,
        RedactionCategory::Custom => report.custom_count,
    }
}

/// Which of `CREDENTIAL_CATEGORIES` actually fired in `report`, in fixed declaration order.
fn credential_categories_in(report: &RedactionReport) -> Vec<RedactionCategory> {
    CREDENTIAL_CATEGORIES
        .into_iter()
        .filter(|&c| category_count(report, c) > 0)
        .collect()
}

/// Human label for where a finding was found, for the safety report's `title` field.
/// Presentation only -- unlike `credential_categories_in`, this does not need to be exhaustive
/// for correctness. A new/uncommon event kind just falls back to its `EventType` debug name
/// until someone gives it a nicer label; detection itself never depends on this function.
fn event_kind_label(payload: &EventPayload) -> String {
    match payload {
        EventPayload::ShellCommand(_) => "Shell Command".to_string(),
        EventPayload::ToolCall(tc) => format!("Tool Call '{}'", tc.name),
        EventPayload::ToolResult(tr) => match &tr.name {
            Some(name) => format!("Tool Result '{}'", name),
            None => "Tool Result".to_string(),
        },
        EventPayload::UserMessage { .. } => "User Prompt".to_string(),
        EventPayload::AssistantMessage { .. } => "Assistant Response".to_string(),
        EventPayload::FileAction { path, .. } => format!("File Action '{}'", path),
        EventPayload::Error { .. } => "Error Message".to_string(),
        EventPayload::ModelSwitch(_) => "Model Switch".to_string(),
        EventPayload::HumanIntervention(_) => "Human Intervention".to_string(),
        other => format!("{:?}", other.event_type()),
    }
}

/// Best-effort human-readable text for a finding's `offending_snippet`, extracted from an
/// *already-redacted* payload (see `detect_credential_leak`, which is the only caller) so this
/// never has to re-derive which substring was the secret -- whatever it returns is always safe
/// to display verbatim. The fallback (`other => format!("{:?}", other)`) covers event kinds
/// unlikely to ever carry a credential (`ModelInvocation`, `OutcomeEvidence`, `Custom`) with a
/// plain struct dump rather than a bespoke branch for each.
fn snippet_from_payload(payload: &EventPayload) -> String {
    match payload {
        EventPayload::ShellCommand(cmd) => {
            let mut parts = vec![cmd.command.clone()];
            if let Some(out) = &cmd.output {
                parts.push(format!("output: {}", out));
            }
            parts.join(" | ")
        }
        EventPayload::ToolCall(tc) => format!("{}({})", tc.name, tc.arguments),
        EventPayload::ToolResult(tr) => tr.output.to_string(),
        EventPayload::UserMessage { content } => content.clone(),
        EventPayload::AssistantMessage { content, .. } => content.clone(),
        EventPayload::FileAction { diff, path, .. } => diff.clone().unwrap_or_else(|| path.clone()),
        EventPayload::Error { message, .. } => message.clone(),
        EventPayload::ModelSwitch(ms) => format!(
            "{} -> {}{}",
            ms.from_model.as_deref().unwrap_or("?"),
            ms.to_model,
            ms.reason.as_ref().map(|r| format!(" ({})", r)).unwrap_or_default()
        ),
        EventPayload::HumanIntervention(hi) => match &hi.details {
            Some(d) => format!("{}: {}", hi.action, d),
            None => hi.action.clone(),
        },
        other => format!("{:?}", other),
    }
}

/// Scans one event for a leaked credential/secret via the shared `Redactor` -- the same rule
/// set `agentworth threat-digest` and `agentworth export --redact` use, so `agwt audit --safety`
/// can no longer disagree with either of them about what counts as a leak. Runs over the
/// *whole* event via `Redactor::redact_event_with_counts`, which already knows the full field
/// list for every event kind (command/cwd/output for a shell command, name/arguments for a tool
/// call, output for a tool result, content/thinking for messages, and so on) -- so this covers
/// every event kind Redactor covers, not just the three the old hand-rolled regex checked.
///
/// Runs unconditionally regardless of `safety_only`: a leaked credential is a safety concern in
/// both modes, matching how the old ShellCommand/ToolCall/UserMessage checks were never gated
/// behind `safety_only` either (only the fake-test-claim/apology-cascade *quality* checks are).
fn detect_credential_leak(
    event: &NormalizedEvent,
    redactor: &Redactor,
    trace: &AgentWorthTrace,
    project: &str,
    turn_num: usize,
    ts: &str,
) -> Option<SafetyFinding> {
    let mut event_report = RedactionReport::new();
    let redacted_event = redactor.redact_event_with_counts(event, &mut event_report);

    let categories = credential_categories_in(&event_report);
    if categories.is_empty() {
        return None;
    }

    let category_names: Vec<String> = categories.iter().map(|c| c.to_string()).collect();

    Some(SafetyFinding {
        severity: SafetySeverity::High,
        session_id: trace.session_id.clone(),
        adapter: trace.adapter.clone(),
        timestamp: ts.to_string(),
        rule_id: "CREDENTIAL_LEAK".to_string(),
        title: format!("Exposed Secret in {}", event_kind_label(&event.payload)),
        description: format!(
            "Event content contained plaintext secrets or credentials ({}).",
            category_names.join(", ")
        ),
        offending_snippet: truncate_str(&snippet_from_payload(&redacted_event.payload), 200),
        turn_index: turn_num,
        project: project.to_string(),
    })
}

fn truncate_str(s: &str, max_len: usize) -> String {
    let trimmed = s.trim();
    if trimmed.len() <= max_len {
        trimmed.to_string()
    } else {
        format!("{}...", &trimmed[..max_len - 3])
    }
}

/// Render the ASCII Safety Audit Report.
fn render_ascii_safety_report(report: &SafetyAuditReport) {
    println!();
    println!(
        "{}",
        style("┌─ 🛡️  AgentWorth Agent Safety & Forensic Threat Audit ────────────────┐")
            .bold()
            .cyan()
    );
    println!(
        "│ Sessions Audited: {:<50} │",
        style(report.total_sessions_audited).bold()
    );
    println!(
        "│ Threat Summary:   {} Critical  •  {} High  •  {} Warn {:>10} │",
        style(format!("{}", report.critical_count)).bold().red(),
        style(format!("{}", report.high_count)).bold().yellow(),
        style(format!("{}", report.warn_count)).bold().cyan(),
        ""
    );
    println!(
        "{}",
        style("├────────────────────────────────────────────────────────────────────────┤")
            .bold()
    );

    if report.findings.is_empty() {
        println!("│ ✓ Zero security threats or dangerous tool calls detected across index. │");
        println!(
            "{}",
            style("└────────────────────────────────────────────────────────────────────────┘")
                .bold()
        );
        println!();
        return;
    }

    for (i, f) in report.findings.iter().enumerate() {
        let (_sev_label, sev_styled) = match f.severity {
            SafetySeverity::Critical => ("CRITICAL", style("[CRITICAL]").bold().red()),
            SafetySeverity::High => ("HIGH", style("[HIGH]").bold().yellow()),
            SafetySeverity::Warn => ("WARN", style("[WARN]").bold().cyan()),
        };

        println!(
            "│ {}  {:<12} {:<49} │",
            style(format!("#{:02}", i + 1)).dim(),
            sev_styled,
            style(&f.title).bold()
        );
        println!(
            "│ Rule ID:   {:<24} Project: {:<27} │",
            style(&f.rule_id).magenta(),
            style(&f.project).cyan()
        );
        println!(
            "│ Session:   {:<24} Adapter: {:<27} │",
            style(&f.session_id).bold(),
            style(&f.adapter).green()
        );
        println!(
            "│ Timestamp: {:<24} Turn:    {:<27} │",
            style(&f.timestamp).dim(),
            style(format!("#{}", f.turn_index)).yellow()
        );

        println!(
            "│ {}",
            style("──────────────────────────────────────────────────────────────────────").dim()
        );

        // Description wrapped
        println!("│ Description:                                                           │");
        for line in wrap_line(&f.description, 66) {
            println!("│   {:<68} │", style(line).dim());
        }

        // Snippet wrapped
        println!("│ Forensic Snippet:                                                      │");
        for line in wrap_line(&f.offending_snippet, 66) {
            println!("│   {:<68} │", style(line).red());
        }

        if i + 1 < report.findings.len() {
            println!(
                "{}",
                style("├────────────────────────────────────────────────────────────────────────┤")
                    .bold()
            );
        }
    }

    println!(
        "{}",
        style("└────────────────────────────────────────────────────────────────────────┘")
            .bold()
    );
    println!();
}

fn wrap_line(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.len() + word.len() + 1 > max_width {
                if !current.is_empty() {
                    lines.push(current);
                    current = String::new();
                }
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    if lines.is_empty() {
        lines.push(text.to_string());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{NormalizedEvent, Provenance, ShellCommand, ToolResult};
    use chrono::Utc;
    use serde_json::json;

    #[test]
    fn test_audit_safety_only_filters_warn_checks() {
        let start = Utc::now();
        let prov = Provenance::new("/test/path.jsonl", "claude_code", 100, 12345, "fp123");
        let mut trace = AgentWorthTrace::new("sess-audit-test", "claude_code", prov, start);

        // Turn 1: Critical safety threat (rm -rf $d)
        trace.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::ShellCommand(ShellCommand {
                command: "rm -rf $d/cache".to_string(),
                cwd: None,
                exit_code: Some(0),
                output: None,
            }),
        ));

        // Turns 2, 3, 4: Apology loops (quality warning)
        trace.events.push(NormalizedEvent::new(
            2,
            start,
            EventPayload::AssistantMessage {
                content: "I apologize for the mistake.".to_string(),
                thinking: None,
            },
        ));
        trace.events.push(NormalizedEvent::new(
            3,
            start,
            EventPayload::AssistantMessage {
                content: "I am sorry about this error.".to_string(),
                thinking: None,
            },
        ));
        trace.events.push(NormalizedEvent::new(
            4,
            start,
            EventPayload::AssistantMessage {
                content: "My apologies, let me fix it now.".to_string(),
                thinking: None,
            },
        ));

        let redactor = Redactor::new();

        // 1. Audit with safety_only = false (standard mode: both safety and quality warnings)
        let mut full_report = SafetyAuditReport::default();
        audit_trace(&trace, "test-proj", false, &redactor, &mut full_report);
        assert_eq!(full_report.critical_count, 1);
        assert_eq!(full_report.warn_count, 1);
        assert_eq!(full_report.findings.len(), 2);
        assert!(full_report.findings.iter().any(|f| f.rule_id == "LEAKED_SHELL_VARIABLE"));
        assert!(full_report.findings.iter().any(|f| f.rule_id == "APOLOGY_PANIC_CASCADE"));

        // 2. Audit with safety_only = true (safety-only mode: only critical/high safety threats)
        let mut safety_report = SafetyAuditReport::default();
        audit_trace(&trace, "test-proj", true, &redactor, &mut safety_report);
        assert_eq!(safety_report.critical_count, 1);
        assert_eq!(safety_report.warn_count, 0);
        assert_eq!(safety_report.findings.len(), 1);
        assert_eq!(safety_report.findings[0].rule_id, "LEAKED_SHELL_VARIABLE");
    }

    /// Proves the rm-detection guard fix: the Critical/High classification chain in the
    /// `ShellCommand` arm must only run for commands that actually look like a recursive/bulk
    /// deletion. Before `is_recursive_delete_command` was added as a guard, *every* one of the
    /// "must not fire" cases below produced a spurious HIGH `RECURSIVE_DELETION` finding
    /// ("Agent invoked recursive file removal ('rm -rf')") even though none of them touch `rm`
    /// at all -- found while unifying credential detection in this same file (unrelated to that
    /// change), see docs/DECISION-INBOX.md.
    #[test]
    fn test_shell_command_classification_requires_actual_deletion() {
        let cases: &[(&str, Option<&str>)] = &[
            // Benign commands: must produce zero rm-related findings.
            ("ls -la", None),
            ("git status", None),
            ("echo hello", None),
            ("cargo build --workspace", None),
            // Force but not recursive: a single-file delete, not a "recursive directory
            // deletion" -- deliberately excluded, see `is_recursive_delete_command`'s doc.
            ("rm -f somefile.txt", None),
            // Real deletions: all four branches must still fire, with the right rule_id.
            ("rm -rf $d/cache", Some("LEAKED_SHELL_VARIABLE")),
            ("rm -rf /", Some("FORBIDDEN_RM_RF")),
            ("rmdir /s /q c:\\", Some("WINDOWS_DRIVE_WIPE")),
            ("rm -rf ./some/nested/build-output", Some("RECURSIVE_DELETION")),
            ("rm -r ./some/nested/build-output", Some("RECURSIVE_DELETION")),
            ("rm -f -r ./some/nested/build-output", Some("RECURSIVE_DELETION")),
        ];

        for (cmd, expected_rule_id) in cases {
            let start = Utc::now();
            let prov = Provenance::new("/test/path.jsonl", "claude_code", 100, 12345, "fp-rm");
            let mut trace = AgentWorthTrace::new("sess-rm-test", "claude_code", prov, start);
            trace.events.push(NormalizedEvent::new(
                1,
                start,
                EventPayload::ShellCommand(ShellCommand {
                    command: cmd.to_string(),
                    cwd: None,
                    exit_code: Some(0),
                    output: None,
                }),
            ));

            let redactor = Redactor::new();
            let mut report = SafetyAuditReport::default();
            audit_trace(&trace, "test-proj", true, &redactor, &mut report);

            let rm_findings: Vec<&SafetyFinding> = report
                .findings
                .iter()
                .filter(|f| {
                    matches!(
                        f.rule_id.as_str(),
                        "LEAKED_SHELL_VARIABLE"
                            | "FORBIDDEN_RM_RF"
                            | "WINDOWS_DRIVE_WIPE"
                            | "RECURSIVE_DELETION"
                    )
                })
                .collect();

            match expected_rule_id {
                None => assert!(
                    rm_findings.is_empty(),
                    "command {:?} must not produce an rm-related finding, got: {:?}",
                    cmd,
                    rm_findings
                ),
                Some(rule_id) => {
                    assert_eq!(
                        rm_findings.len(),
                        1,
                        "command {:?} should produce exactly one rm-related finding, got: {:?}",
                        cmd,
                        rm_findings
                    );
                    assert_eq!(
                        rm_findings[0].rule_id, *rule_id,
                        "wrong rule_id for command {:?}",
                        cmd
                    );
                }
            }
        }
    }

    /// Proves the two coverage gaps the hand-rolled `cred_regex` had, both called out in the
    /// dispatch brief: (1) it only ever checked ShellCommand/ToolCall/UserMessage, never
    /// ToolResult or AssistantMessage; (2) it had no high-entropy fallback, so a real secret in
    /// a format with no recognized vendor prefix sailed through undetected. Uses the redaction
    /// crate's own `Redactor` (via `audit_trace`) to prove both are now caught, and that each
    /// still surfaces through the same `CREDENTIAL_LEAK` rule_id / `High` severity `audit
    /// --safety` already used for the three original event kinds.
    #[test]
    fn test_credential_leak_covers_new_event_kinds_and_high_entropy_fallback() {
        let start = Utc::now();
        let prov = Provenance::new("/test/path.jsonl", "claude_code", 100, 12345, "fp124");
        let mut trace = AgentWorthTrace::new("sess-secret-test", "claude_code", prov, start);

        // Turn 1: a ToolResult carrying a *newer*-format GitHub PAT (`github_pat_...`) in its
        // output -- a format the old regex's `ghp_[a-zA-Z0-9]{36}` alternative never matched at
        // all, and ToolResult is an event kind the old code never inspected for credentials.
        let github_pat_body = format!("11{}", "ABCDEFGHIJ".repeat(7)); // 72 chars, within {60,100}
        trace.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::ToolResult(ToolResult {
                call_id: Some("t1".to_string()),
                name: Some("Bash".to_string()),
                output: json!(format!("token: github_pat_{github_pat_body}")),
                is_error: false,
            }),
        ));

        // Turn 2: an AssistantMessage containing a high-entropy secret with no recognized vendor
        // prefix -- exactly the shape the old code had *no* fallback for at all, and
        // AssistantMessage is another event kind the old code never credential-scanned.
        let high_entropy_secret = "K7xQ2mZpL9vNaC4tRfY6sJ1hWbE8dU3o"; // 32 chars, ~5 bits/char
        trace.events.push(NormalizedEvent::new(
            2,
            start,
            EventPayload::AssistantMessage {
                content: format!("Sure, here's the token you asked for: {high_entropy_secret}"),
                thinking: None,
            },
        ));

        let redactor = Redactor::new();
        let mut report = SafetyAuditReport::default();
        audit_trace(&trace, "test-proj", true, &redactor, &mut report);

        let leaks: Vec<&SafetyFinding> = report
            .findings
            .iter()
            .filter(|f| f.rule_id == "CREDENTIAL_LEAK")
            .collect();
        assert_eq!(
            leaks.len(),
            2,
            "expected one CREDENTIAL_LEAK finding per event, got: {:?}",
            report.findings
        );
        assert!(leaks.iter().all(|f| f.severity == SafetySeverity::High));

        let tool_result_leak = leaks
            .iter()
            .find(|f| f.turn_index == 1)
            .expect("ToolResult credential leak detected");
        assert!(tool_result_leak.description.contains("API Key"));
        assert!(!tool_result_leak.offending_snippet.contains(&github_pat_body));

        let assistant_leak = leaks
            .iter()
            .find(|f| f.turn_index == 2)
            .expect("AssistantMessage credential leak detected");
        assert!(assistant_leak.description.contains("High-Entropy Secret"));
        assert!(!assistant_leak.offending_snippet.contains(high_entropy_secret));

        // Sanity check against the redaction crate directly: proves this isn't a coincidence of
        // the test's own regex but a real property of the shared default rule set.
        assert!(Redactor::new().redact_text(high_entropy_secret) != high_entropy_secret);
    }

    /// Negative case: routine PII (a home path, an email, a private IP) must *not* trigger
    /// `CREDENTIAL_LEAK`, even though `Redactor` itself does redact all three. Without this
    /// filter, scanning every event with the full `Redactor` would turn "credential leak audit"
    /// into "flag every file path," which would make `agwt audit --safety` useless noise on
    /// almost every real session.
    ///
    /// Uses a `ToolResult` fixture rather than `ShellCommand` deliberately: `audit_trace`'s
    /// `EventPayload::ShellCommand` arm has a separate, pre-existing bug (unrelated to this
    /// change, not fixed here -- flagged instead, see `docs/DECISION-INBOX.md`) where its
    /// rm-detection `if/else-if/else` has no guard confirming the command is actually an `rm`
    /// before falling through to an unconditional `RECURSIVE_DELETION` finding. A `ShellCommand`
    /// fixture here would trip that unrelated bug and make this test's failure ambiguous about
    /// which behavior broke.
    #[test]
    fn test_credential_leak_does_not_fire_on_routine_pii_categories() {
        let start = Utc::now();
        let prov = Provenance::new("/test/path.jsonl", "claude_code", 100, 12345, "fp125");
        let mut trace = AgentWorthTrace::new("sess-pii-only", "claude_code", prov, start);

        trace.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::ToolResult(ToolResult {
                call_id: Some("t1".to_string()),
                name: Some("ls".to_string()),
                output: json!("contact me@example.com from /Users/dev/project at 192.168.1.5"),
                is_error: false,
            }),
        ));

        let redactor = Redactor::new();
        let mut report = SafetyAuditReport::default();
        audit_trace(&trace, "test-proj", true, &redactor, &mut report);

        assert!(
            report.findings.is_empty(),
            "a home path / email / private IP alone must not read as a credential leak: {:?}",
            report.findings
        );
    }
}
