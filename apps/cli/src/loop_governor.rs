//! The governor's host: the live state `crates/loop`'s pure decisions need, and the one call
//! that turns a hook event into an answer (docs/specs/governor.md, "The brake, v0.1.22").
//!
//! `crates/loop` knows no files, no clock and no database. This is where the meter gets its
//! transcript, the ledger gets its edits, the policy gets read off disk, and a `Halt` becomes a
//! row. It runs inside `LoopRuntime`, single-threaded, on the same blocking thread as `apply`.
//!
//! The first line of every path is `Policy::is_empty()`. A machine with no `policy.toml` pays
//! for nothing here: no transcript is opened, no ledger is built, no row is written.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use agentworth_loop::{
    is_verification_command, Action, Decision, EditLedger, GateOutput, GateRequest, HookEvent,
    HookEventName, Policy, Rates, Rule, SessionSpend, Suspension, TranscriptTail, TurnUsage,
};
use agentworth_storage::{GovernorEventRow, Storage, SuspensionRow, TurnUsageRow};
use chrono::Utc;

/// Tools whose call is an edit to a file, across both harnesses. `apply_patch` is Codex's, and
/// names its files inside the patch body rather than in a `file_path` field.
const EDIT_TOOLS: &[&str] = &["Edit", "Write", "MultiEdit", "NotebookEdit", "apply_patch"];

/// Tools that run a shell command. The name differs per harness; the command field does not.
const SHELL_TOOLS: &[&str] = &["Bash", "bash", "shell", "local_shell", "run_command"];

/// The most of a failing command's output the halt quotes back. One line, and a bounded one:
/// this text is read by the model and paid for per token.
const MAX_OUTPUT_LINE: usize = 200;

/// A repo's `policy.toml` merged over the home one, and what it was when that was read.
struct CachedPolicy {
    file: PathBuf,
    mtime: Option<SystemTime>,
    policy: Policy,
}

/// Everything the governor needs that is not in the index yet: the transcript cursor, the spend
/// so far, the edit ledger, and the policy files as they are on disk right now.
pub struct GovernorHost {
    storage: Arc<Storage>,
    home_file: PathBuf,
    /// Keyed by the event's `cwd`, since resolving a git top-level is a process spawn.
    by_cwd: HashMap<String, CachedPolicy>,
    meters: HashMap<String, TranscriptTail>,
    spends: HashMap<String, SessionSpend>,
    ledgers: HashMap<String, EditLedger>,
    /// `tool_use_id` -> the command it ran, so a `PostToolUse` knows what it is the result of.
    commands: HashMap<String, String>,
    /// The last tool call signature per session and how many times it has repeated.
    repeats: HashMap<String, (String, usize)>,
    /// The `tool_use_id`s whose edit is already in the ledger. The batch payload repeats every
    /// edit the `PreToolUse` already reported, and counting one edit twice halves the
    /// threshold the person wrote down.
    counted: HashMap<String, std::collections::HashSet<String>>,
}

impl GovernorHost {
    pub fn new(storage: Arc<Storage>) -> Self {
        let home_file = agentworth_storage::default_db_dir()
            .unwrap_or_else(|_| PathBuf::from(".agentworth"))
            .join("policy.toml");
        Self {
            storage,
            home_file,
            by_cwd: HashMap::new(),
            meters: HashMap::new(),
            spends: HashMap::new(),
            ledgers: HashMap::new(),
            commands: HashMap::new(),
            repeats: HashMap::new(),
            counted: HashMap::new(),
        }
    }

    /// The policy in force for an event, home file plus this repo's override. Re-read only when
    /// the repo file's mtime moves, so the common case is a map lookup and one `stat`.
    pub fn policy_for(&mut self, cwd: Option<&str>) -> Policy {
        let key = cwd.unwrap_or_default().to_string();
        let file = match self.by_cwd.get(&key) {
            Some(cached) => cached.file.clone(),
            None => repo_policy_file(cwd),
        };
        let mtime = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
        if let Some(cached) = self.by_cwd.get(&key) {
            if cached.mtime == mtime {
                return cached.policy.clone();
            }
        }
        let policy = Policy::load(&self.home_file, &file).unwrap_or_else(|e| {
            tracing::warn!("governor: {e:#}");
            Policy::default()
        });
        self.by_cwd.insert(
            key,
            CachedPolicy {
                file,
                mtime,
                policy: policy.clone(),
            },
        );
        policy
    }

    /// Folds one hook event into the meter and the ledger. Called for every event `apply` sees,
    /// and the first thing it does is decide there is nothing to do.
    pub fn observe(&mut self, event: &HookEvent, seq: u64) {
        if self.policy_for(event.cwd.as_deref()).is_empty() {
            return;
        }
        self.seed(&event.session_id);
        self.meter(event);
        self.fold_event(event, seq);
    }

    /// One gate call, answered. Fails open on anything it cannot decide.
    pub fn decide(&mut self, request: &GateRequest, seq: u64) -> GateOutput {
        let event = &request.event;
        let policy = self.policy_for(event.cwd.as_deref());
        if policy.is_empty() {
            return GateOutput::allow();
        }
        self.seed(&event.session_id);
        // The batch carries the whole turn's tool calls, including ones no `PreToolUse` reached
        // this process -- a hook that missed the socket, or a restarted server. Each is counted
        // once: `tool_use_id` is the key both payloads share.
        for tool in request.batch_tools.clone() {
            let Some(name) = tool.get("tool_name").and_then(|n| n.as_str()) else {
                continue;
            };
            if !EDIT_TOOLS.contains(&name) {
                continue;
            }
            if let Some(id) = tool.get("tool_use_id").and_then(|i| i.as_str()) {
                if !self.mark_counted(&event.session_id, id) {
                    continue;
                }
            }
            for path in edited_paths(name, tool.get("tool_input"), event.cwd.as_deref()) {
                self.ledger_mut(&event.session_id).record_edit(&path, seq);
            }
        }

        let suspension = self
            .storage
            .active_suspension(&event.session_id)
            .ok()
            .flatten()
            .map(|row| Suspension {
                session_id: row.session_id,
                reason: row.reason,
                since: row.since,
            });

        match event.hook_event_name.as_str() {
            "UserPromptSubmit" => self.decide_prompt(event, suspension.as_ref()),
            "PreToolUse" => match suspension {
                Some(s) => GateOutput::pre_tool_use_deny(&s.reason),
                None => GateOutput::allow(),
            },
            "PostToolUse" => self.decide_post_tool_use(event, seq, &policy, suspension.as_ref()),
            // `PostToolBatch` and anything else batch-shaped: the one lever that stops the next
            // model call.
            _ => self.decide_batch(event, seq, &policy, suspension.as_ref()),
        }
    }

    fn decide_batch(
        &mut self,
        event: &HookEvent,
        seq: u64,
        policy: &Policy,
        suspension: Option<&Suspension>,
    ) -> GateOutput {
        let decisions = self.evaluate(event, policy, suspension);
        // A session over its cap gets the spend halt even when it is also thrashing: only the
        // spend halt suspends, and the thrash truth is still on the row it wrote.
        let halt = decisions
            .iter()
            .find(|d| d.action == Action::Halt && d.rule == Rule::Spend)
            .or_else(|| decisions.iter().find(|d| d.action == Action::Halt));
        if let Some(halt) = halt {
            self.record(event, seq, halt);
            if halt.rule == Rule::Spend && suspension.is_none() {
                let row = SuspensionRow {
                    session_id: event.session_id.clone(),
                    rule: halt.rule.as_str().to_string(),
                    reason: halt.reason.clone(),
                    since: Utc::now(),
                    lifted_at: None,
                };
                if let Err(e) = self.storage.suspend_session(&row) {
                    tracing::warn!("governor: could not suspend the session: {e:#}");
                }
            }
            return GateOutput::post_tool_batch_halt(&halt.reason, &halt.ground_truth);
        }
        match decisions.first() {
            Some(note) => {
                self.record(event, seq, note);
                GateOutput::post_tool_batch_note(&note.ground_truth)
            }
            None => GateOutput::allow(),
        }
    }

    /// Codex fires `PostToolUse` and has no batch hook, so this is where its brake lives:
    /// `decision: block` replaces the tool result the model sees. Claude Code's `PostToolUse`
    /// cannot block, so there it is context and nothing more.
    fn decide_post_tool_use(
        &mut self,
        event: &HookEvent,
        seq: u64,
        policy: &Policy,
        suspension: Option<&Suspension>,
    ) -> GateOutput {
        let decisions = self.evaluate(event, policy, suspension);
        let Some(halt) = decisions.iter().find(|d| d.action == Action::Halt) else {
            return GateOutput::allow();
        };
        self.record(event, seq, halt);
        if is_codex(event) {
            GateOutput::post_tool_use_replace(&halt.reason, &halt.ground_truth)
        } else {
            GateOutput::post_tool_use_context(&halt.ground_truth)
        }
    }

    fn decide_prompt(&mut self, event: &HookEvent, suspension: Option<&Suspension>) -> GateOutput {
        if let Some(suspension) = suspension {
            let row = GovernorEventRow {
                id: None,
                session_id: event.session_id.clone(),
                at: Utc::now(),
                seq: None,
                rule: Rule::Spend.as_str().to_string(),
                action: "blocked_prompt".to_string(),
                reason: suspension.reason.clone(),
                evidence: serde_json::to_string(&serde_json::json!({
                    "since": suspension.since,
                    "lift": format!("archie policy lift {}", suspension.session_id),
                }))
                .ok(),
            };
            if let Err(e) = self.storage.insert_governor_event(&row) {
                tracing::warn!("governor: could not record the blocked prompt: {e:#}");
            }
            return GateOutput::user_prompt_block(&suspension.reason);
        }
        // The halt ended the turn; the person is starting the next one. They get the same
        // evidence again, so the model resumes knowing why it stopped -- unless something has
        // passed since, which is exactly what the halt asked for.
        let Some(last) = self
            .storage
            .governor_events_for_session(&event.session_id, 1)
            .ok()
            .and_then(|rows| rows.into_iter().next())
        else {
            return GateOutput::allow();
        };
        if last.rule != Rule::Thrash.as_str() || last.action != Action::Halt.as_str() {
            return GateOutput::allow();
        }
        let passed_since = self
            .ledgers
            .get(&event.session_id)
            .and_then(EditLedger::last_pass_seq)
            .is_some_and(|pass| Some(pass as i64) > last.seq);
        if passed_since {
            return GateOutput::allow();
        }
        match last
            .evidence
            .as_deref()
            .and_then(|e| serde_json::from_str::<serde_json::Value>(e).ok())
            .and_then(|e| {
                e.get("ground_truth")
                    .and_then(|g| g.as_str())
                    .map(str::to_string)
            }) {
            Some(truth) => GateOutput::user_prompt_context(&truth),
            None => GateOutput::user_prompt_context(&last.reason),
        }
    }

    fn evaluate(
        &mut self,
        event: &HookEvent,
        policy: &Policy,
        suspension: Option<&Suspension>,
    ) -> Vec<Decision> {
        let spend = self
            .spends
            .get(&event.session_id)
            .cloned()
            .unwrap_or_default();
        let repeats = self
            .repeats
            .get(&event.session_id)
            .map(|(_, count)| *count)
            .unwrap_or(0);
        let ledger = self.ledger_mut(&event.session_id);
        agentworth_loop::Governor::evaluate(policy, ledger, &spend, repeats, suspension)
    }

    fn record(&self, event: &HookEvent, seq: u64, decision: &Decision) {
        let mut evidence = decision.evidence.clone();
        if let Some(object) = evidence.as_object_mut() {
            object.insert(
                "ground_truth".to_string(),
                serde_json::Value::String(decision.ground_truth.clone()),
            );
        }
        let row = GovernorEventRow {
            id: None,
            session_id: event.session_id.clone(),
            at: Utc::now(),
            seq: Some(seq as i64),
            rule: decision.rule.as_str().to_string(),
            action: decision.action.as_str().to_string(),
            reason: decision.reason.clone(),
            evidence: serde_json::to_string(&evidence).ok(),
        };
        if let Err(e) = self.storage.insert_governor_event(&row) {
            tracing::warn!("governor: could not record a decision: {e:#}");
        }
    }

    /// Puts a session's spend and ledger back where storage left them, once. Without it a
    /// restarted `serve` would meet a session mid-flight with a spend of zero and no history of
    /// what it has been editing.
    fn seed(&mut self, session_id: &str) {
        if self.spends.contains_key(session_id) {
            return;
        }
        let turns: Vec<TurnUsage> = self
            .storage
            .turn_usage_for_session(session_id, None)
            .unwrap_or_default()
            .iter()
            .map(turn_from_row)
            .collect();
        self.spends
            .insert(session_id.to_string(), SessionSpend::from_turns(&turns));

        let mut ledger = EditLedger::new();
        for intent in self
            .storage
            .intents_for_session(session_id)
            .unwrap_or_default()
        {
            let seq = intent.seq.max(0) as u64;
            if EDIT_TOOLS.contains(&intent.tool.as_str()) {
                for path in &intent.predicted_paths {
                    ledger.record_edit(path, seq);
                }
                continue;
            }
            let Some(command) = intent.command.as_deref() else {
                continue;
            };
            if !is_verification_command(command) {
                continue;
            }
            // `result` is the loop's own correlation: `ok` when the `PostToolUse` came back
            // clean, `error` when it did not, and `None` while the call is still in flight.
            match intent.result.as_deref() {
                Some("ok") => ledger.record_verification(None, true, seq, command, ""),
                Some("error") => ledger.record_verification(None, false, seq, command, ""),
                _ => {}
            }
        }
        self.ledgers.insert(session_id.to_string(), ledger);
    }

    fn ledger_mut(&mut self, session_id: &str) -> &mut EditLedger {
        self.ledgers.entry(session_id.to_string()).or_default()
    }

    /// Reads whatever the transcript has grown by since the last event, prices it, and stores
    /// both the turns and the cursor.
    fn meter(&mut self, event: &HookEvent) {
        let Some(path) = event.transcript_path.as_deref().filter(|p| !p.is_empty()) else {
            return;
        };
        let session_id = event.session_id.clone();
        let tail = match self.meters.get_mut(&session_id) {
            Some(tail) if tail.path == Path::new(path) => tail,
            _ => {
                let mut tail = TranscriptTail::new(path);
                if let Ok(Some((stored, offset))) = self.storage.get_transcript_cursor(&session_id)
                {
                    if stored == path {
                        tail.offset = offset;
                    }
                }
                self.meters.entry(session_id.clone()).or_insert(tail)
            }
        };
        let turns = match tail.read_new(&rates_for) {
            Ok(turns) => turns,
            Err(e) => {
                tracing::debug!("governor: could not read the transcript: {e:#}");
                return;
            }
        };
        let offset = tail.offset;
        if turns.is_empty() {
            return;
        }
        let rows: Vec<TurnUsageRow> = turns
            .iter()
            .map(|turn| TurnUsageRow {
                session_id: session_id.clone(),
                seq: turn.seq as i64,
                at: turn.at,
                model: turn.model.clone(),
                input: Some(turn.input as i64),
                output: Some(turn.output as i64),
                cache_read: Some(turn.cache_read as i64),
                cache_creation: Some(turn.cache_creation as i64),
                usd: turn.usd,
            })
            .collect();
        if let Err(e) = self.storage.insert_turn_usage(&rows) {
            tracing::warn!("governor: could not record turn usage: {e:#}");
        }
        if let Err(e) = self
            .storage
            .set_transcript_cursor(&session_id, path, offset)
        {
            tracing::warn!("governor: could not stamp the transcript cursor: {e:#}");
        }
        let spend = self.spends.entry(session_id).or_default();
        for turn in &turns {
            spend.add(turn);
        }
    }

    fn fold_event(&mut self, event: &HookEvent, seq: u64) {
        let tool = event.tool_name.clone().unwrap_or_default();
        match event.hook_event_name {
            HookEventName::PreToolUse => {
                self.bump_repeats(event, &tool);
                if let (Some(id), Some(command)) = (event.tool_use_id.clone(), command_of(event)) {
                    self.commands.insert(id, command);
                }
                if EDIT_TOOLS.contains(&tool.as_str()) {
                    let fresh = match event.tool_use_id.as_deref() {
                        Some(id) => self.mark_counted(&event.session_id, id),
                        None => true,
                    };
                    if fresh {
                        for path in
                            edited_paths(&tool, event.tool_input.as_ref(), event.cwd.as_deref())
                        {
                            self.ledger_mut(&event.session_id).record_edit(&path, seq);
                        }
                    }
                }
            }
            HookEventName::PostToolUse | HookEventName::PostToolUseFailure => {
                if !SHELL_TOOLS.contains(&tool.as_str()) {
                    return;
                }
                let command = command_of(event)
                    .or_else(|| {
                        event
                            .tool_use_id
                            .as_ref()
                            .and_then(|id| self.commands.get(id).cloned())
                    })
                    .unwrap_or_default();
                if command.is_empty() || !is_verification_command(&command) {
                    return;
                }
                let passed = passed(event);
                let line = last_output_line(event);
                self.ledger_mut(&event.session_id)
                    .record_verification(None, passed, seq, &command, &line);
            }
            _ => {}
        }
    }

    /// `true` the first time this session sees a `tool_use_id`, `false` every time after.
    fn mark_counted(&mut self, session_id: &str, tool_use_id: &str) -> bool {
        self.counted
            .entry(session_id.to_string())
            .or_default()
            .insert(tool_use_id.to_string())
    }

    fn bump_repeats(&mut self, event: &HookEvent, tool: &str) {
        let signature = format!(
            "{tool}\u{1}{}",
            event
                .tool_input
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default()
        );
        let entry = self
            .repeats
            .entry(event.session_id.clone())
            .or_insert_with(|| (signature.clone(), 0));
        if entry.0 == signature {
            entry.1 += 1;
        } else {
            *entry = (signature, 1);
        }
    }
}

/// The pricing table, in the shape `crates/loop` asks for. That crate holds no prices of its
/// own on purpose: the table is one fact, in one place.
pub fn rates_for(model: &str) -> Rates {
    let rates = agentworth_storage::get_model_rates(model);
    Rates {
        input_per_mtok: rates.input_per_m,
        output_per_mtok: rates.output_per_m,
        cache_read_per_mtok: rates.cache_read_per_m,
        cache_write_per_mtok: rates.cache_write_per_m,
    }
}

/// A stored row read back as the turn it was written from.
pub fn turn_from_row(row: &TurnUsageRow) -> TurnUsage {
    TurnUsage {
        seq: row.seq.max(0) as u64,
        at: row.at,
        model: row.model.clone(),
        input: row.input.unwrap_or(0).max(0) as u64,
        output: row.output.unwrap_or(0).max(0) as u64,
        cache_read: row.cache_read.unwrap_or(0).max(0) as u64,
        cache_creation: row.cache_creation.unwrap_or(0).max(0) as u64,
        usd: row.usd,
    }
}

/// `<repo>/.agentworth/policy.toml`, where the repo is the checkout the agent is standing in.
/// A cwd that is not a checkout is its own repo for this purpose.
pub fn repo_policy_file(cwd: Option<&str>) -> PathBuf {
    let Some(cwd) = cwd.filter(|c| !c.is_empty()) else {
        return PathBuf::from("/nonexistent/.agentworth/policy.toml");
    };
    let root = std::process::Command::new("git")
        .args(["-C", cwd, "rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|out| PathBuf::from(out.trim()))
        .filter(|root| root.is_dir())
        .unwrap_or_else(|| PathBuf::from(cwd));
    root.join(".agentworth").join("policy.toml")
}

/// Codex writes `turn_id` on every hook payload and names its transcripts `rollout-*.jsonl`;
/// Claude Code does neither. The difference decides which lever the gate reaches for.
fn is_codex(event: &HookEvent) -> bool {
    if event.raw.get("turn_id").is_some() {
        return true;
    }
    event
        .transcript_path
        .as_deref()
        .and_then(|p| Path::new(p).file_name())
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("rollout-"))
}

fn command_of(event: &HookEvent) -> Option<String> {
    event
        .tool_input
        .as_ref()?
        .get("command")
        .and_then(|c| match c {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Array(parts) => Some(
                parts
                    .iter()
                    .filter_map(|p| p.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            _ => None,
        })
        .filter(|c| !c.is_empty())
}

/// Passed means the harness reported no error and no non-zero exit. Anything ambiguous counts
/// as not passing: a halt that fires one edit late is recoverable, one that never fires is not.
fn passed(event: &HookEvent) -> bool {
    if event.hook_event_name == HookEventName::PostToolUseFailure || event.tool_error.is_some() {
        return false;
    }
    let Some(response) = event.tool_response.as_ref() else {
        return true;
    };
    if response
        .get("is_error")
        .or_else(|| response.get("isError"))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return false;
    }
    let exit = response
        .get("exit_code")
        .or_else(|| response.get("exitCode"))
        .and_then(serde_json::Value::as_i64);
    !matches!(exit, Some(code) if code != 0)
}

/// The last line with anything on it. A structured tool result carries the output in a field
/// rather than as its whole self, and quoting the serialised object at the model instead of the
/// line that says what broke is the difference between evidence and noise.
fn last_output_line(event: &HookEvent) -> String {
    let text = match event.tool_response.as_ref() {
        Some(serde_json::Value::Object(fields)) => ["stdout", "output", "content", "stderr"]
            .iter()
            .filter_map(|key| fields.get(*key).and_then(|v| v.as_str()))
            .find(|text| !text.trim().is_empty())
            .map(str::to_string)
            .or_else(|| event.tool_response_text()),
        _ => event.tool_response_text(),
    };
    let Some(text) = text else {
        return String::new();
    };
    let line = text
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    agentworth_schema::text::truncate_chars(line, MAX_OUTPUT_LINE).to_string()
}

/// The files one edit call touches, absolute. `apply_patch` names them inside the patch body
/// (`*** Update File: <path>`), which is the only place Codex puts them.
fn edited_paths(
    tool: &str,
    input: Option<&serde_json::Value>,
    cwd: Option<&str>,
) -> Vec<String> {
    let Some(input) = input else {
        return Vec::new();
    };
    let mut paths: Vec<String> = Vec::new();
    if tool == "apply_patch" {
        let patch = input
            .get("input")
            .or_else(|| input.get("patch"))
            .and_then(|p| p.as_str())
            .unwrap_or_default();
        for line in patch.lines() {
            let line = line.trim();
            for marker in ["*** Update File:", "*** Add File:", "*** Delete File:"] {
                if let Some(rest) = line.strip_prefix(marker) {
                    paths.push(rest.trim().to_string());
                }
            }
        }
    } else {
        let mut prediction = agentworth_loop::predicted_writes(tool, Some(input));
        paths.append(
            &mut prediction
                .paths
                .drain(..)
                .map(|p| p.to_string_lossy().to_string())
                .collect::<Vec<_>>(),
        );
    }
    match cwd {
        Some(cwd) => paths
            .into_iter()
            .map(|path| {
                agentworth_loop::absolutise(Path::new(cwd), Path::new(&path))
                    .to_string_lossy()
                    .to_string()
            })
            .collect(),
        None => paths,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap as Map;

    fn event(value: serde_json::Value) -> HookEvent {
        HookEvent::from_stdin_json(value, &Map::new()).expect("parses")
    }

    #[test]
    fn an_apply_patch_call_names_the_files_inside_its_own_patch() {
        let input = serde_json::json!({
            "input": "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-a\n+b\n*** End Patch\n"
        });
        let paths = edited_paths("apply_patch", Some(&input), Some("/repo"));
        assert_eq!(paths, vec!["/repo/src/lib.rs".to_string()]);
    }

    #[test]
    fn a_failing_shell_result_does_not_pass_however_the_harness_says_so() {
        let ok = event(serde_json::json!({
            "session_id": "s", "hook_event_name": "PostToolUse", "tool_response": "all good"
        }));
        assert!(passed(&ok));

        for failing in [
            serde_json::json!({"session_id": "s", "hook_event_name": "PostToolUse",
                "tool_response": {"is_error": true, "stdout": "test result: FAILED"}}),
            serde_json::json!({"session_id": "s", "hook_event_name": "PostToolUse",
                "tool_response": {"exit_code": 101}}),
            serde_json::json!({"session_id": "s", "hook_event_name": "PostToolUseFailure",
                "tool_response": "boom"}),
        ] {
            assert!(!passed(&event(failing)));
        }
    }

    #[test]
    fn the_quoted_line_is_the_last_one_with_anything_on_it_and_is_capped() {
        let e = event(serde_json::json!({
            "session_id": "s", "hook_event_name": "PostToolUse",
            "tool_response": "running 4 tests\ntest result: FAILED. 1 failed\n\n  \n"
        }));
        assert_eq!(last_output_line(&e), "test result: FAILED. 1 failed");

        let long = event(serde_json::json!({
            "session_id": "s", "hook_event_name": "PostToolUse",
            "tool_response": "x".repeat(500)
        }));
        assert_eq!(last_output_line(&long).chars().count(), MAX_OUTPUT_LINE);
    }

    #[test]
    fn codex_is_told_from_claude_code_by_its_own_fields() {
        assert!(is_codex(&event(serde_json::json!({
            "session_id": "s", "hook_event_name": "PreToolUse", "turn_id": "t1", "model": "gpt-5.3-codex"
        }))));
        assert!(is_codex(&event(serde_json::json!({
            "session_id": "s", "hook_event_name": "PreToolUse",
            "transcript_path": "/x/rollout-2026-09-06.jsonl"
        }))));
        assert!(!is_codex(&event(serde_json::json!({
            "session_id": "s", "hook_event_name": "PreToolUse",
            "transcript_path": "/x/8f2a.jsonl"
        }))));
    }

    #[test]
    fn nothing_is_governed_and_nothing_is_read_without_a_policy_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let storage = Arc::new(agentworth_storage::Storage::open_in_memory().expect("storage"));
        let mut host = GovernorHost::new(storage);
        host.home_file = dir.path().join("policy.toml");
        let cwd = dir.path().to_string_lossy().to_string();
        assert!(host.policy_for(Some(&cwd)).is_empty());

        let request = agentworth_loop::GateRequest::new(event(serde_json::json!({
            "session_id": "s", "hook_event_name": "PostToolBatch", "cwd": cwd,
        })));
        assert_eq!(host.decide(&request, 1), GateOutput::allow());
    }

    #[test]
    fn a_rewritten_policy_file_is_re_read_and_a_stable_one_is_not() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("policy.toml");
        std::fs::write(&home, "[thrash]\nedits = 3\n").expect("write");
        let storage = Arc::new(agentworth_storage::Storage::open_in_memory().expect("storage"));
        let mut host = GovernorHost::new(storage);
        host.home_file = home.clone();
        let cwd = dir.path().to_string_lossy().to_string();

        let first = host.policy_for(Some(&cwd));
        assert_eq!(first.thrash.expect("thrash").edits, 3);

        let repo_dir = dir.path().join(".agentworth");
        std::fs::create_dir_all(&repo_dir).expect("mkdir");
        std::fs::write(repo_dir.join("policy.toml"), "[thrash]\nedits = 9\n").expect("write");
        assert_eq!(
            host.policy_for(Some(&cwd)).thrash.expect("thrash").edits,
            9,
            "a repo file that appeared is picked up"
        );
    }
}
