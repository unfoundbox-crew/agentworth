use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Provenance metadata detailing the physical origin and fingerprint of an agent history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// Absolute or canonical path to the raw source file.
    pub source_path: String,
    /// Identifier of the adapter responsible for discovering and parsing this source.
    pub adapter_name: String,
    /// Size of the raw file in bytes.
    pub file_size_bytes: u64,
    /// Last modified timestamp of the source file in seconds since UNIX epoch.
    pub mtime_epoch_secs: i64,
    /// Cryptographic or fast content fingerprint (e.g. SHA-256 / BLAKE3 of contents or header).
    pub content_fingerprint: String,
}

impl Provenance {
    pub fn new(
        source_path: impl Into<String>,
        adapter_name: impl Into<String>,
        file_size_bytes: u64,
        mtime_epoch_secs: i64,
        content_fingerprint: impl Into<String>,
    ) -> Self {
        Self {
            source_path: source_path.into(),
            adapter_name: adapter_name.into(),
            file_size_bytes,
            mtime_epoch_secs,
            content_fingerprint: content_fingerprint.into(),
        }
    }
}

/// Extract repository or workspace name/path from a session source path.
///
/// Moved here from `agentworth-storage` (still re-exported from there for existing callers)
/// so `agentworth-redaction` can derive a trace's own project identity to redact it, without
/// pulling in storage's SQLite dependency for one pure string function.
#[allow(
    clippy::string_slice,
    reason = "every idx comes from find() on an ASCII literal, offset by its own byte length: always a char boundary"
)]
pub fn extract_repository_or_workspace(source_path: &str) -> String {
    let path = Path::new(source_path);
    let path_str = source_path.replace('\\', "/");

    // Skip plugin/package internal cache artifacts
    if path_str.contains("/plugins/cache/")
        || path_str.contains("/node_modules/")
        || path_str.contains("/.bun/")
    {
        return "plugins/cache".to_string();
    }

    // 1. Check if path has a Claude Code project slug format:
    // e.g. ~/.claude/projects/-Users-dev-code-unfoundbox-agentworth/uuid.jsonl
    // e.g. ~/.claude/projects/-Users-dev-code-example-project--claude-worktrees-repo-branches-inventory-108a70/...
    if let Some(idx) = path_str.find("/projects/-") {
        let after = &path_str[idx + "/projects/-".len()..];
        let full_slug = after.split('/').next().unwrap_or(after);
        // Prune worktree / sub-branch suffix starting at '--'
        let base_slug = full_slug.split("--").next().unwrap_or(full_slug);

        // Decode -Users-dev-code-foo-bar -> /Users/dev/code/foo/bar
        let decoded = format!("/{}", base_slug.replace('-', "/"));
        let parts: Vec<&str> = decoded.split('/').filter(|s| !s.is_empty()).collect();
        if parts.len() >= 2 {
            return format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1]);
        } else if let Some(last) = parts.last() {
            return last.to_string();
        }
    }

    // 2. Check if path directly contains a code/ or projects/ path
    if let Some(idx) = path_str.find("/code/") {
        let after = &path_str[idx + "/code/".len()..];
        let parts: Vec<&str> = after.split('/').filter(|s| !s.is_empty()).collect();
        if parts.len() >= 2 {
            return format!("{}/{}", parts[0], parts[1]);
        } else if let Some(first) = parts.first() {
            return first.to_string();
        }
    }

    // 3. Check for hidden directory boundary (.claude, .cursor, .agentworth, .git, .gemini)
    let components: Vec<&str> = path_str.split('/').filter(|s| !s.is_empty()).collect();
    for (i, comp) in components.iter().enumerate() {
        if comp.starts_with('.') && i > 0 {
            let parent = components[i - 1];
            if i >= 2 {
                return format!("{}/{}", components[i - 2], parent);
            }
            return parent.to_string();
        }
    }

    // 4. Fallback: parent folder name or relative repo path
    if let Some(parent) = path.parent() {
        let parent_str = parent.to_string_lossy();
        let comps: Vec<&str> = parent_str.split('/').filter(|s| !s.is_empty()).collect();
        if comps.len() >= 2 {
            return format!("{}/{}", comps[comps.len() - 2], comps[comps.len() - 1]);
        } else if let Some(last) = comps.last() {
            return last.to_string();
        }
    }

    "unknown".to_string()
}

/// The repo key for a **directory** — a live `cwd`, a `--workspace`, a recorded agent `cwd`, or
/// a checkout root. This is the one function every directory-shaped caller must use.
///
/// # Why this exists
///
/// [`extract_repository_or_workspace`] was written for a session's `source_path`: a *file*,
/// usually a transcript under `~/.claude/projects/-Users-.../<uuid>.jsonl`. Its rule 1 decodes
/// that slug and always takes the last two components of the decoded directory. Directory
/// callers fell through to rule 2 instead, which keeps two components only when the path has a
/// subdirectory under `/code/` — so a repository checked out directly at
/// `/Users/x/code/motionvector` keyed as `"motionvector"` live and as `"code/motionvector"` in
/// the index, and the two never matched. Every "no session for this repo in the index" on a
/// repo with a thousand indexed sessions was that mismatch.
///
/// The fix is not a third string rule. Identity comes from the filesystem: resolve the git
/// checkout root, canonicalize it (symlinks, `..`, `.`), and derive the key from *that*. Two
/// spellings of one directory now reduce to one key because they resolve to one inode path,
/// not because two parsers happen to agree.
///
/// The string heuristic stays as an explicit, named fallback for a directory that is not on
/// disk any more — a moved or deleted repo, which is most historical data — so nothing is
/// orphaned.
pub fn repo_key_for_dir(dir: &Path) -> String {
    match canonical_repo_root(dir) {
        Some(root) => repo_key_from_root(&root),
        // repo-key-gate: allow -- this IS the documented fallback. `dir` is not on disk, so
        // there is no checkout root to resolve and the string heuristic is all that is left.
        None => extract_repository_or_workspace(&dir.to_string_lossy()),
    }
}

/// Same as [`repo_key_for_dir`] for a directory that is only available as a string.
pub fn repo_key_for_dir_str(dir: &str) -> String {
    repo_key_for_dir(Path::new(dir))
}

/// The canonicalized root of the git checkout `dir` sits in, or `None` when `dir` is not on
/// disk or is not inside a checkout.
///
/// Walks up looking for a `.git` entry rather than shelling out to `git rev-parse
/// --show-toplevel`: same answer for every layout this product sees, no child process, and it
/// works in a unit test with no `git` on `PATH`.
///
/// **A linked worktree resolves to the repository that owns it.** Git spells the difference
/// out on disk: a main checkout has `.git` as a *directory*, while a linked worktree (this
/// repo keeps them at `<repo>/.claude/worktrees/<name>`) has `.git` as a *file* pointing at the
/// real git dir. So a `.git` file is remembered as a fallback and the walk continues; the first
/// `.git` directory above it wins. That matches what
/// [`extract_repository_or_workspace`]'s rule 1 does when it prunes a slug's `--` worktree
/// suffix, so a worktree and its parent share one key.
///
/// The signal is the *type of the `.git` entry*, never the name of a directory. An earlier
/// draft folded at the first dot-prefixed path component instead, which also fired on any
/// hidden *ancestor* — `/tmp/.tmp<rand>/code/motionvector` keyed as `tmp`, and so would a real
/// repo under `~/.local/src/`.
pub fn canonical_repo_root(dir: &Path) -> Option<PathBuf> {
    let start = std::fs::canonicalize(dir).ok()?;
    let mut linked_worktree: Option<PathBuf> = None;
    let mut current: Option<&Path> = Some(start.as_path());
    while let Some(candidate) = current {
        let dot_git = candidate.join(".git");
        if dot_git.is_dir() {
            return Some(candidate.to_path_buf());
        }
        if dot_git.is_file() && linked_worktree.is_none() {
            linked_worktree = Some(candidate.to_path_buf());
        }
        current = candidate.parent();
    }
    // A worktree (or submodule) whose owning checkout is not one of its ancestors. It is still
    // a checkout, so it keys as itself rather than as nothing.
    linked_worktree
}

/// The key a canonical checkout root reduces to: its last two path components.
pub fn repo_key_from_root(root: &Path) -> String {
    let as_str = root.to_string_lossy().replace('\\', "/");
    let components: Vec<&str> = as_str.split('/').filter(|c| !c.is_empty()).collect();
    match components.len() {
        0 => "unknown".to_string(),
        1 => components[0].to_string(),
        n => format!("{}/{}", components[n - 2], components[n - 1]),
    }
}

/// The human-readable label for a repo key. Today the key is already `parent/repo`-shaped, so
/// this is the identity function — it exists so display sites can be spelled as display sites,
/// and so a future canonical-path key can change its rendering in one place.
pub fn repo_display_label(repo_key: &str) -> &str {
    repo_key
}

/// True when `source_path` is a subagent transcript rather than a primary session.
///
/// Lives beside [`extract_repository_or_workspace`] because it already knows the same path
/// shape. The only known shape today: Claude Code writes subagent transcripts to
/// `<project>/<session-uuid>/subagents/agent-<hex>.jsonl`, a `/subagents/` directory component
/// holding files named `agent-*.jsonl`.
pub fn is_subagent_transcript(source_path: &str) -> bool {
    let path_str = source_path.replace('\\', "/");
    let Some(file_name) = path_str.rsplit('/').next() else {
        return false;
    };
    if !(file_name.starts_with("agent-") && file_name.ends_with(".jsonl")) {
        return false;
    }
    path_str.split('/').any(|component| component == "subagents")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_subagent_transcript_true_for_claude_subagent_path() {
        assert!(is_subagent_transcript(
            "/Users/dev/.claude/projects/-Users-dev-code-foo/uuid1234/subagents/agent-abc123.jsonl"
        ));
    }

    #[test]
    fn test_is_subagent_transcript_false_for_parent_session() {
        assert!(!is_subagent_transcript(
            "/Users/dev/.claude/projects/-Users-dev-code-foo/uuid1234.jsonl"
        ));
    }

    #[test]
    fn test_is_subagent_transcript_false_for_codex_rollout() {
        assert!(!is_subagent_transcript(
            "/Users/dev/.codex/sessions/2026/09/05/rollout-2026-09-05T12-00-00-uuid.jsonl"
        ));
    }

    #[test]
    fn test_is_subagent_transcript_true_for_windows_separators() {
        assert!(is_subagent_transcript(
            r"C:\Users\dev\.claude\projects\-Users-foo\uuid1234\subagents\agent-abc123.jsonl"
        ));
    }

    /// Build `<tmp>/<rel>` with a `.git` directory at `<tmp>/<repo_rel>`, and return the tmp
    /// dir (kept alive by the caller) plus the leaf path.
    fn checkout(tmp: &tempfile::TempDir, repo_rel: &str, leaf_rel: &str) -> std::path::PathBuf {
        let repo = tmp.path().join(repo_rel);
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");
        let leaf = tmp.path().join(leaf_rel);
        std::fs::create_dir_all(&leaf).expect("mkdir leaf");
        leaf
    }

    /// The bug: a repository checked out directly under `code/` with no subdirectory. The live
    /// `wake` path passed the raw cwd and got `"motionvector"`; the indexed path passed the
    /// transcript slug and got `"code/motionvector"`. Same repository, two keys, zero sessions
    /// found. Both must now answer the same string.
    #[test]
    fn test_repo_key_agrees_for_live_cwd_and_transcript_slug() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cwd = checkout(&tmp, "code/motionvector", "code/motionvector");

        let live = repo_key_for_dir(&cwd);
        let indexed = extract_repository_or_workspace(
            "/Users/dev/.claude/projects/-Users-dev-code-motionvector/abc.jsonl",
        );

        assert_eq!(live, "code/motionvector");
        assert_eq!(live, indexed, "live cwd and indexed slug must key the same");
    }

    /// The old rule 2 only kept two components when something sat *under* the repo. It does not
    /// get to decide any more: the git root does.
    #[test]
    fn test_repo_key_for_dir_beats_the_old_string_rule() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cwd = checkout(&tmp, "code/motionvector", "code/motionvector");
        assert_eq!(
            extract_repository_or_workspace("/Users/dev/code/motionvector"),
            "motionvector",
            "the string heuristic's answer, kept here so the regression is visible"
        );
        assert_eq!(repo_key_for_dir(&cwd), "code/motionvector");
    }

    /// A subdirectory deep inside the checkout keys as the checkout, not as its own last two
    /// components.
    #[test]
    fn test_repo_key_from_subdirectory_resolves_to_the_checkout() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let deep = checkout(
            &tmp,
            "code/unfoundbox/agentworth",
            "code/unfoundbox/agentworth/crates/schema/src",
        );
        assert_eq!(repo_key_for_dir(&deep), "unfoundbox/agentworth");
    }

    /// Canonicalization is the point: `..` and `.` in a path are normalized away, so two
    /// spellings of one directory cannot key differently.
    #[test]
    fn test_repo_key_is_canonical_across_dot_dot_spellings() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cwd = checkout(&tmp, "code/motionvector", "code/motionvector/src");
        let noisy = cwd.join("..").join(".").join("src");
        assert_eq!(repo_key_for_dir(&cwd), repo_key_for_dir(&noisy));
        assert_eq!(repo_key_for_dir(&noisy), "code/motionvector");
    }

    /// A symlink to the checkout resolves to the checkout.
    #[cfg(unix)]
    #[test]
    fn test_repo_key_follows_symlinks_to_one_root() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cwd = checkout(&tmp, "code/motionvector", "code/motionvector");
        let link = tmp.path().join("shortcut");
        std::os::unix::fs::symlink(&cwd, &link).expect("symlink");
        assert_eq!(repo_key_for_dir(&link), repo_key_for_dir(&cwd));
    }

    /// A linked worktree at `<repo>/.claude/worktrees/<name>` is its own checkout, but it must
    /// key as the repository it belongs to — the same fold rule 1 applies to a slug's `--`
    /// suffix. Git's own on-disk signal does the folding: the worktree's `.git` is a *file*,
    /// the owning checkout's is a *directory*.
    #[test]
    fn test_worktree_keys_as_its_parent_repository() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = checkout(&tmp, "code/unfoundbox/agentworth", "code/unfoundbox/agentworth");
        let worktree = repo.join(".claude/worktrees/agent-a63e");
        std::fs::create_dir_all(&worktree).expect("mkdir worktree");
        std::fs::write(
            worktree.join(".git"),
            "gitdir: /code/unfoundbox/agentworth/.git/worktrees/agent-a63e\n",
        )
        .expect("write gitfile");

        assert_eq!(repo_key_for_dir(&worktree), "unfoundbox/agentworth");
        assert_eq!(repo_key_for_dir(&worktree), repo_key_for_dir(&repo));
    }

    /// The fold must key off the *type of the `.git` entry*, never off a dot-prefixed directory
    /// name. A name-based rule also fires on a hidden **ancestor**, which is how
    /// `/tmp/.tmp<rand>/code/motionvector` once keyed as `tmp` — and would do the same to a
    /// real repo cloned under `~/.local/src/`.
    #[test]
    fn test_a_hidden_ancestor_does_not_truncate_the_key() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = checkout(&tmp, ".local/src/code/motionvector", ".local/src/code/motionvector");
        assert_eq!(repo_key_for_dir(&repo), "code/motionvector");
    }

    /// A worktree whose owning checkout is not one of its ancestors still keys as a checkout.
    #[test]
    fn test_a_detached_worktree_keys_as_itself() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let worktree = tmp.path().join("scratch/loose-worktree");
        std::fs::create_dir_all(&worktree).expect("mkdir");
        std::fs::write(worktree.join(".git"), "gitdir: /elsewhere/.git/worktrees/x\n")
            .expect("write gitfile");
        assert_eq!(repo_key_for_dir(&worktree), "scratch/loose-worktree");
    }

    /// A directory that is not on disk any more — a moved or deleted repo, which is most
    /// historical data — falls back to the string heuristic rather than being orphaned.
    #[test]
    fn test_missing_directory_falls_back_to_the_string_heuristic() {
        let gone = Path::new("/Users/dev/code/unfoundbox/agentworth-deleted-9f2c");
        assert_eq!(
            repo_key_for_dir(gone),
            // repo-key-gate: allow -- asserting the fallback's own answer.
            extract_repository_or_workspace(&gone.to_string_lossy())
        );
    }

    /// A directory on disk that is not inside a checkout is not a repository, and takes the
    /// same fallback.
    #[test]
    fn test_directory_outside_a_checkout_falls_back() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let plain = tmp.path().join("not-a-repo");
        std::fs::create_dir_all(&plain).expect("mkdir");
        assert_eq!(
            repo_key_for_dir(&plain),
            // repo-key-gate: allow -- asserting the fallback's own answer.
            extract_repository_or_workspace(&plain.to_string_lossy())
        );
    }

    #[test]
    fn test_repo_key_from_root_shapes() {
        assert_eq!(
            repo_key_from_root(Path::new("/Users/x/code/unfoundbox/agentworth")),
            "unfoundbox/agentworth"
        );
        assert_eq!(repo_key_from_root(Path::new("/solo")), "solo");
        assert_eq!(repo_key_from_root(Path::new("/")), "unknown");
    }

    #[test]
    fn test_provenance_serde() {
        let prov = Provenance::new(
            "/home/user/.claude/projects/foo.jsonl",
            "claude_code",
            1024,
            1720000000,
            "abcdef123456",
        );
        let json = serde_json::to_string(&prov).expect("serialize");
        let deserialized: Provenance = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(prov, deserialized);
    }
}
