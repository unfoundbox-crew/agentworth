use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use agentworth_adapter_sdk::{
    reuse_or_compute_fingerprint, AgentAdapter, DetectionResult, ParseResult, ScanOptions,
    SessionSource,
};
use agentworth_schema::{
    extract_repository_or_workspace, AgentWorthTrace, EventPayload, FileActionType, ModelSwitch,
    NormalizedEvent, OutcomeEvidence, OutcomeKind, Provenance, ShellCommand, TokenUsage, ToolCall,
    ToolResult,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use directories::BaseDirs;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use walkdir::WalkDir;

use crate::exit_status::backfill_shell_exit_codes;
use crate::normalize_mcp_tool_name;
use crate::usage_ledger::UsageLedger;

/// Adapter for discovering and normalizing Google Gemini / Antigravity agent sessions.
pub struct GeminiAdapter;

impl Default for GeminiAdapter {
    fn default() -> Self {
        Self
    }
}

impl GeminiAdapter {
    /// 2: token usage is credited once per record `id` rather than once per record, and
    /// `extract_token_usage` learned the field names Gemini CLI actually writes (`input` /
    /// `output` / `cached` / `thoughts` / `tool` inside `tokens`). Before this, every Gemini
    /// CLI session on this machine indexed as zero tokens despite 25M real ones; a reparse is
    /// what makes them appear, correctly counted.
    pub const PARSER_VERSION: i64 = 2;

    pub fn new() -> Self {
        Self
    }

    /// Candidate directory paths for Google Antigravity (agy / Antigravity IDE / Gemini) on
    /// the host machine, used for presence detection only. Broader than `session_roots()`:
    /// bare `~/.antigravity` is the Antigravity IDE's own app-config/extensions directory
    /// (argv.json, VS Code-style `extensions/<ext>/...json`), not session data, and bare
    /// `~/.gemini` mixes in `config/`, `tmp/`, and OAuth/account files alongside real
    /// session directories.
    pub fn candidate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".gemini").join("antigravity-cli").join("brain"));
            roots.push(home.join(".gemini").join("antigravity-ide").join("brain"));
            roots.push(home.join(".gemini").join("history"));
            roots.push(home.join(".gemini").join("sessions"));
            roots.push(home.join(".antigravity").join("sessions"));
            roots.push(home.join(".config").join("antigravity"));
            roots.push(home.join(".config").join("gemini"));
            roots.push(home.join(".gemini"));
        }
        roots.push(PathBuf::from(".gemini"));
        roots.push(PathBuf::from(".antigravity"));
        roots
    }

    /// Directories that actually hold Gemini/Antigravity session transcripts, used for the
    /// default (unscoped) `enumerate()` walk. Deliberately narrower than bare
    /// `antigravity-cli`/`antigravity-ide`, which also hold `mcp/*.json` tool schemas,
    /// `cache/`, and (in newer Antigravity builds) `conversations/*.db` -- SQLite, not
    /// JSON at all -- alongside the real `brain/` transcripts and the single rolling
    /// `history.jsonl`.
    pub fn session_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".gemini").join("antigravity-cli").join("brain"));
            roots.push(home.join(".gemini").join("antigravity-cli").join("history.jsonl"));
            roots.push(home.join(".gemini").join("antigravity-ide").join("brain"));
            // Bare `.gemini/antigravity` (no `-cli`/`-ide` suffix) is a real, currently
            // populated alternate install layout on at least one real machine, mirroring
            // `antigravity-cli`'s own directory shape exactly (brain/, playground/,
            // mcp/, conversations/, ...). Verified: a real 26-event transcript_full.jsonl
            // lived here and would otherwise go undetected.
            roots.push(home.join(".gemini").join("antigravity").join("brain"));
            roots.push(home.join(".gemini").join("antigravity").join("history.jsonl"));
            roots.push(home.join(".gemini").join("history"));
            roots.push(home.join(".gemini").join("sessions"));
            roots.push(home.join(".antigravity").join("sessions"));
            // Plain Gemini CLI (non-Antigravity) keeps its own per-project session logs
            // here: `tmp/<project>/chats/session-<ts>-<id>.json[l]` and, in an older
            // build, a flat `tmp/<project>/logs.json`. Both are real conversation data --
            // verified by content, not just by path -- unlike everything else sampled
            // directly under bare `.gemini` (config/, oauth creds, account state).
            roots.push(home.join(".gemini").join("tmp"));
        }
        roots
    }
}

/// Determines whether a transcript path belongs to Google Antigravity or Gemini CLI.
pub fn detect_product_identity(path: &Path) -> &'static str {
    let p = path.to_string_lossy().to_lowercase();
    if p.contains("antigravity") || p.contains("/brain/") || p.contains(".antigravity") {
        "antigravity"
    } else {
        "gemini"
    }
}

// ---------------------------------------------------------------------------
// Antigravity CLI (`agy`) conversation store.
//
// Newer Antigravity builds keep conversations in SQLite rather than JSONL:
// `~/.gemini/antigravity-cli/conversations/<uuid>.db` (one database per
// conversation; the `steps` table holds protobuf blobs) alongside a
// `conversation_summaries.db` index (workspace, status, timestamps) and a
// rolling `history.jsonl` of user prompts keyed by conversation id. The
// `brain/` transcripts enumerated below are an older, separate surface;
// without this section every `agy` session is discovered-but-never-indexed:
// the scan counts the files while the index holds zero rows for them.
//
// The step payloads are length-delimited protobuf without a published schema,
// so `parse_agy_conversation` walks the wire format generically
// (`pb_walk_strings`) and classifies by `step_type` plus content shape: 14
// carries the user prompt, 15 the assistant's prose, 101 a `[Message]
// timestamp=... content=...` record with a real timestamp, 132 a tool call
// (`call_*` id, snake_case name, JSON args), 90 the agent persona
// (configuration, not history -- skipped). Anything else is skipped with a
// warning rather than guessed at: formats are unstable and a wrong event is
// worse than a missing one. Token counters exist nowhere in this store, so
// agy rows honestly index with zero tokens.
// ---------------------------------------------------------------------------

/// Marker separating the synthetic repo-anchored identity prefix from the real
/// locator, mirroring opencode.rs's `OPENCODE_REPO_MARKER`. Everything from
/// the marker onward is invisible to `extract_repository_or_workspace`'s
/// generic rules, which resolve the repo from the `<workspace>/` prefix
/// instead of the store's own `~/.gemini/...` path; `parse()` recovers the
/// real locator with `strip_agy_marker`. `SessionSource.path` and
/// `Provenance.source_path` always carry this exact same string: the scanner
/// compares them to decide what changed, so diverging them would reparse
/// every agy session on every scan.
const AGY_REPO_MARKER: &str = "::agy-repo::";

/// Recover the real locator from a possibly-marked identity path. Without the
/// marker (a workspace that could not be resolved at enumerate time) the whole
/// string already is the real locator.
#[allow(
    clippy::string_slice,
    reason = "idx comes from rfind() on an ASCII marker, offset by its own byte length: always a char boundary"
)]
fn strip_agy_marker(path_str: &str) -> &str {
    match path_str.rfind(AGY_REPO_MARKER) {
        Some(idx) => &path_str[idx + AGY_REPO_MARKER.len()..],
        None => path_str,
    }
}

/// Split a real locator into its `<db_path>#<conversation_id>` halves.
fn split_agy_locator(identity: &str) -> Option<(&str, &str)> {
    let tail = strip_agy_marker(identity);
    let (db, conv) = tail.rsplit_once('#')?;
    if db.is_empty() || conv.is_empty() {
        return None;
    }
    Some((db, conv))
}

/// True when `identity` addresses an agy conversation database, marked or not.
/// Legacy transcript paths never match: they carry no `#` locator tail.
fn is_agy_locator(identity: &str) -> bool {
    match split_agy_locator(identity) {
        Some((db, conv)) => {
            db.ends_with(".db") && db.contains("conversations") && !conv.is_empty()
        }
        None => false,
    }
}

/// File or directory `name` inside `~/.gemini/antigravity-cli/`.
fn agy_store_path(name: &str) -> Option<PathBuf> {
    BaseDirs::new()
        .map(|b| b.home_dir().join(".gemini").join("antigravity-cli").join(name))
}

/// One row of `conversation_summaries.db`: session-level metadata the per-step
/// blobs do not carry (workspace, liveness, wall-clock end).
struct AgySummary {
    workspace: Option<String>,
    killed: bool,
    last_modified: Option<DateTime<Utc>>,
}

/// Parse the timestamp shapes the agy store writes: RFC 3339 inside `[Message]`
/// records, and `"2026-09-13 04:57:26.703385+00:00"` in the summaries table.
fn parse_agy_ts(raw: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f%:z") {
        return Some(dt.with_timezone(&Utc));
    }
    None
}

/// Best-effort metadata for one conversation. `None` when the summaries index
/// is absent or unreadable -- parsing still proceeds on the steps alone.
fn read_agy_summary(summaries_db: &Path, conv_id: &str) -> Option<AgySummary> {
    let conn = Connection::open_with_flags(
        summaries_db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .ok()?;
    let (killed, modified, workspaces): (bool, String, Option<String>) = conn
        .query_row(
            "SELECT killed, last_modified_time, workspace_uris \
             FROM conversation_summaries WHERE conversation_id = ?",
            [conv_id],
            |row| {
                // Each column degrades independently: a NULL workspace (the
                // common case -- most rows carry none) must not take down
                // the timestamps with it.
                Ok((
                    row.get::<_, bool>(0).unwrap_or(false),
                    row.get::<_, String>(1).unwrap_or_default(),
                    row.get::<_, Option<String>>(2).ok().flatten(),
                ))
            },
        )
        .ok()?;
    let workspace = workspaces
        .as_deref()
        .and_then(|ws| serde_json::from_str::<Vec<String>>(ws).ok())
        .and_then(|uris| uris.into_iter().next())
        .map(|u| u.strip_prefix("file://").map(str::to_string).unwrap_or(u));
    Some(AgySummary {
        workspace,
        killed,
        last_modified: parse_agy_ts(&modified),
    })
}

/// `(timestamp, user prompt)` pairs from the rolling `history.jsonl`, oldest
/// first. Missing or unreadable history yields an empty vec; prompt events
/// then fall back to step order without wall-clock times.
fn read_agy_history(
    history_path: &Path,
    conv_id: &str,
) -> Vec<(Option<DateTime<Utc>>, String)> {
    let mut out = Vec::new();
    let file = match File::open(history_path) {
        Ok(f) => f,
        Err(_) => return out,
    };
    for line in BufReader::new(file).lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v.get("conversationId").and_then(|c| c.as_str()) != Some(conv_id) {
            continue;
        }
        let display = v.get("display").and_then(|d| d.as_str()).unwrap_or("").to_string();
        if display.is_empty() {
            continue;
        }
        let ts = v.get("timestamp").and_then(|t| t.as_i64()).unwrap_or(0);
        let dt = if ts > 0 {
            let millis = if ts > 1_000_000_000_000 { ts } else { ts * 1000 };
            DateTime::from_timestamp_millis(millis)
        } else {
            None
        };
        out.push((dt, display));
    }
    out.sort_by_key(|(dt, _)| *dt);
    out
}

/// Read one protobuf varint at `*pos`, advancing past it. `None` on truncation
/// or overflow -- the caller abandons this blob, it never panics.
fn pb_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut shift = 0u32;
    let mut out = 0u64;
    loop {
        let b = *buf.get(*pos)?;
        *pos += 1;
        out |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Some(out);
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
}

/// Walk length-delimited protobuf wire format, collecting `(field_number,
/// string)` pairs. Nested messages recurse: a LEN field whose bytes walk
/// cleanly *and* yield strings contributes the inner pairs, otherwise its raw
/// UTF-8 (when valid) is kept verbatim. Returns false the moment the bytes
/// stop looking like protobuf, so the caller can fall back to substring
/// scraping instead of emitting half-decoded garbage.
fn pb_walk_strings(buf: &[u8], out: &mut Vec<(u32, String)>) -> bool {
    let mut pos = 0usize;
    while pos < buf.len() {
        let tag = match pb_varint(buf, &mut pos) {
            Some(t) => t,
            None => return false,
        };
        let field = (tag >> 3) as u32;
        match tag & 7 {
            0 => {
                if pb_varint(buf, &mut pos).is_none() {
                    return false;
                }
            }
            1 => {
                pos = match pos.checked_add(8) {
                    Some(p) => p,
                    None => return false,
                };
                if pos > buf.len() {
                    return false;
                }
            }
            2 => {
                let len = match pb_varint(buf, &mut pos) {
                    Some(l) => l as usize,
                    None => return false,
                };
                let end = match pos.checked_add(len) {
                    Some(e) => e,
                    None => return false,
                };
                if end > buf.len() {
                    return false;
                }
                let slice = &buf[pos..end];
                let mut nested = Vec::new();
                if !slice.is_empty() && pb_walk_strings(slice, &mut nested) && !nested.is_empty()
                {
                    out.extend(nested);
                } else if let Ok(s) = std::str::from_utf8(slice) {
                    if !s.is_empty() {
                        out.push((field, s.to_string()));
                    }
                }
                pos = end;
            }
            5 => {
                pos = match pos.checked_add(4) {
                    Some(p) => p,
                    None => return false,
                };
                if pos > buf.len() {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

/// Last-resort strings for a blob that fails the wire walk: printable ASCII
/// runs, the same shape a human sees in a hexdump. Never wrong, just lossy.
fn ascii_strings(buf: &[u8]) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    for &b in buf {
        if (0x20..=0x7e).contains(&b) {
            cur.push(b);
        } else if cur.len() >= 8 {
            if let Ok(s) = String::from_utf8(std::mem::take(&mut cur)) {
                out.push((0, s));
            }
        } else {
            cur.clear();
        }
    }
    if cur.len() >= 8 {
        if let Ok(s) = String::from_utf8(cur) {
            out.push((0, s));
        }
    }
    out
}

/// True for hex-with-dashes session/span identifiers: 8-4-4-4-12 hex.
fn is_uuid_like(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    for (i, c) in b.iter().enumerate() {
        if matches!(i, 8 | 13 | 18 | 23) {
            if *c != b'-' {
                return false;
            }
        } else if !c.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

/// True for routing identifiers that repeat on every step and carry no
/// conversation content: span ids, the `sessionID` key, bare numbers.
fn is_step_noise(s: &str, conv_id: &str) -> bool {
    if s == conv_id || s == "sessionID" || s.starts_with("bot-") {
        return true;
    }
    if is_uuid_like(s) {
        return true;
    }
    if !s.is_empty() && s.len() < 24 && s.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
        return true;
    }
    false
}

/// Split a type-101 `[Message] timestamp=<rfc3339> sender=<id>
/// priority=<p> content=<markdown>` record. The `content=` tail runs to the
/// end of the string; anything else is a shape change and returns None.
/// All slicing goes through `str::get`: blob text is arbitrary UTF-8 and a
/// mid-character split must yield None, never a panic.
fn split_message_record(s: &str) -> Option<(Option<DateTime<Utc>>, String)> {
    let tag = "[Message]".len();
    let body = s.find("[Message]").and_then(|i| s.get(i + tag..))?;
    let ts = body
        .find("timestamp=")
        .and_then(|i| body.get(i + "timestamp=".len()..))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(parse_agy_ts);
    let content = body
        .find("content=")
        .and_then(|i| body.get(i + "content=".len()..))
        .map(str::to_string)?;
    Some((ts, content))
}

/// Build the identity path for one conversation: the workspace-anchored form
/// when it resolves to a real repository through the generic rules, the bare
/// locator otherwise. A workspace like `~/code` itself would resolve to the
/// synthetic leaf (the `/code/` rule grabs whatever follows it), which is
/// worse than no anchor at all -- so the candidate is validated and rejected
/// when the leaf leaks through.
fn agy_identity_for(workspace: Option<String>, short: &str, real_locator: &str) -> String {
    if let Some(dir) = workspace.map(|w| w.trim().to_string()).filter(|w| !w.is_empty()) {
        let candidate = format!(
            "{}/.agy-conversation-{short}.sqlite{AGY_REPO_MARKER}{real_locator}",
            dir.trim_end_matches('/')
        );
        let repo = extract_repository_or_workspace(&candidate);
        if !repo.contains("agy-conversation") && !repo.contains(AGY_REPO_MARKER) {
            return candidate;
        }
    }
    real_locator.to_string()
}

/// True for the id-shaped tokens agy scatters through assistant steps:
/// mixed-case alphanumerics with a separator or digit, no spaces, long enough
/// that a plain English word rarely qualifies (`0SWmaoty3N-DxQ_kj4zgAQ`).
/// Applied to assistant prose only -- never to user prompts or tool args.
fn is_agy_id_token(s: &str) -> bool {
    if s.len() < 16 || s.contains(' ') || s.contains('/') || s.contains('{') {
        return false;
    }
    let has_lower = s.bytes().any(|b| b.is_ascii_lowercase());
    let has_upper = s.bytes().any(|b| b.is_ascii_uppercase());
    let has_extra = s.bytes().any(|b| b.is_ascii_digit() || b == b'_' || b == b'-');
    has_lower && has_upper && has_extra
}

/// Enumerate one [`SessionSource`] per agy conversation database, scoping to
/// `options.custom_paths` when set (tests, `--path`) and to the home store
/// otherwise. The identity path anchors on the workspace from the summaries
/// index so repository resolution sees a real project path; without a
/// resolvable workspace the bare locator is used and still parses (see
/// [`is_agy_locator`]).
fn enumerate_agy_conversations(options: &ScanOptions) -> Vec<SessionSource> {
    if !options.custom_paths.is_empty() {
        let mut out = Vec::new();
        for root in &options.custom_paths {
            let mut conv_dirs = vec![
                root.join("conversations"),
                root.join(".gemini").join("antigravity-cli").join("conversations"),
            ];
            if root.file_name().and_then(|n| n.to_str()) == Some("conversations") {
                conv_dirs.push(root.clone());
            }
            for conv_dir in conv_dirs {
                let summaries = std::fs::canonicalize(conv_dir.join(".."))
                    .map(|parent| parent.join("conversation_summaries.db"))
                    .ok()
                    .filter(|p| p.is_file());
                out.extend(enumerate_agy_conversations_in(
                    &conv_dir,
                    summaries.as_deref(),
                    options,
                ));
            }
        }
        return out;
    }
    let conv_dir = match agy_store_path("conversations") {
        Some(d) => d,
        None => return Vec::new(),
    };
    enumerate_agy_conversations_in(
        &conv_dir,
        agy_store_path("conversation_summaries.db").as_deref(),
        options,
    )
}

/// First workspace seen per conversation in the rolling `history.jsonl`.
/// The summaries index leaves `workspace_uris` NULL on most rows; history
/// records the workspace with every prompt, so this is the fallback behind
/// [`read_agy_summary`] at enumerate time.
fn agy_history_workspaces(history_path: &Path) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let file = match File::open(history_path) {
        Ok(f) => f,
        Err(_) => return map,
    };
    for line in BufReader::new(file).lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let (Some(conv), Some(ws)) = (
            v.get("conversationId").and_then(|c| c.as_str()),
            v.get("workspace").and_then(|w| w.as_str()),
        ) else {
            continue;
        };
        if ws.is_empty() {
            continue;
        }
        map.entry(conv.to_string()).or_insert_with(|| ws.to_string());
    }
    map
}

/// Enumerate conversation databases under an explicit directory; the
/// custom-paths branch above and the home-based default are the only callers
/// outside tests.
fn enumerate_agy_conversations_in(
    conv_dir: &Path,
    summaries_path: Option<&Path>,
    options: &ScanOptions,
) -> Vec<SessionSource> {
    let mut sources = Vec::new();
    if !conv_dir.is_dir() {
        return sources;
    }
    // The rolling history file sits next to `conversations/` in every layout
    // (store root, scoped root, fixture) and records each prompt's workspace
    // -- the fallback when the summaries row carries none (the common case).
    let history_workspaces = std::fs::canonicalize(conv_dir.join(".."))
        .map(|parent| parent.join("history.jsonl"))
        .ok()
        .filter(|p| p.is_file())
        .map(|p| agy_history_workspaces(&p))
        .unwrap_or_default();
    let entries = match std::fs::read_dir(conv_dir) {
        Ok(e) => e,
        Err(_) => return sources,
    };
    let mut dbs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("db"))
        .collect();
    dbs.sort();
    for db_path in dbs {
        let conv_id = match db_path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let workspace = summaries_path
            .and_then(|sp| read_agy_summary(sp, &conv_id))
            .and_then(|sum| sum.workspace)
            .or_else(|| history_workspaces.get(&conv_id).cloned());
        let real_locator = format!("{}#{}", db_path.to_string_lossy(), conv_id);
        // Chars, not bytes: `file_stem` is an arbitrary filename and byte
        // slicing could split a character boundary and panic the scan.
        let short: String = conv_id.chars().take(8).collect();
        let identity = agy_identity_for(workspace, &short, &real_locator);
        let (size, mtime) = match std::fs::metadata(&db_path) {
            Ok(m) => (
                m.len(),
                m.modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
            ),
            Err(_) => continue,
        };
        let fingerprint = match reuse_or_compute_fingerprint(
            &options.known_sources,
            &identity,
            &db_path,
            size,
            mtime,
        ) {
            Ok(f) => f,
            Err(_) => continue,
        };
        sources.push(SessionSource {
            path: PathBuf::from(identity),
            adapter_name: "antigravity".to_string(),
            file_size_bytes: size,
            mtime_epoch_secs: mtime,
            fingerprint,
        });
    }
    sources
}

/// Parse one agy conversation database into prompt, response, and tool-call
/// events. Thin wrapper resolving the store paths; tests call
/// `parse_agy_conversation_with_store` with fixture paths directly.
fn parse_agy_conversation(source: &SessionSource) -> Result<ParseResult> {
    parse_agy_conversation_with_store(
        source,
        agy_store_path("conversation_summaries.db").as_deref(),
        agy_store_path("history.jsonl").as_deref(),
    )
}

#[allow(clippy::too_many_lines, reason = "one step_type dispatch; splitting would scatter the shape map")]
fn parse_agy_conversation_with_store(
    source: &SessionSource,
    summaries_db: Option<&Path>,
    history_path: Option<&Path>,
) -> Result<ParseResult> {
    let identity = source.path.to_string_lossy().to_string();
    let (db_str, conv_id) = split_agy_locator(&identity)
        .map(|(d, c)| (d.to_string(), c.to_string()))
        .unwrap_or_else(|| (identity.clone(), derive_session_id(&source.path)));
    let db_path = Path::new(&db_str);

    let provenance = Provenance::new(
        identity,
        "antigravity",
        source.file_size_bytes,
        source.mtime_epoch_secs,
        &source.fingerprint,
    );
    let mut trace = AgentWorthTrace::new(&conv_id, "antigravity", provenance, Utc::now());
    let mut malformed_lines = 0;
    let mut warnings = Vec::new();
    let mut sequence = 0u64;

    let summary = summaries_db.and_then(|sp| read_agy_summary(sp, &conv_id));
    let mut history = history_path
        .map(|hp| read_agy_history(hp, &conv_id))
        .unwrap_or_default()
        .into_iter();

    let mtime_ts = DateTime::from_timestamp_secs(source.mtime_epoch_secs).unwrap_or_else(Utc::now);
    let mut earliest: Option<DateTime<Utc>> = None;
    let mut latest: Option<DateTime<Utc>> = None;
    let mut last_ts = mtime_ts;
    let mut bump = |ts: Option<DateTime<Utc>>| {
        let ts = ts.unwrap_or(last_ts);
        if earliest.is_none_or(|e| ts < e) {
            earliest = Some(ts);
        }
        if latest.is_none_or(|l| ts > l) {
            latest = Some(ts);
        }
        last_ts = ts;
        ts
    };

    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?;
    // Two passes: first collect every step's strings so the tool names
    // invoked anywhere in the conversation are known before any assistant
    // prose is cleaned of their echoes.
    let mut stmt = conn.prepare("SELECT step_type, step_payload FROM steps ORDER BY idx ASC")?;
    let raw_rows = stmt.query_map([], |row| {
        let step_type: i64 = row.get(0)?;
        let payload: Vec<u8> = row.get(1)?;
        Ok((step_type, payload))
    })?;
    let mut steps: Vec<(i64, Vec<(u32, String)>)> = Vec::new();
    for row in raw_rows {
        let (step_type, payload) = match row {
            Ok(r) => r,
            Err(e) => {
                malformed_lines += 1;
                warnings.push(format!("agy step row unreadable: {e}"));
                continue;
            }
        };
        let mut fields = Vec::new();
        if !pb_walk_strings(&payload, &mut fields) {
            fields = ascii_strings(&payload);
        }
        steps.push((step_type, fields));
    }
    let mut tool_names = std::collections::BTreeSet::new();
    for (step_type, fields) in &steps {
        if *step_type != 132 {
            continue;
        }
        for (_, s) in fields {
            if !s.starts_with("call_")
                && !(s.starts_with('{') && s.ends_with('}'))
                && !s.contains(' ')
                && !s.contains('/')
                && !is_step_noise(s, &conv_id)
            {
                tool_names.insert(s.clone());
            }
        }
    }
    // Assistant prose keeps everything except routing noise, tool-call
    // echoes, and id-shaped tokens. User prompts and tool args are never
    // filtered: their exact text is the evidence.
    let clean_assistant = |texts: &[&str], tool_names: &std::collections::BTreeSet<String>| {
        texts
            .iter()
            .filter(|s| {
                !s.starts_with("call_")
                    && !(s.starts_with('{') && s.ends_with('}'))
                    && !tool_names.contains(**s)
                    && !is_agy_id_token(s)
            })
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    };

    for (step_type, fields) in &steps {
        let texts: Vec<&str> = fields
            .iter()
            .map(|(_, s)| s.as_str())
            .filter(|s| !is_step_noise(s, &conv_id))
            .collect();

        match step_type {
            // Agent persona / system prompt: configuration, not history.
            90 => {}
            // User prompt. One history entry per user step, in order; the blob
            // is the fallback when history never recorded this prompt.
            14 => {
                let (ts, content) = match history.next() {
                    Some((hts, htext)) => (hts, htext),
                    None => {
                        let joined = texts.join("\n");
                        if joined.trim().is_empty() {
                            malformed_lines += 1;
                            warnings.push("agy user step with no text".to_string());
                            continue;
                        }
                        (None, joined)
                    }
                };
                sequence += 1;
                let ts = bump(ts);
                trace.events.push(NormalizedEvent::new(
                    sequence,
                    ts,
                    EventPayload::UserMessage { content },
                ));
            }
            // Assistant prose (thinking summaries and responses share this shape;
            // recorded as content, never claimed as thinking).
            15 => {
                let content = clean_assistant(&texts, &tool_names);
                if content.trim().is_empty() {
                    continue;
                }
                sequence += 1;
                let ts = bump(None);
                trace.events.push(NormalizedEvent::new(
                    sequence,
                    ts,
                    EventPayload::AssistantMessage {
                        content,
                        thinking: None,
                    },
                ));
            }
            // Tool call: `call_*` id, snake_case name, JSON args.
            132 => {
                let call_id = texts.iter().find(|s| s.starts_with("call_")).map(|s| s.to_string());
                let args_raw = texts
                    .iter()
                    .find(|s| s.starts_with('{') && s.ends_with('}'))
                    .map(|s| s.to_string());
                let name = texts
                    .iter()
                    .filter(|s| {
                        !s.starts_with("call_")
                            && !(s.starts_with('{') && s.ends_with('}'))
                            && !s.contains(' ')
                            && !s.contains('/')
                    })
                    .map(|s| s.to_string())
                    .next();
                let (Some(name), Some(args_raw)) = (name, args_raw) else {
                    malformed_lines += 1;
                    warnings.push("agy tool step without name/args".to_string());
                    continue;
                };
                let arguments = serde_json::from_str(&args_raw).unwrap_or(Value::String(args_raw));
                sequence += 1;
                let ts = bump(None);
                trace.events.push(NormalizedEvent::new(
                    sequence,
                    ts,
                    EventPayload::ToolCall(ToolCall {
                        id: call_id,
                        name,
                        arguments,
                    }),
                ));
            }
            // Message record with its own timestamp; otherwise assistant prose.
            101 => {
                let record = texts.iter().find(|s| s.contains("[Message]"));
                match record.and_then(|r| split_message_record(r)) {
                    Some((ts, content)) if !content.trim().is_empty() => {
                        sequence += 1;
                        let ts = bump(ts);
                        trace.events.push(NormalizedEvent::new(
                            sequence,
                            ts,
                            EventPayload::AssistantMessage {
                                content,
                                thinking: None,
                            },
                        ));
                    }
                    _ => {
                        let joined = clean_assistant(&texts, &tool_names);
                        if joined.trim().is_empty() {
                            continue;
                        }
                        sequence += 1;
                        let ts = bump(None);
                        trace.events.push(NormalizedEvent::new(
                            sequence,
                            ts,
                            EventPayload::AssistantMessage {
                                content: joined,
                                thinking: None,
                            },
                        ));
                    }
                }
            }
            _ => {
                malformed_lines += 1;
                warnings.push(format!("agy step_type {step_type} not mapped"));
            }
        }
    }

    if summary.as_ref().is_some_and(|s| s.killed) {
        warnings.push("agy conversation was killed before finishing".to_string());
    }
    if let Some(started) =
        earliest.or_else(|| summary.as_ref().and_then(|s| s.last_modified).map(|_| mtime_ts))
    {
        trace.started_at = started;
    }
    // The parsed steps carry sparse timestamps (only `[Message]` records and
    // history prompts have real ones), so the summaries row's last write is
    // the honest end -- but never earlier than the latest parsed event.
    let ended = match (latest, summary.as_ref().and_then(|s| s.last_modified)) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };
    if let Some(latest) = ended {
        trace.ended_at = Some(latest);
    }

    backfill_shell_exit_codes(&mut trace.events);
    trace.recalculate_stats();

    Ok(ParseResult {
        trace,
        malformed_lines,
        warnings,
    })
}

impl AgentAdapter for GeminiAdapter {
    fn name(&self) -> &'static str {
        "gemini"
    }

    fn parser_version(&self) -> i64 {
        Self::PARSER_VERSION
    }

    /// `parse()` tags each session with `detect_product_identity(path)`, which returns
    /// "antigravity" rather than "gemini" for sessions under an Antigravity brain/session
    /// root. Both identities must be listed here or a join keyed on `name()` alone (the
    /// adapter coverage matrix, most notably) silently drops every antigravity row.
    fn identity_names(&self) -> Vec<&'static str> {
        vec!["gemini", "antigravity"]
    }

    fn capabilities(&self) -> agentworth_adapter_sdk::AdapterCapabilities {
        agentworth_adapter_sdk::AdapterCapabilities {
            prompts: true,
            tokens: true,
            tools: true,
            shell: true,
            diffs: true,
            thinking: true,
            outcomes: true,
        }
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
            if custom.ends_with(".gemini") || s.contains("gemini") || s.contains("antigravity") {
                discovered.push(custom.clone());
            } else if custom.is_dir() {
                // custom_paths may point at a generic parent directory rather than
                // the adapter-specific dir itself; look a few levels in before
                // giving up, matching how `enumerate()` already recurses.
                let mut found_nested = false;
                for sub in &[
                    custom.join(".gemini"),
                    custom.join(".antigravity"),
                    custom.join(".config").join("gemini"),
                    custom.join(".config").join("antigravity"),
                ] {
                    if sub.exists() {
                        discovered.push(sub.clone());
                        found_nested = true;
                    }
                }
                if !found_nested {
                    for entry in WalkDir::new(custom).max_depth(4).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        let ps = path.to_string_lossy().to_lowercase();
                        if ps.contains("gemini") || ps.contains("antigravity") {
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

    /// Synthetic agy identities (`::agy-repo::` marker) address a live SQLite
    /// database, not a file at the identity path: strip the marker and check
    /// the real locator, mirroring opencode.rs. Everything else keeps the
    /// default file-existence check.
    fn source_exists(&self, source: &SessionSource) -> bool {
        let identity = source.path.to_string_lossy().to_string();
        if is_agy_locator(&identity) {
            if let Some((db_str, _)) = split_agy_locator(&identity) {
                return Path::new(db_str).is_file();
            }
            return false;
        }
        source.path.exists()
    }

    fn enumerate(&self, options: &ScanOptions) -> Result<Vec<SessionSource>> {
        let mut sources = Vec::new();

        let should_skip = |entry: &walkdir::DirEntry| -> bool {
            if entry.file_type().is_dir() {
                let name = entry.file_name().to_string_lossy();
                name == ".git"
                    || name == "node_modules"
                    || name == "tasks"
                    || name == "scratch"
                    || name == "steps"
                    || name == "plugins"
                    || name == "target"
                    || name == "dist"
                    || name == ".venv"
                    // Antigravity IDE's sandboxed scratch projects: full npm/next.js
                    // scaffolds (package.json, tsconfig.json, ...) that happen to live
                    // under `.gemini/antigravity-ide/`.
                    || name == "playground"
            } else {
                false
            }
        };

        let roots_to_scan = if !options.custom_paths.is_empty() {
            options.custom_paths.clone()
        } else {
            let mut roots = Vec::new();
            for r in self.session_roots() {
                if r.exists() && !roots.iter().any(|existing: &PathBuf| r.starts_with(existing)) {
                    roots.push(r);
                }
            }
            roots
        };

        for root in roots_to_scan {
            if root.is_file() {
                if is_candidate_gemini_file(&root) {
                    let adapter_name = detect_product_identity(&root);
                    if let Ok(source) = SessionSource::from_path_with_known(&root, adapter_name, &options.known_sources) {
                        sources.push(source);
                    }
                }
            } else if root.is_dir() {
                for entry in WalkDir::new(&root)
                    .into_iter()
                    .filter_entry(|e| !should_skip(e))
                    .filter_map(|e| e.ok())
                {
                    let path = entry.path();
                    if path.is_file() && is_candidate_gemini_file(path) {
                        let adapter_name = detect_product_identity(path);
                        if let Ok(source) = SessionSource::from_path_with_known(path, adapter_name, &options.known_sources) {
                            sources.push(source);
                        }
                    }
                }
            }
        }

        // Antigravity CLI (`agy`) conversation store: SQLite, enumerated
        // separately because each conversation needs its workspace from the
        // summaries index before its identity path can be built.
        sources.extend(enumerate_agy_conversations(options));

        // Deduplicate sources by canonical path
        sources.sort_by(|a, b| a.path.cmp(&b.path));
        sources.dedup_by(|a, b| a.path == b.path);

        Ok(sources)
    }

    fn parse(&self, source: &SessionSource) -> Result<ParseResult> {
        if is_agy_locator(&source.path.to_string_lossy()) {
            return parse_agy_conversation(source);
        }
        let file = File::open(&source.path)?;
        let reader = BufReader::new(file);

        let adapter_identity = detect_product_identity(&source.path);
        let session_id = derive_session_id(&source.path);
        let provenance = Provenance::new(
            source.path.to_string_lossy().to_string(),
            adapter_identity,
            source.file_size_bytes,
            source.mtime_epoch_secs,
            &source.fingerprint,
        );

        let mut trace = AgentWorthTrace::new(&session_id, adapter_identity, provenance, Utc::now());
        let mut malformed_lines = 0;
        let mut warnings = Vec::new();
        let mut sequence = 0u64;
        let mut last_model: Option<String> = None;
        let mut usage_ledger = UsageLedger::default();

        let mut earliest_ts: Option<DateTime<Utc>> = None;
        let mut latest_ts: Option<DateTime<Utc>> = None;

        for (line_idx, line_res) in reader.lines().enumerate() {
            let line_num = line_idx + 1;
            let line_str = match line_res {
                Ok(l) => l,
                Err(e) => {
                    malformed_lines += 1;
                    warnings.push(format!("I/O read error on line {}: {}", line_num, e));
                    continue;
                }
            };

            let trimmed = line_str.trim();
            if trimmed.is_empty() {
                continue;
            }

            let val: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(e) => {
                    malformed_lines += 1;
                    warnings.push(format!("JSON syntax error on line {}: {}", line_num, e));
                    continue;
                }
            };

            let timestamp = parse_timestamp(&val).unwrap_or_else(Utc::now);
            if earliest_ts.is_none_or(|ts| timestamp < ts) {
                earliest_ts = Some(timestamp);
            }
            if latest_ts.is_none_or(|ts| timestamp > ts) {
                latest_ts = Some(timestamp);
            }

            let events = parse_gemini_record(&val, &mut sequence, timestamp, line_num, &mut last_model, &mut usage_ledger);
            trace.events.extend(events);
        }

        if let Some(earliest) = earliest_ts {
            trace.started_at = earliest;
        }
        if let Some(latest) = latest_ts {
            trace.ended_at = Some(latest);
        }

        backfill_shell_exit_codes(&mut trace.events);
        trace.recalculate_stats();

        Ok(ParseResult {
            trace,
            malformed_lines,
            warnings,
        })
    }
}

fn is_candidate_gemini_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    if !path_str.contains("gemini") && !path_str.contains("antigravity") {
        return false;
    }
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if filename.starts_with('.') && !filename.ends_with(".jsonl") && !filename.ends_with(".json") {
        return false;
    }
    let lower = filename.to_lowercase();
    if lower == "config.json"
        || lower == "settings.json"
        || lower == "credentials.json"
        || lower == "oauth.json"
        || lower == "oauth_creds.json"
        || lower == "trustedfolders.json"
        || lower == "projects.json"
        || lower == "google_accounts.json"
        || lower == "state.json"
        || lower == "package.json"
        || lower == "package-lock.json"
        || lower == "tsconfig.json"
        || lower == "hooks.json"
        || lower == "mcp_config.json"
        || lower == "import_manifest.json"
        || lower == "manifest.json"
    {
        return false;
    }
    // Plain Gemini CLI keeps real conversation content in `.json`, not just `.jsonl`:
    // `tmp/<project>/logs.json` (a flat message array) and `tmp/<project>/chats/
    // session-<ts>-<id>.json[l]` both carry real turns, confirmed by content. Everywhere
    // else, a non-jsonl `.json` sampled under `.gemini`/`.antigravity` on a real machine
    // turned out to be a sidecar (`*.metadata.json`), an MCP tool schema, or IDE/extension
    // config -- never a transcript -- so `.json` is accepted only for these two shapes.
    // `starts_with("session-")` alone is too loose here: it also matches Antigravity
    // planning sidecars like `SESSION-REPORT.md.metadata.json`, so also require the
    // character right after the prefix to be a digit -- the real filenames always
    // continue with a year (`session-2026-...`).
    let is_session_log_name = lower
        .strip_prefix("session-")
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| c.is_ascii_digit());
    if lower == "logs.json" || is_session_log_name {
        return filename.ends_with(".jsonl") || filename.ends_with(".json");
    }
    filename.ends_with(".jsonl")
}

fn derive_session_id(path: &Path) -> String {
    // If inside brain/<conversation-id>/..., try to extract conversation-id
    let components: Vec<_> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();

    for (i, part) in components.iter().enumerate() {
        if (part == "brain" || part == "sessions" || part == "history") && i + 1 < components.len()
        {
            let next_comp = &components[i + 1];
            if !next_comp.is_empty() && !next_comp.starts_with('.') {
                return next_comp.clone();
            }
        }
    }

    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

fn parse_timestamp(val: &Value) -> Option<DateTime<Utc>> {
    if let Some(ts_str) = val.get("created_at").and_then(|v| v.as_str()) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    if let Some(ts_str) = val.get("timestamp").and_then(|v| v.as_str()) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    if let Some(millis) = val.get("timestamp").and_then(|v| v.as_i64()) {
        return DateTime::from_timestamp_millis(millis);
    }
    None
}

fn extract_token_usage(usage_val: &Value) -> TokenUsage {
    let input_tokens = usage_val
        .get("promptTokenCount")
        .or_else(|| usage_val.get("input_tokens"))
        .or_else(|| usage_val.get("prompt_tokens"))
        // Gemini CLI's own `tokens` block, measured 2026-09-10 on
        // `~/.gemini/tmp/*/chats/session-*.jsonl`:
        // `{"input":14549,"output":70,"cached":0,"thoughts":240,"tool":0,"total":14859}`.
        // None of the camelCase or OpenAI-shaped names above match it, so before this every
        // Gemini CLI session on this machine indexed as zero tokens despite 25M real ones.
        .or_else(|| usage_val.get("input"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let output_tokens = usage_val
        .get("candidatesTokenCount")
        .or_else(|| usage_val.get("output_tokens"))
        .or_else(|| usage_val.get("completion_tokens"))
        .or_else(|| usage_val.get("output"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        // `thoughts` and `tool` are billed as output and are counted in Gemini's own `total`,
        // so folding them into output_tokens keeps `TokenUsage::total()` equal to it.
        .saturating_add(
            usage_val
                .get("thoughts")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        )
        .saturating_add(usage_val.get("tool").and_then(|v| v.as_u64()).unwrap_or(0));

    let cache_read_tokens = usage_val
        .get("cachedContentTokenCount")
        .or_else(|| usage_val.get("cached_tokens"))
        .or_else(|| usage_val.get("cache_read_tokens"))
        .or_else(|| usage_val.get("cached"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let cache_creation_tokens = usage_val
        .get("cache_creation_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    TokenUsage::new(
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
    )
}

fn parse_gemini_record(
    val: &Value,
    seq: &mut u64,
    ts: DateTime<Utc>,
    line_num: usize,
    last_model: &mut Option<String>,
    usage_ledger: &mut UsageLedger,
) -> Vec<NormalizedEvent> {
    let mut events = Vec::new();
    let raw_ref = format!("line:{}", line_num);

    // Model invocation / Token usage extraction
    if let Some(usage_val) = val
        .get("usageMetadata")
        .or_else(|| val.get("usage"))
        .or_else(|| val.get("tokens"))
    {
        // Gemini CLI repeats a message's whole `tokens` block on every streamed revision of
        // that message, keyed by the record's `id` -- measured 1.90x inflation across this
        // machine's chat logs. See `crate::usage_ledger::UsageLedger`.
        let message_id = val
            .get("id")
            .and_then(|v| v.as_str())
            .or_else(|| val.get("message").and_then(|m| m.get("id")).and_then(|v| v.as_str()));
        let usage = usage_ledger.credit_delta(message_id, extract_token_usage(usage_val));
        if usage.total() > 0 {
            let model = val
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("gemini-2.5-pro")
                .to_string();

            if last_model.as_deref() != Some(model.as_str()) {
                if let Some(prev) = last_model.take() {
                    *seq += 1;
                    events.push(
                        NormalizedEvent::new(
                            *seq,
                            ts,
                            EventPayload::ModelSwitch(ModelSwitch {
                                from_model: Some(prev),
                                to_model: model.clone(),
                                reason: None,
                            }),
                        )
                        .with_raw_ref(&raw_ref),
                    );
                }
                *last_model = Some(model.clone());
            }

            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::ModelInvocation {
                        model,
                        token_usage: usage,
                        cost_usd: val.get("cost").and_then(|c| c.as_f64()),
                        latency_ms: val.get("latency_ms").and_then(|d| d.as_u64()),
                        effort: None,
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    // Direct error check
    let is_step_error = val
        .get("status")
        .and_then(|s| s.as_str())
        .map(|s| s == "ERROR")
        .unwrap_or(false);

    if is_step_error {
        let msg = val
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("Step failed with ERROR");
        *seq += 1;
        events.push(
            NormalizedEvent::new(
                *seq,
                ts,
                EventPayload::Error {
                    message: msg.to_string(),
                    is_recovered: false,
                },
            )
            .with_raw_ref(&raw_ref),
        );
    }

    let raw_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let raw_role = val.get("role").and_then(|v| v.as_str()).unwrap_or("");
    let effective_type = if !raw_type.is_empty() {
        raw_type
    } else {
        raw_role
    };

    match effective_type {
        "USER_INPUT" | "USER_EXPLICIT" | "user" | "human" => {
            let content = extract_gemini_content(val);
            *seq += 1;
            events.push(
                NormalizedEvent::new(*seq, ts, EventPayload::UserMessage { content })
                    .with_raw_ref(&raw_ref),
            );
        }

        "PLANNER_RESPONSE" | "MODEL" | "assistant" | "model" => {
            let thinking = val
                .get("thinking")
                .and_then(|v| v.as_str())
                .map(String::from);

            let content = extract_gemini_content(val);

            // Extract tool calls / function calls
            if let Some(tool_calls_arr) = val
                .get("tool_calls")
                .or_else(|| val.get("function_calls"))
                .and_then(|v| v.as_array())
            {
                for tc in tool_calls_arr {
                    let id = tc.get("id").and_then(|v| v.as_str()).map(String::from);
                    let raw_name = tc
                        .get("name")
                        .or_else(|| tc.get("functionName"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();

                    let args = tc
                        .get("arguments")
                        .or_else(|| tc.get("args"))
                        .cloned()
                        .unwrap_or(Value::Null);

                    let name = normalize_mcp_tool_name(&raw_name, &args);

                    *seq += 1;
                    events.push(
                        NormalizedEvent::new(
                            *seq,
                            ts,
                            EventPayload::ToolCall(ToolCall {
                                id: id.clone(),
                                name: name.clone(),
                                arguments: args.clone(),
                            }),
                        )
                        .with_raw_ref(&raw_ref),
                    );

                    process_specific_gemini_tool_call(&raw_name, &name, &args, seq, ts, &raw_ref, &mut events);
                }
            }

            // Also check parts with functionCall
            if let Some(parts_arr) = val.get("parts").and_then(|v| v.as_array()) {
                for part in parts_arr {
                    if let Some(fc) = part.get("functionCall") {
                        let raw_name = fc
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let args = fc.get("args").cloned().unwrap_or(Value::Null);
                        let name = normalize_mcp_tool_name(&raw_name, &args);

                        *seq += 1;
                        events.push(
                            NormalizedEvent::new(
                                *seq,
                                ts,
                                EventPayload::ToolCall(ToolCall {
                                    id: None,
                                    name: name.clone(),
                                    arguments: args.clone(),
                                }),
                            )
                            .with_raw_ref(&raw_ref),
                        );

                        process_specific_gemini_tool_call(
                            &raw_name,
                            &name,
                            &args,
                            seq,
                            ts,
                            &raw_ref,
                            &mut events,
                        );
                    }
                }
            }

            if !content.is_empty() || thinking.is_some() {
                *seq += 1;
                events.push(
                    NormalizedEvent::new(
                        *seq,
                        ts,
                        EventPayload::AssistantMessage { content, thinking },
                    )
                    .with_raw_ref(&raw_ref),
                );
            }
        }

        "TOOL_OUTPUT" | "tool_result" | "function_response" | "tool" | "function" => {
            let call_id = val
                .get("call_id")
                .or_else(|| val.get("tool_call_id"))
                .and_then(|v| v.as_str())
                .map(String::from);
            let is_error = val
                .get("is_error")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let output = val
                .get("output")
                .or_else(|| val.get("content"))
                .or_else(|| val.get("response"))
                .cloned()
                .unwrap_or(Value::Null);

            let tool_name = val
                .get("name")
                .and_then(|v| v.as_str())
                .map(|n| normalize_mcp_tool_name(n, &Value::Null));

            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::ToolResult(ToolResult {
                        call_id,
                        name: tool_name,
                        output: output.clone(),
                        is_error,
                    }),
                )
                .with_raw_ref(&raw_ref),
            );

            // Infer outcome evidence
            if let Some(out_str) = output.as_str() {
                if out_str.contains("test result: ok.")
                    || out_str.contains("PASSED")
                    || out_str.contains("100% tests passed")
                {
                    *seq += 1;
                    events.push(
                        NormalizedEvent::new(
                            *seq,
                            ts,
                            EventPayload::OutcomeEvidence(OutcomeEvidence {
                                kind: OutcomeKind::TestOrBuildPassed,
                                summary: "Test suite executed successfully".to_string(),
                                confidence: 0.9,
                            }),
                        )
                        .with_raw_ref(&raw_ref),
                    );
                } else if out_str.contains("[main ")
                    || out_str.contains("commit ")
                    || out_str.contains("files changed,")
                {
                    *seq += 1;
                    events.push(
                        NormalizedEvent::new(
                            *seq,
                            ts,
                            EventPayload::OutcomeEvidence(OutcomeEvidence {
                                kind: OutcomeKind::CommitObserved,
                                summary: "Git commit observed in tool result".to_string(),
                                confidence: 0.85,
                            }),
                        )
                        .with_raw_ref(&raw_ref),
                    );
                }
            }
        }

        _ => {
            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::Custom {
                        kind: effective_type.to_string(),
                        data: val.clone(),
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    events
}

fn process_specific_gemini_tool_call(
    raw_name: &str,
    name: &str,
    args: &Value,
    seq: &mut u64,
    ts: DateTime<Utc>,
    raw_ref: &str,
    events: &mut Vec<NormalizedEvent>,
) {
    let lower_raw = raw_name.to_lowercase();
    let lower_name = name.to_lowercase();
    if lower_raw == "run_command"
        || lower_raw == "bash"
        || lower_raw == "exec"
        || lower_raw == "shell"
        || lower_name.ends_with(":run_command")
        || lower_name.ends_with(":bash")
        || lower_name.ends_with(":shell")
        || lower_name.ends_with(":exec")
    {
        if let Some(cmd) = args
            .get("CommandLine")
            .or_else(|| args.get("command"))
            .or_else(|| args.get("cmd"))
            .and_then(|v| v.as_str())
        {
            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::ShellCommand(ShellCommand {
                        command: cmd.to_string(),
                        cwd: args
                            .get("Cwd")
                            .or_else(|| args.get("cwd"))
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        exit_code: None,
                        output: None,
                    }),
                )
                .with_raw_ref(raw_ref),
            );
        }
    } else if lower_raw == "replace_file_content"
        || lower_raw == "write_to_file"
        || lower_raw == "edit"
        || lower_raw == "edit_file"
        || lower_name.ends_with(":replace_file_content")
        || lower_name.ends_with(":write_to_file")
        || lower_name.ends_with(":edit_file")
        || lower_name.ends_with(":edit")
    {
        let path = args
            .get("TargetFile")
            .or_else(|| args.get("target_file"))
            .or_else(|| args.get("path"))
            .or_else(|| args.get("file_path"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if !path.is_empty() {
            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::FileAction {
                        path,
                        action: FileActionType::Edit,
                        diff: args
                            .get("ReplacementContent")
                            .or_else(|| args.get("diff"))
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        lines_changed: None,
                    },
                )
                .with_raw_ref(raw_ref),
            );
        }
    }
}

fn extract_gemini_content(val: &Value) -> String {
    if let Some(text) = val.get("content").and_then(|v| v.as_str()) {
        return text.to_string();
    }
    if let Some(parts) = val.get("parts").and_then(|v| v.as_array()) {
        let mut texts = Vec::new();
        for p in parts {
            if let Some(t) = p.get("text").and_then(|v| v.as_str()) {
                texts.push(t);
            }
        }
        if !texts.is_empty() {
            return texts.join("\n");
        }
    }
    if let Some(c) = val.get("content") {
        if !c.is_null() {
            return c.to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::{tempdir, NamedTempFile};

    #[test]
    fn test_parse_standard_gemini_jsonl() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"type":"USER_INPUT","created_at":"2026-08-29T10:00:00Z","content":"Implement a new feature"}
{"type":"PLANNER_RESPONSE","created_at":"2026-08-29T10:00:05Z","model":"gemini-2.5-pro","thinking":"I will run cargo check first.","usageMetadata":{"promptTokenCount":600,"candidatesTokenCount":180,"cachedContentTokenCount":150},"tool_calls":[{"id":"call_1","name":"run_command","arguments":{"CommandLine":"cargo test","Cwd":"/workspace"}}],"content":"Running test suite..."}
{"type":"TOOL_OUTPUT","created_at":"2026-08-29T10:00:08Z","call_id":"call_1","output":"test result: ok. 8 passed; 0 failed","is_error":false}
{"type":"PLANNER_RESPONSE","created_at":"2026-08-29T10:00:10Z","content":"Feature implemented and all tests pass."}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = GeminiAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 0);
        let trace = result.trace;
        // A bare tempfile path carries no antigravity signature (no `/brain/`, no
        // `.antigravity`), so detect_product_identity() correctly resolves this to the
        // "gemini" default rather than the old hardcoded "antigravity" name.
        assert_eq!(trace.adapter, "gemini");
        assert_eq!(trace.stats.models_used, vec!["gemini-2.5-pro".to_string()]);
        assert_eq!(trace.stats.token_usage.input_tokens, 600);
        assert_eq!(trace.stats.token_usage.output_tokens, 180);
        assert_eq!(trace.stats.token_usage.cache_read_tokens, 150);
        assert_eq!(trace.stats.token_usage.total(), 930);
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(trace.stats.tools_used.get("run_command"), Some(&1));
        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.assistant_messages_count, 2);
    }

    #[test]
    fn test_parse_gemini_chat_parts_format() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"role":"user","timestamp":"2026-08-29T11:00:00Z","parts":[{"text":"Please edit file"}]}
{"role":"model","timestamp":"2026-08-29T11:00:02Z","model":"gemini-3.7-sonnet","tokens":{"prompt_tokens":300,"completion_tokens":90},"parts":[{"functionCall":{"name":"replace_file_content","args":{"TargetFile":"/src/main.rs","ReplacementContent":"fn main() {}"}}},{"text":"File replaced."}]}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = GeminiAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        let trace = result.trace;
        assert_eq!(
            trace.stats.models_used,
            vec!["gemini-3.7-sonnet".to_string()]
        );
        assert_eq!(trace.stats.token_usage.input_tokens, 300);
        assert_eq!(trace.stats.token_usage.output_tokens, 90);
        assert_eq!(trace.stats.tool_calls_count, 1);
    }

    #[test]
    fn test_parse_graceful_on_empty_and_corrupt_lines() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = "{\"type\":\"USER_INPUT\",\"content\":\"hello\"}\n\n{INVALID_JSON}\n{\"type\":\"PLANNER_RESPONSE\",\"content\":\"world\"}\n";
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = GeminiAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 1);
        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
    }

    #[test]
    fn test_detect_and_enumerate_gemini_and_antigravity() {
        let temp = tempdir().unwrap();

        // 1. Gemini CLI history
        let gemini_dir = temp.path().join(".gemini").join("history");
        std::fs::create_dir_all(&gemini_dir).unwrap();
        let gemini_file = gemini_dir.join("history_001.jsonl");
        let mut f1 = File::create(&gemini_file).unwrap();
        writeln!(f1, "{{\"type\":\"USER_INPUT\",\"content\":\"gemini prompt\"}}").unwrap();

        // 2. Antigravity CLI brain trajectory
        let agy_dir = temp.path().join(".gemini").join("antigravity-cli").join("brain").join("sess-1").join(".system_generated").join("logs");
        std::fs::create_dir_all(&agy_dir).unwrap();
        let agy_file = agy_dir.join("transcript.jsonl");
        let mut f2 = File::create(&agy_file).unwrap();
        writeln!(f2, "{{\"type\":\"USER_INPUT\",\"content\":\"agy prompt\"}}").unwrap();

        let adapter = GeminiAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
            ..Default::default()
        };

        let detection = adapter.detect(&options).unwrap();
        assert!(detection.is_present);

        let enumerated = adapter.enumerate(&options).unwrap();
        assert_eq!(enumerated.len(), 2);

        let gemini_src = enumerated.iter().find(|s| s.path == gemini_file).unwrap();
        assert_eq!(gemini_src.adapter_name, "gemini");

        let agy_src = enumerated.iter().find(|s| s.path == agy_file).unwrap();
        assert_eq!(agy_src.adapter_name, "antigravity");

        // Parsing verifies trace.adapter is correctly assigned
        let gemini_parsed = adapter.parse(gemini_src).unwrap();
        assert_eq!(gemini_parsed.trace.adapter, "gemini");

        let agy_parsed = adapter.parse(agy_src).unwrap();
        assert_eq!(agy_parsed.trace.adapter, "antigravity");
    }

    /// Regression test: plain Gemini CLI (not Antigravity) keeps real per-project
    /// conversation logs at `tmp/<project>/chats/session-<ts>-<id>.json[l]` and
    /// `tmp/<project>/logs.json` -- both real content, confirmed on a live machine --
    /// which the old bare-`.gemini` recursive walk found only by accident. They must
    /// still be found now that discovery is scoped to named subdirectories, and the
    /// `.metadata.json` sidecars plus a filename that merely starts with "session-" but
    /// isn't the real timestamped shape (e.g. an Antigravity planning doc sidecar) must
    /// still be rejected.
    #[test]
    fn test_enumerate_gemini_finds_plain_cli_tmp_logs_and_rejects_lookalikes() {
        let temp = tempdir().unwrap();

        let chats_dir = temp.path().join(".gemini").join("tmp").join("my-project").join("chats");
        std::fs::create_dir_all(&chats_dir).unwrap();
        let mut real_session = File::create(chats_dir.join("session-2026-01-07T06-44-f969d3b9.json")).unwrap();
        writeln!(real_session, "{{\"sessionId\":\"f969d3b9\",\"messages\":[]}}").unwrap();

        let logs_json = temp.path().join(".gemini").join("tmp").join("my-project").join("logs.json");
        let mut logs = File::create(&logs_json).unwrap();
        writeln!(logs, "[{{\"sessionId\":\"abc\",\"type\":\"user\",\"message\":\"hi\"}}]").unwrap();

        // Lookalike: starts with "session-" but is an Antigravity planning-doc sidecar,
        // not a timestamped session log -- must NOT be treated as real content.
        let brain_dir = temp.path().join(".gemini").join("antigravity-cli").join("brain").join("sess-2");
        std::fs::create_dir_all(&brain_dir).unwrap();
        File::create(brain_dir.join("SESSION-REPORT.md.metadata.json")).unwrap();

        let adapter = GeminiAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
            ..Default::default()
        };

        let enumerated = adapter.enumerate(&options).unwrap();
        let paths: Vec<_> = enumerated.iter().map(|s| s.path.clone()).collect();
        assert!(paths.contains(&chats_dir.join("session-2026-01-07T06-44-f969d3b9.json")));
        assert!(paths.contains(&logs_json));
        assert_eq!(enumerated.len(), 2, "the metadata.json lookalike must be rejected");
    }

    #[test]
    fn test_parse_gemini_mcp_tool_call() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"type":"USER_INPUT","created_at":"2026-08-29T10:00:00Z","content":"Browse webpage"}
{"type":"PLANNER_RESPONSE","created_at":"2026-08-29T10:00:02Z","model":"gemini-2.5-pro","tool_calls":[{"id":"call_mcp_1","name":"call_mcp_tool","arguments":{"ServerName":"chrome-devtools","ToolName":"navigate_page","Arguments":{"Url":"https://example.com"}}}]}
{"type":"TOOL_OUTPUT","created_at":"2026-08-29T10:00:04Z","call_id":"call_mcp_1","name":"call_mcp_tool","output":"Navigated successfully"}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = GeminiAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        let trace = result.trace;
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(
            trace.stats.tools_used.get("mcp:chrome-devtools:navigate_page"),
            Some(&1)
        );
    }

    // --- Antigravity CLI (`agy`) conversation store ---

    /// Encode one protobuf varint (test fixtures only).
    fn agy_pb_varint(mut v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        }
        out
    }

    /// Encode one protobuf length-delimited field (test fixtures only).
    fn agy_pb_str(field_no: u32, s: &str) -> Vec<u8> {
        let mut out = agy_pb_varint(((field_no << 3) | 2) as u64);
        out.extend(agy_pb_varint(s.len() as u64));
        out.extend(s.as_bytes());
        out
    }

    fn agy_join(parts: Vec<Vec<u8>>) -> Vec<u8> {
        parts.into_iter().flatten().collect()
    }

    /// Fixture conversation DB plus its summaries row and history file.
    /// Returns `(db_path, summaries_path, history_path)`. Shapes mirror the
    /// real store: uuid-keyed steps with id noise, a tool call with JSON
    /// args, a `[Message]` record with a real timestamp, a persona step that
    /// must be skipped, and an unmapped step type.
    fn agy_fixture_store(dir: &Path, conv_id: &str) -> (PathBuf, PathBuf, PathBuf) {
        use rusqlite::{params, Connection};

        let conv_dir = dir.join("conversations");
        std::fs::create_dir_all(&conv_dir).unwrap();
        let db_path = conv_dir.join(format!("{conv_id}.db"));
        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "CREATE TABLE steps (idx INTEGER PRIMARY KEY, step_type INTEGER NOT NULL DEFAULT 0, \
             status INTEGER NOT NULL DEFAULT 0, step_payload BLOB, step_format INTEGER NOT NULL DEFAULT 0)",
            [],
        )
        .unwrap();
        let user = agy_join(vec![
            agy_pb_str(1, "550e8400-e29b-41d4-a716-446655440000"),
            agy_pb_str(4, "Investigate token density in agent runtimes"),
        ]);
        let tool = agy_join(vec![
            agy_pb_str(1, "call_9"),
            agy_pb_str(2, "grep_search"),
            agy_pb_str(3, r#"{"Query":"DocIR"}"#),
        ]);
        let assistant = agy_join(vec![
            agy_pb_str(1, "Considering the request, a deep dive seems necessary"),
            // Tool-call echoes the cleaner must strip (see `clean_assistant`).
            agy_pb_str(2, "call_9"),
            agy_pb_str(3, "grep_search"),
            agy_pb_str(4, "0SWmaoty3N-DxQ_kj4zgAQ"),
        ]);
        let message = agy_join(vec![agy_pb_str(
            1,
            "[Message] timestamp=2026-09-13T04:38:05Z sender=fe75ef85 priority=MESSAGE_PRIORITY_HIGH content=## Findings",
        )]);
        let persona = agy_join(vec![agy_pb_str(1, "[AGENT PERSONA] You are Donna")]);
        let unknown = agy_join(vec![agy_pb_str(1, "whatever")]);
        for (i, (step_type, payload)) in
            [(14, user), (132, tool), (15, assistant), (101, message), (90, persona), (777, unknown)]
                .into_iter()
                .enumerate()
        {
            conn.execute(
                "INSERT INTO steps (idx, step_type, status, step_payload) VALUES (?, ?, 3, ?)",
                params![i as i64, step_type, payload],
            )
            .unwrap();
        }

        let summaries_path = dir.join("conversation_summaries.db");
        let sums = Connection::open(&summaries_path).unwrap();
        sums
            .execute(
                "CREATE TABLE conversation_summaries (conversation_id TEXT PRIMARY KEY, title TEXT, \
                 agent_name TEXT, status TEXT, killed INTEGER, last_modified_time TEXT, workspace_uris TEXT)",
                [],
            )
            .unwrap();
        let workspace = dir.join("proj");
        std::fs::create_dir_all(&workspace).unwrap();
        sums
            .execute(
                "INSERT INTO conversation_summaries VALUES (?, ?, ?, ?, ?, ?, ?)",
                params![
                    conv_id,
                    "DocIR feasibility",
                    "research",
                    "CASCADE_RUN_STATUS_IDLE",
                    0,
                    "2026-09-13 05:00:00+00:00",
                    format!("[\"file://{}\"]", workspace.to_string_lossy()),
                ],
            )
            .unwrap();

        let history_path = dir.join("history.jsonl");
        std::fs::write(
            &history_path,
            format!(
                "{{\"conversationId\":\"{conv_id}\",\"display\":\"Investigate token density in agent runtimes\",\"timestamp\":1789000000000,\"workspace\":\"{}\"}}\n",
                workspace.to_string_lossy(),
            ),
        )
        .unwrap();
        (db_path, summaries_path, history_path)
    }

    #[test]
    fn agy_wire_walk_extracts_nested_strings() {
        let inner = agy_pb_str(2, "deep");
        let mut outer = agy_pb_varint(((1 << 3) | 2) as u64);
        outer.extend(agy_pb_varint(inner.len() as u64));
        outer.extend(inner);
        outer.extend(agy_pb_str(3, "shallow"));

        let mut fields = Vec::new();
        assert!(pb_walk_strings(&outer, &mut fields));
        assert!(fields.contains(&(2, "deep".to_string())));
        assert!(fields.contains(&(3, "shallow".to_string())));

        let mut bad = Vec::new();
        assert!(!pb_walk_strings(
            b"\xff\xff\xff\xff\xff\xff\xff\xff\xff\x7f",
            &mut bad
        ));
    }

    #[test]
    fn agy_parse_conversation_db() {
        let temp = tempdir().unwrap();
        let conv_id = "31562521-ce8f-47e0-89f8-901594ae66c6";
        let (db_path, summaries_path, history_path) = agy_fixture_store(temp.path(), conv_id);
        let short: String = conv_id.chars().take(8).collect();

        let identity = format!(
            "{}/.agy-conversation-{short}.sqlite{AGY_REPO_MARKER}{}#{conv_id}",
            temp.path().join("proj").to_string_lossy(),
            db_path.to_string_lossy(),
        );
        let meta = std::fs::metadata(&db_path).unwrap();
        let source = SessionSource {
            path: PathBuf::from(identity),
            adapter_name: "antigravity".to_string(),
            file_size_bytes: meta.len(),
            mtime_epoch_secs: 1789000100,
            fingerprint: "test".to_string(),
        };
        let result =
            parse_agy_conversation_with_store(&source, Some(&summaries_path), Some(&history_path))
                .expect("agy parse failed");

        assert_eq!(result.trace.adapter, "antigravity");
        // user + tool + assistant + [Message]: persona skipped, 777 warned.
        assert_eq!(result.trace.events.len(), 4);
        assert_eq!(result.malformed_lines, 1);

        match &result.trace.events[0].payload {
            EventPayload::UserMessage { content } => {
                assert!(content.contains("Investigate token density"))
            }
            other => panic!("expected UserMessage, got {other:?}"),
        }
        match &result.trace.events[1].payload {
            EventPayload::ToolCall(call) => {
                assert_eq!(call.name, "grep_search");
                assert_eq!(call.id.as_deref(), Some("call_9"));
                assert_eq!(
                    call.arguments.get("Query").and_then(|q| q.as_str()),
                    Some("DocIR")
                );
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
        match &result.trace.events[3].payload {
            EventPayload::AssistantMessage { content, .. } => {
                assert!(content.contains("## Findings"))
            }
            other => panic!("expected AssistantMessage, got {other:?}"),
        }
        // The type-15 assistant event keeps its prose but drops the echoed
        // call id, tool name, and span id.
        match &result.trace.events[2].payload {
            EventPayload::AssistantMessage { content, .. } => {
                assert_eq!(content, "Considering the request, a deep dive seems necessary");
            }
            other => panic!("expected AssistantMessage, got {other:?}"),
        }
        // History timestamp starts the trace; the summaries row ends it.
        assert_eq!(
            result.trace.started_at,
            DateTime::from_timestamp_millis(1789000000000).unwrap()
        );
        assert_eq!(result.trace.ended_at.unwrap().to_rfc3339(), "2026-09-13T05:00:00+00:00");
    }

    #[test]
    fn agy_enumerate_anchors_identity_on_workspace() {
        let temp = tempdir().unwrap();
        let conv_id = "92e14033-3fe4-492d-88b1-eb388b0de6e4";
        agy_fixture_store(temp.path(), conv_id);
        let options = ScanOptions {
            custom_paths: vec![],
            force: false,
            ..Default::default()
        };
        let sources = enumerate_agy_conversations_in(
            &temp.path().join("conversations"),
            Some(&temp.path().join("conversation_summaries.db")),
            &options,
        );
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].adapter_name, "antigravity");
        let identity = sources[0].path.to_string_lossy().to_string();
        assert!(identity.contains(AGY_REPO_MARKER));
        assert!(identity.contains("proj/.agy-conversation-92e14033.sqlite"));
        assert!(is_agy_locator(&identity));
    }

    #[test]
    fn agy_enumerate_falls_back_to_history_workspace() {
        // Most summaries rows carry NULL workspace_uris; the rolling history
        // file records the workspace with every prompt, so enumeration must
        // still anchor instead of emitting a bare locator.
        let temp = tempdir().unwrap();
        let conv_id = "92e14033-3fe4-492d-88b1-eb388b0de6e4";
        agy_fixture_store(temp.path(), conv_id);
        let options = ScanOptions {
            custom_paths: vec![],
            force: false,
            ..Default::default()
        };
        let sources = enumerate_agy_conversations_in(
            &temp.path().join("conversations"),
            None,
            &options,
        );
        assert_eq!(sources.len(), 1);
        let identity = sources[0].path.to_string_lossy().to_string();
        assert!(identity.contains("proj/.agy-conversation-92e14033.sqlite"));
    }

    #[test]
    fn agy_identity_falls_back_when_workspace_is_a_container() {
        // A workspace like `~/code` itself would resolve to the synthetic leaf
        // through the `/code/` rule -- the bare locator is used instead.
        let locator = "/Users/saurabh/.gemini/antigravity-cli/conversations/abc.db#abc";
        let anchored =
            agy_identity_for(Some("/Users/saurabh/code".to_string()), "abc", locator);
        assert_eq!(anchored, locator);
        assert!(!anchored.contains(AGY_REPO_MARKER));
        assert!(is_agy_locator(&anchored));
    }
}

