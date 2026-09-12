//! Herdr adapter.
//!
//! Herdr (herdr.dev) is a terminal workspace manager for coding agents, not an agent. It never
//! writes a transcript; the agents in its panes write their own, which the Claude Code, Codex
//! and Gemini adapters already index. What Herdr persists is one file,
//! `~/.config/herdr/session.json` (`"version": 3` as of herdr 0.8.2): the workspaces, tabs and
//! panes of the running session, and for every pane the agent Herdr recognized inside it plus
//! that agent's own session id.
//!
//! That id is what this adapter is for. `agent_session.value` is the harness session id, the
//! same string the Claude Code / Codex / Gemini adapters use as `session_id`, and it is the only
//! key. Everything else is display: pane labels and agent names are renamed freely
//! (`pane.rename` is among the most frequent calls in `herdr-server.log`), so each is recorded
//! with the time it was seen and never treated as identity. The runtime pane id the socket API
//! uses (`w9:pA`) is not in the file at all; it is a live allocation and is never stored.
//!
//! Shape, read from the real file and cross-checked against `herdr api schema --json`
//! (protocol 20: `AgentSessionInfo`, `AgentSessionRefKind`):
//!
//! ```text
//! { "version": 3,
//!   "workspaces": [ { "id": "w9", "custom_name": "fleet", "identity_cwd": "/abs/path",
//!       "public_pane_numbers": { "20": 18, ... },
//!       "tabs": [ { "custom_name": null, "layout": { ... }, "focused": 25, "root_pane": 12,
//!           "panes": { "25": { "cwd": "/abs/path", "label": "chief-gemini",
//!                              "agent_name": "chief", "managed_agent_kind": "agy",
//!                              "agent_session": { "source": "herdr:antigravity_cli",
//!                                                 "agent": "agy", "kind": "id",
//!                                                 "value": "<uuid>" } } } } ],
//!       "active_tab": 0 } ],
//!   "active": 0, "selected": 0, "sidebar_width": 26, ... }
//! ```
//!
//! `pane_key` is the map key (`"25"`), Herdr's internal number; `pane_number` is what the user
//! sees, from `public_pane_numbers`. Both are display.
//!
//! What this yields is a `TraceKind::FleetSnapshot`: the fleet as of one moment, one
//! `herdr.pane` event per pane, all stamped with the file's mtime. The file is replaced whole on
//! every save, so that is the one timestamp it has and it moves forward with every save; every
//! scan re-reads it (a few kilobytes) and the row is replaced. History survives the replacement
//! through `AgentWorthTrace::identities`: every (harness session id, name, time seen) the
//! snapshot carries goes to the append-only sightings table, so a rename adds a row and never
//! rewrites one. A file with no panes yields no events and is not indexed.
//!
//! Fields are decoded leniently. Missing or wrong-typed scalars degrade to `null`, unknown
//! fields are ignored, a newer `version` is a warning, and a workspace, tab, pane or
//! `agent_session` that does not decode costs only itself.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use agentworth_adapter_sdk::{
    AdapterCapabilities, AgentAdapter, DetectionResult, ParseResult, ScanOptions, SessionSource,
};
use agentworth_schema::{
    AgentWorthTrace, EventPayload, IdentitySighting, NormalizedEvent, Provenance, TraceKind,
};
use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use walkdir::WalkDir;

/// Adapter for Herdr workspace snapshots: which agent session ran in which pane.
pub struct HerdrAdapter;

impl Default for HerdrAdapter {
    fn default() -> Self {
        Self
    }
}

/// The `session.json` layout this adapter was written against (herdr 0.8.2).
const SESSION_FILE_VERSION: u64 = 3;

/// Event kind emitted once per pane.
const PANE_EVENT_KIND: &str = "herdr.pane";

impl HerdrAdapter {
    /// 2: reads the real `session.json` v3 shape. Version 1 looked for `coordination_traces`,
    /// `parent_agent_id` and `delegation_id`, fields no Herdr file has ever carried, and
    /// produced nothing; the bump makes an incremental scan re-read every file it had already
    /// fingerprinted.
    pub const PARSER_VERSION: i64 = 2;

    pub fn new() -> Self {
        Self
    }

    /// Candidate directory paths for Herdr on the host machine. `~/.config/herdr` is where
    /// herdr 0.8 writes; `~/.herdr` is kept for older layouts and hand-set config dirs.
    pub fn candidate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".config").join("herdr"));
            roots.push(home.join(".herdr"));
        }
        roots.push(PathBuf::from(".config").join("herdr"));
        roots.push(PathBuf::from(".herdr"));
        roots
    }
}

impl AgentAdapter for HerdrAdapter {
    fn name(&self) -> &'static str {
        "herdr"
    }

    fn parser_version(&self) -> i64 {
        Self::PARSER_VERSION
    }

    fn trace_kind(&self) -> TraceKind {
        TraceKind::FleetSnapshot
    }

    /// A snapshot carries no prompts, tokens, tools or outcomes. Claiming any of the seven
    /// would put a YES on the coverage page that nothing backs.
    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::default()
    }

    fn detect(&self, options: &ScanOptions) -> Result<DetectionResult> {
        let mut discovered = Vec::new();

        for root in self.candidate_roots() {
            if root.exists() {
                discovered.push(root);
            }
        }

        for custom in &options.custom_paths {
            if !custom.exists() {
                continue;
            }
            let s = custom.to_string_lossy().to_lowercase();
            if s.contains("herdr") {
                discovered.push(custom.clone());
            } else if custom.is_dir() {
                // custom_paths may point at a generic parent directory rather than the
                // adapter-specific dir itself; look a few levels in before giving up,
                // matching how `enumerate()` already recurses.
                let mut found_nested = false;
                for sub in &[custom.join(".herdr"), custom.join(".config").join("herdr")] {
                    if sub.exists() {
                        discovered.push(sub.clone());
                        found_nested = true;
                    }
                }
                if !found_nested {
                    for entry in WalkDir::new(custom)
                        .max_depth(4)
                        .into_iter()
                        .filter_map(|e| e.ok())
                    {
                        let path = entry.path();
                        if path.to_string_lossy().to_lowercase().contains("herdr") {
                            discovered.push(path.to_path_buf());
                            break;
                        }
                    }
                }
            }
        }

        let is_present = !discovered.is_empty();
        let confidence = if is_present { 0.95 } else { 0.0 };

        Ok(DetectionResult {
            adapter_name: self.name(),
            is_present,
            discovered_roots: discovered,
            confidence,
        })
    }

    fn enumerate(&self, options: &ScanOptions) -> Result<Vec<SessionSource>> {
        let mut sources = Vec::new();
        let roots: Vec<PathBuf> = if options.custom_paths.is_empty() {
            self.candidate_roots()
        } else {
            options.custom_paths.clone()
        };

        for root in roots {
            if root.is_file() {
                if is_candidate_herdr_file(&root) {
                    if let Ok(source) = SessionSource::from_path_with_known(
                        &root,
                        self.name(),
                        &options.known_sources,
                    ) {
                        sources.push(source);
                    }
                }
            } else if root.is_dir() {
                for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.is_file() && is_candidate_herdr_file(path) {
                        if let Ok(source) = SessionSource::from_path_with_known(
                            path,
                            self.name(),
                            &options.known_sources,
                        ) {
                            sources.push(source);
                        }
                    }
                }
            }
        }

        sources.sort_by(|a, b| a.path.cmp(&b.path));
        sources.dedup_by(|a, b| a.path == b.path);

        Ok(sources)
    }

    fn parse(&self, source: &SessionSource) -> Result<ParseResult> {
        let display = source.path.to_string_lossy().to_string();
        let session_id = derive_session_id(&source.path);
        let provenance = Provenance::new(
            display.clone(),
            self.name(),
            source.file_size_bytes,
            source.mtime_epoch_secs,
            &source.fingerprint,
        );

        let saved_at = epoch_to_datetime(source.mtime_epoch_secs);
        let mut trace = AgentWorthTrace::new(&session_id, self.name(), provenance, saved_at);
        trace.kind = TraceKind::FleetSnapshot;
        trace.ended_at = Some(saved_at);
        let mut warnings = Vec::new();
        let mut malformed_lines = 0usize;

        let raw: Value = match serde_json::from_reader(BufReader::new(File::open(&source.path)?)) {
            Ok(v) => v,
            Err(e) => {
                warnings.push(format!("{display}: not valid JSON: {e}"));
                return Ok(ParseResult {
                    trace,
                    malformed_lines: 1,
                    warnings,
                });
            }
        };

        let Some(obj) = raw.as_object() else {
            warnings.push(format!(
                "{display}: not a Herdr session file (top level is not an object)"
            ));
            return Ok(ParseResult {
                trace,
                malformed_lines: 0,
                warnings,
            });
        };
        if !obj.contains_key("workspaces") {
            warnings.push(format!(
                "{display}: not a Herdr session file (no `workspaces` key)"
            ));
            return Ok(ParseResult {
                trace,
                malformed_lines: 0,
                warnings,
            });
        }

        let file: SessionFile = match serde_json::from_value(raw) {
            Ok(f) => f,
            Err(e) => {
                warnings.push(format!("{display}: session file did not decode: {e}"));
                return Ok(ParseResult {
                    trace,
                    malformed_lines: 1,
                    warnings,
                });
            }
        };

        let file_version = file.version.as_ref().and_then(value_as_u64);
        match (&file.version, file_version) {
            (_, Some(v)) if v == SESSION_FILE_VERSION => {}
            (_, Some(v)) => warnings.push(format!(
                "{display}: session file version {v}; this adapter was written against version {SESSION_FILE_VERSION}, reading what it can"
            )),
            (None, None) => warnings.push(format!(
                "{display}: session file has no `version`; assuming {SESSION_FILE_VERSION}"
            )),
            (Some(other), None) => warnings.push(format!(
                "{display}: session file `version` is {other}, not a number; assuming {SESSION_FILE_VERSION}"
            )),
        }
        let active_workspace = file.active.as_ref().and_then(value_as_u64);

        let mut seq = 0u64;
        let mut agent_sessions = Vec::new();
        let mut workspace_summaries = Vec::new();
        let mut tab_count = 0usize;
        let mut pane_count = 0usize;

        for (w_idx, ws_raw) in file.workspaces.iter().enumerate() {
            let ws: Workspace = match serde_json::from_value(ws_raw.clone()) {
                Ok(w) => w,
                Err(e) => {
                    malformed_lines += 1;
                    warnings.push(format!(
                        "{display}: workspaces[{w_idx}] did not decode: {e}"
                    ));
                    continue;
                }
            };
            let workspace_is_active = active_workspace == Some(w_idx as u64);
            let active_tab = ws.active_tab.as_ref().and_then(value_as_u64);
            let mut ws_panes = 0usize;

            for (t_idx, tab_raw) in ws.tabs.iter().enumerate() {
                let tab: Tab = match serde_json::from_value(tab_raw.clone()) {
                    Ok(t) => t,
                    Err(e) => {
                        malformed_lines += 1;
                        warnings.push(format!(
                            "{display}: workspaces[{w_idx}].tabs[{t_idx}] did not decode: {e}"
                        ));
                        continue;
                    }
                };
                tab_count += 1;
                let tab_is_active = workspace_is_active && active_tab == Some(t_idx as u64);
                let tab_focused_key = tab.focused.as_ref().and_then(value_as_key);

                let mut panes: Vec<(&String, &Value)> = tab.panes.iter().collect();
                panes.sort_by_key(|(key, _)| pane_sort_key(key));
                for (key, pane_raw) in panes {
                    let raw_ref = format!("workspaces[{w_idx}].tabs[{t_idx}].panes[{key}]");
                    let pane: PaneState = match serde_json::from_value(pane_raw.clone()) {
                        Ok(p) => p,
                        Err(e) => {
                            malformed_lines += 1;
                            warnings.push(format!("{display}: {raw_ref} did not decode: {e}"));
                            continue;
                        }
                    };
                    pane_count += 1;
                    ws_panes += 1;

                    let agent_session: Option<AgentSessionRef> = match pane.agent_session {
                        None | Some(Value::Null) => None,
                        Some(v) => match serde_json::from_value(v) {
                            Ok(a) => Some(a),
                            Err(e) => {
                                malformed_lines += 1;
                                warnings.push(format!(
                                    "{display}: {raw_ref}.agent_session did not decode, pane kept without it: {e}"
                                ));
                                None
                            }
                        },
                    };
                    let pane_number = ws.public_pane_numbers.get(key).and_then(value_as_u64);
                    let focused = tab_is_active && tab_focused_key.as_deref() == Some(key.as_str());

                    if let Some(agent) = &agent_session {
                        agent_sessions.push(json!({
                            "agent": agent.agent,
                            "source": agent.source,
                            "kind": agent.kind,
                            "value": agent.value,
                            "pane_key": key,
                            "pane_number": pane_number,
                            "workspace_id": ws.id,
                            "cwd": pane.cwd,
                            "label": pane.label,
                            "agent_name": pane.agent_name,
                            "seen_at": saved_at,
                        }));
                        // The harness session id is the key; the names are what this pane was
                        // called when the file was saved, kept as sightings so a rename adds a
                        // row instead of rewriting one.
                        let via = format!(
                            "herdr:{}:pane {}",
                            ws.id.as_deref().unwrap_or("?"),
                            pane_number.map_or_else(|| key.clone(), |n| n.to_string())
                        );
                        for name in [pane.label.as_deref(), pane.agent_name.as_deref()]
                            .into_iter()
                            .flatten()
                            .filter(|n| !n.trim().is_empty())
                        {
                            trace.identities.push(IdentitySighting {
                                session_id: agent.value.clone(),
                                agent: Some(agent.agent.clone()),
                                name: name.to_string(),
                                seen_at: saved_at,
                                via: via.clone(),
                            });
                        }
                    }

                    seq += 1;
                    trace.events.push(
                        NormalizedEvent::new(
                            seq,
                            saved_at,
                            EventPayload::Custom {
                                kind: PANE_EVENT_KIND.to_string(),
                                data: json!({
                                    "workspace_id": ws.id,
                                    "workspace_name": ws.custom_name,
                                    "workspace_cwd": ws.identity_cwd,
                                    "tab_index": t_idx,
                                    "tab_name": tab.custom_name,
                                    "pane_key": key,
                                    "pane_number": pane_number,
                                    "focused": focused,
                                    "cwd": pane.cwd,
                                    "label": pane.label,
                                    "agent_name": pane.agent_name,
                                    "managed_agent_kind": pane.managed_agent_kind,
                                    "agent_session": agent_session,
                                    "seen_at": saved_at,
                                }),
                            },
                        )
                        .with_raw_ref(raw_ref),
                    );
                }
            }
            workspace_summaries.push(json!({
                "id": ws.id,
                "name": ws.custom_name,
                "identity_cwd": ws.identity_cwd,
                "active": workspace_is_active,
                "tab_count": ws.tabs.len(),
                "pane_count": ws_panes,
            }));
        }
        trace
            .identities
            .dedup_by(|a, b| a.session_id == b.session_id && a.name == b.name && a.via == b.via);

        let mut metadata = Map::new();
        metadata.insert(
            "herdr".to_string(),
            json!({
                "file_version": file_version,
                "saved_at": saved_at,
                "workspace_count": workspace_summaries.len(),
                "tab_count": tab_count,
                "pane_count": pane_count,
                "agent_pane_count": agent_sessions.len(),
                "workspaces": workspace_summaries,
                "agent_sessions": agent_sessions,
            }),
        );
        // Same key the Claude Code adapter writes, so anything that reads
        // `metadata.workspace.cwd` sees the active workspace's identity directory.
        let active_cwd = active_workspace
            .and_then(|i| file.workspaces.get(i as usize))
            .or_else(|| file.workspaces.first())
            .and_then(|w| w.get("identity_cwd"))
            .and_then(|v| v.as_str());
        if let Some(cwd) = active_cwd {
            metadata.insert("workspace".to_string(), json!({ "cwd": cwd }));
        }
        trace.metadata = Value::Object(metadata);

        trace.recalculate_stats();

        Ok(ParseResult {
            trace,
            malformed_lines,
            warnings,
        })
    }
}

#[derive(Debug, Default, Deserialize)]
struct SessionFile {
    #[serde(default)]
    version: Option<Value>,
    #[serde(default)]
    workspaces: Vec<Value>,
    #[serde(default)]
    active: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
struct Workspace {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    custom_name: Option<String>,
    #[serde(default)]
    identity_cwd: Option<String>,
    #[serde(default)]
    public_pane_numbers: BTreeMap<String, Value>,
    #[serde(default)]
    tabs: Vec<Value>,
    #[serde(default)]
    active_tab: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
struct Tab {
    #[serde(default)]
    custom_name: Option<String>,
    #[serde(default)]
    panes: BTreeMap<String, Value>,
    #[serde(default)]
    focused: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
struct PaneState {
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    agent_name: Option<String>,
    #[serde(default)]
    managed_agent_kind: Option<String>,
    #[serde(default)]
    agent_session: Option<Value>,
}

/// `AgentSessionInfo` in Herdr's API schema: how a pane points at the agent session inside it.
/// All four fields are `required` there, and that is kept: a ref without a `value` is no join.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct AgentSessionRef {
    /// Which Herdr integration reported it, e.g. `herdr:claude`, `herdr:codex`.
    source: String,
    /// The agent's short name as Herdr knows it: `claude`, `codex`, `agy`, `grok`, `cursor`, ...
    agent: String,
    kind: AgentSessionRefKind,
    /// The agent's own session id (or path, per `kind`). The join key to that agent's trace.
    value: String,
}

/// `AgentSessionRefKind` in Herdr's API schema. `Other` keeps a future kind from failing the ref.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum AgentSessionRefKind {
    Id,
    Path,
    Other,
}

impl<'de> Deserialize<'de> for AgentSessionRefKind {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        Ok(match String::deserialize(deserializer)?.as_str() {
            "id" => Self::Id,
            "path" => Self::Path,
            _ => Self::Other,
        })
    }
}

/// A number, or a string holding one. Anything else is `None`, never an error.
fn value_as_u64(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// The pane map keys are strings (`"25"`); `focused` is written as a number. Same identity.
fn value_as_key(v: &Value) -> Option<String> {
    match v {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// `session.json` and any `session*.json` next to it, or any JSON under a `sessions/` directory,
/// as long as `herdr` is somewhere in the path. Herdr's logs, `config.toml` and lock files sit
/// in the same directory and are not sessions.
fn is_candidate_herdr_file(path: &Path) -> bool {
    if !path.to_string_lossy().to_lowercase().contains("herdr") {
        return false;
    }
    if path.extension().is_none_or(|ext| ext != "json") {
        return false;
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    let in_sessions_dir = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("sessions"));
    stem.starts_with("session") || in_sessions_dir
}

/// Stable across rewrites of the same file (the file is replaced on every save, so neither
/// size, mtime nor fingerprint can be part of it) and distinct across named sessions that all
/// call their file `session.json`. The path is hashed as given, so the same file reached through
/// a symlink or a moved home directory is a different row; the sources table keys on the same
/// path string, so the two stay consistent.
fn derive_session_id(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("session");
    format!(
        "herdr-{stem}-{:08x}",
        fnv1a_32(path.to_string_lossy().as_bytes())
    )
}

fn fnv1a_32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5u32, |h, b| {
        (h ^ u32::from(*b)).wrapping_mul(0x0100_0193)
    })
}

fn pane_sort_key(key: &str) -> (u64, String) {
    match key.parse::<u64>() {
        Ok(n) => (n, String::new()),
        Err(_) => (u64::MAX, key.to_string()),
    }
}

fn epoch_to_datetime(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0).single().unwrap_or_else(Utc::now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// A redacted copy of a real `~/.config/herdr/session.json` from herdr 0.8.2: one
    /// workspace, one tab, eight panes, every pane running a different agent.
    const FIXTURE: &str = include_str!("../tests/fixtures/herdr-session-v3.json");

    fn write(root: &Path, rel: &str, body: &str) -> PathBuf {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        path
    }

    fn parse_body(body: &str) -> ParseResult {
        let temp = TempDir::new().unwrap();
        let path = write(temp.path(), ".config/herdr/session.json", body);
        let source = SessionSource::from_path(&path, "herdr").unwrap();
        HerdrAdapter::new().parse(&source).unwrap()
    }

    fn pane_data(event: &NormalizedEvent) -> &Value {
        match &event.payload {
            EventPayload::Custom { kind, data } => {
                assert_eq!(kind, PANE_EVENT_KIND);
                data
            }
            other => panic!("expected a herdr.pane event, got {other:?}"),
        }
    }

    #[test]
    fn real_shape_session_file_yields_one_event_per_pane() {
        let result = parse_body(FIXTURE);
        assert_eq!(result.malformed_lines, 0);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        assert_eq!(result.trace.kind, TraceKind::FleetSnapshot);
        assert_eq!(result.trace.events.len(), 8);
        assert_eq!(result.trace.stats.total_events, 8);
        assert_eq!(result.trace.stats.token_usage.total(), 0);
        assert_eq!(result.trace.adapter, "herdr");
        assert_eq!(result.trace.ended_at, Some(result.trace.started_at));
        let seqs: Vec<u64> = result.trace.events.iter().map(|e| e.sequence).collect();
        assert_eq!(seqs, (1..=8).collect::<Vec<_>>());
    }

    #[test]
    fn the_harness_session_id_is_the_key_and_names_are_sightings() {
        let result = parse_body(FIXTURE);
        let herdr = &result.trace.metadata["herdr"];
        assert_eq!(herdr["file_version"], 3);
        assert_eq!(herdr["workspace_count"], 1);
        assert_eq!(herdr["tab_count"], 1);
        assert_eq!(herdr["pane_count"], 8);
        assert_eq!(herdr["agent_pane_count"], 8);

        let sessions = herdr["agent_sessions"].as_array().unwrap();
        let claude = sessions
            .iter()
            .find(|s| s["agent"] == "claude" && s["pane_key"] == "20")
            .expect("the claude pane");
        assert_eq!(claude["source"], "herdr:claude");
        assert_eq!(claude["kind"], "id");
        assert_eq!(claude["value"], "adec3bdc-0fad-5212-bd43-efb9ebfa648d");
        assert_eq!(claude["pane_number"], 18);
        assert_eq!(claude["cwd"], "/home/dev/code/acme/agentworth");
        assert_eq!(claude["label"], "worker-claude");
        assert!(
            claude.get("pane_id").is_none(),
            "the live pane id is never stored"
        );

        let agents: Vec<&str> = sessions
            .iter()
            .map(|s| s["agent"].as_str().unwrap())
            .collect();
        for expected in ["claude", "codex", "agy", "grok", "cursor"] {
            assert!(
                agents.contains(&expected),
                "missing {expected} in {agents:?}"
            );
        }

        // Sightings: one per distinct name per pane. Pane 20 has only a label; pane 12 has a
        // label and an agent_name that happen to be equal, so it contributes one.
        let mine: Vec<&IdentitySighting> = result
            .trace
            .identities
            .iter()
            .filter(|s| s.session_id == "adec3bdc-0fad-5212-bd43-efb9ebfa648d")
            .collect();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].name, "worker-claude");
        assert_eq!(mine[0].agent.as_deref(), Some("claude"));
        assert_eq!(mine[0].via, "herdr:w9:pane 18");
        assert_eq!(mine[0].seen_at, result.trace.started_at);
        let partner: Vec<&IdentitySighting> = result
            .trace
            .identities
            .iter()
            .filter(|s| s.session_id == "21553d04-cadd-503c-8b11-ad03f51cd8bf")
            .collect();
        assert_eq!(partner.len(), 1);
        let chief: Vec<&str> = result
            .trace
            .identities
            .iter()
            .filter(|s| s.session_id == "85eca68d-6a82-5c0e-8bef-a90f22029c44")
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(chief, ["chief-gemini", "chief"]);

        assert_eq!(
            result.trace.metadata["workspace"]["cwd"],
            "/home/dev/code/acme/engine/.claude/worktrees/architect"
        );
    }

    #[test]
    fn panes_carry_both_the_key_and_the_public_number_in_key_order() {
        let result = parse_body(FIXTURE);
        let panes: Vec<&Value> = result.trace.events.iter().map(pane_data).collect();
        let keys: Vec<&str> = panes
            .iter()
            .map(|p| p["pane_key"].as_str().unwrap())
            .collect();
        assert_eq!(keys, ["12", "14", "15", "18", "20", "22", "25", "33"]);
        let numbers: Vec<u64> = panes
            .iter()
            .map(|p| p["pane_number"].as_u64().unwrap())
            .collect();
        assert_eq!(numbers, [10, 12, 13, 16, 18, 20, 23, 31]);

        let first = panes[0];
        assert_eq!(first["label"], "partner-claude");
        assert_eq!(first["managed_agent_kind"], "claude");
        assert_eq!(first["agent_session"]["agent"], "claude");
        assert_eq!(first["workspace_id"], "w9");
        assert_eq!(first["workspace_name"], "fleet");
        assert_eq!(first["focused"], false);
        assert!(first.get("pane_id").is_none());
        assert_eq!(
            result.trace.events[0].raw_ref.as_deref(),
            Some("workspaces[0].tabs[0].panes[12]")
        );

        let focused: Vec<&str> = panes
            .iter()
            .filter(|p| p["focused"] == true)
            .map(|p| p["pane_key"].as_str().unwrap())
            .collect();
        assert_eq!(focused, ["25"]);
    }

    #[test]
    fn a_one_pane_file_is_one_event_and_a_snapshot() {
        let body = r#"{"version": 3, "active": 0, "workspaces": [{"id": "w1", "active_tab": 0,
            "tabs": [{"focused": 1, "panes": {"1": {"cwd": "/w", "agent_session":
            {"source": "herdr:claude", "agent": "claude", "kind": "id", "value": "abc"}}}}]}]}"#;
        let result = parse_body(body);
        assert_eq!(result.trace.kind, TraceKind::FleetSnapshot);
        assert_eq!(result.trace.events.len(), 1);
        assert_eq!(pane_data(&result.trace.events[0])["focused"], true);
        assert_eq!(
            pane_data(&result.trace.events[0])["pane_number"],
            Value::Null
        );
        assert_eq!(result.trace.metadata["herdr"]["agent_pane_count"], 1);
        // No label, no agent_name: nothing to sight.
        assert!(result.trace.identities.is_empty());
    }

    #[test]
    fn a_file_with_no_panes_yields_no_events() {
        let result =
            parse_body(r#"{"version": 3, "workspaces": [{"id": "w1", "tabs": [{"panes": {}}]}]}"#);
        assert!(result.trace.events.is_empty());
        assert_eq!(result.trace.metadata["herdr"]["pane_count"], 0);
        let result = parse_body(r#"{"version": 3, "workspaces": []}"#);
        assert!(result.trace.events.is_empty());
    }

    #[test]
    fn only_the_active_tab_of_the_active_workspace_has_a_focused_pane() {
        let body = r#"{"version": 3, "active": 1, "workspaces": [
            {"id": "w0", "active_tab": 0, "tabs": [{"focused": 1, "panes": {"1": {"cwd": "/a"}}}]},
            {"id": "w1", "active_tab": 1, "tabs": [
                {"focused": 2, "panes": {"2": {"cwd": "/b"}}},
                {"focused": 3, "panes": {"3": {"cwd": "/c"}, "4": {"cwd": "/d"}}}
            ]}
        ]}"#;
        let result = parse_body(body);
        let focused: Vec<&str> = result
            .trace
            .events
            .iter()
            .map(pane_data)
            .filter(|p| p["focused"] == true)
            .map(|p| p["pane_key"].as_str().unwrap())
            .collect();
        assert_eq!(focused, ["3"]);
        assert_eq!(result.trace.metadata["workspace"]["cwd"], Value::Null);
        assert_eq!(
            result.trace.metadata["herdr"]["workspaces"][1]["active"],
            true
        );
    }

    #[test]
    fn wrong_typed_scalars_degrade_instead_of_losing_the_file() {
        let body = r#"{"version": "3", "active": "zero", "workspaces": [{"id": "w1",
            "active_tab": null, "public_pane_numbers": {"1": "7", "2": true},
            "tabs": [{"focused": "1", "panes": {"1": {"cwd": "/w"}, "2": {"cwd": "/x"}}}]}]}"#;
        let result = parse_body(body);
        assert_eq!(result.malformed_lines, 0);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        assert_eq!(result.trace.metadata["herdr"]["file_version"], 3);
        let panes: Vec<&Value> = result.trace.events.iter().map(pane_data).collect();
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0]["pane_number"], 7);
        assert_eq!(panes[1]["pane_number"], Value::Null);
        // `active` is not a number, so no workspace is active and nothing is focused.
        assert!(panes.iter().all(|p| p["focused"] == false));

        let result = parse_body(r#"{"version": "later", "workspaces": []}"#);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("not a number"));
        assert_eq!(result.trace.metadata["herdr"]["file_version"], Value::Null);
    }

    #[test]
    fn future_version_and_unknown_fields_still_parse_with_a_warning() {
        let body = r#"{
          "version": 4, "theme": "dark",
          "workspaces": [{"id": "w1", "pinned": true, "identity_cwd": "/w",
            "tabs": [{"panes": {"3": {"cwd": "/w", "label": "shell", "scrollback": 9},
                                "1": {"cwd": "/w", "agent_session": {"source": "herdr:codex", "agent": "codex", "kind": "handle", "value": "abc", "extra": 1}}}}]}],
          "active": 0
        }"#;
        let result = parse_body(body);
        assert_eq!(result.malformed_lines, 0);
        assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
        assert!(result.warnings[0].contains("version 4"));
        let panes: Vec<&Value> = result.trace.events.iter().map(pane_data).collect();
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0]["pane_key"], "1");
        assert_eq!(panes[0]["agent_session"]["kind"], "other");
        assert_eq!(result.trace.metadata["herdr"]["agent_pane_count"], 1);
        assert_eq!(result.trace.metadata["workspace"]["cwd"], "/w");
    }

    #[test]
    fn a_bad_agent_session_costs_the_ref_not_the_pane() {
        let body = r#"{"version": 3, "workspaces": [{"id": "w1", "tabs": [{"panes": {
            "1": {"cwd": "/w", "label": "ok"},
            "2": "not an object",
            "3": {"cwd": "/w", "label": "half", "agent_session": {"source": "herdr:claude"}},
            "4": {"cwd": "/w", "agent_session": null}
        }}]}]}"#;
        let result = parse_body(body);
        let panes: Vec<&Value> = result.trace.events.iter().map(pane_data).collect();
        assert_eq!(panes.len(), 3);
        assert_eq!(panes[1]["label"], "half");
        assert_eq!(panes[1]["agent_session"], Value::Null);
        assert_eq!(panes[2]["agent_session"], Value::Null);
        assert_eq!(result.malformed_lines, 2);
        assert_eq!(result.warnings.len(), 2);
        assert!(result.warnings[0].contains("panes[2]"));
        assert!(result.warnings[1].contains("panes[3].agent_session"));
        assert_eq!(result.trace.metadata["herdr"]["agent_pane_count"], 0);
        assert!(result.trace.identities.is_empty());
    }

    #[test]
    fn foreign_json_is_a_warning_not_a_session() {
        let result = parse_body(r#"{"plugins": [], "version": 3}"#);
        assert!(result.trace.events.is_empty());
        assert_eq!(result.malformed_lines, 0);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("workspaces"));

        let result = parse_body(r#"[1, 2, 3]"#);
        assert!(result.trace.events.is_empty());
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn invalid_json_counts_as_malformed() {
        let result = parse_body("{\"version\": 3, \"workspaces\": [");
        assert!(result.trace.events.is_empty());
        assert_eq!(result.malformed_lines, 1);
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn detect_and_enumerate_find_the_session_file_and_skip_logs_and_config() {
        let temp = TempDir::new().unwrap();
        let session = write(temp.path(), ".config/herdr/session.json", FIXTURE);
        write(temp.path(), ".config/herdr/config.toml", "[keys]\n");
        write(
            temp.path(),
            ".config/herdr/herdr-server.log",
            "2026-09-07T00:00:00Z INFO x\n",
        );
        write(temp.path(), ".config/herdr/herdr-client.log", "\n");
        write(temp.path(), ".config/herdr/.plugins.lock", "");
        write(temp.path(), ".config/herdr/plugins/registry.json", "{}");

        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            ..Default::default()
        };
        let adapter = HerdrAdapter::new();
        let detected = adapter.detect(&options).unwrap();
        assert!(detected.is_present);
        assert_eq!(detected.adapter_name, "herdr");
        assert_eq!(adapter.trace_kind(), TraceKind::FleetSnapshot);

        let sources = adapter.enumerate(&options).unwrap();
        assert_eq!(sources.len(), 1, "{sources:?}");
        assert_eq!(sources[0].path, session);
        assert_eq!(sources[0].adapter_name, "herdr");
    }

    #[test]
    fn named_sessions_under_a_sessions_dir_are_candidates_too() {
        assert!(is_candidate_herdr_file(Path::new(
            "/h/.config/herdr/session.json"
        )));
        assert!(is_candidate_herdr_file(Path::new(
            "/h/.config/herdr/session-lab.json"
        )));
        assert!(is_candidate_herdr_file(Path::new(
            "/h/.config/herdr/sessions/lab.json"
        )));
        assert!(!is_candidate_herdr_file(Path::new(
            "/h/.config/herdr/herdr-server.log"
        )));
        assert!(!is_candidate_herdr_file(Path::new(
            "/h/.config/herdr/config.toml"
        )));
        assert!(!is_candidate_herdr_file(Path::new(
            "/h/.config/herdr/plugins/registry.json"
        )));
        assert!(!is_candidate_herdr_file(Path::new(
            "/h/.config/other/session.json"
        )));
    }

    #[test]
    fn session_id_is_stable_across_rewrites_and_distinct_across_paths() {
        let temp = TempDir::new().unwrap();
        let path = write(temp.path(), ".config/herdr/session.json", FIXTURE);
        let first = SessionSource::from_path(&path, "herdr").unwrap();
        let first_id = HerdrAdapter::new().parse(&first).unwrap().trace.session_id;

        fs::write(&path, r#"{"version": 3, "workspaces": []}"#).unwrap();
        let second = SessionSource::from_path(&path, "herdr").unwrap();
        let second_id = HerdrAdapter::new().parse(&second).unwrap().trace.session_id;
        assert_eq!(first_id, second_id);
        assert!(first_id.starts_with("herdr-session-"), "{first_id}");

        let other = write(temp.path(), ".config/herdr/sessions/lab.json", FIXTURE);
        let other = SessionSource::from_path(&other, "herdr").unwrap();
        let other_id = HerdrAdapter::new().parse(&other).unwrap().trace.session_id;
        assert_ne!(first_id, other_id);
    }
}
