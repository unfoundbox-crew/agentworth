//! The hook payload, as the harness hands it over.
//!
//! Claude Code writes one JSON object to a hook's stdin and reads nothing back when the hook is
//! `async`. This module is the whole of what AgentWorth understands about that object, and it is
//! deliberately generous: `hook_event_name` keeps unknown names instead of failing, every field
//! outside `session_id` is optional, and the untouched `raw` value rides along so a later reader
//! can find a field this crate did not know about when it was written.
//!
//! `received_at` is stamped by whoever receives the event. The harness does not send a time, and
//! a time this process invented is honest only if it says where it came from.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The environment variable herdr's own hooks export, and the only join to a terminal pane.
pub const PANE_ID_ENV: &str = "HERDR_PANE_ID";

/// Which hook fired. `Other` keeps the name verbatim rather than dropping the event: harness
/// formats are unstable, and an event AgentWorth cannot classify is still an event that happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum HookEventName {
    SessionStart,
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    Stop,
    SubagentStart,
    SubagentStop,
    PreCompact,
    PostCompact,
    CwdChanged,
    SessionEnd,
    Other(String),
}

impl HookEventName {
    pub fn as_str(&self) -> &str {
        match self {
            HookEventName::SessionStart => "SessionStart",
            HookEventName::UserPromptSubmit => "UserPromptSubmit",
            HookEventName::PreToolUse => "PreToolUse",
            HookEventName::PostToolUse => "PostToolUse",
            HookEventName::PostToolUseFailure => "PostToolUseFailure",
            HookEventName::Stop => "Stop",
            HookEventName::SubagentStart => "SubagentStart",
            HookEventName::SubagentStop => "SubagentStop",
            HookEventName::PreCompact => "PreCompact",
            HookEventName::PostCompact => "PostCompact",
            HookEventName::CwdChanged => "CwdChanged",
            HookEventName::SessionEnd => "SessionEnd",
            HookEventName::Other(name) => name,
        }
    }
}

impl From<String> for HookEventName {
    fn from(name: String) -> Self {
        match name.as_str() {
            "SessionStart" => HookEventName::SessionStart,
            "UserPromptSubmit" => HookEventName::UserPromptSubmit,
            "PreToolUse" => HookEventName::PreToolUse,
            "PostToolUse" => HookEventName::PostToolUse,
            "PostToolUseFailure" => HookEventName::PostToolUseFailure,
            "Stop" => HookEventName::Stop,
            "SubagentStart" => HookEventName::SubagentStart,
            "SubagentStop" => HookEventName::SubagentStop,
            "PreCompact" => HookEventName::PreCompact,
            "PostCompact" => HookEventName::PostCompact,
            "CwdChanged" => HookEventName::CwdChanged,
            "SessionEnd" => HookEventName::SessionEnd,
            _ => HookEventName::Other(name),
        }
    }
}

impl From<HookEventName> for String {
    fn from(name: HookEventName) -> Self {
        match name {
            HookEventName::Other(name) => name,
            other => other.as_str().to_string(),
        }
    }
}

/// One hook firing. Everything the harness sends, plus the two things it does not: the pane the
/// agent is running in, and when this process saw the line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookEvent {
    pub session_id: String,
    pub hook_event_name: HookEventName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Present only on a subagent's events. Its presence, not its value, is what the state
    /// machine acts on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    /// A string on some tools and an object on others; kept as it arrived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_response: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_assistant_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    #[serde(default = "Utc::now")]
    pub received_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub raw: serde_json::Value,
}

impl HookEvent {
    /// Builds an event from the JSON on a hook's stdin and the process environment it ran in.
    pub fn from_stdin_json(
        value: serde_json::Value,
        env: &HashMap<String, String>,
    ) -> Result<Self, serde_json::Error> {
        let mut event: HookEvent = serde_json::from_value(value.clone())?;
        event.received_at = Utc::now();
        event.pane_id = env.get(PANE_ID_ENV).filter(|v| !v.is_empty()).cloned();
        // `raw` exists so a later reader can find a field this crate did not know about. The
        // tool result is not that: it is already on `tool_response`, it is the largest thing in
        // the payload by far, and keeping both stores every tool output twice.
        let mut raw = value;
        if let Some(object) = raw.as_object_mut() {
            object.remove("tool_response");
        }
        event.raw = raw;
        Ok(event)
    }

    /// True when the event belongs to a subagent rather than the pane's own session.
    pub fn is_subagent(&self) -> bool {
        self.agent_id.is_some()
    }

    /// The tool result as text, whichever shape it arrived in. `None` when there was no result.
    pub fn tool_response_text(&self) -> Option<String> {
        match self.tool_response.as_ref()? {
            serde_json::Value::String(s) => Some(s.clone()),
            other => Some(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_event_name_parses_instead_of_failing() {
        let value = serde_json::json!({
            "session_id": "s1",
            "hook_event_name": "SomethingNewInVersion42"
        });
        let event = HookEvent::from_stdin_json(value, &HashMap::new()).expect("parses");
        assert_eq!(
            event.hook_event_name,
            HookEventName::Other("SomethingNewInVersion42".to_string())
        );
        assert_eq!(event.hook_event_name.as_str(), "SomethingNewInVersion42");
    }

    #[test]
    fn the_pane_id_comes_from_the_environment_not_the_payload() {
        let value = serde_json::json!({"session_id": "s1", "hook_event_name": "SessionStart"});
        let env = HashMap::from([(PANE_ID_ENV.to_string(), "%17".to_string())]);
        let event = HookEvent::from_stdin_json(value, &env).expect("parses");
        assert_eq!(event.pane_id.as_deref(), Some("%17"));
    }

    #[test]
    fn an_empty_pane_id_is_no_pane_id() {
        let value = serde_json::json!({"session_id": "s1", "hook_event_name": "Stop"});
        let env = HashMap::from([(PANE_ID_ENV.to_string(), String::new())]);
        let event = HookEvent::from_stdin_json(value, &env).expect("parses");
        assert_eq!(event.pane_id, None);
    }

    #[test]
    fn the_raw_payload_survives_fields_this_crate_does_not_know() {
        let value = serde_json::json!({
            "session_id": "s1",
            "hook_event_name": "Stop",
            "some_future_field": 7
        });
        let event = HookEvent::from_stdin_json(value, &HashMap::new()).expect("parses");
        assert_eq!(
            event.raw.get("some_future_field"),
            Some(&serde_json::json!(7))
        );
    }

    #[test]
    fn the_tool_result_is_kept_once_and_not_again_inside_raw() {
        let value = serde_json::json!({
            "session_id": "s1",
            "hook_event_name": "PostToolUse",
            "tool_response": "a very long tool result",
            "tool_name": "Read"
        });
        let event = HookEvent::from_stdin_json(value, &HashMap::new()).expect("parses");
        assert_eq!(
            event.tool_response_text().as_deref(),
            Some("a very long tool result")
        );
        assert_eq!(event.raw.get("tool_response"), None, "stored once, not twice");
        assert_eq!(event.raw.get("tool_name"), Some(&serde_json::json!("Read")));
    }

    #[test]
    fn a_tool_response_reads_as_text_whether_it_is_a_string_or_an_object() {
        let string_response = serde_json::json!({
            "session_id": "s1", "hook_event_name": "PostToolUse", "tool_response": "ok"
        });
        let event = HookEvent::from_stdin_json(string_response, &HashMap::new()).expect("parses");
        assert_eq!(event.tool_response_text().as_deref(), Some("ok"));

        let object_response = serde_json::json!({
            "session_id": "s1", "hook_event_name": "PostToolUse", "tool_response": {"run_id": "x"}
        });
        let event = HookEvent::from_stdin_json(object_response, &HashMap::new()).expect("parses");
        assert!(event
            .tool_response_text()
            .is_some_and(|t| t.contains("run_id")));
    }

    #[test]
    fn every_known_name_round_trips_through_its_string() {
        for name in [
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "PostToolUseFailure",
            "Stop",
            "SubagentStart",
            "SubagentStop",
            "PreCompact",
            "PostCompact",
            "CwdChanged",
            "SessionEnd",
        ] {
            let parsed = HookEventName::from(name.to_string());
            assert!(
                !matches!(parsed, HookEventName::Other(_)),
                "{name} fell through"
            );
            assert_eq!(parsed.as_str(), name);
        }
    }
}
