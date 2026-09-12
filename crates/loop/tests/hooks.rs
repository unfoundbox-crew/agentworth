//! Every fixture in `tests/fixtures/hooks` is a payload shaped like the ones Claude Code writes
//! to a hook's stdin. They exist so a change to `HookEvent` has to answer to a real payload
//! rather than to a hand-made one that happens to fit the struct.

use agentworth_loop::{
    predicted_writes, AgentState, HookEvent, HookEventName, Intent, LoopState, PANE_ID_ENV,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> HookEvent {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hooks")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let value: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let env = HashMap::from([(PANE_ID_ENV.to_string(), "%12".to_string())]);
    HookEvent::from_stdin_json(value, &env).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

const SESSION: &str = "8f2a1c40-3d5e-4b7a-9c1e-2f6d8a0b3e51";

#[test]
fn every_fixture_parses_and_carries_the_common_fields() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hooks");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("fixtures directory")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".json"))
        .collect();
    names.sort();
    assert_eq!(names.len(), 8, "one fixture per hook event: {names:?}");

    for name in names {
        let event = fixture(&name);
        assert_eq!(event.session_id, SESSION, "{name}");
        assert!(event.transcript_path.is_some(), "{name}");
        assert!(event.cwd.is_some(), "{name}");
        assert!(
            !matches!(event.hook_event_name, HookEventName::Other(_)),
            "{name}"
        );
        assert_eq!(event.pane_id.as_deref(), Some("%12"), "{name}");
        assert!(event.raw.is_object(), "{name}");
    }
}

#[test]
fn session_start_carries_the_permission_mode_and_the_effort() {
    let event = fixture("01-session-start.json");
    assert_eq!(event.hook_event_name, HookEventName::SessionStart);
    assert_eq!(event.permission_mode.as_deref(), Some("default"));
    assert_eq!(event.effort.as_deref(), Some("high"));
    assert!(!event.is_subagent());
}

#[test]
fn a_submitted_prompt_carries_its_prompt_id() {
    let event = fixture("02-user-prompt-submit.json");
    assert_eq!(event.hook_event_name, HookEventName::UserPromptSubmit);
    assert_eq!(event.prompt_id.as_deref(), Some("prompt_01HQ8Z3M2K"));
}

#[test]
fn a_write_predicts_the_file_it_is_about_to_create() {
    let event = fixture("03-pre-tool-use-write.json");
    assert_eq!(event.hook_event_name, HookEventName::PreToolUse);
    assert_eq!(event.tool_name.as_deref(), Some("Write"));
    let intent = Intent::from_pre_tool_use(&event, 3).expect("an intent");
    assert_eq!(
        intent.prediction.paths,
        vec![PathBuf::from(
            "/Users/dev/code/archie/crates/loop/src/spool.rs"
        )]
    );
    assert!(intent.prediction.known);
    assert_eq!(
        intent.tool_use_id.as_deref(),
        Some("toolu_01A9FvKp2mQx7Yr3Nt6Bw8Ld")
    );
}

#[test]
fn a_read_result_arrives_as_text_and_predicts_no_writes() {
    let event = fixture("04-post-tool-use-read.json");
    assert_eq!(event.hook_event_name, HookEventName::PostToolUse);
    assert!(event
        .tool_response_text()
        .is_some_and(|t| t.contains("# Loop")));
    let prediction = predicted_writes("Read", event.tool_input.as_ref());
    assert!(prediction.paths.is_empty());
    assert!(prediction.known);
}

#[test]
fn a_failed_bash_carries_its_error_and_a_command_whose_writes_are_unknown() {
    let event = fixture("05-post-tool-use-failure-bash.json");
    assert_eq!(event.hook_event_name, HookEventName::PostToolUseFailure);
    assert!(event.tool_error.is_some_and(|e| e.contains("nextest")));
    let prediction = predicted_writes("Bash", event.tool_input.as_ref());
    assert_eq!(
        prediction.command.as_deref(),
        Some("cargo test -p agentworth-loop")
    );
    assert!(!prediction.known);
    assert!(event.tool_response.is_some_and(|r| r.is_object()));
}

#[test]
fn a_subagent_edit_is_activity_on_the_parent_and_never_its_state() {
    let event = fixture("06-subagent-pre-tool-use-edit.json");
    assert!(event.is_subagent());
    assert_eq!(event.agent_type.as_deref(), Some("Explore"));

    let mut state = LoopState::new();
    state.apply(&fixture("01-session-start.json"));
    state.apply(&fixture("08-stop.json"));
    assert!(state.apply(&event).is_none());
    let live = state.get(SESSION).expect("the parent session");
    assert_eq!(live.state, AgentState::Idle);
    assert_eq!(live.last_seq, 3);
}

#[test]
fn cwd_changed_carries_nothing_but_the_common_fields() {
    let event = fixture("07-cwd-changed.json");
    assert_eq!(event.hook_event_name, HookEventName::CwdChanged);
    assert_eq!(
        event.cwd.as_deref(),
        Some("/Users/dev/code/archie/.claude/worktrees/loop")
    );
    assert!(event.tool_name.is_none());
    assert!(event.tool_input.is_none());
}

#[test]
fn stop_carries_the_last_assistant_message_and_idles_the_session() {
    let event = fixture("08-stop.json");
    assert_eq!(event.hook_event_name, HookEventName::Stop);
    assert!(event
        .last_assistant_message
        .is_some_and(|m| m.contains("lenovo")));

    let mut state = LoopState::new();
    state.apply(&fixture("02-user-prompt-submit.json"));
    let transition = state.apply(&fixture("08-stop.json")).expect("a transition");
    assert_eq!(transition.from, Some(AgentState::Working));
    assert_eq!(transition.to, AgentState::Idle);
}

#[test]
fn the_fixtures_replayed_in_order_walk_the_whole_lifecycle() {
    let mut state = LoopState::new();
    for name in [
        "01-session-start.json",
        "02-user-prompt-submit.json",
        "03-pre-tool-use-write.json",
        "04-post-tool-use-read.json",
        "05-post-tool-use-failure-bash.json",
        "06-subagent-pre-tool-use-edit.json",
        "07-cwd-changed.json",
        "08-stop.json",
    ] {
        state.apply(&fixture(name));
    }
    let live = state.get(SESSION).expect("the session");
    assert_eq!(live.state, AgentState::Idle);
    assert_eq!(live.last_seq, 8);
    assert_eq!(live.pane_id.as_deref(), Some("%12"));
    assert_eq!(
        live.cwd.as_deref(),
        Some("/Users/dev/code/archie/.claude/worktrees/loop")
    );
}
