use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use agentworth_adapter_sdk::{
    reuse_or_compute_fingerprint, AgentAdapter, DetectionResult, ParseResult, ScanOptions,
    SessionSource,
};
use agentworth_schema::{
    AgentWorthTrace, CompactionEvent, EventPayload, FileActionType, ModelSwitch, NormalizedEvent,
    OutcomeEvidence, OutcomeKind, Provenance, ShellCommand, TokenUsage, ToolCall, ToolResult,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use directories::BaseDirs;
use serde_json::Value;
use walkdir::WalkDir;
use crate::exit_status::backfill_shell_exit_codes;

/// Adapter for discovering and normalizing Pi agent task sessions and step logs.
pub struct PiAdapter;

impl Default for PiAdapter {
    fn default() -> Self {
        Self
    }
}

impl PiAdapter {
    pub fn new() -> Self {
        Self
    }

    /// 2: the v1 parser matched a session shape pi never wrote
    /// (`task_input`/`step`/`observation` with `prompt_tokens`), so every real
    /// session file parsed to zero events and zero tokens. This version reads
    /// the documented v3 format (`session` header plus `message`,
    /// `model_change`, `compaction` and companion entries). A reparse is what
    /// makes pi sessions appear at all.
    pub const PARSER_VERSION: i64 = 2;

    /// Candidate directory paths for Pi on the host machine.
    pub fn candidate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".pi"));
            roots.push(home.join(".pi").join("tasks"));
            roots.push(home.join(".pi").join("sessions"));
            roots.push(home.join(".config").join("pi"));
        }
        roots.push(PathBuf::from(".pi"));
        roots.push(PathBuf::from(".pi").join("tasks"));
        roots.push(PathBuf::from(".pi").join("sessions"));
        roots
    }
}

// ---------------------------------------------------------------------------
// Pi v3 session files.
//
// Sessions live at `~/.pi/agent/sessions/--<path>--/<timestamp>_<uuid>.jsonl`
// where the `--...--` slug is the working directory with `/` rewritten as
// `-`. Each line is one entry: a `session` header (id, cwd, version) followed
// by `message` entries carrying an `AgentMessage` (user / assistant with
// usage and cost / toolResult / bashExecution with exit codes), plus
// `model_change`, `thinking_level_change`, `compaction`, `branch_summary`,
// `label`, `session_info`, `custom` and `custom_message` entries.
// (See the pi session-format spec; the pre-v2 parser matched a shape pi
// never wrote, which is why the index held zero pi rows.)
// ---------------------------------------------------------------------------

/// Marker separating the synthetic repo-anchored identity prefix from the real
/// file path, mirroring opencode.rs's `OPENCODE_REPO_MARKER`. `SessionSource.path`
/// and `Provenance.source_path` always carry this exact same string: the
/// scanner compares them to decide what changed.
const PI_REPO_MARKER: &str = "::pi-repo::";

/// Recover the real file path from a possibly-marked identity path.
#[allow(
    clippy::string_slice,
    reason = "idx comes from rfind() on an ASCII marker, offset by its own byte length: always a char boundary"
)]
fn strip_pi_marker(path_str: &str) -> &str {
    match path_str.rfind(PI_REPO_MARKER) {
        Some(idx) => &path_str[idx + PI_REPO_MARKER.len()..],
        None => path_str,
    }
}

/// True when `identity` addresses a pi session file: marked, or a plain
/// path inside a sessions slug directory. Other `.pi` files (task outputs
/// and the like) match neither.
fn is_pi_locator(identity: &str) -> bool {
    if identity.contains(PI_REPO_MARKER) {
        return true;
    }
    identity.ends_with(".jsonl") && identity.contains("/sessions/--")
}

/// Decode a session-directory slug (`--Users-saurabh-code-foo--`) back to its
/// working directory (`/Users/saurabh/code/foo`). Dashes inside real names
/// are ambiguous with separators, exactly like Claude Code's own slug
/// decoding; the header `cwd` remains authoritative at parse time.
fn decode_pi_slug(dir_name: &str) -> Option<String> {
    let inner = dir_name.strip_prefix("--")?.strip_suffix("--")?;
    if inner.is_empty() {
        return None;
    }
    Some(format!("/{}", inner.replace('-', "/")))
}

/// Session uuid from a `<timestamp>_<uuid>.jsonl` filename, for the identity
/// leaf and as the session-id fallback when the header is unreadable.
fn pi_file_session_id(path: &Path) -> Option<String> {
    let stem = path.file_stem().and_then(|s| s.to_str())?;
    let id = stem.rsplit('_').next().unwrap_or(stem);
    if id.is_empty() {
        return None;
    }
    Some(id.to_string())
}

/// Build the identity for one candidate file: workspace-anchored with the
/// marker when the sessions slug decodes to a plausible project directory,
/// the bare file path otherwise. The depth bar (more than three components)
/// keeps container directories (`~`, `~/code`, `/tmp/x`) from anchoring:
/// a project checkout is virtually always deeper, and a shallow anchor
/// resolves to the synthetic leaf instead of a repository.
fn pi_identity_for(path: &Path) -> String {
    let real = path.to_string_lossy().to_string();
    let slug_cwd = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .and_then(decode_pi_slug);
    let plausible = slug_cwd.as_deref().is_some_and(|d| {
        d != "/" && d.split('/').filter(|c| !c.is_empty()).count() > 3
    });
    if !plausible {
        return real;
    }
    let dir = slug_cwd.unwrap_or_default();
    let short: String = pi_file_session_id(path)
        .unwrap_or_else(|| "session".to_string())
        .chars()
        .take(8)
        .collect();
    format!("{dir}/.pi-session-{short}.jsonl{PI_REPO_MARKER}{real}")
}

/// Build a [`SessionSource`] for one candidate file, with metadata read from
/// the real file on disk and the (possibly synthetic) identity for change
/// tracking.
fn build_pi_source(
    path: &Path,
    known: &agentworth_adapter_sdk::KnownSourceMap,
) -> Result<SessionSource> {
    let metadata = std::fs::metadata(path)?;
    let size = metadata.len();
    let mtime = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let identity = pi_identity_for(path);
    let fingerprint =
        reuse_or_compute_fingerprint(known, &identity, path, size, mtime)?;
    Ok(SessionSource {
        path: PathBuf::from(identity),
        adapter_name: "pi".to_string(),
        file_size_bytes: size,
        mtime_epoch_secs: mtime,
        fingerprint,
    })
}

impl AgentAdapter for PiAdapter {
    fn name(&self) -> &'static str {
        "pi"
    }

    fn parser_version(&self) -> i64 {
        Self::PARSER_VERSION
    }

    fn capabilities(&self) -> agentworth_adapter_sdk::AdapterCapabilities {
        agentworth_adapter_sdk::AdapterCapabilities {
            prompts: true,
            tokens: true,
            tools: true,
            shell: true,
            diffs: false,
            thinking: true,
            outcomes: true,
        }
    }

    /// Synthetic pi identities (`::pi-repo::` marker) address a live session
    /// file, not a file at the identity path: strip the marker and check the
    /// real file, mirroring opencode.rs. Everything else keeps the default
    /// file-existence check.
    fn source_exists(&self, source: &SessionSource) -> bool {
        let identity = source.path.to_string_lossy().to_string();
        if identity.contains(PI_REPO_MARKER) {
            return Path::new(strip_pi_marker(&identity)).is_file();
        }
        source.path.exists()
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
            let s = custom.to_string_lossy();
            if custom.ends_with(".pi")
                || custom.ends_with("pi")
                || s.contains("/.pi/")
                || s.contains("/pi/")
            {
                discovered.push(custom.clone());
            } else if custom.is_dir() {
                // custom_paths may point at a generic parent directory rather than
                // the adapter-specific dir itself; look a few levels in before
                // giving up, matching how `enumerate()` already recurses.
                // "pi" is short enough to false-positive on an unrelated substring
                // (e.g. a random tempdir suffix), so match whole path components
                // instead of a bare `contains("pi")`.
                let mut found_nested = false;
                for sub in &[custom.join(".pi"), custom.join(".config").join("pi")] {
                    if sub.exists() {
                        discovered.push(sub.clone());
                        found_nested = true;
                    }
                }
                if !found_nested {
                    for entry in WalkDir::new(custom).max_depth(4).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        let is_pi_component = path.components().any(|c| {
                            let cs = c.as_os_str().to_string_lossy();
                            cs == "pi" || cs == ".pi"
                        });
                        if is_pi_component {
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

        if !options.custom_paths.is_empty() {
            for custom in &options.custom_paths {
                if custom.is_file() {
                    if is_candidate_pi_file(custom) {
                        if let Ok(source) = build_pi_source(custom, &options.known_sources) {
                            sources.push(source);
                        }
                    }
                } else if custom.is_dir() {
                    for entry in WalkDir::new(custom).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_pi_file(path) {
                            if let Ok(source) = build_pi_source(path, &options.known_sources) {
                                sources.push(source);
                            }
                        }
                    }
                }
            }
        } else {
            for root in self.candidate_roots() {
                if root.is_file() {
                    if is_candidate_pi_file(&root) {
                        if let Ok(source) = build_pi_source(&root, &options.known_sources) {
                            sources.push(source);
                        }
                    }
                } else if root.is_dir() {
                    for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_file() && is_candidate_pi_file(path) {
                            if let Ok(source) = build_pi_source(path, &options.known_sources) {
                                sources.push(source);
                            }
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
        // Non-session `.pi` files (task outputs and the like) share the walk
        // but have no entry shape: index nothing for them rather than erroring
        // the scan. Session files always live under a `--slug--` sessions
        // directory.
        let identity = source.path.to_string_lossy().to_string();
        let file_str = strip_pi_marker(&identity).to_string();
        let provenance = Provenance::new(
            identity.clone(),
            self.name(),
            source.file_size_bytes,
            source.mtime_epoch_secs,
            &source.fingerprint,
        );
        let file_path = Path::new(&file_str);
        if !is_pi_locator(&identity) && !file_str.contains("/sessions/--") {
            let trace = AgentWorthTrace::new(
                derive_session_id(file_path),
                self.name(),
                provenance,
                Utc::now(),
            );
            return Ok(ParseResult {
                trace,
                malformed_lines: 0,
                warnings: vec!["not a pi session file; skipped".to_string()],
            });
        }

        // Pre-pass: the `session` header carries the authoritative id, start
        // time, and cwd. The filename uuid is the fallback.
        let (mut session_id, mut header_ts) = (None, None);
        if let Ok(file) = File::open(file_path) {
            for line in BufReader::new(file).lines().take(16) {
                let line = match line {
                    Ok(l) => l,
                    Err(_) => continue,
                };
                let v: Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if v.get("type").and_then(|t| t.as_str()) == Some("session") {
                    session_id = v.get("id").and_then(|i| i.as_str()).map(String::from);
                    header_ts = parse_timestamp(&v);
                    break;
                }
            }
        }
        let session_id = session_id
            .or_else(|| pi_file_session_id(file_path))
            .unwrap_or_else(|| derive_session_id(file_path));

        let mut trace = AgentWorthTrace::new(&session_id, self.name(), provenance, Utc::now());
        let mut malformed_lines = 0;
        let mut warnings = Vec::new();
        let mut sequence = 0u64;
        let mut last_model: Option<String> = None;
        let mut effort: Option<String> = None;

        let mtime_ts =
            DateTime::from_timestamp_secs(source.mtime_epoch_secs).unwrap_or_else(Utc::now);
        let mut earliest: Option<DateTime<Utc>> = header_ts;
        let mut latest: Option<DateTime<Utc>> = header_ts;

        let file = File::open(file_path)?;
        for (line_idx, line_res) in BufReader::new(file).lines().enumerate() {
            let line_num = line_idx + 1;
            let line_str = match line_res {
                Ok(l) => l,
                Err(e) => {
                    malformed_lines += 1;
                    warnings.push(format!("unreadable line {line_num}: {e}"));
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
                    warnings.push(format!("JSON syntax error on line {line_num}: {e}"));
                    continue;
                }
            };

            let ts = parse_pi_entry_ts(&val).unwrap_or(mtime_ts);
            if earliest.is_none_or(|e| ts < e) {
                earliest = Some(ts);
            }
            if latest.is_none_or(|l| ts > l) {
                latest = Some(ts);
            }

            parse_pi_entry(
                &val,
                &mut sequence,
                ts,
                line_num,
                &mut last_model,
                &mut effort,
                &mut trace.events,
                &mut warnings,
            );
        }

        if let Some(started) = earliest {
            trace.started_at = started;
        }
        if let Some(latest) = latest {
            trace.ended_at = Some(latest);
        }
        trace.stats.effort = effort;

        backfill_shell_exit_codes(&mut trace.events);
        trace.recalculate_stats();

        Ok(ParseResult {
            trace,
            malformed_lines,
            warnings,
        })
    }
}

fn is_candidate_pi_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let lower = filename.to_lowercase();

    if !path_str.contains(".pi")
        && !path_str.contains("/pi/")
        && !path_str.contains("\\pi\\")
        && !path_str.contains("inflection")
        && !lower.starts_with("pi")
    {
        return false;
    }
    if filename.starts_with('.') && !filename.ends_with(".jsonl") && !filename.ends_with(".json") {
        return false;
    }
    let lower = filename.to_lowercase();
    if lower == "config.json"
        || lower == "settings.json"
        || lower == "credentials.json"
        || lower == "auth.json"
        || lower == "package.json"
    {
        return false;
    }
    path.extension()
        .is_some_and(|ext| ext == "jsonl" || ext == "json")
}

fn derive_session_id(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

fn parse_timestamp(val: &Value) -> Option<DateTime<Utc>> {
    if let Some(ts_str) = val.get("timestamp").and_then(|v| v.as_str()) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    if let Some(ts_str) = val.get("created_at").and_then(|v| v.as_str()) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    if let Some(millis) = val.get("timestamp").and_then(|v| v.as_i64()) {
        return DateTime::from_timestamp_millis(millis);
    }
    if let Some(epoch) = val.get("created").and_then(|v| v.as_i64()) {
        if epoch > 1_000_000_000_000 {
            return DateTime::from_timestamp_millis(epoch);
        } else {
            return DateTime::from_timestamp(epoch, 0);
        }
    }
    None
}

/// Entry timestamp: the entry-level ISO `timestamp` first, then the message's
/// unix-millis `timestamp`. Both exist on `message` entries; the entry one is
/// authoritative and always an ISO string.
fn parse_pi_entry_ts(val: &Value) -> Option<DateTime<Utc>> {
    if let Some(ts) = parse_timestamp(val) {
        return Some(ts);
    }
    val.get("message")
        .and_then(|m| m.get("timestamp"))
        .and_then(|t| t.as_i64())
        .and_then(DateTime::from_timestamp_millis)
}

/// Token usage in the shape pi actually writes: `input` / `output` /
/// `cacheRead` / `cacheWrite` with an optional `reasoning` count. Reasoning
/// folds into output, matching how the gemini adapter treats `thoughts`:
/// generated text the session paid for.
fn extract_token_usage(usage_val: &Value) -> TokenUsage {
    let input_tokens = usage_val.get("input").and_then(|v| v.as_u64()).unwrap_or(0);
    let output_tokens = usage_val
        .get("output")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        + usage_val.get("reasoning").and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_read_tokens = usage_val
        .get("cacheRead")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_creation_tokens = usage_val
        .get("cacheWrite")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    TokenUsage::new(
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
    )
}

/// Text of one content block; images carry base64 payloads that must never
/// land in the index, so only their presence is noted.
fn pi_block_text(block: &Value, out: &mut Vec<String>) {
    match block.get("type").and_then(|t| t.as_str()) {
        Some("text") => {
            if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                out.push(t.to_string());
            }
        }
        Some("image") => {
            out.push("[image]".to_string());
        }
        _ => {}
    }
}

/// Render a user message to plain text.
fn pi_user_text(msg: &Value) -> String {
    if let Some(s) = msg.get("content").and_then(|c| c.as_str()) {
        return s.to_string();
    }
    let mut parts = Vec::new();
    if let Some(blocks) = msg.get("content").and_then(|c| c.as_array()) {
        for b in blocks {
            pi_block_text(b, &mut parts);
        }
    }
    parts.join("\n")
}

/// Dispatch one v3 entry into normalized events. `last_model` tracks the
/// model in force across `model_change` entries and assistant messages alike;
/// `effort` carries the latest thinking level into invocations.
#[allow(clippy::too_many_arguments, reason = "one entry-type dispatch; a context struct would hide the shape map")]
fn parse_pi_entry(
    val: &Value,
    seq: &mut u64,
    ts: DateTime<Utc>,
    line_num: usize,
    last_model: &mut Option<String>,
    effort: &mut Option<String>,
    events: &mut Vec<NormalizedEvent>,
    warnings: &mut Vec<String>,
) {
    let raw_ref = format!("line:{line_num}");
    let entry_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");

    // The session header is consumed by the pre-pass; labels, names, and
    // extension state are not conversation and stay out of the index.
    match entry_type {
        "session" | "label" | "session_info" | "custom" => return,
        _ => {}
    }

    if entry_type == "model_change" {
        let model = val
            .get("modelId")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown")
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
            *last_model = Some(model);
        }
        return;
    }

    if entry_type == "thinking_level_change" {
        if let Some(level) = val.get("thinkingLevel").and_then(|l| l.as_str()) {
            *effort = Some(level.to_string());
        }
        return;
    }

    if entry_type == "compaction" {
        let summary = val.get("summary").and_then(|s| s.as_str()).unwrap_or("");
        if summary.is_empty() {
            return;
        }
        *seq += 1;
        events.push(
            NormalizedEvent::new(
                *seq,
                ts,
                EventPayload::Compaction(CompactionEvent {
                    trigger: "auto".to_string(),
                    pre_tokens: val.get("tokensBefore").and_then(|t| t.as_u64()),
                    post_tokens: None,
                    dropped_tokens: None,
                    duration_ms: None,
                }),
            )
            .with_raw_ref(&raw_ref),
        );
        return;
    }

    if entry_type == "branch_summary" {
        let summary = val.get("summary").and_then(|s| s.as_str()).unwrap_or("");
        if summary.is_empty() {
            return;
        }
        *seq += 1;
        events.push(
            NormalizedEvent::new(
                *seq,
                ts,
                EventPayload::Custom {
                    kind: "branch_summary".to_string(),
                    data: Value::String(summary.to_string()),
                },
            )
            .with_raw_ref(&raw_ref),
        );
        return;
    }

    if entry_type == "custom_message" {
        let custom_type = val
            .get("customType")
            .and_then(|c| c.as_str())
            .unwrap_or("custom");
        let content = val.get("content").cloned().unwrap_or(Value::Null);
        *seq += 1;
        events.push(
            NormalizedEvent::new(
                *seq,
                ts,
                EventPayload::Custom {
                    kind: custom_type.to_string(),
                    data: content,
                },
            )
            .with_raw_ref(&raw_ref),
        );
        return;
    }

    if entry_type != "message" {
        warnings.push(format!("pi entry type '{entry_type}' not mapped"));
        return;
    }

    let msg = match val.get("message") {
        Some(m) => m,
        None => {
            warnings.push(format!("pi message entry without message on {raw_ref}"));
            return;
        }
    };
    let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");

    match role {
        "user" => {
            let content = pi_user_text(msg);
            if content.trim().is_empty() {
                return;
            }
            *seq += 1;
            events.push(
                NormalizedEvent::new(*seq, ts, EventPayload::UserMessage { content })
                    .with_raw_ref(&raw_ref),
            );
        }

        "assistant" => {
            let model = msg
                .get("model")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown")
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

            if let Some(usage_val) = msg.get("usage") {
                let usage = extract_token_usage(usage_val);
                let cost_usd = usage_val
                    .get("cost")
                    .and_then(|c| c.get("total"))
                    .and_then(|t| t.as_f64());
                *seq += 1;
                events.push(
                    NormalizedEvent::new(
                        *seq,
                        ts,
                        EventPayload::ModelInvocation {
                            model: model.clone(),
                            token_usage: usage,
                            cost_usd,
                            latency_ms: None,
                            effort: effort.clone(),
                        },
                    )
                    .with_raw_ref(&raw_ref),
                );
            }

            let mut content_parts = Vec::new();
            let mut thinking_parts = Vec::new();
            if let Some(blocks) = msg.get("content").and_then(|c| c.as_array()) {
                for b in blocks {
                    match b.get("type").and_then(|t| t.as_str()) {
                        Some("thinking") => {
                            if let Some(t) = b.get("thinking").and_then(|t| t.as_str()) {
                                thinking_parts.push(t.to_string());
                            }
                        }
                        Some("toolCall") => {
                            let id = b.get("id").and_then(|v| v.as_str()).map(String::from);
                            let name = b
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let args = b.get("arguments").cloned().unwrap_or(Value::Null);
                            *seq += 1;
                            events.push(
                                NormalizedEvent::new(
                                    *seq,
                                    ts,
                                    EventPayload::ToolCall(ToolCall {
                                        id,
                                        name: name.clone(),
                                        arguments: args.clone(),
                                    }),
                                )
                                .with_raw_ref(&raw_ref),
                            );
                            process_specific_pi_tool_call(&name, &args, seq, ts, &raw_ref, events);
                        }
                        _ => pi_block_text(b, &mut content_parts),
                    }
                }
            }
            let content = content_parts.join("\n");
            let thinking = if thinking_parts.is_empty() {
                None
            } else {
                Some(thinking_parts.join("\n"))
            };
            if !content.trim().is_empty() || thinking.is_some() {
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

        "toolResult" => {
            let call_id = msg
                .get("toolCallId")
                .and_then(|v| v.as_str())
                .map(String::from);
            let name = msg.get("toolName").and_then(|v| v.as_str()).map(String::from);
            let is_error = msg.get("isError").and_then(|v| v.as_bool()).unwrap_or(false);
            let mut output_parts = Vec::new();
            if let Some(blocks) = msg.get("content").and_then(|c| c.as_array()) {
                for b in blocks {
                    pi_block_text(b, &mut output_parts);
                }
            }
            if let Some(s) = msg.get("content").and_then(|c| c.as_str()) {
                output_parts.push(s.to_string());
            }
            let output_str = output_parts.join("\n");
            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::ToolResult(ToolResult {
                        call_id,
                        name,
                        output: Value::String(output_str.clone()),
                        is_error,
                    }),
                )
                .with_raw_ref(&raw_ref),
            );

            if output_str.contains("test result: ok.")
                || output_str.contains("PASSED")
                || output_str.contains("100% tests passed")
            {
                *seq += 1;
                events.push(
                    NormalizedEvent::new(
                        *seq,
                        ts,
                        EventPayload::OutcomeEvidence(OutcomeEvidence {
                            kind: OutcomeKind::TestOrBuildPassed,
                            summary: "Test suite executed successfully in Pi step".to_string(),
                            confidence: 0.9,
                        }),
                    )
                    .with_raw_ref(&raw_ref),
                );
            } else if output_str.contains("[main ")
                || output_str.contains("commit ")
                || output_str.contains("files changed,")
            {
                *seq += 1;
                events.push(
                    NormalizedEvent::new(
                        *seq,
                        ts,
                        EventPayload::OutcomeEvidence(OutcomeEvidence {
                            kind: OutcomeKind::CommitObserved,
                            summary: "Git commit observed in Pi tool output".to_string(),
                            confidence: 0.85,
                        }),
                    )
                    .with_raw_ref(&raw_ref),
                );
            }
        }

        "bashExecution" => {
            let command = msg.get("command").and_then(|c| c.as_str()).unwrap_or("").to_string();
            if command.is_empty() {
                return;
            }
            let output = msg.get("output").and_then(|o| o.as_str()).map(String::from);
            let exit_code = msg.get("exitCode").and_then(|e| e.as_i64()).map(|c| c as i32);
            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::ShellCommand(ShellCommand {
                        command,
                        cwd: None,
                        exit_code,
                        output,
                    }),
                )
                .with_raw_ref(&raw_ref),
            );
        }

        _ => {
            warnings.push(format!("pi message role '{role}' not mapped"));
        }
    }
}fn process_specific_pi_tool_call(
    name: &str,
    args: &Value,
    seq: &mut u64,
    ts: DateTime<Utc>,
    raw_ref: &str,
    events: &mut Vec<NormalizedEvent>,
) {
    let lower = name.to_lowercase();
    if lower.contains("command")
        || lower.contains("shell")
        || lower.contains("bash")
        || lower.contains("exec")
    {
        if let Some(cmd) = args
            .get("command")
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
                        cwd: args.get("cwd").and_then(|v| v.as_str()).map(String::from),
                        exit_code: None,
                        output: None,
                    }),
                )
                .with_raw_ref(raw_ref),
            );
        }
    }

    if lower.contains("edit")
        || lower.contains("write")
        || lower.contains("patch")
        || lower.contains("file")
        || lower.contains("read")
    {
        let path = args
            .get("path")
            .or_else(|| args.get("file_path"))
            .or_else(|| args.get("filePath"))
            .or_else(|| args.get("target_file"))
            .or_else(|| args.get("file"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if !path.is_empty() {
            // Reads are touches too (anchors and blame follow them); only the
            // action kind differs.
            let action = if lower.contains("read") && !lower.contains("write") {
                FileActionType::Read
            } else if lower.contains("write") || lower.contains("create") {
                FileActionType::Write
            } else {
                FileActionType::Edit
            };
            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::FileAction {
                        path,
                        action,
                        diff: args
                            .get("diff")
                            .or_else(|| args.get("content"))
                            .or_else(|| args.get("patch"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    #[test]
    fn test_detect_and_enumerate_pi() {
        let temp = tempdir().unwrap();
        let pi_dir = temp.path().join(".pi").join("tasks");
        std::fs::create_dir_all(&pi_dir).unwrap();

        let task_file = pi_dir.join("task_001.jsonl");
        let mut f = File::create(&task_file).unwrap();
        writeln!(f, "{{\"type\":\"task\",\"content\":\"Analyze telemetry\"}}").unwrap();

        let adapter = PiAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
            ..Default::default()
        };

        let detection = adapter.detect(&options).unwrap();
        assert!(detection.is_present);

        let enumerated = adapter.enumerate(&options).unwrap();
        assert_eq!(enumerated.len(), 1);
        assert_eq!(enumerated[0].adapter_name, "pi");
    }

    /// Minimal v3 session fixture mirroring the documented format: a header
    /// with cwd, a model switch mid-session, a user prompt, an assistant turn
    /// with thinking, text, a tool call and usage, plus the tool result.
    fn pi_v3_fixture() -> String {
        r#"{"type":"session","version":3,"id":"01a0991c-8c21-717a-9beb-fc4687089ec4","timestamp":"2026-09-13T04:53:00.066Z","cwd":"/Users/saurabh/code/demo"}
{"type":"model_change","id":"f27b0612","parentId":null,"timestamp":"2026-09-13T04:53:01.782Z","provider":"litellm","modelId":"go-glm-5.3-flash"}
{"type":"thinking_level_change","id":"1b150dcf","parentId":"f27b0612","timestamp":"2026-09-13T04:53:01.782Z","thinkingLevel":"high"}
{"type":"message","id":"a1b2c3d4","parentId":null,"timestamp":"2026-09-13T04:53:02.000Z","message":{"role":"user","content":"Refactor the router","timestamp":1789000002000}}
{"type":"model_change","id":"aa11bb22","parentId":"a1b2c3d4","timestamp":"2026-09-13T04:53:03.000Z","provider":"opencode","modelId":"muse-spark-1.3-contributor-free"}
{"type":"message","id":"b2c3d4e5","parentId":"aa11bb22","timestamp":"2026-09-13T04:53:04.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"Check the router first."},{"type":"text","text":"On it."},{"type":"toolCall","id":"call_1","name":"read","arguments":{"path":"src/router.rs"}}],"api":"openai-compatible","provider":"opencode","model":"muse-spark-1.3-contributor-free","usage":{"input":17383,"output":32,"cacheRead":753,"cacheWrite":0,"reasoning":13,"totalTokens":18168,"cost":{"input":0.0,"output":0.0,"cacheRead":0.0,"cacheWrite":0.0,"total":0.0}},"stopReason":"toolUse","timestamp":1789000004000}}
{"type":"message","id":"c3d4e5f6","parentId":"b2c3d4e5","timestamp":"2026-09-13T04:53:05.000Z","message":{"role":"toolResult","toolCallId":"call_1","toolName":"read","content":[{"type":"text","text":"router source"}],"isError":false,"timestamp":1789000005000}}
{"type":"message","id":"d4e5f6a7","parentId":"c3d4e5f6","timestamp":"2026-09-13T04:53:06.000Z","message":{"role":"bashExecution","command":"cargo test","output":"test result: ok. 8 passed; 0 failed","exitCode":0,"cancelled":false,"truncated":false,"timestamp":1789000006000}}
"#
        .to_string()
    }

    #[test]
    fn test_parse_pi_v3_session() {
        let temp = tempdir().unwrap();
        let session_dir = temp.path().join("sessions").join("--Users-saurabh-code-demo--");
        std::fs::create_dir_all(&session_dir).unwrap();
        let session_file = session_dir.join("2026-09-13T04-53-00-066Z_01a0991c-8c21-717a-9beb-fc4687089ec4.jsonl");
        std::fs::write(&session_file, pi_v3_fixture()).unwrap();

        let adapter = PiAdapter::new();
        let source = build_pi_source(&session_file, &agentworth_adapter_sdk::KnownSourceMap::new())
            .unwrap();
        // The slug decodes to the fixture cwd, so the identity anchors there.
        assert!(source.path.to_string_lossy().contains("code/demo/.pi-session-01a0991c"));
        assert!(source.path.to_string_lossy().contains(PI_REPO_MARKER));

        let result = adapter.parse(&source).expect("parse failed");
        assert_eq!(result.malformed_lines, 0);
        let trace = result.trace;
        assert_eq!(trace.adapter, "pi");
        assert_eq!(trace.session_id, "01a0991c-8c21-717a-9beb-fc4687089ec4");

        // Tokens: input verbatim, reasoning folded into output.
        assert_eq!(trace.stats.token_usage.input_tokens, 17383);
        assert_eq!(trace.stats.token_usage.output_tokens, 45);
        assert_eq!(trace.stats.token_usage.cache_read_tokens, 753);
        assert_eq!(trace.stats.token_usage.total(), 18181);

        // Models: the session genuinely used two (header-era default, then
        // the mid-session switch); the switch -- not the settings file -- is
        // what attribution must follow.
        assert_eq!(trace.stats.models_used.len(), 2);
        assert!(trace.stats.models_used.contains(&"muse-spark-1.3-contributor-free".to_string()));
        assert!(trace.stats.models_used.contains(&"go-glm-5.3-flash".to_string()));
        assert_eq!(trace.stats.effort.as_deref(), Some("high"));

        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(trace.stats.tools_used.get("read"), Some(&1));
        // read carries a path (file action), bashExecution feeds the shell
        // ledger, and the mid-session model change emits a switch.
        let kinds: Vec<&str> = trace
            .events
            .iter()
            .map(|e| match &e.payload {
                EventPayload::ToolCall(_) => "tool",
                EventPayload::ShellCommand(_) => "shell",
                EventPayload::FileAction { .. } => "file",
                EventPayload::ModelSwitch(_) => "switch",
                EventPayload::ModelInvocation { .. } => "inv",
                EventPayload::UserMessage { .. } => "user",
                EventPayload::AssistantMessage { .. } => "asst",
                EventPayload::ToolResult(_) => "result",
                _ => "other",
            })
            .collect();
        for want in ["user", "switch", "inv", "tool", "file", "asst", "result", "shell"] {
            assert!(kinds.contains(&want), "missing {want} in {kinds:?}");
        }
    }

    #[test]
    fn test_pi_slug_decoding_and_locator() {
        assert_eq!(
            decode_pi_slug("--Users-saurabh-code-demo--").as_deref(),
            Some("/Users/saurabh/code/demo")
        );
        assert_eq!(decode_pi_slug("sessions").as_deref(), None);
        assert_eq!(decode_pi_slug("--x--").as_deref(), Some("/x"));

        let identity = "/Users/saurabh/code/demo/.pi-session-01a0991c.jsonl::pi-repo::/Users/saurabh/.pi/agent/sessions/--Users-saurabh-code-demo--/f.jsonl";
        assert!(is_pi_locator(identity));
        assert!(!is_pi_locator("/Users/saurabh/.pi/agent/tasks/output.jsonl"));
        // A `~/code`-level workspace resolves to the leaf: fall back to bare.
        let bare = pi_identity_for(Path::new(
            "/Users/saurabh/.pi/agent/sessions/--Users-saurabh-code--/f.jsonl",
        ));
        assert!(!bare.contains(PI_REPO_MARKER));
    }

    #[test]
    fn test_parse_graceful_on_malformed_lines() {
        let temp = tempdir().unwrap();
        let session_dir = temp.path().join("sessions").join("--tmp--");
        std::fs::create_dir_all(&session_dir).unwrap();
        let session_file = session_dir.join("2026-09-13T04-53-00-066Z_s1.jsonl");
        std::fs::write(
            &session_file,
            "{\"type\":\"session\",\"version\":3,\"id\":\"s1\",\"timestamp\":\"2026-09-13T04:53:00.066Z\",\"cwd\":\"/tmp\"}\n{CORRUPT_PI_JSON}\n{\"type\":\"message\",\"id\":\"a\",\"parentId\":null,\"timestamp\":\"2026-09-13T04:53:02.000Z\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}\n",
        )
        .unwrap();

        let adapter = PiAdapter::new();
        let source = build_pi_source(&session_file, &agentworth_adapter_sdk::KnownSourceMap::new())
            .unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 1);
        assert_eq!(result.trace.stats.user_messages_count, 1);
    }

    #[test]
    fn test_pi_identity_falls_back_for_container_workspace() {
        // A `--Users-saurabh-code--` slug decodes to `~/code` itself: no
        // anchor, bare file path, still a parseable locator.
        let bare = pi_identity_for(Path::new(
            "/Users/saurabh/.pi/agent/sessions/--Users-saurabh-code--/f.jsonl",
        ));
        assert!(!bare.contains(PI_REPO_MARKER));
        assert!(is_pi_locator(&bare));
    }
}
