use serde::{Deserialize, Serialize};
use std::path::Path;

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
    // e.g. ~/.claude/projects/-Users-saurabh-code-unfoundbox-agentworth/uuid.jsonl
    // e.g. ~/.claude/projects/-Users-saurabh-code-motionvector-pluto--claude-worktrees-repo-branches-inventory-108a70/...
    if let Some(idx) = path_str.find("/projects/-") {
        let after = &path_str[idx + "/projects/-".len()..];
        let full_slug = after.split('/').next().unwrap_or(after);
        // Prune worktree / sub-branch suffix starting at '--'
        let base_slug = full_slug.split("--").next().unwrap_or(full_slug);

        // Decode -Users-saurabh-code-foo-bar -> /Users/saurabh/code/foo/bar
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

#[cfg(test)]
mod tests {
    use super::*;

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
