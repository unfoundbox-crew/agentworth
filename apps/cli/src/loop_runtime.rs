//! The loop's sync core: hook events in, `agent_state` / `tool_intents` / `support_set` /
//! `trace_anchors` rows out (docs/specs/loop.md section 1).
//!
//! Everything here is synchronous and single-threaded by design. The socket task in
//! `crate::server::loop_socket` hands each line over through `spawn_blocking`, `archie scan`
//! drains the spool through the same type at the end of a scan, and neither path needs a
//! second copy of the rules.
//!
//! Two numbers matter, and they answer different questions. `seq` is one session's own event
//! counter, persisted in `agent_state.last_seq`, and it orders that session against itself.
//! Across sessions it means nothing -- "my seq 5" and "your seq 2" are not comparable -- so
//! every cross-session question ("who wrote this after I read it") goes through the event's
//! own timestamp instead.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agentworth_loop::{
    absolutise, classify, extract_anchors, hash_paths_for_anchors, observe_checkout,
    support_from_read, AgentState, Anchor, HookEvent, HookEventName, Intent, LoopState,
    SessionLive, SpoolReader,
};
use agentworth_schema::MachineInfo;
use agentworth_storage::{AgentStateRow, AnchorRow, IntentRow, MachineRow, Storage, SupportRow};
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// How many events may pass without persisting a session's state row when nothing transitioned.
/// A transition always persists immediately; this is the ceiling on how stale `last_seq` can be
/// if an agent works for a long time without stopping.
const PERSIST_EVERY: u64 = 20;

/// What the last `Stop` saw: the changed paths this session predicted, the ones it did not, and
/// for each of those the other session that did predict it. Stored as JSON in
/// `agent_state.last_stop`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StopReport {
    pub at: Option<chrono::DateTime<Utc>>,
    /// This session's own sequence at the `Stop`, so a restarted process classifies only what
    /// happened after it. `#[serde(default)]` because rows written before this field existed
    /// read back as zero, which classifies more than it should rather than less.
    #[serde(default)]
    pub seq: u64,
    pub cwd: Option<String>,
    pub head: Option<String>,
    pub own: Vec<String>,
    pub world: Vec<String>,
    /// `path -> (session_id, seq)` for the world paths another session on this machine
    /// predicted. A world path absent from this map is "not an agent on this machine".
    pub world_writers: HashMap<String, (String, i64)>,
}

/// Folds hook events into the index. Holds the in-memory state machine so a running
/// `archie serve` answers without a query per event; every fact it reports is still a row.
pub struct LoopRuntime {
    storage: Arc<Storage>,
    state: LoopState,
    /// Events applied since this session's state row was last written.
    unpersisted: HashMap<String, u64>,
    /// The `seq` of this session's previous `Stop`, so a `Stop` classifies only what happened
    /// since the last one.
    last_stop_seq: HashMap<String, u64>,
    /// The same boundary in wall-clock time, for the one question `seq` cannot answer: which
    /// *other* session wrote a path, and whether it did so after this session's last `Stop`.
    last_stop_at: HashMap<String, chrono::DateTime<Utc>>,
}

impl LoopRuntime {
    pub fn new(storage: Arc<Storage>) -> Self {
        Self {
            storage,
            state: LoopState::new(),
            unpersisted: HashMap::new(),
            last_stop_seq: HashMap::new(),
            last_stop_at: HashMap::new(),
        }
    }

    /// Applies one event. Errors are returned, never propagated to the agent: every caller
    /// logs and carries on, because a hook that fails must not become an agent's problem.
    pub fn apply(&mut self, event: HookEvent) -> Result<()> {
        self.rehydrate(&event.session_id);
        let transition = self.state.apply(&event);
        let session_id = event.session_id.clone();
        let seq = self
            .state
            .get(&session_id)
            .map(|live| live.last_seq)
            .unwrap_or(0);

        if event.hook_event_name == HookEventName::SessionStart && !event.is_subagent() {
            self.on_session_start(&session_id);
        }

        // A subagent bumps its parent's sequence and does nothing else (docs/specs/loop.md
        // section 1): its edits are its own, and attributing them to the parent would make the
        // parent's own `Stop` classification wrong in both directions.
        if !event.is_subagent() {
            match event.hook_event_name {
                HookEventName::PreToolUse => self.on_pre_tool_use(&event, seq)?,
                HookEventName::PostToolUse => self.on_post_tool_use(&event, seq)?,
                HookEventName::PostToolUseFailure => {
                    if let Some(tool_use_id) = &event.tool_use_id {
                        self.storage.set_intent_result(tool_use_id, "error")?;
                    }
                }
                HookEventName::Stop => self.on_stop(&event, seq)?,
                _ => {}
            }
        }

        let counter = self.unpersisted.entry(session_id.clone()).or_insert(0);
        *counter += 1;
        if transition.is_some() || *counter >= PERSIST_EVERY {
            *counter = 0;
            self.persist_state(&session_id, None)?;
        }
        Ok(())
    }

    /// Drains `dir`: every spool file is renamed aside, read, applied, and only then deleted.
    ///
    /// The rename is the whole point. `archie hook` appends to `<session>.jsonl` and a drain
    /// that read the file, applied it, and deleted the path would lose every line a hook wrote
    /// in between -- and hooks fire while the scan runs. A rename is atomic within the
    /// directory, so the writer's next append recreates the original name and is picked up by
    /// the next drain. A file whose events did not all apply is left as `.ingesting` rather
    /// than deleted: late is recoverable, gone is not.
    pub fn ingest_spool(&mut self, dir: &Path) -> Result<SpoolIngest> {
        let mut ingest = SpoolIngest::default();
        for path in SpoolReader::files(dir)? {
            let claimed = path.with_extension(format!("jsonl.{}.ingesting", std::process::id()));
            if std::fs::rename(&path, &claimed).is_err() {
                // Another drain took it, or it vanished. Either way it is not ours.
                continue;
            }
            let read = SpoolReader::read_file(&claimed)?;
            ingest.events += read.events.len();
            ingest.skipped_lines += read.skipped;
            let mut all_applied = true;
            for event in read.events {
                if let Err(e) = self.apply(event) {
                    all_applied = false;
                    tracing::warn!("loop: a spooled event could not be applied: {e:#}");
                }
            }
            if all_applied {
                if std::fs::remove_file(&claimed).is_ok() {
                    ingest.files += 1;
                }
            } else {
                tracing::warn!(
                    "loop: {} kept -- not every event in it applied",
                    claimed.display()
                );
            }
        }
        Ok(ingest)
    }

    /// Puts a session back where storage left it, once, the first time this process sees it.
    ///
    /// Without this every restart -- a `serve` that came back, or the `archie scan` that
    /// drains the spool -- would start the session's sequence again at one and re-classify
    /// every path it ever predicted at the next `Stop`.
    fn rehydrate(&mut self, session_id: &str) {
        if self.state.get(session_id).is_some() {
            return;
        }
        let Ok(Some(row)) = self.storage.get_agent_state(session_id) else {
            return;
        };
        let Some(state) = AgentState::from_label(&row.state) else {
            return;
        };
        self.state.seed(SessionLive {
            session_id: row.session_id.clone(),
            state,
            since: row.since,
            pane_id: row.pane_id.clone(),
            cwd: row.cwd.clone(),
            last_seq: row.last_seq.max(0) as u64,
            updated_at: row.updated_at,
        });
        if let Ok(Some(json)) = self.storage.get_agent_last_stop(session_id) {
            if let Ok(report) = serde_json::from_str::<StopReport>(&json) {
                self.last_stop_seq.insert(session_id.to_string(), report.seq);
                if let Some(at) = report.at {
                    self.last_stop_at.insert(session_id.to_string(), at);
                }
            }
        }
    }

    fn on_session_start(&mut self, session_id: &str) {
        let machine = MachineInfo::probe();
        let Some(fingerprint) = machine.host_fingerprint.clone() else {
            return;
        };
        let row = MachineRow {
            host_fingerprint: fingerprint.clone(),
            system_id: None,
            os: Some(machine.os),
            arch: Some(machine.arch),
            first_seen: Utc::now(),
        };
        if let Err(e) = self.storage.upsert_machine(&row) {
            tracing::warn!("loop: could not record this machine: {e:#}");
        }
        // The session row is written by a scan, which may not have happened yet -- this is an
        // UPDATE, so it is a no-op until then and the fingerprint below is the copy that
        // survives either way.
        if let Err(e) = self.storage.set_session_host(session_id, &fingerprint) {
            tracing::warn!("loop: could not stamp the session's machine: {e:#}");
        }
    }

    fn on_pre_tool_use(&mut self, event: &HookEvent, seq: u64) -> Result<()> {
        let Some(intent) = Intent::from_pre_tool_use(event, seq) else {
            return Ok(());
        };
        // No `tool_use_id` means no key to fill the result in against later, so the intent is
        // not recorded rather than recorded under an invented one.
        let Some(tool_use_id) = intent.tool_use_id.clone() else {
            return Ok(());
        };
        let paths: Vec<String> = intent
            .prediction
            .paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        let row = IntentRow {
            tool_use_id,
            session_id: intent.session_id.clone(),
            seq: seq as i64,
            tool: intent.tool.clone(),
            predicted_paths: paths.clone(),
            command: intent.prediction.command.clone(),
            known: intent.prediction.known,
            at: intent.at,
            result: None,
        };
        self.storage.insert_intent(&row, &paths)?;
        Ok(())
    }

    fn on_post_tool_use(&mut self, event: &HookEvent, seq: u64) -> Result<()> {
        if let Some(tool_use_id) = &event.tool_use_id {
            self.storage.set_intent_result(tool_use_id, "ok")?;
        }

        if let Some(entry) = support_from_read(event, seq) {
            self.storage.upsert_support(&SupportRow {
                session_id: event.session_id.clone(),
                path: entry.path.to_string_lossy().to_string(),
                sha256: entry.sha256.clone(),
                size: Some(entry.size as i64),
                read_seq: seq as i64,
                read_at: entry.read_at,
            })?;
        }

        let mut anchors: Vec<Anchor> = Vec::new();
        if matches!(
            event.tool_name.as_deref(),
            Some("Write" | "Edit" | "MultiEdit" | "NotebookEdit")
        ) {
            let mut prediction = agentworth_loop::predicted_writes(
                event.tool_name.as_deref().unwrap_or_default(),
                event.tool_input.as_ref(),
            );
            if let Some(cwd) = event.cwd.as_deref() {
                let cwd = Path::new(cwd);
                prediction.paths = prediction
                    .paths
                    .iter()
                    .map(|path| absolutise(cwd, path))
                    .collect();
            }
            anchors.extend(hash_paths_for_anchors(&event.session_id, seq, &prediction));
        }
        if let Some(text) = event.tool_response_text() {
            anchors.extend(extract_anchors(&event.session_id, seq, &text));
        }
        self.storage.insert_anchors(&anchor_rows(&anchors))?;
        Ok(())
    }

    fn on_stop(&mut self, event: &HookEvent, seq: u64) -> Result<()> {
        let cwd = event
            .cwd
            .clone()
            .or_else(|| {
                self.state
                    .get(&event.session_id)
                    .and_then(|live| live.cwd.clone())
            })
            .map(PathBuf::from);
        let Some(cwd) = cwd else {
            return Ok(());
        };
        let since = self
            .last_stop_seq
            .get(&event.session_id)
            .copied()
            .unwrap_or(0);
        let since_at = self
            .last_stop_at
            .get(&event.session_id)
            .copied()
            .unwrap_or(chrono::DateTime::<Utc>::MIN_UTC);
        self.last_stop_seq.insert(event.session_id.clone(), seq);
        self.last_stop_at
            .insert(event.session_id.clone(), event.received_at);

        let observation = match observe_checkout(&cwd) {
            Ok(observation) => observation,
            // Not a checkout, no git, or a stalled mount: `Stop` still closes the loop, it just
            // has nothing to say about the working tree. The boundary is still recorded --
            // without it a restarted process would classify this session's whole history at
            // its next Stop.
            Err(e) => {
                tracing::debug!("loop: no checkout observation at Stop: {e:#}");
                self.persist_state(&event.session_id, None)?;
                let report = StopReport {
                    at: Some(event.received_at),
                    seq,
                    cwd: Some(cwd.to_string_lossy().to_string()),
                    ..StopReport::default()
                };
                self.storage
                    .set_agent_last_stop(&event.session_id, &serde_json::to_string(&report)?)?;
                return Ok(());
            }
        };

        let predicted = self
            .storage
            .intent_paths_for_session(&event.session_id, since as i64)?;
        let predicted_paths: Vec<PathBuf> = predicted.iter().map(PathBuf::from).collect();
        let classified = classify(&observation, &cwd, &predicted_paths);

        let mut world_writers = HashMap::new();
        for path in &classified.world {
            let key = path.to_string_lossy().to_string();
            let writers =
                self.storage
                    .writers_of_path_since(&key, since_at, Some(&event.session_id))?;
            if let Some((session, writer_seq)) = writers.into_iter().next() {
                world_writers.insert(key, (session, writer_seq));
            }
        }

        let report = StopReport {
            at: Some(event.received_at),
            seq,
            cwd: Some(cwd.to_string_lossy().to_string()),
            head: observation.head.clone(),
            own: classified
                .own
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
            world: classified
                .world
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
            world_writers,
        };

        self.persist_state(&event.session_id, observation.head.clone())?;
        self.storage
            .set_agent_last_stop(&event.session_id, &serde_json::to_string(&report)?)?;
        Ok(())
    }

    fn persist_state(&mut self, session_id: &str, git_head: Option<String>) -> Result<()> {
        let Some(live) = self.state.get(session_id) else {
            return Ok(());
        };
        // `git_head` is only known at `Stop`; every other write keeps whatever is already
        // stored rather than blanking it.
        let head = match git_head {
            Some(head) => Some(head),
            None => self
                .storage
                .get_agent_state(session_id)
                .ok()
                .flatten()
                .and_then(|row| row.git_head),
        };
        self.storage.upsert_agent_state(&AgentStateRow {
            session_id: live.session_id.clone(),
            state: live.state.as_str().to_string(),
            since: live.since,
            pane_id: live.pane_id.clone(),
            cwd: live.cwd.clone(),
            git_head: head,
            last_seq: live.last_seq as i64,
            updated_at: live.updated_at,
        })?;
        Ok(())
    }
}

/// What one spool drain found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpoolIngest {
    pub events: usize,
    pub skipped_lines: usize,
    pub files: usize,
}

fn anchor_rows(anchors: &[Anchor]) -> Vec<AnchorRow> {
    anchors
        .iter()
        .map(|anchor| AnchorRow {
            session_id: anchor.session_id.clone(),
            seq: anchor.seq as i64,
            kind: anchor.kind.as_str().to_string(),
            value: anchor.value.clone(),
        })
        .collect()
}

/// Drains the default spool directory into the index. Used by `archie scan`, where the loop
/// has no server to deliver through and the spool is the only path the events took.
pub fn ingest_default_spool(storage: Arc<Storage>) -> Result<SpoolIngest> {
    let dir = agentworth_storage::default_spool_dir()?;
    LoopRuntime::new(storage).ingest_spool(&dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_loop::SpoolWriter;
    use std::collections::HashMap as Map;

    fn storage() -> Arc<Storage> {
        Arc::new(Storage::open_in_memory().expect("open storage"))
    }

    fn event(session: &str, name: &str, extra: serde_json::Value) -> HookEvent {
        let mut value = serde_json::json!({
            "session_id": session,
            "hook_event_name": name,
        });
        if let (Some(object), Some(extra)) = (value.as_object_mut(), extra.as_object()) {
            for (key, item) in extra {
                object.insert(key.clone(), item.clone());
            }
        }
        HookEvent::from_stdin_json(value, &Map::new()).expect("parses")
    }

    /// The race the rename exists for: a hook appends while the drain is mid-file. The line
    /// written after the drain read the file must still be there afterwards.
    #[test]
    fn an_event_written_during_a_drain_survives_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spool = dir.path().join("spool");
        SpoolWriter::append(&spool, &event("s1", "SessionStart", serde_json::json!({})))
            .expect("append");

        let mut runtime = LoopRuntime::new(storage());
        // Stands in for the hook that fires between the rename and the delete: it recreates
        // the original path, which the drain has already moved aside.
        let during = event("s1", "UserPromptSubmit", serde_json::json!({}));
        let claimed = spool.join("s1.jsonl");
        std::fs::rename(&claimed, spool.join("s1.jsonl.test.ingesting")).expect("claim");
        SpoolWriter::append(&spool, &during).expect("append during");
        std::fs::rename(spool.join("s1.jsonl.test.ingesting"), &claimed).ok();

        let ingest = runtime.ingest_spool(&spool).expect("drain");
        assert_eq!(ingest.events, 1, "the drain took the file as it stood");
        assert!(
            std::fs::read_dir(&spool)
                .expect("spool")
                .filter_map(Result::ok)
                .all(|entry| entry.path().extension().is_some_and(|e| e != "jsonl")),
            "the drained file is gone"
        );

        // And the event written during the drain is still deliverable: it is a fresh file at
        // the original path, which the next drain picks up.
        SpoolWriter::append(&spool, &during).expect("append after");
        let second = runtime.ingest_spool(&spool).expect("second drain");
        assert_eq!(second.events, 1, "nothing was lost");
    }

    #[test]
    fn a_second_runtime_continues_the_sequence_and_the_stop_boundary() {
        let storage = storage();
        let cwd = tempfile::tempdir().expect("tempdir");
        let cwd_json = serde_json::json!({ "cwd": cwd.path() });

        let mut first = LoopRuntime::new(storage.clone());
        first
            .apply(event("s1", "SessionStart", cwd_json.clone()))
            .expect("start");
        first
            .apply(event("s1", "UserPromptSubmit", cwd_json.clone()))
            .expect("prompt");
        first.apply(event("s1", "Stop", cwd_json.clone())).expect("stop");
        let after_first = storage
            .get_agent_state("s1")
            .expect("read")
            .expect("a row")
            .last_seq;
        assert_eq!(after_first, 3);

        let mut second = LoopRuntime::new(storage.clone());
        second
            .apply(event("s1", "UserPromptSubmit", cwd_json))
            .expect("prompt again");
        let row = storage.get_agent_state("s1").expect("read").expect("a row");
        assert_eq!(row.last_seq, 4, "the sequence continued rather than restarting");
        assert_eq!(row.state, "working");

        let stop: StopReport = serde_json::from_str(
            &storage
                .get_agent_last_stop("s1")
                .expect("read")
                .expect("a stop"),
        )
        .expect("parses");
        assert_eq!(stop.seq, 3, "the Stop boundary is stored and read back");
    }

    /// A relative `file_path` is only meaningful next to the event's `cwd`. Stored relative, it
    /// would never match the path another session wrote.
    #[test]
    fn a_relative_tool_path_is_stored_absolute_and_still_finds_its_writer() {
        let storage = storage();
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("notes.md");
        std::fs::write(&file, "before\n").expect("write");

        let mut runtime = LoopRuntime::new(storage.clone());
        let reader = serde_json::json!({
            "cwd": dir.path(),
            "tool_name": "Read",
            "tool_use_id": "toolu_read",
            "tool_input": {"file_path": "notes.md"},
            "tool_response": "before",
        });
        runtime
            .apply(event("reader", "PostToolUse", reader))
            .expect("read");

        let support = storage.support_for_session("reader").expect("support");
        assert_eq!(support.len(), 1);
        assert_eq!(
            support[0].path,
            file.to_string_lossy(),
            "the stored path is absolute"
        );

        let writer = serde_json::json!({
            "cwd": dir.path(),
            "tool_name": "Edit",
            "tool_use_id": "toolu_edit",
            "tool_input": {"file_path": "notes.md"},
        });
        runtime
            .apply(event("writer", "PreToolUse", writer))
            .expect("edit");
        std::fs::write(&file, "after\n").expect("rewrite");

        let drift = crate::commands::loop_cmds::session_drift_json(&storage, "reader")
            .expect("drift");
        let drifted = drift["drift"].as_array().expect("a list");
        assert_eq!(drifted.len(), 1, "{drift}");
        assert_eq!(drifted[0]["writer"]["session_id"], "writer");
    }
}
