//! The gate: a decision turned into what the harness reads on the hook's stdout.
//!
//! Claude Code reads one JSON object from a hook's stdout and also acts on its exit code. The
//! shapes here are the documented ones (code.claude.com/docs/en/hooks): `continue: false` at
//! `PostToolBatch` stops the agentic loop before the next model call, `additionalContext` is
//! text the model reads, exit 2 at `UserPromptSubmit` blocks the prompt with stderr as the
//! reason, and `PreToolUse` denies one call through `hookSpecificOutput`.
//!
//! Everything else fails open. [`GateOutput::allow`] is exit 0 with no stdout, and it is what a
//! gate returns when it cannot reach `archie serve`, cannot parse the payload, or has no policy
//! to apply -- a governor that bricks every agent on the machine when its own server dies is a
//! worse failure than one missed halt.

use crate::hook::HookEvent;
use serde::{Deserialize, Serialize};

/// What the hook process writes and exits with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateOutput {
    pub exit_code: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
}

impl GateOutput {
    /// Nothing to say: exit 0, no stdout. The fail-open answer.
    pub fn allow() -> Self {
        Self {
            exit_code: 0,
            stdout: None,
            stderr: None,
        }
    }

    /// `PostToolBatch`: stop the agentic loop before the next model call, with the receipt for
    /// the model and the sentence for the person.
    pub fn post_tool_batch_halt(reason: &str, ground_truth: &str) -> Self {
        Self {
            exit_code: 0,
            stdout: Some(serde_json::json!({
                "continue": false,
                "additionalContext": ground_truth,
                "systemMessage": reason,
            })),
            stderr: None,
        }
    }

    /// `PostToolBatch` in shadow mode: the model reads it and keeps working.
    pub fn post_tool_batch_note(text: &str) -> Self {
        Self {
            exit_code: 0,
            stdout: Some(serde_json::json!({"additionalContext": text})),
            stderr: None,
        }
    }

    /// `UserPromptSubmit`: exit 2 blocks the prompt before any model call, and stderr is the
    /// reason the person sees. This is the closest thing any harness has to a suspension.
    pub fn user_prompt_block(reason: &str) -> Self {
        Self {
            exit_code: 2,
            stdout: None,
            stderr: Some(reason.to_string()),
        }
    }

    /// `UserPromptSubmit`: the halt's ground truth again, so the model resumes knowing why it
    /// was stopped.
    pub fn user_prompt_context(text: &str) -> Self {
        Self {
            exit_code: 0,
            stdout: Some(serde_json::json!({"additionalContext": text})),
            stderr: None,
        }
    }

    /// `PreToolUse`: deny this one call. A denial is not a pause -- the model gets a new error
    /// to loop on -- so the reason has to say what would satisfy the gate.
    pub fn pre_tool_use_deny(reason: &str) -> Self {
        Self {
            exit_code: 0,
            stdout: Some(serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": reason,
                }
            })),
            stderr: None,
        }
    }
}

/// One gate call: the event as the harness sent it, plus the batch's own tool calls when it has
/// them. The tools stay raw JSON on purpose: the payload shape is the harness's to change, and
/// an unrecognised shape must degrade to an empty list rather than a failed gate.
#[derive(Debug, Clone, PartialEq)]
pub struct GateRequest {
    pub event: HookEvent,
    pub batch_tools: Vec<serde_json::Value>,
}

impl GateRequest {
    pub fn new(event: HookEvent) -> Self {
        let batch_tools = event
            .raw
            .get("tools")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();
        Self { event, batch_tools }
    }

    /// `(tool_name, tool_input)` for each call in the batch that carries them.
    pub fn tool_calls(&self) -> Vec<(&str, Option<&serde_json::Value>)> {
        self.batch_tools
            .iter()
            .filter_map(|tool| {
                let name = tool.get("tool_name").and_then(|n| n.as_str())?;
                Some((name, tool.get("tool_input")))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn every_builder_serialises_to_the_documented_json() {
        let halt = GateOutput::post_tool_batch_halt("thrash", "lib.rs edited 3 times");
        assert_eq!(halt.exit_code, 0);
        assert_eq!(
            halt.stdout.expect("stdout"),
            serde_json::json!({
                "continue": false,
                "additionalContext": "lib.rs edited 3 times",
                "systemMessage": "thrash"
            })
        );

        assert_eq!(
            GateOutput::post_tool_batch_note("62% on reads").stdout.expect("stdout"),
            serde_json::json!({"additionalContext": "62% on reads"})
        );

        let blocked = GateOutput::user_prompt_block("over the cap");
        assert_eq!(blocked.exit_code, 2);
        assert!(blocked.stdout.is_none());
        assert_eq!(blocked.stderr.as_deref(), Some("over the cap"));

        assert_eq!(
            GateOutput::user_prompt_context("why you stopped").stdout.expect("stdout"),
            serde_json::json!({"additionalContext": "why you stopped"})
        );

        assert_eq!(
            GateOutput::pre_tool_use_deny("40 reads in a row").stdout.expect("stdout"),
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": "40 reads in a row"
                }
            })
        );

        let allow = GateOutput::allow();
        assert_eq!(allow.exit_code, 0);
        assert!(allow.stdout.is_none() && allow.stderr.is_none());
    }

    #[test]
    fn a_batch_payload_yields_its_tool_calls_and_a_strange_one_yields_none() {
        let payload = serde_json::json!({
            "session_id": "s1",
            "hook_event_name": "PostToolBatch",
            "tools": [
                {"tool_name": "Edit", "tool_input": {"file_path": "/a/lib.rs"}},
                {"tool_name": "Bash", "tool_input": {"command": "cargo test"}},
                {"something_else": true}
            ]
        });
        let event = HookEvent::from_stdin_json(payload, &HashMap::new()).expect("parses");
        let request = GateRequest::new(event);
        assert_eq!(request.batch_tools.len(), 3);
        let calls = request.tool_calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "Edit");
        assert_eq!(calls[1].1.expect("input")["command"], "cargo test");

        let odd = serde_json::json!({
            "session_id": "s1",
            "hook_event_name": "PostToolBatch",
            "tools": {"not": "an array"}
        });
        let event = HookEvent::from_stdin_json(odd, &HashMap::new()).expect("parses");
        assert!(GateRequest::new(event).batch_tools.is_empty());
    }
}
