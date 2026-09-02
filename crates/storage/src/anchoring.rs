//! Deciding whether a recorded `file_modifications` row belongs to one specific repository.
//!
//! `file_modifications.file_path` is not always absolute. Measured on a 25,206-row index
//! (`docs/specs/suspect-commits.md`): 25,050 rows absolute, 156 relative — every relative row
//! written by the `opencode` adapter, whose transcripts record paths relative to a cwd the
//! schema then throws away. Those 0.6% of rows are what makes a naive suffix join useless: a
//! bare `Cargo.lock` suffix-matches every Rust repository on the disk, and in the spec's own
//! sample that one collision produced 16 of 17 flags, 9 of the first 10 of them wrong.
//!
//! So a blame row counts as evidence about a repository only when it can be *anchored*: shown
//! to lie inside that repository's tree, not merely to end with the same characters.
//!
//! | recorded path | anchored when |
//! | :--- | :--- |
//! | absolute | it is `repo_root`, or sits under `repo_root/` |
//! | relative | the writing session's own repository identity matches this repository's |
//!
//! Nothing else is anchored. A relative path from a session that worked somewhere else is
//! **unanchored** — dropped, and counted, so a caller can say how much evidence it could not
//! place instead of quietly reporting a smaller number.

use agentworth_schema::extract_repository_or_workspace;

/// Why a candidate row was accepted, or the fact that it was not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Anchor {
    /// The recorded path is absolute and sits inside the repository tree.
    Absolute,
    /// The recorded path is relative and the writing session's repository identity matches.
    RelativeToSessionRepo,
    /// The recorded path is absolute and sits outside this repository. Not evidence here, and
    /// not a failure either — it is anchored, just to somewhere else.
    ElsewhereAbsolute,
    /// A relative path that could not be placed in any repository. Counted, never used.
    Unanchored,
}

/// Strip a trailing `/` (but never turn `/` itself into an empty string) so prefix comparisons
/// have exactly one form to handle.
pub(crate) fn normalize_root(repo_root: &str) -> String {
    let trimmed = repo_root.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

/// The `org/repo` identity of a checkout directory, in the same shape
/// `extract_repository_or_workspace` derives from a session's `source_path` — which is what it
/// has to be compared against. Appending `/.git` makes the directory look like the file paths
/// that function was written for, so `/Users/x/code/unfoundbox/agentworth` and a session
/// transcript under `~/.claude/projects/-Users-x-code-unfoundbox-agentworth/` both reduce to
/// `unfoundbox/agentworth`.
pub(crate) fn repo_identity(repo_root: &str) -> String {
    extract_repository_or_workspace(&format!("{}/.git", normalize_root(repo_root)))
}

/// Classify one recorded path against one repository, and return the path relative to that
/// repository's root when it is usable evidence.
///
/// `session_source_path` is the session's own transcript path, used only for the relative case:
/// it is the only surviving clue about where a relative path was written.
/// `roots` must be sorted longest-first: a linked git worktree normally sits *inside* the main
/// checkout (`<repo>/.claude/worktrees/<name>`), so both roots match the same path and only the
/// deeper one yields a path a commit can be compared against. See `sorted_roots`.
pub(crate) fn anchor_path(
    recorded_path: &str,
    roots: &[String],
    repo_identity: &str,
    session_source_path: &str,
) -> (Anchor, Option<String>) {
    if recorded_path.starts_with('/') {
        for root in roots {
            if let Some(rel) = strip_root(recorded_path, root) {
                return (Anchor::Absolute, Some(rel));
            }
        }
        return (Anchor::ElsewhereAbsolute, None);
    }

    let cleaned = recorded_path.trim_start_matches("./");
    // A `..` segment escapes the tree it is resolved against, so the path is not provably
    // inside this repository even when the identity matches.
    if cleaned.is_empty() || cleaned.split('/').any(|seg| seg == "..") {
        return (Anchor::Unanchored, None);
    }

    if extract_repository_or_workspace(session_source_path) == repo_identity {
        (Anchor::RelativeToSessionRepo, Some(cleaned.to_string()))
    } else {
        (Anchor::Unanchored, None)
    }
}

/// Normalize and order the roots a path may be anchored against: longest first, deduplicated.
/// The main checkout and its linked worktrees overlap on disk, so order is what decides whether
/// `<repo>/.claude/worktrees/x/crates/a.rs` reads as `crates/a.rs` (right) or as
/// `.claude/worktrees/x/crates/a.rs` (a path no commit will ever name).
pub(crate) fn sorted_roots(repo_root: &str, additional: &[String]) -> Vec<String> {
    let mut roots: Vec<String> = std::iter::once(normalize_root(repo_root))
        .chain(additional.iter().map(|r| normalize_root(r)))
        .collect();
    roots.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    roots.dedup();
    roots
}

/// `Some(path relative to root)` when `absolute` is the root itself or sits under it, matching
/// on whole path components — so `/a/repo-two/x` is never treated as living in `/a/repo`.
fn strip_root(absolute: &str, root: &str) -> Option<String> {
    if absolute == root {
        return Some(String::new());
    }
    let with_sep = if root.ends_with('/') {
        root.to_string()
    } else {
        format!("{root}/")
    };
    absolute
        .strip_prefix(&with_sep)
        .map(|rest| rest.trim_start_matches('/').to_string())
}

/// Escape a string for use as a literal prefix inside a SQL `LIKE ... ESCAPE '\'` pattern.
/// Real checkout paths contain `_` far more often than anyone expects (`my_repo`, `web_ui`),
/// and an unescaped `_` in LIKE matches any single character.
pub(crate) fn like_prefix_escaped(prefix: &str) -> String {
    let mut out = String::with_capacity(prefix.len() + 8);
    for ch in prefix.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/Users/dev/code/unfoundbox/agentworth";
    const IN_REPO_SESSION: &str =
        "/Users/dev/.claude/projects/-Users-dev-code-unfoundbox-agentworth/s1.jsonl";
    const OTHER_REPO_SESSION: &str =
        "/Users/dev/.local/share/opencode/project/-Users-dev-code-motionvector-studio/s2.json";
    /// A session whose transcript sits inside the checkout, so its identity resolves to the
    /// repository. This is the shape the relative-path branch can actually place.
    const IN_REPO_LOCAL_SESSION: &str = "/Users/dev/code/unfoundbox/agentworth/.sessions/s3.json";

    fn anchor(path: &str, session: &str) -> (Anchor, Option<String>) {
        anchor_path(path, &sorted_roots(ROOT, &[]), &repo_identity(ROOT), session)
    }

    /// A linked git worktree lives inside the main checkout, so both roots match the same path.
    /// The deeper one has to win, or every worktree edit anchors to a path no commit names.
    #[test]
    fn a_worktree_root_wins_over_the_checkout_that_contains_it() {
        let worktree = format!("{ROOT}/.claude/worktrees/feat-x");
        let roots = sorted_roots(ROOT, std::slice::from_ref(&worktree));
        assert_eq!(roots[0], worktree, "longest root first");

        let (a, rel) = anchor_path(
            &format!("{worktree}/crates/storage/src/lib.rs"),
            &roots,
            &repo_identity(ROOT),
            IN_REPO_SESSION,
        );
        assert_eq!(a, Anchor::Absolute);
        assert_eq!(rel.as_deref(), Some("crates/storage/src/lib.rs"));
    }

    /// Without the worktree root, the same path still anchors — just to a relative path no
    /// commit will ever match. This is the failure the ordering above prevents, pinned so the
    /// two behaviours cannot be confused.
    #[test]
    fn without_the_worktree_root_the_path_is_useless_for_matching() {
        let (_, rel) = anchor(&format!("{ROOT}/.claude/worktrees/feat-x/Cargo.toml"), IN_REPO_SESSION);
        assert_eq!(rel.as_deref(), Some(".claude/worktrees/feat-x/Cargo.toml"));
    }

    #[test]
    fn absolute_inside_the_repo_is_anchored() {
        let (a, rel) = anchor(&format!("{ROOT}/crates/storage/src/lib.rs"), IN_REPO_SESSION);
        assert_eq!(a, Anchor::Absolute);
        assert_eq!(rel.as_deref(), Some("crates/storage/src/lib.rs"));
    }

    #[test]
    fn absolute_in_another_repo_is_not_this_repos_evidence() {
        let (a, rel) = anchor("/Users/dev/code/motionvector/studio/Cargo.lock", IN_REPO_SESSION);
        assert_eq!(a, Anchor::ElsewhereAbsolute);
        assert!(rel.is_none());
    }

    #[test]
    fn a_sibling_directory_sharing_a_prefix_is_not_inside() {
        // `/…/agentworth-web` starts with `/…/agentworth` as a string but is a different tree.
        let (a, _) = anchor(&format!("{ROOT}-web/Cargo.lock"), IN_REPO_SESSION);
        assert_eq!(a, Anchor::ElsewhereAbsolute);
    }

    /// The measured trap, in one test: a bare `Cargo.lock` from a session that worked in
    /// another repository suffix-matches this repository and must not count.
    #[test]
    fn the_cargo_lock_collision_is_excluded() {
        let (a, rel) = anchor("Cargo.lock", OTHER_REPO_SESSION);
        assert_eq!(a, Anchor::Unanchored);
        assert!(rel.is_none());
    }

    #[test]
    fn a_relative_path_from_this_repos_own_session_is_anchored() {
        let (a, rel) = anchor("Cargo.lock", IN_REPO_LOCAL_SESSION);
        assert_eq!(a, Anchor::RelativeToSessionRepo);
        assert_eq!(rel.as_deref(), Some("Cargo.lock"));
    }

    #[test]
    fn a_relative_path_that_climbs_out_is_unanchored() {
        let (a, _) = anchor("../other/Cargo.lock", IN_REPO_LOCAL_SESSION);
        assert_eq!(a, Anchor::Unanchored);
    }

    #[test]
    fn a_dot_slash_prefix_is_stripped() {
        let (_, rel) = anchor("./apps/cli/src/main.rs", IN_REPO_LOCAL_SESSION);
        assert_eq!(rel.as_deref(), Some("apps/cli/src/main.rs"));
    }

    /// Measured, and the reason the relative branch places almost nothing in practice.
    ///
    /// `opencode` is the only adapter that records relative paths, and it stores its
    /// transcripts under `~/.local/share/opencode/project/<mangled>/`. That path contains no
    /// `/code/` and no `/projects/-`, so `extract_repository_or_workspace` falls through to its
    /// hidden-directory rule and returns `Users/dev` -- the home directory, not the repository
    /// the session worked in. Every one of those rows is therefore unanchored, which is what
    /// the measurement in docs/specs/suspect-commits.md shows.
    ///
    /// The branch is still right for any adapter whose transcript path does resolve. Repairing
    /// opencode's is the spec's own open question, and belongs in the adapter, not here.
    #[test]
    fn an_opencode_transcript_path_does_not_resolve_to_its_repo() {
        let opencode_here =
            "/Users/dev/.local/share/opencode/project/-Users-dev-code-unfoundbox-agentworth/s3.json";
        assert_eq!(extract_repository_or_workspace(opencode_here), "Users/dev");
        let (a, _) = anchor("Cargo.lock", opencode_here);
        assert_eq!(
            a,
            Anchor::Unanchored,
            "unanchored, and counted -- never quietly attributed to this repo"
        );
    }

    #[test]
    fn root_identity_matches_a_session_transcript_identity() {
        assert_eq!(repo_identity(ROOT), "unfoundbox/agentworth");
        assert_eq!(repo_identity(&format!("{ROOT}/")), "unfoundbox/agentworth");
        assert_eq!(
            extract_repository_or_workspace(IN_REPO_SESSION),
            repo_identity(ROOT)
        );
    }

    #[test]
    fn like_wildcards_in_a_real_path_are_escaped() {
        assert_eq!(like_prefix_escaped("/Users/d/my_repo/"), r"/Users/d/my\_repo/");
        assert_eq!(like_prefix_escaped("/a/100%/"), r"/a/100\%/");
    }
}
