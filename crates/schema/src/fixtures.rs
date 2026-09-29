//! The one fake identity every test fixture in this workspace builds paths from.
//!
//! # Why this module exists
//!
//! This is a PUBLIC repository. On 2026-09-10 it was found to carry the maintainer's real
//! system username in 101 places and, worse, a third party's project name inside shipped test
//! fixtures and user-facing product copy -- including a `rm -rf` narrative about destroying
//! 30 GB of someone else's repository. None of it was deliberate. Every one of those literals
//! was typed by someone building a fixture from whatever was on their screen at the time.
//!
//! A CI grep was considered and rejected: it taxes every build forever to catch a mistake at
//! the last possible moment, and it teaches nothing. The fix is to make the mistake awkward
//! instead of policed. Build fixture paths from these helpers and there is no moment where
//! typing a real path is the convenient option.
//!
//! # Using it
//!
//! ```
//! use agentworth_schema::fixtures;
//!
//! let transcript = fixtures::claude_transcript("example-repo", "uuid1234");
//! assert_eq!(
//!     transcript,
//!     "/Users/dev/.claude/projects/-Users-dev-code-example-repo/uuid1234.jsonl"
//! );
//! ```
//!
//! If you need a name these helpers do not cover, add it HERE rather than inlining a literal
//! at the call site. A name that only exists in one test is a name nobody will think to check.

/// The fake home directory. Deliberately `dev`, not a real person's login.
pub const HOME: &str = "/Users/dev";

/// The fake Windows home, for the path rules that are platform-specific.
pub const WINDOWS_HOME: &str = r"C:\Users\dev";

/// Neutral repository names. Use these rather than inventing one per test -- a fixture that
/// names a real project is exactly the failure this module exists to prevent.
pub const REPO: &str = "example-repo";
pub const OTHER_REPO: &str = "example-app";
pub const ORG: &str = "acme";

/// `~/code/<repo>` -- a checkout under the fake home.
pub fn repo_path(repo: &str) -> String {
    format!("{HOME}/code/{repo}")
}

/// Claude Code's project-directory encoding: `/Users/dev/code/foo` becomes
/// `-Users-dev-code-foo`. The round-trip is load-bearing -- `provenance.rs` decodes it back --
/// so this builds the encoded form rather than leaving each fixture to hand-write it.
pub fn claude_project_dir(repo: &str) -> String {
    format!("-Users-dev-code-{repo}")
}

/// A Claude Code transcript path for a top-level session.
pub fn claude_transcript(repo: &str, uuid: &str) -> String {
    format!(
        "{HOME}/.claude/projects/{}/{uuid}.jsonl",
        claude_project_dir(repo)
    )
}

/// A Claude Code SUBAGENT transcript path -- a different shape, and the one
/// `is_subagent_transcript` keys on.
pub fn claude_subagent_transcript(repo: &str, uuid: &str, agent: &str) -> String {
    format!(
        "{HOME}/.claude/projects/{}/{uuid}/subagents/agent-{agent}.jsonl",
        claude_project_dir(repo)
    )
}

/// A Codex rollout path.
pub fn codex_rollout(date: &str, uuid: &str) -> String {
    let (y, md) = date.split_at(4);
    let (m, d) = md.trim_start_matches('-').split_at(2);
    let d = d.trim_start_matches('-');
    format!("{HOME}/.codex/sessions/{y}/{m}/{d}/rollout-{date}T12-00-00-{uuid}.jsonl")
}

/// The synthetic home the human-turn fixtures build from (mirrors `synthetic_turn_home`).
pub fn synthetic_turn_home() -> String {
    format!("{HOME}/.agentworth-test/fixtures/turn-home")
}

/// `claude-home/history.jsonl` inside a synthetic home. For tests, the ingested file lives
/// under `root.join("claude-home").join("history.jsonl")`; this is the identity string for
/// provenance and assertions. Tests should use `agw fixtures` helpers, not literals.
pub fn synthetic_claude_history_path() -> String {
    format!("{}/claude-home/history.jsonl", synthetic_turn_home())
}

/// Writes the turn-bearing synthetic home `HumanTurnIngestor::rooted` reads, under
/// `<root>/claude-home` and `<root>/gemini-home/antigravity-cli`. The same golden fixture
/// set the turn-lane test pins its counts to, so every consumer (core insights test, MCP
/// insights tests) asserts against one built shape: 4 stored turns, 1 deduped duplicate,
/// 5 degrades, taxonomy classes and vocabulary counts those tests pin.
pub fn write_human_turns_fixture(root: &std::path::Path) {
    use chrono::{TimeZone, Utc};

    let epoch_ms = |y: i32, m: u32, d: u32, h: u32, min: u32| -> i64 {
        Utc.with_ymd_and_hms(y, m, d, h, min, 0)
            .unwrap()
            .timestamp_millis()
    };
    let iso = |y: i32, m: u32, d: u32, h: u32, min: u32| {
        format!(
            "{}+00:00",
            Utc.with_ymd_and_hms(y, m, d, h, min, 0)
                .unwrap()
                .format("%Y-%m-%dT%H:%M:%S")
        )
    };
    use std::fs;

    let claude_home = root.join("claude-home");
    let projects = claude_home.join("projects").join(claude_project_dir(REPO));
    fs::create_dir_all(&projects).unwrap();

    fs::write(
        claude_home.join("history.jsonl"),
        [
            format!(
                r#"{{"display":{},"pastedContents":{{}},"timestamp":{},"sessionId":{},"project":"proj"}}"#,
                serde_json::to_string("stop looping again, fix the rust bug").unwrap(),
                epoch_ms(2026, 2, 5, 10, 30),
                serde_json::to_string("sess-hist-1").unwrap(),
            ),
            format!(
                r#"{{"display":{},"pastedContents":{{}},"timestamp":{},"sessionId":{},"project":"proj"}}"#,
                serde_json::to_string("you forgot the doppler mcp receipts").unwrap(),
                epoch_ms(2026, 2, 5, 20, 0),
                serde_json::to_string("sess-hist-1").unwrap(),
            ),
            format!(
                r#"{{"display":{},"pastedContents":{{}},"timestamp":{},"sessionId":{},"project":"proj"}}"#,
                serde_json::to_string("<task-notification>the loop ran to completion</task-notification>")
                    .unwrap(),
                epoch_ms(2026, 2, 5, 21, 0),
                serde_json::to_string("not-real-uuid").unwrap(),
            ),
            "not json at all".to_string(),
        ]
        .join("\n")
            + "\n",
    )
    .unwrap();

    let project_session = "11111111-1111-4111-8111-111111111111";
    fs::write(
        projects.join(format!("{project_session}.jsonl")),
        [
            r#"{"type":"assistant","message":{"content":[]}}"#.to_string(),
            format!(
                r#"{{"type":"user","message":{{"role":"user","content":{}}},"timestamp":{}}}"#,
                serde_json::to_string("you forgot the doppler mcp receipts").unwrap(),
                epoch_ms(2026, 2, 5, 20, 0),
            ),
            format!(
                r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","content":"1 file changed"}}]}},"timestamp":{}}}"#,
                epoch_ms(2026, 2, 5, 22, 0),
            ),
        ]
        .join("\n")
            + "\n",
    )
    .unwrap();

    let agy_home = root.join("gemini-home").join("antigravity-cli");
    fs::create_dir_all(&agy_home).unwrap();
    fs::write(
        agy_home.join("history.jsonl"),
        [
            format!(
                r#"{{"display":{},"timestamp":"{}","conversationId":{}}}"#,
                serde_json::to_string("that fake file does not exist at all").unwrap(),
                iso(2026, 2, 6, 18, 0),
                serde_json::to_string("conv-fix-2").unwrap(),
            ),
            r#"{"conversationId":"conv-2","content":"the wrong record shape never parses"}"#
                .to_string(),
        ]
        .join("\n")
            + "\n",
    )
    .unwrap();
    let brain_session = "22222222-2222-4222-8222-222222222222";
    fs::create_dir_all(agy_home.join("brain").join(brain_session)).unwrap();
    let brain_text = "the cargo receipts, truth checked now";
    fs::write(
        agy_home
            .join("brain")
            .join(brain_session)
            .join("transcript.jsonl"),
        format!(
            r#"{{"type":"USER_INPUT","content":{},"created_at":"{}"}}"#,
            serde_json::to_string(&format!("<USER_REQUEST>{brain_text}</USER_REQUEST>")).unwrap(),
            iso(2026, 2, 6, 20, 0),
        ) + "\n",
    )
    .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_helper_can_produce_a_real_looking_home() {
        for p in [
            repo_path(REPO),
            claude_transcript(REPO, "u"),
            claude_subagent_transcript(REPO, "u", "a"),
            codex_rollout("2026-09-05", "u"),
        ] {
            assert!(p.starts_with(HOME), "{p} escaped the fixture home");
        }
    }

    #[test]
    fn the_claude_project_encoding_round_trips() {
        // The decoder in `provenance.rs` turns `-Users-dev-code-example-repo` back into a
        // path; if this encoding drifts, every Claude fixture silently stops representing what
        // the adapter actually sees.
        assert_eq!(
            claude_project_dir("example-repo"),
            "-Users-dev-code-example-repo"
        );
    }

    #[test]
    fn codex_rollout_splits_the_date_into_its_directory_tree() {
        assert_eq!(
            codex_rollout("2026-09-05", "abc"),
            "/Users/dev/.codex/sessions/2026/09/05/rollout-2026-09-05T12-00-00-abc.jsonl"
        );
    }
}
