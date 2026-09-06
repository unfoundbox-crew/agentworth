//! The lifecycle: four states per session, driven only by hooks.
//!
//! `registered → working → idle → ended`, from `SessionStart`, `UserPromptSubmit`, `Stop` and
//! `SessionEnd`. Every other event bumps the sequence and the clock, which is how the loop knows
//! a session is alive without a timer.
//!
//! One rule is load-bearing and comes from herdr's own hook script: an event carrying an
//! `agent_id` belongs to a subagent, and a subagent must never revive an idle pane. Those events
//! bump the parent's `last_seq` and change nothing else.

use crate::hook::{HookEvent, HookEventName};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Registered,
    Working,
    Idle,
    Ended,
}

impl AgentState {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentState::Registered => "registered",
            AgentState::Working => "working",
            AgentState::Idle => "idle",
            AgentState::Ended => "ended",
        }
    }
}

/// What the loop believes about one session right now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionLive {
    pub session_id: String,
    pub state: AgentState,
    /// When the session entered `state`, not when it was last seen.
    pub since: DateTime<Utc>,
    pub pane_id: Option<String>,
    pub cwd: Option<String>,
    pub last_seq: u64,
    pub updated_at: DateTime<Utc>,
}

/// A state change worth telling someone about. `from` is `None` the first time a session is seen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    pub session_id: String,
    pub from: Option<AgentState>,
    pub to: AgentState,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Default, Clone)]
pub struct LoopState {
    sessions: BTreeMap<String, SessionLive>,
}

impl LoopState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, session_id: &str) -> Option<&SessionLive> {
        self.sessions.get(session_id)
    }

    pub fn sessions(&self) -> impl Iterator<Item = &SessionLive> {
        self.sessions.values()
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Folds one event in. Returns the transition it caused, if it caused one.
    pub fn apply(&mut self, event: &HookEvent) -> Option<Transition> {
        let at = event.received_at;
        let is_new = !self.sessions.contains_key(&event.session_id);
        let live = self
            .sessions
            .entry(event.session_id.clone())
            .or_insert_with(|| SessionLive {
                session_id: event.session_id.clone(),
                state: AgentState::Registered,
                since: at,
                pane_id: None,
                cwd: None,
                last_seq: 0,
                updated_at: at,
            });

        live.last_seq += 1;
        live.updated_at = at;
        if live.pane_id.is_none() {
            live.pane_id.clone_from(&event.pane_id);
        }
        if live.cwd.is_none() {
            live.cwd.clone_from(&event.cwd);
        }

        // A subagent's events are the parent's activity, never the parent's state.
        if event.is_subagent() {
            return None;
        }

        if event.hook_event_name == HookEventName::CwdChanged {
            if event.cwd.is_some() {
                live.cwd.clone_from(&event.cwd);
            }
            return None;
        }

        let target = match event.hook_event_name {
            HookEventName::SessionStart => AgentState::Registered,
            HookEventName::UserPromptSubmit => AgentState::Working,
            HookEventName::Stop => AgentState::Idle,
            HookEventName::SessionEnd => AgentState::Ended,
            _ => return None,
        };

        if !is_new && live.state == target {
            return None;
        }
        let from = if is_new { None } else { Some(live.state) };
        live.state = target;
        live.since = at;
        Some(Transition {
            session_id: event.session_id.clone(),
            from,
            to: target,
            at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn event(name: &str) -> HookEvent {
        HookEvent::from_stdin_json(
            serde_json::json!({"session_id": "s1", "hook_event_name": name}),
            &HashMap::new(),
        )
        .expect("parses")
    }

    #[test]
    fn the_four_states_follow_the_four_events_in_order() {
        let mut state = LoopState::new();
        let start = state.apply(&event("SessionStart")).expect("registers");
        assert_eq!(start.from, None);
        assert_eq!(start.to, AgentState::Registered);

        assert_eq!(
            state.apply(&event("UserPromptSubmit")).expect("works").to,
            AgentState::Working
        );
        assert_eq!(
            state.apply(&event("Stop")).expect("idles").to,
            AgentState::Idle
        );
        let end = state.apply(&event("SessionEnd")).expect("ends");
        assert_eq!(end.from, Some(AgentState::Idle));
        assert_eq!(end.to, AgentState::Ended);
        assert_eq!(state.get("s1").expect("known").state, AgentState::Ended);
        assert_eq!(state.get("s1").expect("known").last_seq, 4);
    }

    #[test]
    fn repeating_an_event_bumps_the_sequence_without_a_transition() {
        let mut state = LoopState::new();
        state.apply(&event("SessionStart"));
        state.apply(&event("UserPromptSubmit"));
        assert!(state.apply(&event("UserPromptSubmit")).is_none());
        assert_eq!(state.get("s1").expect("known").last_seq, 3);
        assert_eq!(state.get("s1").expect("known").state, AgentState::Working);
    }

    #[test]
    fn a_tool_event_is_activity_and_not_a_state_change() {
        let mut state = LoopState::new();
        state.apply(&event("SessionStart"));
        assert!(state.apply(&event("PreToolUse")).is_none());
        assert_eq!(
            state.get("s1").expect("known").state,
            AgentState::Registered
        );
        assert_eq!(state.get("s1").expect("known").last_seq, 2);
    }

    #[test]
    fn cwd_changed_moves_the_directory_and_nothing_else() {
        let mut state = LoopState::new();
        state.apply(&event("SessionStart"));
        let moved = HookEvent::from_stdin_json(
            serde_json::json!({
                "session_id": "s1", "hook_event_name": "CwdChanged", "cwd": "/repo/two"
            }),
            &HashMap::new(),
        )
        .expect("parses");
        assert!(state.apply(&moved).is_none());
        let live = state.get("s1").expect("known");
        assert_eq!(live.cwd.as_deref(), Some("/repo/two"));
        assert_eq!(live.state, AgentState::Registered);
    }

    #[test]
    fn a_subagent_bumps_the_sequence_and_never_revives_an_idle_pane() {
        let mut state = LoopState::new();
        state.apply(&event("SessionStart"));
        state.apply(&event("UserPromptSubmit"));
        state.apply(&event("Stop"));

        let subagent = HookEvent::from_stdin_json(
            serde_json::json!({
                "session_id": "s1",
                "hook_event_name": "UserPromptSubmit",
                "agent_id": "agent_01",
                "agent_type": "Explore"
            }),
            &HashMap::new(),
        )
        .expect("parses");
        assert!(state.apply(&subagent).is_none());
        let live = state.get("s1").expect("known");
        assert_eq!(live.state, AgentState::Idle);
        assert_eq!(live.last_seq, 4);
    }

    #[test]
    fn an_unknown_event_name_is_activity_on_a_session_the_loop_had_not_seen() {
        let mut state = LoopState::new();
        assert!(state.apply(&event("SomeFutureHook")).is_none());
        let live = state.get("s1").expect("known");
        assert_eq!(live.state, AgentState::Registered);
        assert_eq!(live.last_seq, 1);
        assert_eq!(state.len(), 1);
        assert!(!state.is_empty());
    }

    #[test]
    fn the_pane_id_is_kept_from_the_first_event_that_carried_one() {
        let mut state = LoopState::new();
        let with_pane = HookEvent::from_stdin_json(
            serde_json::json!({"session_id": "s1", "hook_event_name": "SessionStart"}),
            &HashMap::from([(crate::hook::PANE_ID_ENV.to_string(), "%3".to_string())]),
        )
        .expect("parses");
        state.apply(&with_pane);
        state.apply(&event("Stop"));
        assert_eq!(
            state.get("s1").expect("known").pane_id.as_deref(),
            Some("%3")
        );
    }
}
