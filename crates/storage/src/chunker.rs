//! Semantic trajectory chunking engine for local latent vector embedding.
//!
//! Extracts discrete semantic chunks from raw session traces:
//! 1. `SessionSummary`: Initial user prompt, outcome summary, token & model telemetry.
//! 2. `ErrorRecovery`: Tool failures & compilation errors paired with subsequent corrective responses.
//! 3. `ToolInvocation`: Destructive or critical operations (`rm -rf`, `git reset`, `DROP TABLE`, etc.).
//! 4. `ApologyPanic`: Assistant retreat, panic, apology, and confusion turns.
//! 5. `CodeLineage`: Significant code modifications and diff lineage.

use agentworth_schema::vector::{ChunkKind, TrajectoryChunk};
use agentworth_schema::{AgentWorthTrace, EventPayload, FileActionType, NormalizedEvent};
use serde_json::json;

/// Semantic trajectory chunk extractor.
#[derive(Debug, Clone, Default)]
pub struct TrajectoryChunker {
    /// Maximum character length per extracted text chunk before truncation.
    pub max_chunk_chars: usize,
}

impl TrajectoryChunker {
    /// Create a new chunker with default settings.
    pub fn new() -> Self {
        Self {
            max_chunk_chars: 4096,
        }
    }

    /// Set maximum chunk text characters.
    pub fn with_max_chars(mut self, max_chars: usize) -> Self {
        self.max_chunk_chars = max_chars;
        self
    }

    /// Extract all semantic chunks from an `AgentWorthTrace`.
    pub fn extract_chunks(trace: &AgentWorthTrace) -> Vec<TrajectoryChunk> {
        Self::new().extract(trace)
    }

    /// Extract chunks using instance configuration.
    pub fn extract(&self, trace: &AgentWorthTrace) -> Vec<TrajectoryChunk> {
        let mut chunks = Vec::new();

        // 1. Session Summary Chunk
        if let Some(summary_chunk) = self.extract_session_summary(trace) {
            chunks.push(summary_chunk);
        }

        // 2. Scan events for ErrorRecovery, ToolInvocation, ApologyPanic, and CodeLineage
        let events = &trace.events;
        for (idx, event) in events.iter().enumerate() {
            // Check for ApologyPanic in AssistantMessage
            if let Some(panic_chunk) = self.extract_apology_panic(trace, event, idx) {
                chunks.push(panic_chunk);
            }

            // Check for Destructive / Critical ToolInvocation
            if let Some(tool_chunk) = self.extract_destructive_tool_invocation(trace, event, idx) {
                chunks.push(tool_chunk);
            }

            // Check for ErrorRecovery (error + corrective response)
            if let Some(recovery_chunk) = self.extract_error_recovery(trace, events, idx) {
                chunks.push(recovery_chunk);
            }

            // Check for CodeLineage
            if let Some(lineage_chunk) = self.extract_code_lineage(trace, event, idx) {
                chunks.push(lineage_chunk);
            }
        }

        chunks
    }

    /// Extract high-level session summary chunk.
    fn extract_session_summary(&self, trace: &AgentWorthTrace) -> Option<TrajectoryChunk> {
        let first_user_msg = trace.events.iter().find_map(|e| {
            if let EventPayload::UserMessage { content } = &e.payload {
                Some(content.trim())
            } else {
                None
            }
        });

        let user_prompt = first_user_msg.unwrap_or("No explicit user prompt recorded.");

        // Find outcome summary if any
        let outcome_summary = trace
            .events
            .iter()
            .rev()
            .find_map(|e| match &e.payload {
                EventPayload::OutcomeEvidence(ev) => {
                    Some(format!("{:?}: {}", ev.kind, ev.summary))
                }
                EventPayload::AssistantMessage { content, .. } if !content.trim().is_empty() => {
                    Some(truncate_text(content.trim(), 500))
                }
                _ => None,
            })
            .unwrap_or_else(|| "Session completed.".to_string());

        let primary_model = trace
            .stats
            .models_used
            .first()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());

        let total_tokens = trace.stats.token_usage.total();
        let tools_list: Vec<String> = trace
            .stats
            .tools_used
            .iter()
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect();

        let text_content = format!(
            "Session: {} ({})\nStarted: {}\nPrimary Model: {}\nTotal Tokens: {} (Input: {}, Output: {})\nTools: [{}]\n\nUser Objective:\n{}\n\nOutcome:\n{}",
            trace.session_id,
            trace.adapter,
            trace.started_at.to_rfc3339(),
            primary_model,
            total_tokens,
            trace.stats.token_usage.input_tokens,
            trace.stats.token_usage.output_tokens,
            tools_list.join(", "),
            user_prompt,
            outcome_summary
        );

        let metadata = json!({
            "session_id": trace.session_id,
            "adapter": trace.adapter,
            "primary_model": primary_model,
            "total_tokens": total_tokens,
            "events_count": trace.stats.total_events,
            "tools_count": trace.stats.tool_calls_count,
            "duration_seconds": trace.stats.duration_seconds,
        });

        Some(TrajectoryChunk::new(
            &trace.session_id,
            &trace.adapter,
            ChunkKind::SessionSummary,
            0,
            trace.started_at.to_rfc3339(),
            truncate_text(&text_content, self.max_chunk_chars),
            metadata.to_string(),
        ))
    }

    /// Extract Apology/Panic turns from assistant messages.
    fn extract_apology_panic(
        &self,
        trace: &AgentWorthTrace,
        event: &NormalizedEvent,
        idx: usize,
    ) -> Option<TrajectoryChunk> {
        if let EventPayload::AssistantMessage { content, thinking } = &event.payload {
            let mut matched_phrases = Vec::new();
            let lower_content = content.to_lowercase();
            let lower_thinking = thinking
                .as_deref()
                .map(|t| t.to_lowercase())
                .unwrap_or_default();

            for phrase in APOLOGY_PANIC_PATTERNS {
                if lower_content.contains(phrase) || lower_thinking.contains(phrase) {
                    matched_phrases.push(*phrase);
                }
            }

            if !matched_phrases.is_empty() {
                let mut text = format!(
                    "[Assistant Panic / Apology at Turn {}]\nSignatures: {}\n\nContent:\n{}",
                    event.sequence,
                    matched_phrases.join(", "),
                    content
                );

                if let Some(t) = thinking {
                    text.push_str("\n\nThinking:\n");
                    text.push_str(t);
                }

                let metadata = json!({
                    "sequence": event.sequence,
                    "event_index": idx,
                    "matched_signatures": matched_phrases,
                    "has_thinking": thinking.is_some(),
                });

                return Some(TrajectoryChunk::new(
                    &trace.session_id,
                    &trace.adapter,
                    ChunkKind::ApologyPanic,
                    event.sequence as usize,
                    event.timestamp.to_rfc3339(),
                    truncate_text(&text, self.max_chunk_chars),
                    metadata.to_string(),
                ));
            }
        }
        None
    }

    /// Extract destructive or critical tool invocations.
    fn extract_destructive_tool_invocation(
        &self,
        trace: &AgentWorthTrace,
        event: &NormalizedEvent,
        idx: usize,
    ) -> Option<TrajectoryChunk> {
        match &event.payload {
            EventPayload::ShellCommand(cmd) => {
                if let Some(sig) = find_destructive_command_signature(&cmd.command) {
                    let text = format!(
                        "[Destructive Shell Command at Turn {}]\nSignature: {}\nCommand:\n{}\n\nExit Code: {:?}\nCWD: {:?}",
                        event.sequence,
                        sig,
                        cmd.command,
                        cmd.exit_code,
                        cmd.cwd
                    );
                    let metadata = json!({
                        "sequence": event.sequence,
                        "event_index": idx,
                        "tool": "shell",
                        "danger_signature": sig,
                        "command": truncate_text(&cmd.command, 256),
                        "exit_code": cmd.exit_code,
                    });
                    return Some(TrajectoryChunk::new(
                        &trace.session_id,
                        &trace.adapter,
                        ChunkKind::ToolInvocation,
                        event.sequence as usize,
                        event.timestamp.to_rfc3339(),
                        truncate_text(&text, self.max_chunk_chars),
                        metadata.to_string(),
                    ));
                }
            }
            EventPayload::ToolCall(tool) => {
                let tool_name_lower = tool.name.to_lowercase();
                let args_str = tool.arguments.to_string();
                let args_lower = args_str.to_lowercase();

                let mut matched_sig = None;
                if tool_name_lower.contains("delete")
                    || tool_name_lower.contains("remove")
                    || tool_name_lower.contains("destroy")
                    || tool_name_lower.contains("drop")
                {
                    matched_sig = Some(tool.name.as_str());
                } else if let Some(sig) = find_destructive_command_signature(&args_lower) {
                    matched_sig = Some(sig);
                }

                if let Some(sig) = matched_sig {
                    let text = format!(
                        "[Destructive Tool Invocation at Turn {}]\nTool: {}\nSignature: {}\nArguments:\n{}",
                        event.sequence,
                        tool.name,
                        sig,
                        tool.arguments
                    );
                    let metadata = json!({
                        "sequence": event.sequence,
                        "event_index": idx,
                        "tool": tool.name,
                        "danger_signature": sig,
                        "arguments": tool.arguments,
                    });
                    return Some(TrajectoryChunk::new(
                        &trace.session_id,
                        &trace.adapter,
                        ChunkKind::ToolInvocation,
                        event.sequence as usize,
                        event.timestamp.to_rfc3339(),
                        truncate_text(&text, self.max_chunk_chars),
                        metadata.to_string(),
                    ));
                }
            }
            EventPayload::FileAction {
                path,
                action,
                diff,
                lines_changed,
            } => {
                if *action == FileActionType::Delete {
                    let text = format!(
                        "[Destructive File Deletion at Turn {}]\nPath: {}\nAction: delete\nLines: {:?}",
                        event.sequence,
                        path,
                        lines_changed
                    );
                    let metadata = json!({
                        "sequence": event.sequence,
                        "event_index": idx,
                        "tool": "file_action",
                        "danger_signature": "file_delete",
                        "path": path,
                        "diff": diff,
                    });
                    return Some(TrajectoryChunk::new(
                        &trace.session_id,
                        &trace.adapter,
                        ChunkKind::ToolInvocation,
                        event.sequence as usize,
                        event.timestamp.to_rfc3339(),
                        truncate_text(&text, self.max_chunk_chars),
                        metadata.to_string(),
                    ));
                }
            }
            _ => {}
        }
        None
    }

    /// Extract tool errors along with their corrective responses.
    fn extract_error_recovery(
        &self,
        trace: &AgentWorthTrace,
        events: &[NormalizedEvent],
        idx: usize,
    ) -> Option<TrajectoryChunk> {
        let event = &events[idx];
        let (source, error_text) = extract_error_info(event)?;

        // Look forward for assistant's immediate next corrective response or action
        let mut corrective_text = String::new();
        let mut recovery_seq = event.sequence;
        let mut is_recovered = false;

        for follow_up in &events[idx + 1..] {
            match &follow_up.payload {
                EventPayload::AssistantMessage { content, thinking } => {
                    if corrective_text.is_empty() {
                        recovery_seq = follow_up.sequence;
                        corrective_text.push_str("Assistant: ");
                        corrective_text.push_str(content);
                        if let Some(t) = thinking {
                            corrective_text.push_str("\nThinking: ");
                            corrective_text.push_str(t);
                        }
                    }
                }
                EventPayload::FileAction {
                    path, action, diff, ..
                } => {
                    if !corrective_text.contains("Corrective File Action") {
                        corrective_text.push_str(&format!(
                            "\nCorrective File Action: {:?} on {}",
                            action, path
                        ));
                        if let Some(d) = diff {
                            corrective_text.push_str(&format!("\nDiff: {}", truncate_text(d, 300)));
                        }
                    }
                }
                EventPayload::OutcomeEvidence(ev) => {
                    if matches!(
                        ev.kind,
                        agentworth_schema::OutcomeKind::TestOrBuildPassed
                            | agentworth_schema::OutcomeKind::CommitObserved
                            | agentworth_schema::OutcomeKind::CiOrDeploymentVerified
                    ) {
                        is_recovered = true;
                        corrective_text.push_str(&format!("\nResolved: {:?}", ev.kind));
                        break;
                    }
                }
                EventPayload::ShellCommand(cmd) => {
                    if cmd.exit_code == Some(0) && is_test_command(&cmd.command) {
                        is_recovered = true;
                        corrective_text.push_str(&format!(
                            "\nResolved via successful command: {}",
                            cmd.command
                        ));
                        break;
                    }
                }
                EventPayload::Error { .. } => {
                    // Another subsequent error encountered, stop lookahead
                    break;
                }
                _ => {}
            }
        }

        if corrective_text.is_empty() {
            corrective_text = "No immediate follow-up response recorded.".to_string();
        }

        let text = format!(
            "[Tool / Command Failure at Turn {}]\nSource: {}\nError:\n{}\n\n[Corrective Response / Follow-up Turn {}]\n{}",
            event.sequence,
            source,
            error_text,
            recovery_seq,
            corrective_text
        );

        let metadata = json!({
            "error_sequence": event.sequence,
            "error_source": source,
            "recovery_sequence": recovery_seq,
            "is_recovered": is_recovered,
        });

        Some(TrajectoryChunk::new(
            &trace.session_id,
            &trace.adapter,
            ChunkKind::ErrorRecovery,
            event.sequence as usize,
            event.timestamp.to_rfc3339(),
            truncate_text(&text, self.max_chunk_chars),
            metadata.to_string(),
        ))
    }

    /// Extract code lineage / file modification chunks.
    fn extract_code_lineage(
        &self,
        trace: &AgentWorthTrace,
        event: &NormalizedEvent,
        idx: usize,
    ) -> Option<TrajectoryChunk> {
        if let EventPayload::FileAction {
            path,
            action,
            diff,
            lines_changed,
        } = &event.payload
        {
            if *action != FileActionType::Delete {
                let diff_snippet = diff.as_deref().unwrap_or("No diff recorded.");
                let text = format!(
                    "[Code Lineage at Turn {}]\nFile: {}\nAction: {:?}\nLines Changed: {:?}\nDiff:\n{}",
                    event.sequence,
                    path,
                    action,
                    lines_changed,
                    diff_snippet
                );
                let metadata = json!({
                    "sequence": event.sequence,
                    "event_index": idx,
                    "path": path,
                    "action": format!("{:?}", action).to_lowercase(),
                    "lines_changed": lines_changed,
                });
                return Some(TrajectoryChunk::new(
                    &trace.session_id,
                    &trace.adapter,
                    ChunkKind::CodeLineage,
                    event.sequence as usize,
                    event.timestamp.to_rfc3339(),
                    truncate_text(&text, self.max_chunk_chars),
                    metadata.to_string(),
                ));
            }
        }
        None
    }
}

const APOLOGY_PANIC_PATTERNS: &[&str] = &[
    "my mistake",
    "i apologize",
    "my apologies",
    "sorry about that",
    "i am sorry",
    "i'm sorry",
    "sorry for the confusion",
    "lost the repo",
    "lost track of",
    "lost my place",
    "accidentally deleted",
    "deleted by mistake",
    "deleted by accident",
    "safety mechanism",
    "turned my safety mechanism into a weapon",
    "i broke",
    "i messed up",
    "i was wrong",
    "i hallucinated",
    "hallucinated",
    "hallucination",
    "hard rule i should never invoke",
    "rule i violated",
    "unintended consequence",
    "unexpected damage",
    "reverting my changes",
    "reverting the damage",
    "rolling back",
    "stop. the trace shows",
    "emergency stop",
    "panic:",
];

fn find_destructive_command_signature(cmd: &str) -> Option<&'static str> {
    let lower = cmd.to_lowercase();

    if lower.contains("rm -rf") || lower.contains("rm -fr") {
        return Some("rm -rf");
    }
    if lower.contains("rm -r") || lower.contains("rm -r") {
        return Some("rm -r");
    }
    if lower.contains("rmdir") {
        return Some("rmdir");
    }
    if lower.contains("git reset --hard") {
        return Some("git reset --hard");
    }
    if lower.contains("git reset") {
        return Some("git reset");
    }
    if lower.contains("git clean -f")
        || lower.contains("git clean -df")
        || lower.contains("git clean -xdf")
    {
        return Some("git clean -f");
    }
    if lower.contains("git restore .") || lower.contains("git checkout -- .") {
        return Some("git restore/checkout");
    }
    if lower.contains("git branch -d") || lower.contains("git branch -d") {
        return Some("git branch -D");
    }
    if lower.contains("git push --force") || lower.contains("git push -f") {
        return Some("git push --force");
    }
    if lower.contains("drop table") {
        return Some("DROP TABLE");
    }
    if lower.contains("drop database") {
        return Some("DROP DATABASE");
    }
    if lower.contains("drop schema") {
        return Some("DROP SCHEMA");
    }
    if lower.contains("truncate table") || lower.contains("truncate ") {
        return Some("TRUNCATE");
    }
    if lower.contains("delete from ") {
        return Some("DELETE FROM");
    }
    if lower.contains("dd if=") {
        return Some("dd");
    }
    if lower.contains("mkfs") {
        return Some("mkfs");
    }
    if lower.contains("chmod -r 777") || lower.contains("chmod 777") {
        return Some("chmod 777");
    }
    if lower.contains("kill -9") || lower.contains("pkill -9") {
        return Some("kill -9");
    }
    if lower.contains("aws s3 rm") {
        return Some("aws s3 rm");
    }
    if lower.contains("gsutil rm") || lower.contains("gcloud storage rm") {
        return Some("gcloud/gsutil rm");
    }
    if lower.contains("terraform destroy") || lower.contains("pulumi destroy") {
        return Some("terraform/pulumi destroy");
    }
    if lower.contains("curl ")
        && (lower.contains("| sh") || lower.contains("| bash") || lower.contains("| sudo"))
    {
        return Some("curl | sh");
    }
    None
}

fn extract_error_info(event: &NormalizedEvent) -> Option<(&'static str, String)> {
    match &event.payload {
        EventPayload::Error { message, .. } => Some(("runtime_error", message.clone())),
        EventPayload::ToolResult(res) => {
            if res.is_error {
                let out = extract_output_text(&res.output);
                Some((
                    "tool_result_error",
                    if out.is_empty() {
                        "Tool returned error state".to_string()
                    } else {
                        out
                    },
                ))
            } else {
                let out = extract_output_text(&res.output);
                if has_failure_text(&out) {
                    Some(("tool_output_failure", out))
                } else {
                    None
                }
            }
        }
        EventPayload::ShellCommand(cmd) => {
            if let Some(code) = cmd.exit_code {
                if code != 0 {
                    let out = cmd.output.as_deref().unwrap_or("");
                    return Some((
                        "shell_exit_nonzero",
                        format!(
                            "Command '{}' exited with code {}. Output:\n{}",
                            cmd.command, code, out
                        ),
                    ));
                }
            }
            if let Some(out) = &cmd.output {
                if has_failure_text(out) {
                    return Some((
                        "shell_output_failure",
                        format!("Command '{}' failed with output:\n{}", cmd.command, out),
                    ));
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_output_text(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(o) => {
            if let Some(s) = o.get("output").and_then(|v| v.as_str()) {
                s.to_string()
            } else if let Some(s) = o.get("stdout").and_then(|v| v.as_str()) {
                s.to_string()
            } else if let Some(s) = o.get("stderr").and_then(|v| v.as_str()) {
                s.to_string()
            } else if let Some(s) = o.get("error").and_then(|v| v.as_str()) {
                s.to_string()
            } else {
                val.to_string()
            }
        }
        _ => val.to_string(),
    }
}

fn has_failure_text(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("test result: failed")
        || lower.contains("compilation error")
        || (lower.contains("failures:") && !lower.contains("failures: 0"))
        || (lower.contains("failed:") && !lower.contains("failed: 0"))
        || lower.contains("error[e")
        || lower.contains("syntaxerror:")
        || lower.contains("permission denied")
        || lower.contains("fatal error")
        || lower.contains("panic:")
        || (text.contains("FAIL ") && (text.contains(".test.") || text.contains(".spec.")))
}

fn is_test_command(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    lower.contains("cargo test")
        || lower.contains("cargo check")
        || lower.contains("npm test")
        || lower.contains("pnpm test")
        || lower.contains("pytest")
        || lower.contains("go test")
}

fn truncate_text(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}... [truncated]", &s[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{OutcomeEvidence, OutcomeKind, Provenance, ShellCommand, ToolResult};
    use chrono::Utc;

    #[test]
    fn test_extract_all_chunk_types() {
        let start = Utc::now();
        let prov = Provenance::new("/test/path.jsonl", "claude_code", 100, 12345, "fp123");
        let mut trace = AgentWorthTrace::new("sess-100", "claude_code", prov, start);

        // Turn 1: User objective
        trace.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::UserMessage {
                content: "Please clean up the stale worktrees and run tests".to_string(),
            },
        ));

        // Turn 2: Destructive tool invocation
        trace.events.push(NormalizedEvent::new(
            2,
            start,
            EventPayload::ShellCommand(ShellCommand {
                command: "rm -rf /Users/saurabh/code/katana".to_string(),
                cwd: Some("/Users/saurabh/code".to_string()),
                exit_code: Some(0),
                output: None,
            }),
        ));

        // Turn 3: Assistant Panic / Apology
        trace.events.push(NormalizedEvent::new(
            3,
            start,
            EventPayload::AssistantMessage {
                content: "STOP. That was my mistake — I accidentally deleted the katana directory. A missing local turned my safety mechanism into a weapon.".to_string(),
                thinking: Some("Panic: I deleted the wrong repo!".to_string()),
            },
        ));

        // Turn 4: Error encountered
        trace.events.push(NormalizedEvent::new(
            4,
            start,
            EventPayload::ToolResult(ToolResult {
                call_id: Some("call-1".to_string()),
                name: Some("cargo".to_string()),
                output: serde_json::json!({
                    "stderr": "error[E0425]: cannot find value `foo` in this scope\n  --> src/main.rs:14:5"
                }),
                is_error: true,
            }),
        ));

        // Turn 5: Corrective FileAction
        trace.events.push(NormalizedEvent::new(
            5,
            start,
            EventPayload::FileAction {
                path: "src/main.rs".to_string(),
                action: FileActionType::Edit,
                diff: Some("+ let foo = 42;".to_string()),
                lines_changed: Some(1),
            },
        ));

        // Turn 6: Outcome evidence
        trace.events.push(NormalizedEvent::new(
            6,
            start,
            EventPayload::OutcomeEvidence(OutcomeEvidence {
                kind: OutcomeKind::TestOrBuildPassed,
                summary: "cargo test succeeded with 12 tests passed".to_string(),
                confidence: 0.95,
            }),
        ));

        trace.recalculate_stats();

        let chunks = TrajectoryChunker::extract_chunks(&trace);

        // Verify we got SessionSummary, ToolInvocation, ApologyPanic, ErrorRecovery, and CodeLineage
        assert!(chunks.iter().any(|c| c.kind == ChunkKind::SessionSummary));
        assert!(chunks.iter().any(|c| c.kind == ChunkKind::ToolInvocation));
        assert!(chunks.iter().any(|c| c.kind == ChunkKind::ApologyPanic));
        assert!(chunks.iter().any(|c| c.kind == ChunkKind::ErrorRecovery));
        assert!(chunks.iter().any(|c| c.kind == ChunkKind::CodeLineage));

        // Verify session summary contents
        let summary = chunks
            .iter()
            .find(|c| c.kind == ChunkKind::SessionSummary)
            .unwrap();
        assert!(summary.text_content.contains("User Objective:"));
        assert!(summary.text_content.contains("clean up the stale worktrees"));
        assert_eq!(summary.session_id, "sess-100");

        // Verify tool invocation
        let tool_chunk = chunks
            .iter()
            .find(|c| c.kind == ChunkKind::ToolInvocation)
            .unwrap();
        assert!(tool_chunk.text_content.contains("rm -rf"));
        assert_eq!(tool_chunk.turn_index, 2);

        // Verify apology panic
        let panic_chunk = chunks
            .iter()
            .find(|c| c.kind == ChunkKind::ApologyPanic)
            .unwrap();
        assert!(panic_chunk.text_content.contains("my mistake"));
        assert!(panic_chunk.text_content.contains("safety mechanism"));
        assert_eq!(panic_chunk.turn_index, 3);

        // Verify error recovery
        let err_chunk = chunks
            .iter()
            .find(|c| c.kind == ChunkKind::ErrorRecovery)
            .unwrap();
        assert!(err_chunk.text_content.contains("cannot find value `foo`"));
        assert!(err_chunk.text_content.contains("Corrective File Action"));

        // Verify code lineage
        let lineage_chunk = chunks
            .iter()
            .find(|c| c.kind == ChunkKind::CodeLineage)
            .unwrap();
        assert!(lineage_chunk.text_content.contains("src/main.rs"));
        assert!(lineage_chunk.text_content.contains("+ let foo = 42;"));
    }

    #[test]
    fn test_chunking_destructive_tools_variety() {
        let start = Utc::now();
        let prov = Provenance::new("/test/path.jsonl", "antigravity", 100, 12345, "fp123");
        let mut trace = AgentWorthTrace::new("sess-destructive", "antigravity", prov, start);

        trace.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::ShellCommand(ShellCommand {
                command: "git reset --hard HEAD~5".to_string(),
                cwd: None,
                exit_code: Some(0),
                output: None,
            }),
        ));

        trace.events.push(NormalizedEvent::new(
            2,
            start,
            EventPayload::ShellCommand(ShellCommand {
                command: "psql -c 'DROP TABLE users CASCADE'".to_string(),
                cwd: None,
                exit_code: Some(0),
                output: None,
            }),
        ));

        trace.events.push(NormalizedEvent::new(
            3,
            start,
            EventPayload::FileAction {
                path: "secret.key".to_string(),
                action: FileActionType::Delete,
                diff: None,
                lines_changed: None,
            },
        ));

        let chunks = TrajectoryChunker::extract_chunks(&trace);
        let tool_chunks: Vec<_> = chunks.iter().filter(|c| c.kind == ChunkKind::ToolInvocation).collect();

        assert_eq!(tool_chunks.len(), 3);
        assert!(tool_chunks[0].text_content.contains("git reset --hard"));
        assert!(tool_chunks[1].text_content.contains("DROP TABLE"));
        assert!(tool_chunks[2].text_content.contains("secret.key"));
    }

    #[test]
    fn test_chunking_empty_trace_graceful() {
        let start = Utc::now();
        let prov = Provenance::new("/test/empty.jsonl", "codex", 0, 0, "fp0");
        let trace = AgentWorthTrace::new("sess-empty", "codex", prov, start);

        let chunks = TrajectoryChunker::extract_chunks(&trace);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].kind, ChunkKind::SessionSummary);
        assert!(chunks[0].text_content.contains("No explicit user prompt"));
    }
}
