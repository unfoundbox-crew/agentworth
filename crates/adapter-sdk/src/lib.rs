//! Common SDK and trait abstractions for AgentWorth source discovery and trace adapters.

use agentworth_schema::AgentWorthTrace;
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// `(file_size_bytes, mtime_epoch_secs, fingerprint)` for one previously indexed source, keyed
/// by its `source_path` exactly as stored in the database. Lets enumeration skip hashing a
/// file's content when the cheap stat (size, mtime) already matches what's on record -- see
/// [`SessionSource::from_path_with_known`].
pub type KnownSourceMap = HashMap<String, (u64, i64, String)>;

/// Options passed into adapter discovery and scanning.
#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    /// Custom path overrides supplied by the user.
    pub custom_paths: Vec<PathBuf>,
    /// Force full re-parsing even if file fingerprint has not changed.
    pub force: bool,
    /// Explicit opt-out from stub filtering during scanning. When `false` (the default),
    /// a parsed trace that would fail the storage layer's stub predicate (near-empty, no
    /// real activity) is not written to the index, and a full unscoped scan prunes
    /// already-indexed stub rows the same way. Set `true` to keep storing/prune-skipping
    /// stub rows, e.g. for debugging what a since-tightened adapter is filtering out.
    pub include_stubs: bool,
    /// Every source's `(file_size, mtime, fingerprint)` as already indexed, so adapters can
    /// skip re-hashing an unchanged file's content during enumeration. Populated once by the
    /// scanner from the storage layer before adapters run; empty by default (e.g. in tests and
    /// direct adapter construction), which simply means every source gets hashed as before.
    pub known_sources: Arc<KnownSourceMap>,
}

/// Result of probing the system for an agent's presence.
#[derive(Debug, Clone)]
pub struct DetectionResult {
    pub adapter_name: &'static str,
    pub is_present: bool,
    pub discovered_roots: Vec<PathBuf>,
    pub confidence: f32,
}

/// A discovered file or log artifact representing an agent session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSource {
    pub path: PathBuf,
    pub adapter_name: String,
    pub file_size_bytes: u64,
    pub mtime_epoch_secs: i64,
    pub fingerprint: String,
}

impl SessionSource {
    /// Inspect a file path on disk and construct a SessionSource with fast content fingerprinting.
    pub fn from_path(path: impl Into<PathBuf>, adapter_name: impl Into<String>) -> Result<Self> {
        Self::from_path_with_known(path, adapter_name, &KnownSourceMap::new())
    }

    /// Like [`Self::from_path`], but looks `path` up in `known_sources` (keyed by the exact
    /// string this source's identity will use in the index) before hashing content. If size
    /// and mtime both match what's on record there, the file cannot have changed -- the
    /// fingerprint is a deterministic function of path+size+mtime+content-prefix, so if two of
    /// those three inputs are unchanged and the file's own metadata says the third is too, the
    /// hash is guaranteed to come out identical. Reusing the recorded value then skips the
    /// open+read entirely, which is where a no-op scan of thousands of unchanged transcripts
    /// spent roughly half its time before this existed. Mismatched or missing metadata falls
    /// back to a full hash, same as `from_path`.
    pub fn from_path_with_known(
        path: impl Into<PathBuf>,
        adapter_name: impl Into<String>,
        known_sources: &KnownSourceMap,
    ) -> Result<Self> {
        let path = path.into();
        let metadata = std::fs::metadata(&path)?;
        let file_size_bytes = metadata.len();
        let mtime_epoch_secs = metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let fingerprint = reuse_or_compute_fingerprint(
            known_sources,
            &path.to_string_lossy(),
            &path,
            file_size_bytes,
            mtime_epoch_secs,
        )?;

        Ok(Self {
            path,
            adapter_name: adapter_name.into(),
            file_size_bytes,
            mtime_epoch_secs,
            fingerprint,
        })
    }
}

/// Shared by every `from_path`-style constructor (including adapters that build their own
/// `SessionSource` under a synthetic identity, e.g. codex/opencode): reuse the indexed
/// fingerprint for `identity_key` when its recorded size and mtime match what's on disk now,
/// otherwise hash `path`'s actual content.
pub fn reuse_or_compute_fingerprint(
    known_sources: &KnownSourceMap,
    identity_key: &str,
    path: &Path,
    file_size_bytes: u64,
    mtime_epoch_secs: i64,
) -> Result<String> {
    if let Some((known_size, known_mtime, known_fingerprint)) = known_sources.get(identity_key) {
        if *known_size == file_size_bytes && *known_mtime == mtime_epoch_secs {
            return Ok(known_fingerprint.clone());
        }
    }
    compute_fast_fingerprint(path, file_size_bytes, mtime_epoch_secs)
}

/// Computes a fast fingerprint using file metadata and header/footer sampling.
pub fn compute_fast_fingerprint(path: &Path, file_size: u64, mtime: i64) -> Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    hasher.update(file_size.to_le_bytes());
    hasher.update(mtime.to_le_bytes());

    if file_size > 0 {
        let mut file = File::open(path)?;
        // Sample up to first 4KB
        let sample_size = 4096.min(file_size as usize);
        let mut buffer = vec![0u8; sample_size];
        let bytes_read = file.read(&mut buffer)?;
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hex::encode(hasher.finalize()))
}

/// Result returned from parsing an agent history session.
#[derive(Debug, Clone)]
pub struct ParseResult {
    pub trace: AgentWorthTrace,
    pub malformed_lines: usize,
    pub warnings: Vec<String>,
}

/// Grounded extraction capabilities supported by an adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AdapterCapabilities {
    pub prompts: bool,
    pub tokens: bool,
    pub tools: bool,
    pub shell: bool,
    pub diffs: bool,
    pub thinking: bool,
    pub outcomes: bool,
}

impl AdapterCapabilities {
    pub fn score(&self) -> (usize, usize) {
        let total = 7;
        let mut supported = 0;
        if self.prompts {
            supported += 1;
        }
        if self.tokens {
            supported += 1;
        }
        if self.tools {
            supported += 1;
        }
        if self.shell {
            supported += 1;
        }
        if self.diffs {
            supported += 1;
        }
        if self.thinking {
            supported += 1;
        }
        if self.outcomes {
            supported += 1;
        }
        (supported, total)
    }
}

/// Trait implemented by each agent-specific adapter.
pub trait AgentAdapter: Send + Sync {
    /// Human/machine-readable identifier of the adapter (e.g. "claude_code").
    fn name(&self) -> &'static str;

    /// Return the extraction capability profile of this adapter.
    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            prompts: true,
            ..Default::default()
        }
    }

    /// Version of this adapter's parse output, recorded on every session row it indexes.
    ///
    /// An incremental scan skips a source whose bytes have not changed, so a fix to *how* a
    /// file is read never reaches an already-indexed session -- the index keeps serving the
    /// old, wrong answer until someone runs `--force`. Bumping this makes the scanner reparse
    /// every row an older version produced, exactly once.
    ///
    /// Bump it whenever the parse output changes for files that already parse successfully:
    /// new or corrected fields, different event normalization, changed token accounting. Do
    /// not bump it for discovery/enumeration changes (those alter which files are seen, not
    /// what a seen file yields) or for pure refactors.
    fn parser_version(&self) -> i64 {
        1
    }

    /// Every distinct `adapter` name this adapter's parsed sessions can be stored under.
    /// Defaults to `[name()]`. An adapter that routes sessions into more than one product
    /// identity at parse time (e.g. Gemini vs. Antigravity, both handled by one adapter
    /// implementation) overrides this so callers that join on adapter name -- the coverage
    /// matrix in particular -- can still find every row this adapter produces instead of
    /// only the rows tagged with its own `name()`.
    fn identity_names(&self) -> Vec<&'static str> {
        vec![self.name()]
    }

    /// Whether `source` is still present and readable, for callers deciding if a previously
    /// enumerated/indexed source can still be scanned or re-parsed.
    ///
    /// Defaults to a plain `source.path.exists()`, which is correct for every adapter whose
    /// `SessionSource.path` names a real file. It is wrong for an adapter whose sources are
    /// virtual -- a synthetic identity string built to carry repository/provenance
    /// information (e.g. opencode's `<repo>/.opencode/session-<id>.db::opencode-repo::<db_path>#<session_id>`)
    /// that never exists as a literal path on disk even when the session is fully present in
    /// its backing store. Such an adapter overrides this to check its own notion of presence
    /// (e.g. the backing database file exists and the row is still in it) instead.
    fn source_exists(&self, source: &SessionSource) -> bool {
        source.path.exists()
    }

    /// Detect whether this agent's history directories exist on the local system.
    fn detect(&self, options: &ScanOptions) -> Result<DetectionResult>;

    /// Enumerate all candidate session files for this adapter.
    fn enumerate(&self, options: &ScanOptions) -> Result<Vec<SessionSource>>;

    /// Parse a single session source into a canonical AgentWorth trace.
    fn parse(&self, source: &SessionSource) -> Result<ParseResult>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_session_source_fingerprinting() {
        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(b"{\"type\":\"user\",\"content\":\"hello\"}\n")
            .unwrap();

        let source = SessionSource::from_path(temp.path(), "test_adapter").unwrap();
        assert_eq!(source.adapter_name, "test_adapter");
        assert!(source.file_size_bytes > 0);
        assert!(!source.fingerprint.is_empty());
    }

    /// An unchanged source (its `known_sources` entry has the exact size and mtime the file
    /// still has) must be recognized without ever opening the file for a content read. Proven
    /// here by stripping read permission after recording the known metadata: a fingerprint
    /// recompute would fail with a permission error, so success means the cached value was
    /// reused instead. This is the behavior `needs_backfill`'s missing index and the eager
    /// per-file hash both used to defeat -- see the scan-fast PR.
    #[test]
    #[cfg(unix)]
    fn test_from_path_with_known_skips_hashing_an_unchanged_unreadable_file() {
        use std::os::unix::fs::PermissionsExt;

        let mut temp = NamedTempFile::new().unwrap();
        temp.write_all(b"{\"type\":\"user\",\"content\":\"hello\"}\n")
            .unwrap();
        let path = temp.path().to_path_buf();

        let baseline = SessionSource::from_path(&path, "test_adapter").unwrap();

        let mut known = KnownSourceMap::new();
        known.insert(
            path.to_string_lossy().to_string(),
            (
                baseline.file_size_bytes,
                baseline.mtime_epoch_secs,
                baseline.fingerprint.clone(),
            ),
        );

        // Metadata (size/mtime) is still readable with no read permission on most platforms --
        // only opening the file for its content should fail from here on.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

        let result = SessionSource::from_path_with_known(&path, "test_adapter", &known);

        // Restore permissions before any assertion can early-return and leak an unreadable
        // temp file past the test.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let source = result.expect("unchanged source must resolve without opening the file");
        assert_eq!(source.fingerprint, baseline.fingerprint);

        // Sanity check the premise: an entry that does NOT match (empty cache) really does
        // need to open the file, so it fails while permissions are stripped.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let uncached = SessionSource::from_path_with_known(&path, "test_adapter", &KnownSourceMap::new());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(uncached.is_err(), "an uncached lookup should need to hash content and fail without read access");
    }

    #[test]
    fn test_adapter_capabilities_score() {
        let full = AdapterCapabilities {
            prompts: true,
            tokens: true,
            tools: true,
            shell: true,
            diffs: true,
            thinking: true,
            outcomes: true,
        };
        assert_eq!(full.score(), (7, 7));

        let partial = AdapterCapabilities {
            prompts: true,
            tokens: false,
            tools: false,
            shell: false,
            diffs: true,
            thinking: false,
            outcomes: false,
        };
        assert_eq!(partial.score(), (2, 7));
    }
}
