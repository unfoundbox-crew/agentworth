//! The offline path: one JSONL file per session, appended to when nothing is listening.
//!
//! `docs/specs/loop.md` puts the spool on the critical path, not beside it. The hook connects to
//! the socket with a small budget; when there is no socket, or no time, it appends here and exits
//! zero. The agent is never slowed and never sees an error, and `archie scan` ingests the spool
//! later like any other raw history.
//!
//! Reading is forgiving on purpose. A line half-written when a machine lost power is skipped and
//! counted, because losing one event is cheaper than refusing to read the session.

use crate::hook::HookEvent;
use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub struct SpoolWriter;

impl SpoolWriter {
    /// Appends one event to `<dir>/<session_id>.jsonl`, creating the directory if needed.
    pub fn append(dir: &Path, event: &HookEvent) -> Result<PathBuf> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating spool directory {}", dir.display()))?;
        let path = dir.join(format!("{}.jsonl", sanitise(&event.session_id)));
        let mut line = serde_json::to_string(event).context("serialising a hook event")?;
        line.push('\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening spool file {}", path.display()))?;
        file.write_all(line.as_bytes())
            .with_context(|| format!("appending to {}", path.display()))?;
        Ok(path)
    }
}

/// What one read of a spool directory found. `skipped` is the count of lines that would not
/// parse -- reported, never silently dropped.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SpoolRead {
    pub events: Vec<HookEvent>,
    pub skipped: usize,
}

pub struct SpoolReader;

impl SpoolReader {
    /// Every event in `dir`, in file-name then line order. A missing directory reads as empty.
    pub fn read_dir(dir: &Path) -> Result<SpoolRead> {
        let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(std::result::Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
                .collect(),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SpoolRead::default())
            }
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("reading spool directory {}", dir.display()))
            }
        };
        files.sort();

        let mut read = SpoolRead::default();
        for path in files {
            let file = File::open(&path).with_context(|| format!("opening {}", path.display()))?;
            for line in BufReader::new(file).lines() {
                let line = line.with_context(|| format!("reading {}", path.display()))?;
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<HookEvent>(&line) {
                    Ok(event) => read.events.push(event),
                    Err(_) => read.skipped += 1,
                }
            }
        }
        Ok(read)
    }
}

/// A session id reaches the filesystem as a file name, so anything that is not obviously safe in
/// one becomes an underscore. Ids are uuids in practice; this is for the day one is not.
fn sanitise(session_id: &str) -> String {
    let cleaned: String = session_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "unknown".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook::HookEventName;
    use std::collections::HashMap;

    fn event(session: &str, name: &str) -> HookEvent {
        HookEvent::from_stdin_json(
            serde_json::json!({"session_id": session, "hook_event_name": name, "cwd": "/tmp"}),
            &HashMap::new(),
        )
        .expect("parses")
    }

    #[test]
    fn events_round_trip_through_the_spool_in_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        SpoolWriter::append(dir.path(), &event("s1", "SessionStart")).expect("append");
        SpoolWriter::append(dir.path(), &event("s1", "UserPromptSubmit")).expect("append");
        SpoolWriter::append(dir.path(), &event("s2", "SessionStart")).expect("append");

        let read = SpoolReader::read_dir(dir.path()).expect("read");
        assert_eq!(read.skipped, 0);
        let names: Vec<&str> = read
            .events
            .iter()
            .map(|e| e.hook_event_name.as_str())
            .collect();
        assert_eq!(names, ["SessionStart", "UserPromptSubmit", "SessionStart"]);
        assert_eq!(read.events[2].session_id, "s2");
        assert_eq!(read.events[0].hook_event_name, HookEventName::SessionStart);
        assert_eq!(read.events[0].cwd.as_deref(), Some("/tmp"));
    }

    #[test]
    fn a_half_written_line_is_skipped_and_counted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = SpoolWriter::append(dir.path(), &event("s1", "SessionStart")).expect("append");
        let mut file = OpenOptions::new().append(true).open(&path).expect("open");
        file.write_all(b"{\"session_id\": \"s1\", \"hook_ev\n")
            .expect("write");
        SpoolWriter::append(dir.path(), &event("s1", "Stop")).expect("append");

        let read = SpoolReader::read_dir(dir.path()).expect("read");
        assert_eq!(read.skipped, 1);
        assert_eq!(read.events.len(), 2);
        assert_eq!(read.events[1].hook_event_name, HookEventName::Stop);
    }

    #[test]
    fn a_directory_that_does_not_exist_reads_as_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let read = SpoolReader::read_dir(&dir.path().join("nothing-here")).expect("read");
        assert_eq!(read, SpoolRead::default());
    }

    #[test]
    fn a_session_id_that_is_not_a_uuid_still_lands_in_one_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = SpoolWriter::append(dir.path(), &event("../escape/me", "Stop")).expect("append");
        assert_eq!(path.parent(), Some(dir.path()));
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("___escape_me.jsonl")
        );
    }
}
