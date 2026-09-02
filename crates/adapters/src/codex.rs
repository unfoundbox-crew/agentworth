use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use agentworth_adapter_sdk::{
    compute_fast_fingerprint, AgentAdapter, DetectionResult, ParseResult, ScanOptions,
    SessionSource,
};
use agentworth_schema::{
    AgentWorthTrace, EventPayload, FileActionType, ModelSwitch, NormalizedEvent, OutcomeEvidence, OutcomeKind,
    Provenance, ShellCommand, TokenUsage, ToolCall, ToolResult,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use directories::BaseDirs;
use serde_json::Value;
use walkdir::WalkDir;
use crate::exit_status::backfill_shell_exit_codes;

/// Adapter for discovering and normalizing OpenAI / Codex agent sessions.
pub struct CodexAdapter;

impl Default for CodexAdapter {
    fn default() -> Self {
        Self
    }
}

impl CodexAdapter {
    /// 2: version 1 read a shape Codex rollouts do not use. Every field lives under a
    /// `payload` object keyed by a top-level `type` (`session_meta`, `turn_context`,
    /// `event_msg`, `response_item`), so the top-level `usage`/`model` lookups version 1
    /// made never matched, and every real session was indexed with no tokens and no model.
    /// Version 2 reads `turn_context` (model, effort), the `token_count` event's
    /// cumulative counters, and `session_meta.cwd` for the repository key. The bytes of
    /// those files never changed, so without this bump an incremental scan would keep
    /// serving the empty answer.
    pub const PARSER_VERSION: i64 = 2;

    pub fn new() -> Self {
        Self
    }

    /// Candidate directory paths for Codex on the host machine, used for presence
    /// detection only. Broader than `session_roots()`: bare `~/.codex` also holds
    /// plugin caches and, via `~/.codex/worktrees/...`, entire cloned repos (complete
    /// with their own `node_modules`) that are not Codex session data.
    pub fn candidate_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".codex"));
            roots.push(home.join(".codex").join("sessions"));
            roots.push(home.join(".config").join("codex"));
        }
        roots.push(PathBuf::from(".codex"));
        roots
    }

    /// Directories that actually hold Codex session transcripts, used for the default
    /// (unscoped) `enumerate()` walk.
    pub fn session_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Some(base_dirs) = BaseDirs::new() {
            let home = base_dirs.home_dir();
            roots.push(home.join(".codex").join("sessions"));
            roots.push(home.join(".codex").join("archived_sessions"));
        }
        roots
    }
}

/// Separates a synthetic, repo-anchored path prefix from the real file path needed to read
/// the session.
///
/// `extract_repository_or_workspace` (agentworth-schema) resolves a session's repository
/// purely by pattern-matching `Provenance.source_path`, and per the adapter/storage split no
/// Codex-specific knowledge may be added to it. Every Codex rollout lives under
/// `~/.codex/sessions/<yyyy>/<mm>/<dd>/rollout-*.jsonl`, a path that names no repository at
/// all -- worse, its `.codex` segment matches that function's hidden-directory fallback rule
/// and resolves to the *home directory*, which is why all 874 indexed Codex sessions shared
/// one bogus repo key. The real workspace is recoverable: `session_meta.cwd` is on the first
/// line of all 448 rollout files on the development machine. So enumeration builds a
/// source_path that looks like a real file living inside that directory, with the real path
/// tucked behind this marker -- invisible to `extract_repository_or_workspace`'s
/// substring/component rules, which only ever see a '/'-delimited path up to the marker, but
/// still recoverable in `parse()`. `SessionSource.path` and `Provenance.source_path` are
/// always the same string (never diverged): `Storage::should_scan_source` and
/// `AgentWorthCore::prune_stub_sessions` compare the enumerate-time path against the stored
/// parse-time one, and a mismatch would make every Codex session look permanently changed.
/// Same mechanism, and the same reasoning, as `OPENCODE_REPO_MARKER`.
const CODEX_REPO_MARKER: &str = "::codex-repo::";

/// Invented path segment placed under the resolved workspace directory. It exists only so
/// `extract_repository_or_workspace`'s hidden-directory-boundary rule has something to anchor
/// on for a workspace that is not itself under a recognized `/code/` or `/projects/-` root;
/// without it that rule would keep scanning rightward past the marker and match the real
/// path's own `.codex` segment, reproducing the bug this exists to fix.
const CODEX_REPO_LEAF: &str = ".codex-session.jsonl";

/// Wrap `real_path` behind [`CODEX_REPO_MARKER`] so it resolves to `workspace` through
/// `extract_repository_or_workspace`'s existing generic rules. Returns `real_path` unchanged
/// when no usable workspace is known, which is the pre-existing (wrong, but no worse)
/// behavior.
fn wrap_with_repo_marker(workspace: Option<&str>, real_path: &str) -> String {
    match workspace
        .map(str::trim)
        .map(|d| d.trim_end_matches('/'))
        .filter(|d| d.starts_with('/') && d.len() > 1)
    {
        Some(dir) => format!("{dir}/{CODEX_REPO_LEAF}{CODEX_REPO_MARKER}{real_path}"),
        None => real_path.to_string(),
    }
}

/// Recover the real file path from a possibly-wrapped source path. Absent the marker (an
/// index row written before this landed, or a session whose workspace could not be read), the
/// whole string already is the real path.
#[allow(
    clippy::string_slice,
    reason = "idx comes from rfind() on an ASCII marker, offset by its own byte length: always a char boundary"
)]
fn strip_repo_marker(path_str: &str) -> &str {
    match path_str.rfind(CODEX_REPO_MARKER) {
        Some(idx) => &path_str[idx + CODEX_REPO_MARKER.len()..],
        None => path_str,
    }
}

/// Bytes of a rollout file enumeration will read looking for the session's workspace.
///
/// Measured over 448 real rollout files: the first line is a `session_meta` record of 7-48 KB
/// (it carries the full base-instructions text) and `"cwd"` appears within its first 334
/// bytes. This budget is two orders of magnitude clear of that and still bounded, so a
/// truncated or malformed file costs a bounded read rather than a scan of a multi-GB log.
const CODEX_WORKSPACE_SCAN_BYTES: u64 = 1024 * 1024;

/// Read the workspace directory a Codex session ran in, from the head of its rollout file.
///
/// `session_meta.cwd` is the answer in all 448 files measured; `turn_context.cwd` is read as a
/// fallback for a rollout whose first record is missing or malformed. Returns `None` rather
/// than failing: a session with no readable workspace still indexes, it just keeps the old
/// path-derived repository key.
fn read_codex_workspace(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file).take(CODEX_WORKSPACE_SCAN_BYTES);
    let mut fallback = None;
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        let Ok(val) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let cwd = val
            .get("payload")
            .and_then(|p| p.get("cwd"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        match val.get("type").and_then(|v| v.as_str()) {
            Some("session_meta") => {
                if let Some(cwd) = cwd {
                    return Some(cwd.to_string());
                }
            }
            // `or_else`, so the first `turn_context` that names a cwd wins and a later one
            // that does not cannot clear it.
            Some("turn_context") => fallback = fallback.or_else(|| cwd.map(str::to_string)),
            _ => {}
        }
    }
    fallback
}

/// Build a `SessionSource` whose identity path carries the session's workspace (see
/// [`CODEX_REPO_MARKER`]). Size, mtime and fingerprint always come from the real file --
/// only the identity string embeds the synthetic prefix.
fn build_codex_source(path: &Path, adapter_name: &str) -> Result<SessionSource> {
    let metadata = std::fs::metadata(path)?;
    let file_size_bytes = metadata.len();
    let mtime_epoch_secs = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let fingerprint = compute_fast_fingerprint(path, file_size_bytes, mtime_epoch_secs)?;

    let real_path = path.to_string_lossy().to_string();
    let workspace = read_codex_workspace(path);
    let identity = wrap_with_repo_marker(workspace.as_deref(), &real_path);

    Ok(SessionSource {
        path: PathBuf::from(identity),
        adapter_name: adapter_name.to_string(),
        file_size_bytes,
        mtime_epoch_secs,
        fingerprint,
    })
}

/// Skip directories that are never Codex session data but can appear under a Codex
/// root: cloned repos under `~/.codex/worktrees/<hash>/<repo>/...` carry their own
/// `node_modules`, `.git`, and build output, none of which is session data.
fn should_skip_codex_dir(entry: &walkdir::DirEntry) -> bool {
    if entry.file_type().is_dir() {
        let name = entry.file_name().to_string_lossy();
        name == ".git"
            || name == "node_modules"
            || name == "target"
            || name == "dist"
            || name == ".venv"
    } else {
        false
    }
}

impl AgentAdapter for CodexAdapter {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn parser_version(&self) -> i64 {
        Self::PARSER_VERSION
    }

    /// A Codex `SessionSource.path` is a synthetic identity string, not a real path (see
    /// [`CODEX_REPO_MARKER`]), so the default `path.exists()` would report every session as
    /// gone.
    fn source_exists(&self, source: &SessionSource) -> bool {
        Path::new(strip_repo_marker(&source.path.to_string_lossy())).exists()
    }

    fn capabilities(&self) -> agentworth_adapter_sdk::AdapterCapabilities {
        agentworth_adapter_sdk::AdapterCapabilities {
            prompts: true,
            tokens: true,
            tools: true,
            shell: true,
            diffs: true,
            thinking: false,
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
            if custom.ends_with(".codex") || s.contains("codex") {
                discovered.push(custom.clone());
            } else if custom.is_dir() {
                // custom_paths may point at a generic parent directory rather than
                // the adapter-specific dir itself; look a few levels in before
                // giving up, matching how `enumerate()` already recurses.
                let mut found_nested = false;
                for sub in &[custom.join(".codex"), custom.join(".config").join("codex")] {
                    if sub.exists() {
                        discovered.push(sub.clone());
                        found_nested = true;
                    }
                }
                if !found_nested {
                    for entry in WalkDir::new(custom).max_depth(4).into_iter().filter_map(|e| e.ok()) {
                        let path = entry.path();
                        let ps = path.to_string_lossy().to_lowercase();
                        if ps.contains("codex") {
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
                    if is_candidate_codex_file(custom) {
                        if let Ok(source) = build_codex_source(custom, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if custom.is_dir() {
                    for entry in WalkDir::new(custom)
                        .into_iter()
                        .filter_entry(|e| !should_skip_codex_dir(e))
                        .filter_map(|e| e.ok())
                    {
                        let path = entry.path();
                        if path.is_file() && is_candidate_codex_file(path) {
                            if let Ok(source) = build_codex_source(path, self.name()) {
                                sources.push(source);
                            }
                        }
                    }
                }
            }
        } else {
            for root in self.session_roots() {
                if root.is_file() {
                    if is_candidate_codex_file(&root) {
                        if let Ok(source) = build_codex_source(&root, self.name()) {
                            sources.push(source);
                        }
                    }
                } else if root.is_dir() {
                    for entry in WalkDir::new(&root)
                        .into_iter()
                        .filter_entry(|e| !should_skip_codex_dir(e))
                        .filter_map(|e| e.ok())
                    {
                        let path = entry.path();
                        if path.is_file() && is_candidate_codex_file(path) {
                            if let Ok(source) = build_codex_source(path, self.name()) {
                                sources.push(source);
                            }
                        }
                    }
                }
            }
        }

        // Deduplicate sources by canonical path
        sources.sort_by(|a, b| a.path.cmp(&b.path));
        sources.dedup_by(|a, b| a.path == b.path);

        Ok(sources)
    }

    fn parse(&self, source: &SessionSource) -> Result<ParseResult> {
        let identity = source.path.to_string_lossy().to_string();
        let real_path = PathBuf::from(strip_repo_marker(&identity));
        let file = File::open(&real_path)?;
        let reader = BufReader::new(file);

        let session_id = derive_session_id(&real_path);
        let provenance = Provenance::new(
            identity,
            self.name(),
            source.file_size_bytes,
            source.mtime_epoch_secs,
            &source.fingerprint,
        );

        let mut trace = AgentWorthTrace::new(&session_id, self.name(), provenance, Utc::now());
        let mut malformed_lines = 0;
        let mut warnings = Vec::new();
        let mut sequence = 0u64;
        let mut state = CodexSessionState::default();

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

            let events = parse_codex_record(&val, &mut sequence, timestamp, line_num, &mut state);
            trace.events.extend(events);
        }

        trace
            .events
            .extend(flush_pending_usage(&mut state, &mut sequence));

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

fn is_candidate_codex_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    if !path_str.contains("codex") && !path_str.contains("openai") && !path_str.contains("gpt") {
        return false;
    }
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if filename.starts_with('.') && !filename.ends_with(".jsonl") && !filename.ends_with(".json") {
        return false;
    }
    if filename == "config.json"
        || filename == "settings.json"
        || filename == "credentials.json"
        || filename == "auth.json"
    {
        return false;
    }
    // Real Codex session transcripts are named `rollout-<timestamp>-<uuid>.jsonl` under
    // `sessions/` or `archived_sessions/`. Requiring that prefix (rather than just "path
    // contains codex") is what keeps this out of `~/.codex/worktrees/<hash>/<repo>/...`,
    // which holds complete cloned repos -- package.json, tsconfig.json, and vendored
    // node_modules included -- that happen to sit under a path containing "codex".
    filename.starts_with("rollout-") && filename.ends_with(".jsonl")
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
    if let Some(ts_str) = val.get("time").and_then(|v| v.as_str()) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    if let Some(epoch) = val.get("created").and_then(|v| v.as_i64()) {
        if epoch > 1_000_000_000_000 {
            return DateTime::from_timestamp_millis(epoch);
        } else {
            return DateTime::from_timestamp(epoch, 0);
        }
    }
    if let Some(millis) = val.get("timestamp").and_then(|v| v.as_i64()) {
        return DateTime::from_timestamp_millis(millis);
    }
    None
}

fn extract_token_usage(usage_val: &Value) -> TokenUsage {
    let input_tokens = usage_val
        .get("prompt_tokens")
        .or_else(|| usage_val.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let output_tokens = usage_val
        .get("completion_tokens")
        .or_else(|| usage_val.get("output_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let cache_read_tokens = usage_val
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .or_else(|| usage_val.get("cached_tokens"))
        .or_else(|| usage_val.get("cache_read_tokens"))
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

/// Codex's own cumulative token counters, as carried by `info.total_token_usage` on every
/// `token_count` event.
///
/// The same event also carries `info.last_token_usage`, the counts for the turn that just
/// finished, and summing those is the obvious way to total a session -- but it is wrong. Over
/// 425 real rollout files that carry token events, summing `last_token_usage` matches the
/// final `total_token_usage` in 339 and overshoots it in 86: Codex re-emits a turn's counts
/// more than once. So this adapter tracks `total_token_usage` and attributes each event the
/// *delta* since the previous one. The deltas then sum to exactly the session's real total,
/// and no turn is counted twice.
///
/// The counter is monotonic in 447 of 448 files. In the one that is not, it restarts four
/// times mid-session, and at each restart `total_token_usage` equals that event's own
/// `last_token_usage` -- the signature of a fresh running total, not of a corrupt one. See
/// [`Self::advance`] for how a restart is counted.
#[derive(Default, Clone, Copy)]
struct CodexCumulativeTokens {
    input: u64,
    output: u64,
    cached: u64,
    cache_write: u64,
}

impl CodexCumulativeTokens {
    fn read(usage: &Value) -> Self {
        let field = |name: &str| usage.get(name).and_then(Value::as_u64).unwrap_or(0);
        Self {
            input: field("input_tokens"),
            output: field("output_tokens"),
            cached: field("cached_input_tokens"),
            cache_write: field("cache_write_input_tokens"),
        }
    }

    /// Tokens spent since `prev`, mapped onto `TokenUsage`'s four disjoint buckets.
    ///
    /// Codex nests where AgentWorth separates. Measured over 36,334 usage records: every one
    /// satisfies `total_tokens == input_tokens + output_tokens`, `cached_input_tokens <=
    /// input_tokens` and `reasoning_output_tokens <= output_tokens` -- so `input_tokens` is
    /// the whole prompt with the cached part inside it, and reasoning tokens are already
    /// inside `output_tokens`. Cache reads are therefore subtracted out of input rather than
    /// added alongside it, and `reasoning_output_tokens` is deliberately not read at all:
    /// OpenAI bills it as output and adding it again would double-count.
    ///
    /// Every counter goes through [`Self::advance`], so a restarted counter contributes its
    /// new value in full rather than nothing.
    fn delta_since(self, prev: Self) -> TokenUsage {
        let input = Self::advance(self.input, prev.input);
        let cached = Self::advance(self.cached, prev.cached);
        TokenUsage::new(
            input.saturating_sub(cached),
            Self::advance(self.output, prev.output),
            cached,
            // Never observed non-zero in any of the 36,334 records measured, so whether it
            // nests inside `input_tokens` the way `cached_input_tokens` does is unverified.
            // Carried through as its own bucket rather than guessed at.
            Self::advance(self.cache_write, prev.cache_write),
        )
    }

    /// What one counter added between two `token_count` events.
    ///
    /// Normally that is `current - previous`. A *decrease* means Codex restarted the running
    /// total -- a resumed thread begins its own -- and then `current` is not a delta at all,
    /// it is the whole of the new total so far. Counting it as zero would drop that value and
    /// every value up to the next event: a file reading 1000 -> 100 -> 600 spent 1600, not
    /// 1500, and a restart on the last event would lose the entire tail. So a decrease is
    /// attributed in full. Applied per counter, not to the session total, so a restart that
    /// touches one field and not another is still counted field by field.
    ///
    /// Measured on the one non-monotonic file here: four restarts, five segments, 106,218,334
    /// tokens under this rule against 105,540,401 under the old zero-delta one.
    fn advance(current: u64, previous: u64) -> u64 {
        if current < previous {
            current
        } else {
            current.saturating_sub(previous)
        }
    }
}

/// Token usage measured before any record named a model. 25 of 448 real rollouts open with a
/// `token_count` ahead of their first `turn_context`; holding those counts back and
/// attributing them to the first model that does appear keeps a phantom "unknown" model out
/// of `models_used` and out of per-model usage.
struct PendingCodexUsage {
    usage: TokenUsage,
    timestamp: DateTime<Utc>,
    raw_ref: String,
    effort: Option<String>,
}

/// Session-scoped parse state threaded through `parse_codex_record`.
#[derive(Default)]
struct CodexSessionState {
    /// Model named by the most recent `turn_context` / `thread_settings_applied` record.
    /// Includes `codex-auto-review`, Codex's own reviewer sub-thread, when that is the model
    /// actually running: its tokens are real and belong to it. Which of a session's models is
    /// "the" session model is a question for the query side, not for the parser.
    current_model: Option<String>,
    /// Reasoning effort in force for the current turn.
    current_effort: Option<String>,
    /// Model of the last emitted `ModelInvocation`, for `ModelSwitch` detection.
    last_invoked_model: Option<String>,
    cumulative: CodexCumulativeTokens,
    pending: Option<PendingCodexUsage>,
}

/// Emit a `ModelInvocation` (preceded by a `ModelSwitch` when the model changed) for `usage`,
/// or bank it in `state.pending` when no model has been named yet.
fn push_model_invocation(
    state: &mut CodexSessionState,
    usage: TokenUsage,
    effort: Option<String>,
    seq: &mut u64,
    ts: DateTime<Utc>,
    raw_ref: &str,
    events: &mut Vec<NormalizedEvent>,
) {
    // A flush of banked usage carries the effort captured when it was banked; if none was
    // known then, the first one the session declares is the better answer than nothing.
    let effort = effort.or_else(|| state.current_effort.clone());
    let Some(model) = state.current_model.clone() else {
        match &mut state.pending {
            Some(pending) => pending.usage += usage,
            None => {
                state.pending = Some(PendingCodexUsage {
                    usage,
                    timestamp: ts,
                    raw_ref: raw_ref.to_string(),
                    effort,
                });
            }
        }
        return;
    };

    if state.last_invoked_model.as_deref() != Some(model.as_str()) {
        if let Some(prev) = state.last_invoked_model.take() {
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
                .with_raw_ref(raw_ref),
            );
        }
        state.last_invoked_model = Some(model.clone());
    }

    *seq += 1;
    events.push(
        NormalizedEvent::new(
            *seq,
            ts,
            EventPayload::ModelInvocation {
                model,
                token_usage: usage,
                cost_usd: None,
                latency_ms: None,
                effort,
            },
        )
        .with_raw_ref(raw_ref),
    );
}

/// Emit any usage still banked at end of file. Reached only by a rollout that spends tokens
/// and never names a model in any record; `unknown` is then the honest label.
fn flush_pending_usage(state: &mut CodexSessionState, seq: &mut u64) -> Vec<NormalizedEvent> {
    let Some(pending) = state.pending.take() else {
        return Vec::new();
    };
    let mut events = Vec::new();
    if state.current_model.is_none() {
        state.current_model = Some("unknown".to_string());
    }
    push_model_invocation(
        state,
        pending.usage,
        pending.effort,
        seq,
        pending.timestamp,
        &pending.raw_ref,
        &mut events,
    );
    events
}

/// A trimmed, non-empty string field, or `None`. Codex writes `""` where a setting is
/// unset, and an empty model name is worse than no model name at all.
fn non_empty_str(parent: &Value, key: &str) -> Option<String> {
    parent
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Track the model and reasoning effort a Codex rollout declares, and turn its `token_count`
/// events into per-invocation token usage.
///
/// Everything Codex writes sits under `payload`, keyed by a top-level `type`:
/// `turn_context` carries `model`, `effort` and `cwd` for the turn about to run;
/// `event_msg` wraps a second-level `payload.type`, of which `token_count` carries the
/// counters and `thread_settings_applied` restates model and `reasoning_effort`;
/// `session_meta` opens the file with `cwd`, `originator` and `cli_version`.
fn absorb_codex_metadata(
    val: &Value,
    state: &mut CodexSessionState,
    seq: &mut u64,
    ts: DateTime<Utc>,
    raw_ref: &str,
    events: &mut Vec<NormalizedEvent>,
) {
    let Some(payload) = val.get("payload") else {
        return;
    };

    match val.get("type").and_then(|v| v.as_str()) {
        Some("session_meta") => {
            // Not present in any of the 448 rollouts measured (the model lives on
            // `turn_context`), but read as a fallback for a rollout that opens with one.
            if state.current_model.is_none() {
                state.current_model = non_empty_str(payload, "model");
            }
        }
        Some("turn_context") => {
            if let Some(model) = non_empty_str(payload, "model") {
                state.current_model = Some(model);
            }
            if let Some(effort) = non_empty_str(payload, "effort") {
                state.current_effort = Some(effort);
            }
        }
        Some("event_msg") => match payload.get("type").and_then(|v| v.as_str()) {
            Some("thread_settings_applied") => {
                let Some(settings) = payload.get("thread_settings") else {
                    return;
                };
                if let Some(model) = non_empty_str(settings, "model") {
                    state.current_model = Some(model);
                }
                if let Some(effort) = non_empty_str(settings, "reasoning_effort") {
                    state.current_effort = Some(effort);
                }
            }
            Some("token_count") => {
                let Some(total) = payload
                    .get("info")
                    .and_then(|info| info.get("total_token_usage"))
                else {
                    return;
                };
                let cumulative = CodexCumulativeTokens::read(total);
                let usage = cumulative.delta_since(state.cumulative);
                state.cumulative = cumulative;
                if usage.total() > 0 {
                    let effort = state.current_effort.clone();
                    push_model_invocation(state, usage, effort, seq, ts, raw_ref, events);
                }
            }
            _ => {}
        },
        _ => {}
    }
}

fn parse_codex_record(
    val: &Value,
    seq: &mut u64,
    ts: DateTime<Utc>,
    line_num: usize,
    state: &mut CodexSessionState,
) -> Vec<NormalizedEvent> {
    let mut events = Vec::new();
    let raw_ref = format!("line:{}", line_num);

    absorb_codex_metadata(val, state, seq, ts, &raw_ref, &mut events);

    // A model named by a `turn_context` can be the first one this session has seen, which
    // releases usage banked before any model was known.
    if state.current_model.is_some() {
        if let Some(pending) = state.pending.take() {
            push_model_invocation(
                state,
                pending.usage,
                pending.effort,
                seq,
                pending.timestamp,
                &pending.raw_ref,
                &mut events,
            );
        }
    }

    let role = val
        .get("role")
        .or_else(|| val.get("message").and_then(|m| m.get("role")))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let event_type = val.get("type").and_then(|v| v.as_str()).unwrap_or(role);

    // Model invocation / Token usage extraction
    if let Some(usage_val) = val
        .get("usage")
        .or_else(|| val.get("response").and_then(|r| r.get("usage")))
    {
        let usage = extract_token_usage(usage_val);
        if usage.total() > 0 {
            let model = val
                .get("model")
                .or_else(|| val.get("response").and_then(|r| r.get("model")))
                .and_then(|v| v.as_str())
                .unwrap_or("gpt-4o")
                .to_string();

            if state.last_invoked_model.as_deref() != Some(model.as_str()) {
                if let Some(prev) = state.last_invoked_model.take() {
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
                state.last_invoked_model = Some(model.clone());
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
                        latency_ms: val
                            .get("duration_ms")
                            .or_else(|| val.get("latency_ms"))
                            .and_then(|d| d.as_u64()),
                        effort: None,
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    match event_type {
        "user" | "user_message" => {
            let content = if let Some(text) = val.get("content").and_then(|v| v.as_str()) {
                text.to_string()
            } else if let Some(msg) = val
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
            {
                msg.to_string()
            } else if let Some(arr) = val.get("content").and_then(|v| v.as_array()) {
                extract_text_from_array(arr)
            } else {
                val.to_string()
            };

            *seq += 1;
            events.push(
                NormalizedEvent::new(*seq, ts, EventPayload::UserMessage { content })
                    .with_raw_ref(&raw_ref),
            );
        }

        "assistant" | "assistant_message" => {
            let thinking = val
                .get("thinking")
                .or_else(|| val.get("reasoning_content"))
                .or_else(|| {
                    val.get("message")
                        .and_then(|m| m.get("reasoning_content").or_else(|| m.get("thinking")))
                })
                .and_then(|v| v.as_str())
                .map(String::from);

            let mut text_parts = Vec::new();
            if let Some(content_val) = val
                .get("content")
                .or_else(|| val.get("message").and_then(|m| m.get("content")))
            {
                if let Some(text) = content_val.as_str() {
                    text_parts.push(text.to_string());
                } else if let Some(arr) = content_val.as_array() {
                    for item in arr {
                        if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                            text_parts.push(text.to_string());
                        }
                    }
                }
            }

            // Extract OpenAI-style tool_calls array if present
            let tool_calls_opt = val
                .get("tool_calls")
                .or_else(|| val.get("message").and_then(|m| m.get("tool_calls")))
                .and_then(|v| v.as_array());

            if let Some(tcs) = tool_calls_opt {
                for tc in tcs {
                    let id = tc.get("id").and_then(|v| v.as_str()).map(String::from);
                    let fn_val = tc.get("function").unwrap_or(tc);
                    let name = fn_val
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();

                    let args: Value = match fn_val.get("arguments") {
                        Some(Value::String(s)) => {
                            serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.clone()))
                        }
                        Some(v) => v.clone(),
                        None => Value::Null,
                    };

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

                    process_specific_codex_tool_call(&name, &args, seq, ts, &raw_ref, &mut events);
                }
            }

            if !text_parts.is_empty() || thinking.is_some() {
                *seq += 1;
                events.push(
                    NormalizedEvent::new(
                        *seq,
                        ts,
                        EventPayload::AssistantMessage {
                            content: text_parts.join("\n"),
                            thinking,
                        },
                    )
                    .with_raw_ref(&raw_ref),
                );
            }
        }

        "tool" | "tool_result" | "function_response" => {
            let call_id = val
                .get("tool_call_id")
                .or_else(|| val.get("call_id"))
                .and_then(|v| v.as_str())
                .map(String::from);

            let is_error = val
                .get("is_error")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let output = val
                .get("content")
                .or_else(|| val.get("output"))
                .cloned()
                .unwrap_or(Value::Null);

            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::ToolResult(ToolResult {
                        call_id,
                        name: val.get("name").and_then(|v| v.as_str()).map(String::from),
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
                                summary: "Test suite passed in tool result".to_string(),
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

        "error" => {
            let message = val
                .get("error")
                .or_else(|| val.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error")
                .to_string();

            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::Error {
                        message,
                        is_recovered: false,
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }

        _ => {
            *seq += 1;
            events.push(
                NormalizedEvent::new(
                    *seq,
                    ts,
                    EventPayload::Custom {
                        kind: event_type.to_string(),
                        data: val.clone(),
                    },
                )
                .with_raw_ref(&raw_ref),
            );
        }
    }

    events
}

fn process_specific_codex_tool_call(
    name: &str,
    args: &Value,
    seq: &mut u64,
    ts: DateTime<Utc>,
    raw_ref: &str,
    events: &mut Vec<NormalizedEvent>,
) {
    let lower_name = name.to_lowercase();
    if lower_name == "exec"
        || lower_name == "bash"
        || lower_name == "shell"
        || lower_name == "run_command"
        || lower_name == "cmd"
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
    } else if lower_name == "edit"
        || lower_name == "write_file"
        || lower_name == "patch"
        || lower_name == "create_file"
        || lower_name == "fileedit"
        || lower_name == "apply_diff"
    {
        let path = args
            .get("path")
            .or_else(|| args.get("file_path"))
            .or_else(|| args.get("target_file"))
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
                            .get("diff")
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

fn extract_text_from_array(arr: &[Value]) -> String {
    let mut texts = Vec::new();
    for item in arr {
        if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
            texts.push(text);
        } else if let Some(s) = item.as_str() {
            texts.push(s);
        }
    }
    texts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::{tempdir, NamedTempFile};

    #[test]
    fn test_parse_standard_codex_jsonl() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"role":"user","timestamp":"2026-08-29T10:00:00Z","content":"Refactor the parser"}
{"role":"assistant","timestamp":"2026-08-29T10:00:05Z","model":"gpt-4o","usage":{"prompt_tokens":400,"completion_tokens":150,"prompt_tokens_details":{"cached_tokens":100}},"tool_calls":[{"id":"call_abc","type":"function","function":{"name":"exec","arguments":"{\"command\":\"pytest tests/\"}"}}]}
{"role":"tool","timestamp":"2026-08-29T10:00:08Z","tool_call_id":"call_abc","content":"test result: ok. 12 PASSED"}
{"role":"assistant","timestamp":"2026-08-29T10:00:12Z","content":"Refactoring completed and tests pass."}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = CodexAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 0);
        let trace = result.trace;
        assert_eq!(trace.adapter, "codex");
        assert_eq!(trace.stats.models_used, vec!["gpt-4o".to_string()]);
        assert_eq!(trace.stats.token_usage.input_tokens, 400);
        assert_eq!(trace.stats.token_usage.output_tokens, 150);
        assert_eq!(trace.stats.token_usage.cache_read_tokens, 100);
        assert_eq!(trace.stats.token_usage.total(), 650);
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(trace.stats.tools_used.get("exec"), Some(&1));
        assert!(trace.stats.duration_seconds.unwrap() >= 12.0);
    }

    #[test]
    fn test_parse_codex_file_actions_and_reasoning() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = r#"
{"role":"user","created":1756461600,"content":"Add new helper"}
{"role":"assistant","created":1756461610,"model":"o1-preview","reasoning_content":"Thinking about helper logic...","tool_calls":[{"id":"call_edit","function":{"name":"edit","arguments":{"path":"src/helper.rs","diff":"+fn help() {}"}}}]}
{"role":"tool","created":1756461615,"tool_call_id":"call_edit","content":"[main abc1234] 1 files changed, 5 insertions(+)"}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = CodexAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        let trace = result.trace;
        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.assistant_messages_count, 1);
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(trace.stats.tools_used.get("edit"), Some(&1));
    }

    #[test]
    fn test_parse_graceful_on_empty_and_corrupt_lines() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = "{\"role\":\"user\",\"content\":\"hi\"}\n\n{CORRUPT_JSON}\n{\"role\":\"assistant\",\"content\":\"hello\"}\n";
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = CodexAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let result = adapter.parse(&source).expect("parse failed");

        assert_eq!(result.malformed_lines, 1);
        assert_eq!(result.trace.stats.user_messages_count, 1);
        assert_eq!(result.trace.stats.assistant_messages_count, 1);
    }

    #[test]
    fn test_detect_and_enumerate_codex() {
        let temp = tempdir().unwrap();
        let codex_dir = temp.path().join(".codex").join("sessions");
        std::fs::create_dir_all(&codex_dir).unwrap();

        let session_file = codex_dir.join("rollout-2026-01-01T00-00-00-019eed64-07e0-7ad0-a4bd-3ec244120cdb.jsonl");
        let mut f = File::create(&session_file).unwrap();
        writeln!(f, "{{\"role\":\"user\",\"content\":\"test\"}}").unwrap();

        let adapter = CodexAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
            ..Default::default()
        };

        let detection = adapter.detect(&options).unwrap();
        assert!(detection.is_present);

        let enumerated = adapter.enumerate(&options).unwrap();
        assert_eq!(enumerated.len(), 1);
        assert_eq!(enumerated[0].adapter_name, "codex");
    }

    /// 25 of 448 real rollout files emit a `token_count` before any record names a model.
    /// Those tokens are held back and attributed to the first model that does appear, rather
    /// than inventing an "unknown" bucket that would then show up in `models_used` and in
    /// per-model usage for a session that only ever ran one model.
    #[test]
    fn test_tokens_before_the_first_turn_context_go_to_the_first_model_named() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = concat!(
            r#"{"timestamp":"2026-01-01T00:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":30,"total_tokens":130}}}}"#,
            "\n",
            r#"{"timestamp":"2026-01-01T00:00:01Z","type":"turn_context","payload":{"model":"gpt-5.6-sol","effort":"medium","cwd":"/w"}}"#,
            "\n",
        );
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = CodexAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let trace = adapter.parse(&source).expect("parse failed").trace;

        assert_eq!(trace.stats.models_used, vec!["gpt-5.6-sol".to_string()]);
        assert_eq!(trace.stats.effort.as_deref(), Some("medium"));
        assert_eq!(
            trace.stats.per_model_token_usage.get("gpt-5.6-sol"),
            Some(&TokenUsage::new(80, 30, 20, 0))
        );
    }

    /// A session that spends tokens and never names a model anywhere gets `unknown`, which is
    /// the honest label -- but it must still not lose the tokens.
    #[test]
    fn test_tokens_with_no_model_anywhere_are_kept_under_unknown() {
        let mut temp = NamedTempFile::new().unwrap();
        let sample = concat!(
            r#"{"timestamp":"2026-01-01T00:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}}}}"#,
            "\n",
        );
        temp.write_all(sample.as_bytes()).unwrap();

        let adapter = CodexAdapter::new();
        let source = SessionSource::from_path(temp.path(), adapter.name()).unwrap();
        let trace = adapter.parse(&source).expect("parse failed").trace;

        assert_eq!(trace.stats.models_used, vec!["unknown".to_string()]);
        assert_eq!(trace.stats.token_usage.total(), 15);
    }

    #[test]
    fn test_repo_marker_round_trips_and_resolves_to_the_workspace() {
        let real = "/Users/example/.codex/sessions/2026/01/01/rollout-x.jsonl";

        let wrapped = wrap_with_repo_marker(Some("/Users/example/code/acme/widget/"), real);
        assert_eq!(strip_repo_marker(&wrapped), real);
        assert_eq!(
            agentworth_schema::extract_repository_or_workspace(&wrapped),
            "acme/widget"
        );

        // A workspace outside any recognized code root still resolves, via the invented leaf.
        let outside = wrap_with_repo_marker(Some("/srv/deploy/checkout"), real);
        assert_eq!(strip_repo_marker(&outside), real);
        assert_eq!(
            agentworth_schema::extract_repository_or_workspace(&outside),
            "deploy/checkout"
        );

        // Unusable workspaces leave the real path alone rather than building a broken prefix.
        for unusable in [None, Some(""), Some("   "), Some("/"), Some("relative/dir")] {
            assert_eq!(wrap_with_repo_marker(unusable, real), real);
        }
        assert_eq!(strip_repo_marker(real), real);
    }

    /// Regression test for the "worktrees full of node_modules" bug: a cloned repo under
    /// `~/.codex/worktrees/<hash>/<repo>/...` sits on a path containing "codex", so the
    /// old `contains("codex")`-only filter accepted every `.json` file inside it,
    /// including vendored `node_modules`. Requiring the `rollout-*.jsonl` name shape
    /// rejects all of it regardless of directory.
    #[test]
    fn test_enumerate_codex_skips_worktree_and_node_modules_junk() {
        let temp = tempdir().unwrap();
        let worktree_dir = temp
            .path()
            .join(".codex")
            .join("worktrees")
            .join("7bc9")
            .join("vibelaunch")
            .join("node_modules")
            .join("protobufjs");
        std::fs::create_dir_all(&worktree_dir).unwrap();
        File::create(worktree_dir.join("package.json")).unwrap();

        let sessions_dir = temp.path().join(".codex").join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        File::create(sessions_dir.join("config.json")).unwrap();
        let mut real = File::create(
            sessions_dir.join("rollout-2026-01-01T00-00-00-019eed64-07e0-7ad0-a4bd-3ec244120cdb.jsonl"),
        )
        .unwrap();
        writeln!(real, "{{\"role\":\"user\",\"content\":\"test\"}}").unwrap();

        let adapter = CodexAdapter::new();
        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
            ..Default::default()
        };

        let enumerated = adapter.enumerate(&options).unwrap();
        assert_eq!(enumerated.len(), 1, "only the rollout-*.jsonl file should be enumerated");
        assert!(enumerated[0].path.to_string_lossy().ends_with(".jsonl"));
    }
}
