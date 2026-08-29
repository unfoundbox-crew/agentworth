use serde::{Deserialize, Serialize};

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
