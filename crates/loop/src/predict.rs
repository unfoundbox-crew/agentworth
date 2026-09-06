//! Efference copy: what the agent is about to write, read off the motor command.
//!
//! `PreToolUse` carries the tool and its input before anything happens. For the editing tools the
//! write set is exactly the path in the input, and AgentWorth says so. For `Bash` it is the
//! command string and nothing more -- `known` is false, and a false `known` is the difference
//! between "this session did not touch that file" and "this session cannot say".

use crate::hook::{HookEvent, HookEventName};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The write set predicted from one tool call.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Prediction {
    pub paths: Vec<PathBuf>,
    /// The shell command, for `Bash` only. The paths it will write are not knowable from here.
    pub command: Option<String>,
    /// False only when the tool can write and the prediction cannot say where.
    pub known: bool,
}

/// What this session said it was about to do, ready to be compared with what happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intent {
    pub session_id: String,
    pub tool_use_id: Option<String>,
    pub seq: u64,
    pub tool: String,
    pub prediction: Prediction,
    pub at: DateTime<Utc>,
}

impl Intent {
    /// Builds an intent from a `PreToolUse`. Any other event returns `None`.
    pub fn from_pre_tool_use(event: &HookEvent, seq: u64) -> Option<Self> {
        if event.hook_event_name != HookEventName::PreToolUse {
            return None;
        }
        let tool = event.tool_name.clone()?;
        let mut prediction = predicted_writes(&tool, event.tool_input.as_ref());
        // A relative path means nothing without the directory the tool ran in, and two
        // sessions in different checkouts would both store `src/lib.rs`.
        if let Some(cwd) = event.cwd.as_deref() {
            let cwd = std::path::Path::new(cwd);
            prediction.paths = prediction
                .paths
                .iter()
                .map(|path| crate::observe::absolutise(cwd, path))
                .collect();
        }
        Some(Intent {
            session_id: event.session_id.clone(),
            tool_use_id: event.tool_use_id.clone(),
            seq,
            tool: tool.clone(),
            prediction,
            at: event.received_at,
        })
    }
}

/// The write set a tool call implies.
pub fn predicted_writes(tool_name: &str, tool_input: Option<&serde_json::Value>) -> Prediction {
    let field = match tool_name {
        "Edit" | "Write" | "MultiEdit" => Some("file_path"),
        "NotebookEdit" => Some("notebook_path"),
        "Bash" => {
            let command = tool_input
                .and_then(|input| input.get("command"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            return Prediction {
                paths: Vec::new(),
                command,
                known: false,
            };
        }
        // Everything else is a read, a search, or a tool that does not touch the filesystem.
        // An empty write set here is a claim, not an absence.
        _ => None,
    };

    let Some(field) = field else {
        return Prediction {
            paths: Vec::new(),
            command: None,
            known: true,
        };
    };
    let paths = tool_input
        .and_then(|input| input.get(field))
        .and_then(serde_json::Value::as_str)
        .filter(|path| !path.is_empty())
        .map(|path| vec![PathBuf::from(path)])
        .unwrap_or_default();
    Prediction {
        paths,
        command: None,
        known: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn the_editing_tools_predict_exactly_their_path() {
        for (tool, field) in [
            ("Edit", "file_path"),
            ("Write", "file_path"),
            ("MultiEdit", "file_path"),
            ("NotebookEdit", "notebook_path"),
        ] {
            let value = serde_json::json!({ field: "/repo/src/lib.rs" });
            let prediction = predicted_writes(tool, Some(&value));
            assert_eq!(
                prediction.paths,
                vec![PathBuf::from("/repo/src/lib.rs")],
                "{tool}"
            );
            assert!(prediction.known, "{tool}");
            assert_eq!(prediction.command, None, "{tool}");
        }
    }

    #[test]
    fn bash_predicts_a_command_and_admits_it_does_not_know_the_paths() {
        let value = serde_json::json!({"command": "cargo fmt --all", "description": "format"});
        let prediction = predicted_writes("Bash", Some(&value));
        assert!(prediction.paths.is_empty());
        assert_eq!(prediction.command.as_deref(), Some("cargo fmt --all"));
        assert!(!prediction.known);
    }

    #[test]
    fn a_read_predicts_no_writes_and_says_so_with_confidence() {
        let value = serde_json::json!({"file_path": "/repo/README.md"});
        let prediction = predicted_writes("Read", Some(&value));
        assert!(prediction.paths.is_empty());
        assert!(prediction.known);
    }

    #[test]
    fn a_missing_or_empty_path_predicts_nothing_rather_than_an_empty_path() {
        assert!(predicted_writes("Write", None).paths.is_empty());
        let value = serde_json::json!({"file_path": ""});
        assert!(predicted_writes("Write", Some(&value)).paths.is_empty());
    }

    #[test]
    fn an_intent_is_built_from_a_pre_tool_use_and_from_nothing_else() {
        let pre = HookEvent::from_stdin_json(
            serde_json::json!({
                "session_id": "s1",
                "hook_event_name": "PreToolUse",
                "tool_name": "Write",
                "tool_use_id": "toolu_01",
                "tool_input": {"file_path": "/repo/a.txt", "content": "hi"}
            }),
            &HashMap::new(),
        )
        .expect("parses");
        let intent = Intent::from_pre_tool_use(&pre, 7).expect("an intent");
        assert_eq!(intent.tool, "Write");
        assert_eq!(intent.seq, 7);
        assert_eq!(intent.tool_use_id.as_deref(), Some("toolu_01"));
        assert_eq!(intent.prediction.paths, vec![PathBuf::from("/repo/a.txt")]);

        let post = HookEvent::from_stdin_json(
            serde_json::json!({
                "session_id": "s1", "hook_event_name": "PostToolUse", "tool_name": "Write"
            }),
            &HashMap::new(),
        )
        .expect("parses");
        assert!(Intent::from_pre_tool_use(&post, 8).is_none());
    }
}
