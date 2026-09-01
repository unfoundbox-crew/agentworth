//! Common SDK and trait abstractions for AgentWorth source discovery and trace adapters.

use agentworth_schema::AgentWorthTrace;
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Options passed into adapter discovery and scanning.
#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    /// Custom path overrides supplied by the user.
    pub custom_paths: Vec<PathBuf>,
    /// Force full re-parsing even if file fingerprint has not changed.
    pub force: bool,
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
        let path = path.into();
        let metadata = std::fs::metadata(&path)?;
        let file_size_bytes = metadata.len();
        let mtime_epoch_secs = metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let fingerprint = compute_fast_fingerprint(&path, file_size_bytes, mtime_epoch_secs)?;

        Ok(Self {
            path,
            adapter_name: adapter_name.into(),
            file_size_bytes,
            mtime_epoch_secs,
            fingerprint,
        })
    }
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
