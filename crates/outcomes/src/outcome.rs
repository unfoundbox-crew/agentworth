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

    /// Detect all outcome evidence items from a trace.
    pub fn detect_outcomes(&self, trace: &AgentWorthTrace) -> Vec<OutcomeEvidence> {
        self.detect_from_events(&trace.events)
    }

    /// Detect all outcome evidence items from a sequence of events.
    pub fn detect_from_events(&self, events: &[NormalizedEvent]) -> Vec<OutcomeEvidence> {
        let mut outcomes = Vec::new();

        for event in events {
            match &event.payload {
                // Direct outcome evidence already attached to an event
                EventPayload::OutcomeEvidence(evidence) => {
                    outcomes.push(evidence.clone());
                }

                // File actions -> ArtifactChanged
                EventPayload::FileAction { path, action, .. } => {
                    outcomes.push(OutcomeEvidence {
                        kind: OutcomeKind::ArtifactChanged,
                        summary: format!("File {:?} operation on '{}'", action, path),
                        confidence: 0.60,
                    });
                }

                // Tool calls that write/edit files or run git/tests/ci
                EventPayload::ToolCall(tool_call) => {
                    if let Some(evidence) = self.detect_from_tool_call(tool_call) {
                        outcomes.push(evidence);
                    }
                }

                // Shell commands
                EventPayload::ShellCommand(cmd) => {
                    if let Some(evidence) = self.detect_from_shell_command(cmd) {
                        outcomes.push(evidence);
                    }
                }

                // Tool results
                EventPayload::ToolResult(res) => {
                    if let Some(evidence) = self.detect_from_tool_result(res) {
                        outcomes.push(evidence);
                    }
                }

                // Assistant messages claiming completion
                EventPayload::AssistantMessage { content, .. } => {
                    if let Some(evidence) = self.detect_from_assistant_message(content) {
                        outcomes.push(evidence);
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
        if trimmed.starts_with("gh pr create")
            || trimmed.starts_with("gh pr merge")
            || trimmed.starts_with("gh workflow run")
            || trimmed.starts_with("gh run watch")
            || trimmed.starts_with("vercel deploy")
            || trimmed.starts_with("vercel --prod")
            || trimmed.starts_with("fly deploy")
            || trimmed.starts_with("kubectl apply")
            || trimmed.starts_with("docker push")
            || trimmed.starts_with("cargo publish")
            || trimmed.starts_with("npm publish")
            || trimmed.starts_with("terraform apply")
        {
            let conf = if exit_code == Some(0) { 0.98 } else { 0.90 };
            return Some(OutcomeEvidence {
                kind: OutcomeKind::CiOrDeploymentVerified,
                summary: format!("Deployment or CI command executed: '{}'", cmd_str),
                confidence: conf,
            });
        }

        // 2. Commit observed
        if trimmed.starts_with("git commit")
            || trimmed.starts_with("git merge")
            || trimmed.starts_with("git cherry-pick")
        {
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

fn is_test_or_build_command(cmd: &str) -> bool {
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
