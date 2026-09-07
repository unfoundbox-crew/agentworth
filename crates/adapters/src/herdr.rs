//! Herdr adapter.
//!
//! Herdr (herdr.dev) is a terminal workspace manager for coding agents, not an agent. It never
//! writes a transcript; the agents in its panes write their own, which the Claude Code, Codex
//! and Gemini adapters already index. What Herdr persists is one file,
//! `~/.config/herdr/session.json` (`"version": 3` as of herdr 0.8.2): the workspaces, tabs and
//! panes of the running session, and for every pane the agent Herdr recognized inside it plus
//! that agent's own session id.
//!
//! That id is what this adapter is for. `agent_session.value` is the join from a Herdr pane to
//! the agent session that ran in it, and it is the only stable key: pane labels are renamed
//! freely (`pane.rename` is among the most frequent calls in `herdr-server.log`), pane numbers
//! are reused, and the file is rewritten in place on every `persist.save`.
//!
//! Shape, read from the real file and cross-checked against `herdr api schema --json`
//! (protocol 20: `AgentSessionInfo`, `AgentSessionRefKind`):
//!
//! ```text
//! { "version": 3,
//!   "workspaces": [ { "id": "w9", "custom_name": "fleet", "identity_cwd": "/abs/path",
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
//! Every field is optional on read and unknown fields are ignored, so a future `version` still
//! yields what it can and says so in a warning instead of failing the file. One pane that does
//! not decode is counted as malformed and the rest of the file still lands.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use agentworth_adapter_sdk::{
    AdapterCapabilities, AgentAdapter, DetectionResult, ParseResult, ScanOptions, SessionSource,
};
use agentworth_schema::{AgentWorthTrace, EventPayload, NormalizedEvent, Provenance};
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

impl HerdrAdapter {
    /// 2: reads the real `session.json` v3 shape. Version 1 looked for `coordination_traces`,
    /// `parent_agent_id` and `delegation_id`, fields no Herdr file has ever carried, and
    /// produced nothing; the bump makes an incremental scan re-read every file it had already
    /// fingerprinted.
    pub const PARSER_VERSION: i64 = 2;

    /// The `session.json` layout this adapter was written against (herdr 0.8.2).
    pub const SESSION_FILE_VERSION: u64 = 3;

    /// Event kind emitted once per pane.
    pub const PANE_EVENT_KIND: &'static str = "herdr.pane";

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
                    if let Ok(source) = SessionSource::from_path(&root, self.name()) {
                        sources.push(source);
                    }
                }
            } else if root.is_dir() {
                for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.is_file() && is_candidate_herdr_file(path) {
                        if let Ok(source) = SessionSource::from_path(path, self.name()) {
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

        // The file is rewritten whole on every save, so its mtime is the one timestamp it
        // carries: "the fleet looked like this as of then".
        let saved_at = epoch_to_datetime(source.mtime_epoch_secs);
        let mut trace = AgentWorthTrace::new(&session_id, self.name(), provenance, saved_at);
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

        match file.version {
            Some(v) if v == Self::SESSION_FILE_VERSION => {}
            Some(v) => warnings.push(format!(
                "{display}: session file version {v}; this adapter was written against version {}, reading what it can",
                Self::SESSION_FILE_VERSION
            )),
            None => warnings.push(format!("{display}: session file has no `version`; assuming {}", Self::SESSION_FILE_VERSION)),
        }

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
                let mut panes: Vec<(&String, &Value)> = tab.panes.iter().collect();
                panes.sort_by_key(|(number, _)| pane_sort_key(number));
                for (number, pane_raw) in panes {
                    let raw_ref = format!("workspaces[{w_idx}].tabs[{t_idx}].panes[{number}]");
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
                    let focused = tab.focused.is_some_and(|f| f.to_string() == *number);

                    if let Some(agent) = &pane.agent_session {
                        agent_sessions.push(json!({
                            "agent": agent.agent,
                            "source": agent.source,
                            "kind": agent.kind,
                            "value": agent.value,
                            "pane": number,
                            "workspace_id": ws.id,
                            "cwd": pane.cwd,
                            "label": pane.label,
                        }));
                    }

                    seq += 1;
                    trace.events.push(
                        NormalizedEvent::new(
                            seq,
                            saved_at,
                            EventPayload::Custom {
                                kind: Self::PANE_EVENT_KIND.to_string(),
                                data: json!({
                                    "workspace_id": ws.id,
                                    "workspace_name": ws.custom_name,
                                    "workspace_cwd": ws.identity_cwd,
                                    "tab_index": t_idx,
                                    "tab_name": tab.custom_name,
                                    "pane": number,
                                    "focused": focused,
                                    "cwd": pane.cwd,
                                    "label": pane.label,
                                    "agent_name": pane.agent_name,
                                    "managed_agent_kind": pane.managed_agent_kind,
                                    "agent_session": pane.agent_session,
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
                "tab_count": ws.tabs.len(),
                "pane_count": ws_panes,
            }));
        }

        let mut metadata = Map::new();
        metadata.insert(
            "herdr".to_string(),
            json!({
                "file_version": file.version,
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
        let active_cwd = file
            .active
            .and_then(|i| file.workspaces.get(i))
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
    version: Option<u64>,
    #[serde(default)]
    workspaces: Vec<Value>,
    #[serde(default)]
    active: Option<usize>,
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
    tabs: Vec<Value>,
}

#[derive(Debug, Default, Deserialize)]
struct Tab {
    #[serde(default)]
    custom_name: Option<String>,
    #[serde(default)]
    panes: BTreeMap<String, Value>,
    #[serde(default)]
    focused: Option<u64>,
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
    agent_session: Option<AgentSessionRef>,
}

/// `AgentSessionInfo` in Herdr's API schema: how a pane points at the agent session inside it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentSessionRef {
    /// Which Herdr integration reported it, e.g. `herdr:claude`, `herdr:codex`.
    pub source: String,
    /// The agent's short name as Herdr knows it: `claude`, `codex`, `agy`, `grok`, `cursor`, ...
    pub agent: String,
    pub kind: AgentSessionRefKind,
    /// The agent's own session id (or path, per `kind`). The join key to that agent's trace.
    pub value: String,
}

/// `AgentSessionRefKind` in Herdr's API schema. `Other` keeps a future kind from failing the pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentSessionRefKind {
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
/// call their file `session.json`.
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

fn pane_sort_key(number: &str) -> (u64, String) {
    match number.parse::<u64>() {
        Ok(n) => (n, String::new()),
        Err(_) => (u64::MAX, number.to_string()),
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
                assert_eq!(kind, HerdrAdapter::PANE_EVENT_KIND);
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
        assert_eq!(result.trace.events.len(), 8);
        assert_eq!(result.trace.stats.total_events, 8);
        assert_eq!(result.trace.stats.token_usage.total(), 0);
        assert_eq!(result.trace.adapter, "herdr");
        assert_eq!(result.trace.ended_at, Some(result.trace.started_at));
    }

    #[test]
    fn the_agent_session_id_is_surfaced_as_the_join_key() {
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
            .find(|s| s["agent"] == "claude" && s["pane"] == "20")
            .expect("the claude pane");
        assert_eq!(claude["source"], "herdr:claude");
        assert_eq!(claude["kind"], "id");
        assert_eq!(claude["value"], "adec3bdc-0fad-5212-bd43-efb9ebfa648d");
        assert_eq!(claude["cwd"], "/home/dev/code/acme/agentworth");
        assert_eq!(claude["label"], "worker-claude");

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

        assert_eq!(
            result.trace.metadata["workspace"]["cwd"],
            "/home/dev/code/acme/engine/.claude/worktrees/architect"
        );
    }

    #[test]
    fn panes_are_ordered_numerically_and_carry_their_pane_number() {
        let result = parse_body(FIXTURE);
        let numbers: Vec<&str> = result
            .trace
            .events
            .iter()
            .map(|e| pane_data(e)["pane"].as_str().unwrap())
            .collect();
        assert_eq!(numbers, ["12", "14", "15", "18", "20", "22", "25", "33"]);

        let first = pane_data(&result.trace.events[0]);
        assert_eq!(first["label"], "partner-claude");
        assert_eq!(first["managed_agent_kind"], "claude");
        assert_eq!(first["agent_session"]["agent"], "claude");
        assert_eq!(first["workspace_id"], "w9");
        assert_eq!(first["workspace_name"], "fleet");
        assert_eq!(first["focused"], false);

        let focused: Vec<&str> = result
            .trace
            .events
            .iter()
            .filter(|e| pane_data(e)["focused"] == true)
            .map(|e| pane_data(e)["pane"].as_str().unwrap())
            .collect();
        assert_eq!(focused, ["25"]);
        assert_eq!(
            result.trace.events[0].raw_ref.as_deref(),
            Some("workspaces[0].tabs[0].panes[12]")
        );
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
        assert_eq!(result.trace.events.len(), 2);
        let first = pane_data(&result.trace.events[0]);
        assert_eq!(first["pane"], "1");
        assert_eq!(first["agent_session"]["kind"], "other");
        assert_eq!(result.trace.metadata["herdr"]["agent_pane_count"], 1);
        assert_eq!(result.trace.metadata["workspace"]["cwd"], "/w");
    }

    #[test]
    fn a_pane_that_fails_to_decode_does_not_sink_the_file() {
        let body = r#"{"version": 3, "workspaces": [{"id": "w1", "tabs": [{"panes": {
            "1": {"cwd": "/w", "label": "ok"},
            "2": "not an object",
            "3": {"cwd": "/w", "agent_session": {"source": "herdr:claude"}}
        }}]}]}"#;
        let result = parse_body(body);
        assert_eq!(result.trace.events.len(), 1);
        assert_eq!(result.malformed_lines, 2);
        assert_eq!(result.warnings.len(), 2);
        assert!(result.warnings[0].contains("panes[2]"));
        assert!(result.warnings[1].contains("panes[3]"));
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
