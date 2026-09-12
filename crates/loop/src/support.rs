//! The support set, and whether it moved under you.
//!
//! The support set is narrower than the read set: it is the paths this session actually `Read`,
//! hashed at `PostToolUse`. Drift re-hashes them now and returns the ones that changed. That is
//! the question no harness summary can answer -- "did my ground move, and was it me" -- and it is
//! answerable only because the loop recorded a hash at the moment the agent looked.
//!
//! Files over `MAX_HASH_BYTES` are carried with `sha256: None`. An unhashed file is listed as
//! unhashed; it is never guessed at from a size or a timestamp.

use crate::hook::{HookEvent, HookEventName};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

/// 4 MiB. `docs/specs/loop.md` sets the cap; above it the read cost outweighs the answer.
pub const MAX_HASH_BYTES: u64 = 4 * 1024 * 1024;

/// One path this session read, as it was when the session read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupportEntry {
    pub path: PathBuf,
    /// `None` for a file over the cap, or one that could not be read.
    pub sha256: Option<String>,
    pub size: u64,
    pub read_seq: u64,
    pub read_at: DateTime<Utc>,
}

/// The session that predicted a write to a path, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Writer {
    pub session_id: String,
    pub seq: u64,
}

/// One entry of the support set that no longer matches what the session read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drift {
    pub path: PathBuf,
    pub then: Option<String>,
    /// `None` when the file is gone.
    pub now: Option<String>,
    /// The session that moved it, when one predicted a write after `read_seq`. `None` means
    /// nothing in the loop claims this change -- not that nobody made it.
    pub writer: Option<Writer>,
}

/// The sha256 of a file, or `None` when it is larger than `max_bytes`.
pub fn hash_file(path: &Path, max_bytes: u64) -> Result<Option<String>> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("reading metadata for {}", path.display()))?;
    if !metadata.is_file() {
        return Ok(None);
    }
    if metadata.len() > max_bytes {
        return Ok(None);
    }
    let mut file =
        std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("reading {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(Some(hex::encode(hasher.finalize())))
}

/// The support-set entry a `PostToolUse` of `Read` implies, hashed now. Returns `None` for any
/// other event, or when the file has already gone.
pub fn support_from_read(event: &HookEvent, read_seq: u64) -> Option<SupportEntry> {
    if event.hook_event_name != HookEventName::PostToolUse {
        return None;
    }
    if event.tool_name.as_deref() != Some("Read") {
        return None;
    }
    let path = event
        .tool_input
        .as_ref()?
        .get("file_path")
        .and_then(serde_json::Value::as_str)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)?;
    // Same rule as an intent's predicted paths: what is stored is absolute, resolved against
    // the directory the read happened in, or drift compares two different files.
    let path = match event.cwd.as_deref() {
        Some(cwd) => crate::observe::absolutise(Path::new(cwd), &path),
        None => path,
    };
    let size = std::fs::metadata(&path).ok()?.len();
    Some(SupportEntry {
        path: path.clone(),
        sha256: hash_file(&path, MAX_HASH_BYTES).ok().flatten(),
        size,
        read_seq,
        read_at: event.received_at,
    })
}

/// Re-hashes the support set and returns everything that no longer matches.
///
/// `writers` is the loop's own record of predicted writes, injected rather than imported so this
/// crate stays free of storage. It is asked only about paths that actually drifted.
pub fn drift(entries: &[SupportEntry], writers: &dyn Fn(&Path) -> Option<Writer>) -> Vec<Drift> {
    let mut drifted = Vec::new();
    for entry in entries {
        let now = match std::fs::metadata(&entry.path) {
            Ok(_) => hash_file(&entry.path, MAX_HASH_BYTES).unwrap_or(None),
            Err(_) => None,
        };
        let gone = !entry.path.exists();
        if !gone && now == entry.sha256 {
            continue;
        }
        drifted.push(Drift {
            path: entry.path.clone(),
            then: entry.sha256.clone(),
            now,
            writer: writers(&entry.path),
        });
    }
    drifted
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn entry(path: &Path) -> SupportEntry {
        SupportEntry {
            path: path.to_path_buf(),
            sha256: hash_file(path, MAX_HASH_BYTES).expect("hashes"),
            size: std::fs::metadata(path).expect("metadata").len(),
            read_seq: 3,
            read_at: Utc::now(),
        }
    }

    fn no_writers(_: &Path) -> Option<Writer> {
        None
    }

    #[test]
    fn a_file_hashes_to_the_same_digest_openssl_would_print() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "hello\n").expect("write");
        assert_eq!(
            hash_file(&path, MAX_HASH_BYTES).expect("hashes").as_deref(),
            Some("5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03")
        );
    }

    #[test]
    fn a_file_over_the_cap_is_unhashed_and_never_guessed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("big.bin");
        std::fs::write(&path, vec![0u8; 2048]).expect("write");
        assert_eq!(hash_file(&path, 1024).expect("does not fail"), None);
    }

    #[test]
    fn an_untouched_support_set_has_no_drift() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "one\n").expect("write");
        assert!(drift(&[entry(&path)], &no_writers).is_empty());
    }

    #[test]
    fn a_rewritten_file_drifts_and_names_the_writer_the_callback_knows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "one\n").expect("write");
        let before = entry(&path);
        std::fs::write(&path, "two\n").expect("write");

        let anonymous = drift(std::slice::from_ref(&before), &no_writers);
        assert_eq!(anonymous.len(), 1);
        assert_eq!(anonymous[0].then, before.sha256);
        assert!(anonymous[0].now.is_some());
        assert_ne!(anonymous[0].now, anonymous[0].then);
        assert_eq!(anonymous[0].writer, None);

        let named = drift(std::slice::from_ref(&before), &|_| {
            Some(Writer {
                session_id: "s2".to_string(),
                seq: 9,
            })
        });
        assert_eq!(
            named[0].writer,
            Some(Writer {
                session_id: "s2".to_string(),
                seq: 9
            })
        );
    }

    #[test]
    fn a_deleted_file_drifts_to_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "one\n").expect("write");
        let before = entry(&path);
        std::fs::remove_file(&path).expect("remove");

        let gone = drift(&[before], &no_writers);
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].now, None);
        assert!(gone[0].then.is_some());
    }

    #[test]
    fn only_a_post_tool_use_of_read_joins_the_support_set() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "one\n").expect("write");

        let read = HookEvent::from_stdin_json(
            serde_json::json!({
                "session_id": "s1",
                "hook_event_name": "PostToolUse",
                "tool_name": "Read",
                "tool_input": {"file_path": path.to_str().expect("utf-8")}
            }),
            &HashMap::new(),
        )
        .expect("parses");
        let support = support_from_read(&read, 5).expect("an entry");
        assert_eq!(support.read_seq, 5);
        assert_eq!(support.size, 4);
        assert!(support.sha256.is_some());

        let pre = HookEvent::from_stdin_json(
            serde_json::json!({
                "session_id": "s1",
                "hook_event_name": "PreToolUse",
                "tool_name": "Read",
                "tool_input": {"file_path": path.to_str().expect("utf-8")}
            }),
            &HashMap::new(),
        )
        .expect("parses");
        assert!(support_from_read(&pre, 5).is_none());
    }
}
