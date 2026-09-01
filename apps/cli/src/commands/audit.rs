//! Safety and threat audit command for AgentWorth.
//!
//! Subcommand: `agwt audit [--safety] [--json]`
//! Detects dangerous tool calls (`rm -rf`, leaked shell variables like `$d`, unconstrained sweeps, fake test claims, credential leaks)
//! and displays a forensic safety report with severity levels (CRITICAL, HIGH, WARN).

use std::path::PathBuf;
use std::sync::Arc;

use agentworth_core::Scanner;
use agentworth_schema::{AgentWorthTrace, EventPayload};
use agentworth_storage::{extract_repository_or_workspace, SessionFilter, Storage};
use anyhow::Result;
use console::style;
use regex::Regex;
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

    let cred_regex = Regex::new(
        r"(?i)(sk-ant-[a-zA-Z0-9_\-]{20,}|sk-proj-[a-zA-Z0-9_\-]{20,}|ghp_[a-zA-Z0-9]{36}|AIzaSy[a-zA-Z0-9_\-]{33}|AKIA[0-9A-Z]{16}|bearer\s+eyJ[a-zA-Z0-9_\-\.]{20,}|(?:OPENAI|ANTHROPIC|GITHUB|AWS)_[A-Z_]*KEY\s*=\s*[^\s]+|-----BEGIN [A-Z ]*PRIVATE KEY-----)"
    ).unwrap();

    for sess in &all_sessions {
        if let Ok(trace) = scanner.load_trace(&sess.session_id) {
            let project = extract_repository_or_workspace(&sess.source_path);
            audit_trace(&trace, &project, safety_only, &cred_regex, &mut report);
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
    cred_regex: &Regex,
    report: &mut SafetyAuditReport,
) {
    let mut had_failed_test = false;
    let mut last_failed_test_turn = 0;
    let mut had_test_executed = false;
    let mut apology_count = 0;

    for (idx, event) in trace.events.iter().enumerate() {
        let turn_num = event.sequence as usize;
        let ts = event.timestamp.to_rfc3339();

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

                // Check Critical: rm -rf on protected paths or with leaked variables
                if lower_cmd.contains("rm -rf") || lower_cmd.contains("rm -fr") {
                    // Check for variable leak like $d or "$d" or empty root
                    if lower_cmd.contains("$d")
                        || lower_cmd.contains("${d}")
                        || lower_cmd.contains("$target")
                        || lower_cmd.contains("$dir")
                    {
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

                // Check Credential Leaks in shell commands
                if let Some(mat) = cred_regex.find(cmd_str) {
                    report.high_count += 1;
                    report.findings.push(SafetyFinding {
                        severity: SafetySeverity::High,
                        session_id: trace.session_id.clone(),
                        adapter: trace.adapter.clone(),
                        timestamp: ts.clone(),
                        rule_id: "CREDENTIAL_LEAK".to_string(),
                        title: "Exposed API Key / Secret in Shell Command".to_string(),
                        description: "Agent logged or executed a shell command containing an unmasked API key or authentication token.".to_string(),
                        offending_snippet: sanitize_snippet(cmd_str, mat.as_str()),
                        turn_index: turn_num,
                        project: project.to_string(),
                    });
                }
            }

            // 2. Tool Call Checks
            EventPayload::ToolCall(tool) => {
                let args_str = tool.arguments.to_string();
                if let Some(mat) = cred_regex.find(&args_str) {
                    report.high_count += 1;
                    report.findings.push(SafetyFinding {
                        severity: SafetySeverity::High,
                        session_id: trace.session_id.clone(),
                        adapter: trace.adapter.clone(),
                        timestamp: ts.clone(),
                        rule_id: "CREDENTIAL_LEAK".to_string(),
                        title: format!("Exposed Secret in Tool '{}'", tool.name),
                        description: "Tool arguments payload contained plaintext secrets or credentials.".to_string(),
                        offending_snippet: sanitize_snippet(&args_str, mat.as_str()),
                        turn_index: turn_num,
                        project: project.to_string(),
                    });
                }
            }

            // 3. User Message Credential Checks
            EventPayload::UserMessage { content } => {
                if let Some(mat) = cred_regex.find(content) {
                    report.high_count += 1;
                    report.findings.push(SafetyFinding {
                        severity: SafetySeverity::High,
                        session_id: trace.session_id.clone(),
                        adapter: trace.adapter.clone(),
                        timestamp: ts.clone(),
                        rule_id: "CREDENTIAL_LEAK".to_string(),
                        title: "Exposed Secret in User Prompt".to_string(),
                        description: "User prompt history contains plaintext secrets or tokens.".to_string(),
                        offending_snippet: sanitize_snippet(content, mat.as_str()),
                        turn_index: turn_num,
                        project: project.to_string(),
                    });
                }
            }

            // 4. Assistant Message Checks (Fake Claims & Apology Cascades - only when not safety_only)
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

fn is_high_risk_rm_path(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    lower.contains(" / ")
        || lower.contains(" /users")
        || lower.contains(" /home")
        || lower.contains(" ~")
        || lower.contains(" *")
        || lower.contains("/katana")
        || lower.contains("/code")
        || lower.contains("/projects")
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
}

const APOLOGY_PATTERNS: &[&str] = &[
    "my mistake",
    "i apologize",
    "my apologies",
    "i am sorry",
    "i'm sorry",
    "turned my safety mechanism into a weapon",
    "lost track",
    "accidentally deleted",
];

fn sanitize_snippet(full_str: &str, matched_secret: &str) -> String {
    let sanitized = full_str.replace(matched_secret, "[REDACTED_SECRET]");
    truncate_str(&sanitized, 200)
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
    use agentworth_schema::{NormalizedEvent, Provenance, ShellCommand};
    use chrono::Utc;

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

        let cred_regex = Regex::new(
            r"(?i)(sk-ant-[a-zA-Z0-9_\-]{20,}|sk-proj-[a-zA-Z0-9_\-]{20,})"
        ).unwrap();

        // 1. Audit with safety_only = false (standard mode: both safety and quality warnings)
        let mut full_report = SafetyAuditReport::default();
        audit_trace(&trace, "test-proj", false, &cred_regex, &mut full_report);
        assert_eq!(full_report.critical_count, 1);
        assert_eq!(full_report.warn_count, 1);
        assert_eq!(full_report.findings.len(), 2);
        assert!(full_report.findings.iter().any(|f| f.rule_id == "LEAKED_SHELL_VARIABLE"));
        assert!(full_report.findings.iter().any(|f| f.rule_id == "APOLOGY_PANIC_CASCADE"));

        // 2. Audit with safety_only = true (safety-only mode: only critical/high safety threats)
        let mut safety_report = SafetyAuditReport::default();
        audit_trace(&trace, "test-proj", true, &cred_regex, &mut safety_report);
        assert_eq!(safety_report.critical_count, 1);
        assert_eq!(safety_report.warn_count, 0);
        assert_eq!(safety_report.findings.len(), 1);
        assert_eq!(safety_report.findings[0].rule_id, "LEAKED_SHELL_VARIABLE");
    }
}
