//! Outcome detection engine based on the AgentWorth outcome hierarchy.
//!
//! Outcome Hierarchy:
//! `DoneClaimed < ArtifactChanged < TestOrBuildPassed < CommitObserved < CiOrDeploymentVerified`

use agentworth_schema::{
    AgentWorthTrace, EventPayload, NormalizedEvent, OutcomeEvidence, OutcomeKind,
};

/// Detector that scans normalized events and traces for outcome evidence.
#[derive(Debug, Default, Clone)]
pub struct OutcomeDetector;

/// Alias for OutcomeDetector highlighting the hierarchical detection capabilities.
pub type OutcomeHierarchyDetector = OutcomeDetector;

/// Return the canonical string name for an OutcomeKind.
pub fn outcome_kind_name(kind: OutcomeKind) -> &'static str {
    match kind {
        OutcomeKind::CiOrDeploymentVerified => "CiOrDeploymentVerified",
        OutcomeKind::CommitObserved => "CommitObserved",
        OutcomeKind::TestOrBuildPassed => "TestOrBuildPassed",
        OutcomeKind::ArtifactChanged => "ArtifactChanged",
        OutcomeKind::DoneClaimed => "DoneClaimed",
    }
}

impl OutcomeDetector {
    pub fn new() -> Self {
        Self
    }

    /// Detect all outcome evidence items from a trace, independently verified against real
    /// git/filesystem state and cross-referenced against other real events in the same trace
    /// wherever possible (see `detect_outcomes_with_verification` for the verification detail).
    ///
    /// This is the same detector every real caller (the scanner, the scorer) already uses, so
    /// verification applies automatically — no call site elsewhere needs to change.
    pub fn detect_outcomes(&self, trace: &AgentWorthTrace) -> Vec<OutcomeEvidence> {
        self.detect_outcomes_with_verification(trace).0
    }

    /// Same as `detect_outcomes`, but also returns a note for every claim whose confidence or
    /// kind was adjusted because reality did (or didn't) back it up.
    pub fn detect_outcomes_with_verification(
        &self,
        trace: &AgentWorthTrace,
    ) -> (Vec<OutcomeEvidence>, Vec<crate::verify::VerificationNote>) {
        let mut indexed = self.classify_from_events(&trace.events);
        let repo_root = crate::verify::resolve_repo_root(&trace.events, Some(&trace.provenance));
        let notes =
            crate::verify::verify_outcomes(&trace.events, &mut indexed, repo_root.as_deref());
        (indexed.into_iter().map(|(_, ev)| ev).collect(), notes)
    }

    /// Detect all outcome evidence items from a sequence of events, independently verified
    /// wherever the events themselves resolve to a real, on-disk repository (via `ShellCommand`
    /// cwd values). Prefer `detect_outcomes` when a full `AgentWorthTrace` is available — it can
    /// additionally fall back to Claude Code's transcript-path convention when no cwd is present.
    pub fn detect_from_events(&self, events: &[NormalizedEvent]) -> Vec<OutcomeEvidence> {
        let mut indexed = self.classify_from_events(events);
        let repo_root = crate::verify::resolve_repo_root(events, None);
        let _notes = crate::verify::verify_outcomes(events, &mut indexed, repo_root.as_deref());
        indexed.into_iter().map(|(_, ev)| ev).collect()
    }

    /// Classify raw events into outcome evidence, still paired with the source event's index so
    /// a later verification pass can look back at exactly what produced each claim.
    fn classify_from_events(&self, events: &[NormalizedEvent]) -> Vec<(usize, OutcomeEvidence)> {
        let mut outcomes = Vec::new();

        for (idx, event) in events.iter().enumerate() {
            match &event.payload {
                // Direct outcome evidence already attached to an event
                EventPayload::OutcomeEvidence(evidence) => {
                    outcomes.push((idx, evidence.clone()));
                }

                // File actions -> ArtifactChanged
                EventPayload::FileAction { path, action, .. } => {
                    outcomes.push((
                        idx,
                        OutcomeEvidence {
                            kind: OutcomeKind::ArtifactChanged,
                            summary: format!("File {:?} operation on '{}'", action, path),
                            confidence: 0.60,
                        },
                    ));
                }

                // Tool calls that write/edit files or run git/tests/ci
                EventPayload::ToolCall(tool_call) => {
                    if let Some(evidence) = self.detect_from_tool_call(tool_call) {
                        outcomes.push((idx, evidence));
                    }
                }

                // Shell commands
                EventPayload::ShellCommand(cmd) => {
                    if let Some(evidence) = self.detect_from_shell_command(cmd) {
                        outcomes.push((idx, evidence));
                    }
                }

                // Tool results
                EventPayload::ToolResult(res) => {
                    if let Some(evidence) = self.detect_from_tool_result(res) {
                        outcomes.push((idx, evidence));
                    }
                }

                // Assistant messages claiming completion
                EventPayload::AssistantMessage { content, .. } => {
                    if let Some(evidence) = self.detect_from_assistant_message(content) {
                        outcomes.push((idx, evidence));
                    }
                }

                _ => {}
            }
        }

        outcomes
    }

    /// Returns the highest-confidence and highest-rank outcome evidence if present.
    pub fn strongest_outcome<'a>(
        &self,
        outcomes: &'a [OutcomeEvidence],
    ) -> Option<&'a OutcomeEvidence> {
        outcomes.iter().max_by(|a, b| {
            let rank_a = outcome_rank(a.kind);
            let rank_b = outcome_rank(b.kind);
            rank_a.cmp(&rank_b).then_with(|| {
                a.confidence
                    .partial_cmp(&b.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        })
    }

    fn detect_from_tool_call(&self, tool: &agentworth_schema::ToolCall) -> Option<OutcomeEvidence> {
        let name_lower = tool.name.to_lowercase();

        // File modifications
        if name_lower.contains("write")
            || name_lower.contains("edit")
            || name_lower.contains("replace")
            || name_lower.contains("patch")
            || name_lower.contains("create_file")
            || name_lower.contains("strreplaceedit")
        {
            return Some(OutcomeEvidence {
                kind: OutcomeKind::ArtifactChanged,
                summary: format!(
                    "Tool invocation '{}' modified workspace artifacts",
                    tool.name
                ),
                confidence: 0.60,
            });
        }

        // If tool call is a bash/exec command, inspect arguments
        if name_lower == "bash"
            || name_lower == "execute"
            || name_lower == "run_command"
            || name_lower == "terminal"
        {
            if let Some(cmd_str) = tool.arguments.get("command").and_then(|v| v.as_str()) {
                return self.classify_command_string(cmd_str, None, None);
            }
            if let Some(cmd_str) = tool.arguments.get("CommandLine").and_then(|v| v.as_str()) {
                return self.classify_command_string(cmd_str, None, None);
            }
        }

        None
    }

    fn detect_from_tool_result(
        &self,
        res: &agentworth_schema::ToolResult,
    ) -> Option<OutcomeEvidence> {
        if res.is_error {
            return None;
        }

        let output_str = match &res.output {
            serde_json::Value::String(s) => s.as_str(),
            serde_json::Value::Object(o) => o
                .get("output")
                .or_else(|| o.get("stdout"))
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
            _ => "",
        };

        if output_str.is_empty() {
            return None;
        }

        self.classify_output_text(output_str)
    }

    fn detect_from_shell_command(
        &self,
        cmd: &agentworth_schema::ShellCommand,
    ) -> Option<OutcomeEvidence> {
        // If explicitly failed exit code, it's not a verified positive outcome
        if let Some(code) = cmd.exit_code {
            if code != 0 {
                return None;
            }
        }

        self.classify_command_string(&cmd.command, cmd.exit_code, cmd.output.as_deref())
    }

    fn classify_command_string(
        &self,
        cmd_str: &str,
        exit_code: Option<i32>,
        output: Option<&str>,
    ) -> Option<OutcomeEvidence> {
        let trimmed = cmd_str.trim().to_lowercase();

        // 1. CI / PR / Deployment (Highest Rank)
        if is_ci_or_deploy_command(&trimmed) {
            let conf = if exit_code == Some(0) { 0.98 } else { 0.90 };
            return Some(OutcomeEvidence {
                kind: OutcomeKind::CiOrDeploymentVerified,
                summary: format!("Deployment or CI command executed: '{}'", cmd_str),
                confidence: conf,
            });
        }

        // 2. Commit observed
        if is_commit_command(&trimmed) {
            let conf = if exit_code == Some(0) { 0.90 } else { 0.80 };
            return Some(OutcomeEvidence {
                kind: OutcomeKind::CommitObserved,
                summary: format!("Git commit command observed: '{}'", cmd_str),
                confidence: conf,
            });
        }

        // 3. Test or build execution
        if is_test_or_build_command(&trimmed) {
            // If output is present, verify no failures
            if let Some(out) = output {
                if has_test_failure_markers(out) {
                    return None;
                }
            }

            let conf = if exit_code == Some(0) { 0.85 } else { 0.70 };
            return Some(OutcomeEvidence {
                kind: OutcomeKind::TestOrBuildPassed,
                summary: format!("Test or build suite executed: '{}'", cmd_str),
                confidence: conf,
            });
        }

        // If output has explicit test/commit/ci signals, classify from output
        if let Some(out) = output {
            if let Some(ev) = self.classify_output_text(out) {
                return Some(ev);
            }
        }

        None
    }

    fn classify_output_text(&self, out: &str) -> Option<OutcomeEvidence> {
        let out_lower = out.to_lowercase();

        // Check for test failures first
        if has_test_failure_markers(out) {
            return None;
        }

        // CI / Deployment indicators
        if out_lower.contains("deployment complete")
            || out_lower.contains("deployed to https://")
            || out_lower.contains("merged pull request")
            || out_lower.contains("pull request successfully created")
            || out_lower.contains("workflow run completed with status success")
        {
            return Some(OutcomeEvidence {
                kind: OutcomeKind::CiOrDeploymentVerified,
                summary: "External CI or deployment success output verified".to_string(),
                confidence: 0.95,
            });
        }

        // Commit indicators
        if (out.contains("[main ")
            || out.contains("[master ")
            || out.contains("commit ")
            || out.contains("insertion(+)"))
            && (out.contains("file changed") || out.contains("files changed"))
        {
            return Some(OutcomeEvidence {
                kind: OutcomeKind::CommitObserved,
                summary: "Git commit creation output observed".to_string(),
                confidence: 0.88,
            });
        }

        // Test passed indicators
        if out.contains("test result: ok.")
            || out.contains("Doc-tests") && out.contains("ok")
            || out.contains("Test Suites: ") && out.contains("passed")
            || out.contains("Tests:       ") && out.contains("passed")
            || out.contains("tests passed")
            || out.contains("PASSED [")
            || out_lower.contains("all tests passed")
            || (out.contains("PASS ") && (out.contains(".test.") || out.contains(".spec.")))
        {
            return Some(OutcomeEvidence {
                kind: OutcomeKind::TestOrBuildPassed,
                summary: "Test suite execution output verified passed".to_string(),
                confidence: 0.85,
            });
        }

        None
    }

    fn detect_from_assistant_message(&self, content: &str) -> Option<OutcomeEvidence> {
        let lower = content.to_lowercase();
        let completion_phrases = [
            "i have completed",
            "task is complete",
            "task has been completed",
            "successfully implemented",
            "all tests pass",
            "all tests are passing",
            "everything is in place",
            "implementation is complete",
            "finished implementing",
            "done! i have",
            "done with the changes",
            // Chinese completion phrases for Kimi / Qwen / Zhipu / DeepSeek / MiniMax
            "已完成",
            "任务已完成",
            "全部完成",
            "测试已通过",
            "测试已全部通过",
            "所有测试通过",
            "已成功修复",
            "代码已修改完毕",
            "修改已完成",
            "已实现",
        ];

        for phrase in &completion_phrases {
            if lower.contains(phrase) || content.contains(phrase) {
                return Some(OutcomeEvidence {
                    kind: OutcomeKind::DoneClaimed,
                    summary: format!("Agent self-claimed completion: '{}'", phrase),
                    confidence: 0.35,
                });
            }
        }

        None
    }
}

pub fn outcome_rank(kind: OutcomeKind) -> u8 {
    match kind {
        OutcomeKind::DoneClaimed => 1,
        OutcomeKind::ArtifactChanged => 2,
        OutcomeKind::TestOrBuildPassed => 3,
        OutcomeKind::CommitObserved => 4,
        OutcomeKind::CiOrDeploymentVerified => 5,
    }
}

/// Matches a command string that requests a CI run, PR, or deployment — independent of
/// whether it's paired with a real, observed exit code. Shared with `verify.rs`, which needs
/// the same category test to look for a *real* execution corroborating a bare tool-call intent.
pub(crate) fn is_ci_or_deploy_command(cmd: &str) -> bool {
    cmd.starts_with("gh pr create")
        || cmd.starts_with("gh pr merge")
        || cmd.starts_with("gh workflow run")
        || cmd.starts_with("gh run watch")
        || cmd.starts_with("vercel deploy")
        || cmd.starts_with("vercel --prod")
        || cmd.starts_with("fly deploy")
        || cmd.starts_with("kubectl apply")
        || cmd.starts_with("docker push")
        || cmd.starts_with("cargo publish")
        || cmd.starts_with("npm publish")
        || cmd.starts_with("terraform apply")
}

/// Matches a command string that requests a git commit/merge/cherry-pick. See
/// `is_ci_or_deploy_command` for why this is shared rather than private.
pub(crate) fn is_commit_command(cmd: &str) -> bool {
    cmd.starts_with("git commit") || cmd.starts_with("git merge") || cmd.starts_with("git cherry-pick")
}

pub(crate) fn is_test_or_build_command(cmd: &str) -> bool {
    let patterns = [
        "cargo test",
        "cargo check",
        "cargo build",
        "npm test",
        "npm run test",
        "npm run build",
        "pnpm test",
        "pnpm run build",
        "yarn test",
        "yarn build",
        "pytest",
        "python -m unittest",
        "python -m pytest",
        "go test",
        "go build",
        "mvn test",
        "mvn package",
        "gradle test",
        "gradle build",
        "vitest",
        "jest",
        "make test",
        "make build",
        "tsc",
        "next build",
        "vite build",
    ];

    patterns.iter().any(|p| cmd.contains(p))
}

fn has_test_failure_markers(output: &str) -> bool {
    let lower = output.to_lowercase();
    lower.contains("test result: failed")
        || lower.contains("tests failed")
        || lower.contains("build failed")
        || lower.contains("compilation error")
        || (lower.contains("failed:") && !lower.contains("failed: 0"))
        || (lower.contains("failures:") && !lower.contains("failures: 0"))
        || lower.contains("fail:")
        || (output.contains("FAIL ") && (output.contains(".test.") || output.contains(".spec.")))
}
