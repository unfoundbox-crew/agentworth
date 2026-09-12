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
        assert_eq!(claude_project_dir("example-repo"), "-Users-dev-code-example-repo");
    }

    #[test]
    fn codex_rollout_splits_the_date_into_its_directory_tree() {
        assert_eq!(
            codex_rollout("2026-09-05", "abc"),
            "/Users/dev/.codex/sessions/2026/09/05/rollout-2026-09-05T12-00-00-abc.jsonl"
        );
    }
}
