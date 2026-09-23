//! Human-turn source parsing for the insights turn lane: what the person at the keyboard
//! typed, read back from the local harness histories no session adapter owns as
//! "human input":
//!
//! - `~/.claude/history.jsonl` — Claude Code's own prompt history (`{display, timestamp(ms),
//!   sessionId, project}`)
//! - `~/.claude/projects/**/<session>.jsonl` — Claude Code transcripts' user-role messages
//! - `~/.gemini/antigravity-cli/history.jsonl` — Antigravity CLI prompt history
//!   (`{display, timestamp, conversationId}`)
//! - `~/.gemini/antigravity-cli/brain/<session>/transcript.jsonl` — Antigravity `USER_INPUT`
//!   records (`{content, created_at}`)
//!
//! This is a human-input source, not an agent-conversation source, so it does NOT implement
//! `AgentAdapter` (whose sources normalize into session traces); it follows the same shape —
//! detect / enumerate / parse — and the same conventions from `agentworth_adapter_sdk`
//! (fast fingerprints). Source-specific record shapes live here and nowhere else; the storage
//! layer owns the tables; `agentworth_core::turns` orchestrates incremental ingestion; the
//! insights module reads stored aggregates only. Nothing here touches or rewrites the raw
//! histories.
//!
//! Every per-turn derived feature (friction trigger, vocabulary mentions, word count) is a
//! deterministic, offline classification ported verbatim from the reviewed Python pass
//! (`tools/insights/insights_query.py` and its builder `tools/behavioral_insights.py`), so the
//! numbers match the prototype HTML this replaces. Bump [`INGESTION_VERSION`] whenever any
//! derived output changes — the orchestrator wipes and re-ingests every source on a bump,
//! since file fingerprints alone cannot say the *classification* changed.

mod pipeline;
pub mod taxonomy;

use std::path::{Path, PathBuf};

use anyhow::Result;

pub use pipeline::HumanTurn;
pub use taxonomy::INGESTION_VERSION;
pub use taxonomy::VOCAB_CLUSTERS;

/// The two source families, matching the Python builder's `source` column.
pub const CLAUDE: &str = "claude";
pub const ANTIGRAVITY: &str = "antigravity";

/// One discovered turn-bearing file, fingerprinted with the same sampling hash session
/// sources use, so unchanged files skip re-parsing on rescan.
#[derive(Debug, Clone)]
pub struct TurnFileSource {
    pub path: PathBuf,
    /// `claude` or `antigravity`.
    pub source: &'static str,
    /// Session id the path itself carries, when it does (transcript layouts).
    pub path_session_id: Option<String>,
    pub file_size_bytes: u64,
    pub mtime_epoch_secs: i64,
    pub fingerprint: String,
}

/// chrono's fixed-offset type, re-exported for callers that pin a test offset.
pub type FixedTime = chrono_::FixedOffset;

/// Detect / enumerate / parse the human-turn sources. `claude_home` carries
/// `history.jsonl` and `projects/`; `antigravity_home` carries `history.jsonl` and `brain/`.
#[derive(Debug, Clone)]
pub struct HumanTurnIngestor {
    pub claude_home: PathBuf,
    pub antigravity_home: PathBuf,
    /// `None` uses the system's current local offset; tests pin one.
    pub local_offset: Option<chrono_::FixedOffset>,
}

impl HumanTurnIngestor {
    /// The real machine's homes.
    pub fn from_system() -> Self {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        Self {
            claude_home: home.join(".claude"),
            antigravity_home: home.join(".gemini").join("antigravity-cli"),
            local_offset: None,
        }
    }

    /// A synthetic root for tests: `<root>/claude-home/...` and
    /// `<root>/gemini-home/antigravity-cli/...`.
    pub fn rooted(root: &Path, local_offset: chrono_::FixedOffset) -> Self {
        Self {
            claude_home: root.join("claude-home"),
            antigravity_home: root.join("gemini-home").join("antigravity-cli"),
            local_offset: Some(local_offset),
        }
    }

    /// Cheap presence check for the two roots; never fails (an absent source is a verdict,
    /// not an error).
    pub fn detect(&self) -> Result<bool> {
        Ok(self.claude_home.exists() || self.antigravity_home.exists())
    }

    /// Every file under the homes that carries human turns, with fingerprints. Sorted by
    /// path so scan order is deterministic.
    pub fn enumerate(&self) -> Result<Vec<TurnFileSource>> {
        let mut out = Vec::new();
        for (home, source) in [
            (&self.claude_home, CLAUDE),
            (&self.antigravity_home, ANTIGRAVITY),
        ] {
            if let Some(src) = self.single_file(&home.join("history.jsonl"), source, None)? {
                out.push(src);
            }
        }

        // Claude Code transcripts: every <project>/<session>.jsonl, skipping the whole
        // subagent and paperclip subtrees the Python builder skips (their user-role records
        // carry harness text, not typing).
        let projects = self.claude_home.join("projects");
        if projects.is_dir() {
            for entry in walkdir::WalkDir::new(&projects)
                .into_iter()
                .filter_map(Result::ok)
            {
                if any_ancestor_contains(entry.path(), "--paperclip-") {
                    continue;
                }
                if entry.file_type().is_dir() {
                    if entry.file_name().to_str() == Some("subagents") {
                        continue; // prune the subtree, not just the directory row
                    }
                    continue;
                }
                let name = entry.file_name().to_str().unwrap_or("");
                if !name.ends_with(".jsonl") {
                    continue;
                }
                let stem = name.trim_end_matches(".jsonl").to_string();
                out.push(self.make_source(entry.path(), CLAUDE, Some(stem))?);
            }
        }

        // Antigravity brain transcripts: brain/<session>/transcript.jsonl.
        let brain = self.antigravity_home.join("brain");
        if brain.is_dir() {
            for entry in walkdir::WalkDir::new(&brain)
                .into_iter()
                .filter_map(Result::ok)
            {
                if !entry.file_type().is_file() {
                    continue;
                }
                if entry.file_name().to_str() != Some("transcript.jsonl") {
                    continue;
                }
                let session_id = entry
                    .path()
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .map(str::to_string);
                out.push(self.make_source(entry.path(), ANTIGRAVITY, session_id)?);
            }
        }

        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    fn single_file(
        &self,
        path: &Path,
        source: &'static str,
        path_session_id: Option<String>,
    ) -> Result<Option<TurnFileSource>> {
        if path.is_file() {
            Ok(Some(self.make_source(path, source, path_session_id)?))
        } else {
            Ok(None)
        }
    }

    fn make_source(
        &self,
        path: &Path,
        source: &'static str,
        path_session_id: Option<String>,
    ) -> Result<TurnFileSource> {
        let metadata = std::fs::metadata(path)?;
        let file_size_bytes = metadata.len();
        let mtime_epoch_secs = metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let fingerprint = agentworth_adapter_sdk::compute_fast_fingerprint(
            path,
            file_size_bytes,
            mtime_epoch_secs,
        )?;
        Ok(TurnFileSource {
            path: path.to_path_buf(),
            source,
            path_session_id,
            file_size_bytes,
            mtime_epoch_secs,
            fingerprint,
        })
    }

    /// Parse one line into a turn, or `None` when nothing human is left on it.
    pub fn parse_line(&self, src: &TurnFileSource, line: &str) -> Option<HumanTurn> {
        let value: serde_json::Value = serde_json::from_str(line).ok()?;
        pipeline::parse_line(src, &value, self.local_offset)
    }
}

fn any_ancestor_contains(path: &Path, needle: &str) -> bool {
    path.ancestors()
        .any(|p| p.to_string_lossy().contains(needle))
}

/// chrono is a both-sides dependency either way; the alias keeps signatures short.
pub(crate) mod chrono_ {
    pub use chrono::FixedOffset;
}
