//! `archie repo receipt init | create | show | key`: build, sign and export a Work Receipt
//! (Evidence slice 1, `docs/specs/evidence-check.md` draft §1, §2, §4.1).
//!
//! A receipt is the laptop's signed, privacy-safe account of what its own index saw happen on
//! the commits about to be pushed. It is built from the same anchored attribution `repo
//! suspect` uses (`suspect::attribute_commits`), projected through
//! `receipt::project_work_session` so no free text survives, checked by the publication
//! redactor (fail closed), signed with the archie-owned ed25519 key (`agentworth-attest`),
//! and written as a git note on the tip commit (`refs/notes/agentworth`) or, with
//! `--transport file`, as `.agentworth/receipts/<receipt_id>.dsse.json`.
//!
//! Consent follows `select → redact → preview → explicit approval → export`: the range is the
//! selection, the projection plus the redactor is redaction, an interactive `create` prints
//! the exact JSON and a "what this reveals" list, the first export per repository asks y/N
//! (or `init --write` records it), and the hook path exports only when consent is recorded.
//!
//! Verification (`repo receipt verify`) is a separate command and is not in this module.

use std::collections::BTreeMap;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use agentworth_attest::{
    sign_statement, verify_envelope, Envelope, PublicKey, ReceiptSigningKey, SignatureVerdict,
};
use agentworth_redaction::Redactor;
use agentworth_schema::work_receipt::{
    Attribution, CommitAnchor, CommitAttribution, CommitRange, Generator, IndexInfo,
    ReceiptTimestamp, RedactionInfo, Signer, Subject, WorkReceiptPredicate, WorkReceiptStatement,
    RECEIPT_VERSION,
};
use agentworth_schema::AgentWorthTrace;
use agentworth_storage::Storage;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;

use super::receipt::{project_work_session, WorkSessionInput};
use super::suspect::{self, attribute_commits, AttributedCommits, SuspectQuery};

/// `git notes --ref` name; the full ref is [`NOTES_REF`].
pub const NOTES_REF_NAME: &str = "agentworth";
/// Where receipts live as git notes.
pub const NOTES_REF: &str = "refs/notes/agentworth";
/// Remote-tracking copy used only while merging a concurrent writer's notes.
const NOTES_REMOTE_TMP: &str = "refs/notes/agentworth-remote";
/// `--transport file` directory, relative to the repository root.
pub const RECEIPT_DIR: &str = ".agentworth/receipts";
/// Set on the nested `git push` of the notes ref so the pre-push hook does not run again.
pub const PUSH_GUARD_ENV: &str = "AGENTWORTH_RECEIPT_PUSH";
/// Overrides the signing key path (tests, and anyone keeping keys elsewhere).
pub const KEY_PATH_ENV: &str = "AGENTWORTH_RECEIPT_KEY";
/// `predicate.attribution.rule`: the anchored blame join, `crates/storage/src/anchoring.rs`.
pub const ATTRIBUTION_RULE: &str = "anchored-blame/v1";
/// `predicate.redaction.policy`.
pub const REDACTION_POLICY: &str = "publication/v1";
/// Fields of the local index a receipt never carries (`predicate.redaction.fields_dropped`).
pub const FIELDS_DROPPED: &[&str] = &[
    "source_path",
    "db_path",
    "prompt_preview",
    "commands",
    "quotes",
    "session_id",
];
/// Comment on the generated key and on the `.pub` file.
const KEY_COMMENT: &str = "agentworth-receipt";

/// The workflow `init` prints and `init --write` writes (spec §5).
pub const WORKFLOW_YAML: &str = r#"name: AgentWorth
on: pull_request
permissions:
  contents: read
  checks: write
  pull-requests: read
jobs:
  receipt-verify:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - uses: unfoundbox-crew/agentworth@v0
"#;
/// Where `init --write` puts [`WORKFLOW_YAML`], relative to the repository root.
pub const WORKFLOW_PATH: &str = ".github/workflows/agentworth.yml";
/// The line `init` looks for in an existing pre-push hook.
const HOOK_MARKER: &str = "repo receipt create";

/// Where a receipt goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// `refs/notes/agentworth` on the tip commit (the default, decision row 1).
    #[default]
    Notes,
    /// `.agentworth/receipts/<receipt_id>.dsse.json`, committed by the user.
    File,
}

// ---------------------------------------------------------------------------------------------
// Signing key
// ---------------------------------------------------------------------------------------------

/// `~/.agentworth/keys/receipt_ed25519`, or `$AGENTWORTH_RECEIPT_KEY`.
pub fn default_key_path() -> Result<PathBuf> {
    if let Ok(p) = std::env::var(KEY_PATH_ENV) {
        if !p.is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    Ok(agentworth_storage::default_db_dir()?
        .join("keys")
        .join("receipt_ed25519"))
}

fn pub_path(key_path: &Path) -> PathBuf {
    let mut s = key_path.as_os_str().to_owned();
    s.push(".pub");
    PathBuf::from(s)
}

/// Load the signing key. `Ok(None)` when no key file exists yet.
pub fn load_key(path: &Path) -> Result<Option<ReceiptSigningKey>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = zeroize::Zeroizing::new(
        std::fs::read_to_string(path).with_context(|| format!("failed to read {:?}", path))?,
    );
    let (key, _comment) = ReceiptSigningKey::from_openssh_private(&text).map_err(|e| {
        anyhow::anyhow!("{:?} is not an unencrypted OpenSSH ed25519 key: {e}", path)
    })?;
    Ok(Some(key))
}

/// Load the signing key, creating it (0600, directory 0700) when absent. Returns whether it
/// was created.
pub fn load_or_create_key(path: &Path) -> Result<(ReceiptSigningKey, bool)> {
    if let Some(key) = load_key(path)? {
        return Ok((key, false));
    }
    let key =
        ReceiptSigningKey::generate().map_err(|e| anyhow::anyhow!("key generation failed: {e}"))?;
    let private = key
        .to_openssh_private(KEY_COMMENT)
        .map_err(|e| anyhow::anyhow!("key encoding failed: {e}"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("failed to create {:?}", dir))?;
        set_mode(dir, 0o700)?;
    }
    write_private(path, private.as_bytes())?;
    std::fs::write(
        pub_path(path),
        format!("{} {KEY_COMMENT}\n", key.public_key().to_openssh()),
    )
    .with_context(|| format!("failed to write {:?}", pub_path(path)))?;
    Ok((key, true))
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("failed to create {:?}", path))?;
    f.write_all(bytes)
        .with_context(|| format!("failed to write {:?}", path))
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).with_context(|| format!("failed to write {:?}", path))
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("failed to chmod {:?}", path))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

/// The public half without touching the private key: the `.pub` file when present, else the
/// key itself. `Ok(None)` when there is no key at all.
pub fn read_public_key(path: &Path) -> Result<Option<PublicKey>> {
    let pub_file = pub_path(path);
    if let Ok(line) = std::fs::read_to_string(&pub_file) {
        if let Ok(key) = PublicKey::parse_openssh(line.trim()) {
            return Ok(Some(key));
        }
    }
    Ok(load_key(path)?.map(|k| k.public_key()))
}

// ---------------------------------------------------------------------------------------------
// Consent
// ---------------------------------------------------------------------------------------------

fn consent_key(root: &Path) -> String {
    root.to_string_lossy().to_string()
}

/// Whether export consent is recorded for this repository root.
pub fn has_consent(cfg: &crate::app::config::CliConfig, root: &Path) -> bool {
    let key = consent_key(root);
    cfg.receipts
        .as_ref()
        .is_some_and(|r| r.consented_repos.contains(&key))
}

/// Record export consent for this repository root in the `config.toml` at `config_path`.
pub fn record_consent(config_path: &Path, root: &Path) -> Result<()> {
    let mut cfg = crate::app::config::load_config_from(config_path)?;
    if has_consent(&cfg, root) {
        return Ok(());
    }
    cfg.receipts
        .get_or_insert_with(Default::default)
        .consented_repos
        .push(consent_key(root));
    crate::app::config::save_config_to(config_path, &cfg)
}

// ---------------------------------------------------------------------------------------------
// Building the Statement
// ---------------------------------------------------------------------------------------------

/// Re-reads a session's full trace. The scanner in production; a fixture map in tests.
pub trait TraceLoader {
    fn load(&self, session_id: &str) -> Option<AgentWorthTrace>;
}

/// [`TraceLoader`] over the indexed source files, via `Scanner::load_trace`.
pub struct ScannerTraceLoader(agentworth_core::Scanner);

impl ScannerTraceLoader {
    pub fn new(storage: Arc<Storage>) -> Self {
        Self(agentworth_core::Scanner::new(storage))
    }
}

impl TraceLoader for ScannerTraceLoader {
    fn load(&self, session_id: &str) -> Option<AgentWorthTrace> {
        self.0.load_trace(session_id).ok()
    }
}

/// A loader that knows no traces: every session projects with no rungs.
pub struct NoTraces;

impl TraceLoader for NoTraces {
    fn load(&self, _session_id: &str) -> Option<AgentWorthTrace> {
        None
    }
}

/// What to build a receipt over.
#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub query: SuspectQuery,
    pub include_spend: bool,
    /// `created_at`, floored to the minute.
    pub now: DateTime<Utc>,
}

/// An unsigned Statement plus what only this machine may know about it.
#[derive(Debug, Clone)]
pub struct BuiltReceipt {
    pub statement: WorkReceiptStatement,
    /// Repository root (local only, never in the Statement).
    pub root: PathBuf,
    /// The tip commit the note goes on.
    pub head_sha: String,
    /// Session ref → raw session id. Local only, never serialized into the Statement.
    pub session_ids_by_ref: BTreeMap<String, String>,
}

impl BuiltReceipt {
    /// Highest rung a session in this receipt reached with local verification behind it.
    pub fn highest_attested_rung(&self) -> Option<u8> {
        self.statement
            .predicate
            .sessions
            .iter()
            .flat_map(|s| s.rungs.iter())
            .filter(|r| r.verified_locally)
            .map(|r| r.rung)
            .max()
    }
}

fn rev_parse_commit(root: &Path, rev: &str) -> Result<String> {
    suspect::validate_ref(rev, "ref")?;
    let spec = format!("{rev}^{{commit}}");
    suspect::git_stdout(root, &["rev-parse", "--verify", "--quiet", &spec])
        .with_context(|| format!("'{rev}' does not resolve to a commit"))
}

/// Build the unsigned Statement for the range `opts.query` resolves to. `Ok(None)` when the
/// range holds no commits (nothing to push, nothing to sign).
///
/// Fails closed: the Statement is validated and run through [`check_publication_safe`]
/// before it is returned, so a caller can only ever sign something that passed both.
pub fn build_receipt(
    storage: &Storage,
    loader: &dyn TraceLoader,
    signer: &PublicKey,
    opts: &BuildOptions,
) -> Result<Option<BuiltReceipt>> {
    let attributed = attribute_commits(storage, &opts.query)?;
    if attributed.commits.is_empty() {
        return Ok(None);
    }
    build_from_attribution(storage, loader, signer, opts, &attributed).map(Some)
}

fn build_from_attribution(
    storage: &Storage,
    loader: &dyn TraceLoader,
    signer: &PublicKey,
    opts: &BuildOptions,
    attributed: &AttributedCommits,
) -> Result<BuiltReceipt> {
    let root = &attributed.root;
    let head_sha = rev_parse_commit(root, &attributed.range.head)?;
    let base_sha = match &attributed.range.base {
        Some(b) => Some(rev_parse_commit(root, b)?),
        None => None,
    };
    let signer_raw = signer.raw();

    let session_ids = attributed.session_ids();
    let risks = storage.get_session_risks(&session_ids)?;
    let mut sessions = Vec::with_capacity(session_ids.len());
    let mut ref_of: BTreeMap<String, String> = BTreeMap::new();
    for id in &session_ids {
        // Fall back to the blame row's own fields if the session row is gone.
        let row = attributed
            .commits
            .iter()
            .find_map(|c| c.sessions.get(id))
            .expect("session id came from these commits");
        let summary = storage.get_session_by_id(id)?;
        let (adapter, models, started_at, ended_at, tokens_total) = match &summary {
            Some(s) => (
                s.adapter.clone(),
                s.models_used.clone(),
                Some(s.started_at),
                s.duration_seconds
                    .filter(|d| d.is_finite() && *d >= 0.0)
                    .map(|d| s.started_at + chrono::Duration::milliseconds((d * 1000.0) as i64)),
                s.total_tokens,
            ),
            None => (row.adapter.clone(), row.models_used.clone(), None, None, 0),
        };
        let governor_halts = storage
            .governor_events_for_session(id, 10_000)
            .map(|events| events.iter().filter(|e| e.action == "halt").count())
            .unwrap_or(0);
        let trace = loader.load(id);
        let input = WorkSessionInput {
            session_id: id,
            adapter: &adapter,
            models: &models,
            started_at,
            ended_at: trace.as_ref().and_then(|t| t.ended_at).or(ended_at),
            tokens_total,
            risk: risks.get(id),
            governor_halts: u32::try_from(governor_halts).unwrap_or(u32::MAX),
            signer_pubkey: &signer_raw,
            include_spend: opts.include_spend,
        };
        let session = project_work_session(&input, trace.as_ref());
        ref_of.insert(id.clone(), session.session_ref.clone());
        sessions.push(session);
    }
    sessions.sort_by(|a, b| a.session_ref.cmp(&b.session_ref));

    let commits: Vec<CommitAttribution> = attributed
        .commits
        .iter()
        .map(|c| {
            let mut refs: Vec<String> = c.sessions.keys().map(|id| ref_of[id].clone()).collect();
            refs.sort();
            refs.dedup();
            let mut files = c.session_files.clone();
            files.sort();
            files.dedup();
            let anchor = if c.sessions.is_empty() {
                None
            } else if c
                .sessions
                .values()
                .any(|r| Path::new(&r.file_path).is_absolute())
            {
                Some(CommitAnchor::Absolute)
            } else {
                Some(CommitAnchor::RelativeToSessionRepo)
            };
            CommitAttribution {
                sha: c.sha.clone(),
                sessions: refs,
                files,
                anchor,
            }
        })
        .collect();

    let last_session_at = storage
        .get_aggregate_stats(true)
        .ok()
        .and_then(|s| s.last_session_at)
        .map(ReceiptTimestamp::from_datetime);
    let parser_version_min = storage
        .min_parser_version(&session_ids)?
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0);

    let predicate = WorkReceiptPredicate {
        receipt_version: RECEIPT_VERSION,
        generator: Generator {
            name: "archie".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        signer: Signer {
            public_key: signer.to_openssh(),
        },
        created_at: ReceiptTimestamp::from_datetime(opts.now),
        range: CommitRange {
            base: base_sha,
            head: head_sha.clone(),
            commits: u32::try_from(commits.len()).unwrap_or(u32::MAX),
            rule: attributed.range.rule,
        },
        attribution: Attribution {
            rule: ATTRIBUTION_RULE.to_string(),
            window_hours: u32::try_from(attributed.window_hours.max(0)).unwrap_or(u32::MAX),
            unanchored_rows: u32::try_from(attributed.unanchored_rows).unwrap_or(u32::MAX),
        },
        sessions,
        commits: commits.clone(),
        index: IndexInfo {
            last_session_at,
            parser_version_min,
        },
        redaction: RedactionInfo {
            policy: REDACTION_POLICY.to_string(),
            fields_dropped: FIELDS_DROPPED.iter().map(|s| s.to_string()).collect(),
        },
        truncated: false,
    };
    let subject = commits
        .iter()
        .map(|c| Subject::commit(c.sha.clone()))
        .collect();
    let statement = WorkReceiptStatement::new(subject, predicate);
    statement
        .validate()
        .map_err(|e| anyhow::anyhow!("built receipt failed validation: {e}"))?;
    check_publication_safe(&statement)?;

    Ok(BuiltReceipt {
        statement,
        root: root.clone(),
        head_sha,
        session_ids_by_ref: ref_of.into_iter().map(|(id, r)| (r, id)).collect(),
    })
}

// ---------------------------------------------------------------------------------------------
// Fail-closed redaction check (spec §2.5)
// ---------------------------------------------------------------------------------------------

/// A string in a built receipt that the publication redactor would change.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("refusing to sign: field {field} would be changed by the publication redactor, so free text leaked into a structured field")]
pub struct LeakError {
    /// JSON pointer to the offending value, e.g. `/predicate/sessions/0/models/0`.
    pub field: String,
}

/// Run `Redactor::for_publication()` over every string in the Statement. If it would change a
/// single byte, abort: sign nothing and name the field.
///
/// Values whose format the Statement validator already pins to a closed shape (commit SHAs,
/// session refs, the signer key, minute timestamps) are skipped: they cannot carry free text,
/// and a high-entropy secret detector would rightly be suspicious of a base64 public key.
pub fn check_publication_safe(statement: &WorkReceiptStatement) -> Result<(), LeakError> {
    let value = serde_json::to_value(statement).map_err(|_| LeakError {
        field: "/".to_string(),
    })?;
    let redactor = Redactor::for_publication();
    walk_strings(&value, &mut String::new(), &mut |pointer, s| {
        if is_structural(pointer) {
            return Ok(());
        }
        if redactor.redact_text(s) != s {
            return Err(LeakError {
                field: pointer.to_string(),
            });
        }
        Ok(())
    })
}

/// Pointers whose value is format-validated (hex, key, timestamp) rather than free text.
fn is_structural(pointer: &str) -> bool {
    let segs: Vec<&str> = pointer.split('/').skip(1).collect();
    matches!(
        segs.as_slice(),
        ["subject", _, "digest", "gitCommit"]
            | ["predicate", "signer", "public_key"]
            | ["predicate", "created_at"]
            | ["predicate", "range", "base"]
            | ["predicate", "range", "head"]
            | ["predicate", "sessions", _, "ref"]
            | ["predicate", "sessions", _, "started_at"]
            | ["predicate", "sessions", _, "ended_at"]
            | ["predicate", "commits", _, "sha"]
            | ["predicate", "commits", _, "sessions", _]
            | ["predicate", "index", "last_session_at"]
    )
}

fn walk_strings(
    v: &serde_json::Value,
    pointer: &mut String,
    f: &mut dyn FnMut(&str, &str) -> Result<(), LeakError>,
) -> Result<(), LeakError> {
    use serde_json::Value;
    match v {
        Value::String(s) => f(pointer, s),
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                let len = pointer.len();
                pointer.push('/');
                pointer.push_str(&i.to_string());
                walk_strings(item, pointer, f)?;
                pointer.truncate(len);
            }
            Ok(())
        }
        Value::Object(map) => {
            for (k, item) in map {
                let len = pointer.len();
                pointer.push('/');
                pointer.push_str(&k.replace('~', "~0").replace('/', "~1"));
                walk_strings(item, pointer, f)?;
                pointer.truncate(len);
            }
            // Keys are fixed by the schema (`deny_unknown_fields`), never data.
            Ok(())
        }
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------------------------
// Preview
// ---------------------------------------------------------------------------------------------

/// The "what this reveals" list printed before the first export (spec §2.5, consent).
pub fn reveals(statement: &WorkReceiptStatement, signer: &PublicKey) -> Vec<String> {
    let p = &statement.predicate;
    let files: usize = p.commits.iter().map(|c| c.files.len()).sum();
    let mut adapters: Vec<&str> = p.sessions.iter().map(|s| s.adapter.as_str()).collect();
    adapters.sort();
    adapters.dedup();
    let mut models: Vec<&str> = p
        .sessions
        .iter()
        .flat_map(|s| s.models.iter().map(String::as_str))
        .collect();
    models.sort();
    models.dedup();
    let spend = p.sessions.iter().any(|s| s.spend_micro_usd.is_some());
    let list = |v: &[&str]| {
        if v.is_empty() {
            "none".to_string()
        } else {
            v.join(", ")
        }
    };

    vec![
        format!(
            "{} commit SHA(s): the commits being pushed, plus the range base",
            statement.subject.len()
        ),
        if p.truncated {
            "no file paths: files[] was dropped to fit the 64 KiB cap".to_string()
        } else {
            format!(
                "{files} repo-relative path(s), only paths in those commits' own diffs that an \
                 indexed session also touched"
            )
        },
        format!(
            "{} session(s) as opaque refs, never their ids; adapters: {}; models: {}",
            p.sessions.len(),
            list(&adapters),
            list(&models)
        ),
        "per session: token totals, start and end rounded to the minute, rung kinds, command \
         classes from a fixed allowlist, and integer risk counters"
            .to_string(),
        if spend {
            "API-equivalent spend in micro-USD (you passed --include-spend)".to_string()
        } else {
            "no spend: tokens only (pass --include-spend to add API-equivalent dollars)".to_string()
        },
        format!("your receipt public key, {}", signer.fingerprint()),
        "never: prompts, assistant text, tool output, code, diffs, command lines, absolute \
         paths, hostnames, usernames, emails, branch names"
            .to_string(),
    ]
}

/// What the evidence check will be able to say from this receipt alone, computed on this
/// machine. Only the attested and claimed columns: rungs 2, 4 and 5 are re-derived from CI
/// by `repo receipt verify`, which is authoritative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PredictedCommit {
    pub sha: String,
    pub sessions: Vec<String>,
    /// Highest rung a covering session backs with local verification ("attested locally").
    pub highest_attested_rung: Option<u8>,
    /// Rungs the covering sessions only claim (`verified_locally: false`), highest first.
    pub claimed_only_rungs: Vec<u8>,
}

/// Per-commit local prediction for [`PredictedCommit`].
pub fn predict(statement: &WorkReceiptStatement) -> Vec<PredictedCommit> {
    let by_ref: BTreeMap<&str, &agentworth_schema::work_receipt::WorkSession> = statement
        .predicate
        .sessions
        .iter()
        .map(|s| (s.session_ref.as_str(), s))
        .collect();
    statement
        .predicate
        .commits
        .iter()
        .map(|c| {
            let rungs: Vec<&agentworth_schema::work_receipt::RungEvidence> = c
                .sessions
                .iter()
                .filter_map(|r| by_ref.get(r.as_str()))
                .flat_map(|s| s.rungs.iter())
                .collect();
            let highest_attested_rung = rungs
                .iter()
                .filter(|r| r.verified_locally)
                .map(|r| r.rung)
                .max();
            let mut claimed_only_rungs: Vec<u8> = rungs
                .iter()
                .filter(|r| !r.verified_locally)
                .map(|r| r.rung)
                .filter(|r| highest_attested_rung.is_none_or(|h| *r > h))
                .collect();
            claimed_only_rungs.sort_by(|a, b| b.cmp(a));
            claimed_only_rungs.dedup();
            PredictedCommit {
                sha: c.sha.clone(),
                sessions: c.sessions.clone(),
                highest_attested_rung,
                claimed_only_rungs,
            }
        })
        .collect()
}

/// One line for the hook and the demo: `receipt 7c41e0a9 signed, 2 sessions, rung 3 attested`.
pub fn summary_line(receipt_id: &str, built: &BuiltReceipt) -> String {
    let n = built.statement.predicate.sessions.len();
    let rung = match built.highest_attested_rung() {
        Some(r) => format!("rung {r} attested"),
        None => "no rung attested".to_string(),
    };
    format!(
        "receipt {} signed, {n} session{}, {rung}",
        receipt_id.chars().take(8).collect::<String>(),
        if n == 1 { "" } else { "s" }
    )
}

// ---------------------------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------------------------

fn git_with_stdin(root: &Path, args: &[&str], stdin: &[u8]) -> Result<std::process::Output> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to run `git {}`", args.join(" ")))?;
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(stdin)
        .context("failed to write to git")?;
    Ok(child.wait_with_output()?)
}

/// Write `envelope` as the `refs/notes/agentworth` note on `tip`, replacing this machine's
/// previous note there. One envelope per line: after a `cat_sort_uniq` merge with another
/// writer a note can hold several, so readers split on newlines.
pub fn write_note(root: &Path, tip: &str, envelope: &Envelope) -> Result<()> {
    suspect::validate_ref(tip, "tip")?;
    let mut body = envelope.to_json();
    body.push(b'\n');
    let out = git_with_stdin(
        root,
        &[
            "notes",
            "--ref",
            NOTES_REF_NAME,
            "add",
            "-f",
            "-F",
            "-",
            tip,
        ],
        &body,
    )?;
    if !out.status.success() {
        bail!(
            "git notes add failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// Every envelope in the note on `commit`, or none.
pub fn read_note(root: &Path, commit: &str) -> Result<Vec<Envelope>> {
    suspect::validate_ref(commit, "commit")?;
    let out = suspect::git(root, &["notes", "--ref", NOTES_REF_NAME, "show", commit])?;
    if !out.status.success() {
        return Ok(Vec::new());
    }
    Ok(parse_envelope_lines(&out.stdout))
}

fn parse_envelope_lines(bytes: &[u8]) -> Vec<Envelope> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| Envelope::from_json(l.as_bytes()).ok())
        .collect()
}

/// Write `.agentworth/receipts/<receipt_id>.dsse.json` under `root`.
pub fn write_receipt_file(root: &Path, receipt_id: &str, envelope: &Envelope) -> Result<PathBuf> {
    let dir = root.join(RECEIPT_DIR);
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {:?}", dir))?;
    let path = dir.join(format!("{receipt_id}.dsse.json"));
    let mut body = envelope.to_json();
    body.push(b'\n');
    std::fs::write(&path, body).with_context(|| format!("failed to write {:?}", path))?;
    Ok(path)
}

/// How a notes push went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PushOutcome {
    pub pushed: bool,
    /// True when the first push was rejected and a `cat_sort_uniq` merge was needed.
    pub merged: bool,
    pub error: Option<String>,
}

fn push_notes_once(root: &Path, remote: &str) -> Result<std::process::Output> {
    let refspec = format!("{NOTES_REF}:{NOTES_REF}");
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["push", "--quiet", remote, &refspec])
        .env(PUSH_GUARD_ENV, "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .context("failed to run git push")
}

/// Push `refs/notes/agentworth` to `remote`. On a rejection (another writer pushed first):
/// fetch theirs, `git notes merge -s cat_sort_uniq`, push again, once. Never touches
/// `remote.<name>.push` (spec §1, mechanics 4).
pub fn push_notes(root: &Path, remote: &str) -> Result<PushOutcome> {
    suspect::validate_ref(remote, "remote")?;
    let first = push_notes_once(root, remote)?;
    if first.status.success() {
        return Ok(PushOutcome {
            pushed: true,
            merged: false,
            error: None,
        });
    }
    let first_err = String::from_utf8_lossy(&first.stderr).trim().to_string();
    let fetch_spec = format!("+{NOTES_REF}:{NOTES_REMOTE_TMP}");
    let fetched = suspect::git(root, &["fetch", "--quiet", remote, &fetch_spec])?;
    if !fetched.status.success() {
        return Ok(PushOutcome {
            pushed: false,
            merged: false,
            error: Some(first_err),
        });
    }
    let merged = suspect::git(
        root,
        &[
            "notes",
            "--ref",
            NOTES_REF_NAME,
            "merge",
            "--quiet",
            "-s",
            "cat_sort_uniq",
            NOTES_REMOTE_TMP,
        ],
    )?;
    let _ = suspect::git(root, &["update-ref", "-d", NOTES_REMOTE_TMP]);
    if !merged.status.success() {
        return Ok(PushOutcome {
            pushed: false,
            merged: false,
            error: Some(format!(
                "notes merge failed: {}",
                String::from_utf8_lossy(&merged.stderr).trim()
            )),
        });
    }
    let second = push_notes_once(root, remote)?;
    Ok(PushOutcome {
        pushed: second.status.success(),
        merged: true,
        error: (!second.status.success())
            .then(|| String::from_utf8_lossy(&second.stderr).trim().to_string()),
    })
}

// ---------------------------------------------------------------------------------------------
// `archie repo receipt create`
// ---------------------------------------------------------------------------------------------

/// Everything `archie repo receipt create` accepts.
#[derive(Debug, Clone, Default)]
pub struct CreateArgs {
    pub repo: Option<PathBuf>,
    pub base: Option<String>,
    pub push: bool,
    pub remote: Option<String>,
    pub transport: Transport,
    pub include_spend: bool,
    pub json: bool,
    pub quiet: bool,
}

/// `--json` output of `create`.
#[derive(Debug, Clone, Serialize)]
pub struct CreateReport {
    /// `hex(sha256(canonical Statement))`. Present even when nothing was signed: it is the id
    /// the receipt would get.
    pub receipt_id: Option<String>,
    pub signed: bool,
    /// Why nothing was signed: `empty_range`, `no_key`, `consent_missing`, `declined`.
    pub skipped: Option<String>,
    pub transport: Transport,
    /// Commit the note was written on, or the receipt file path.
    pub written_to: Option<String>,
    pub push: Option<PushOutcome>,
    pub highest_attested_rung: Option<u8>,
    pub statement: Option<WorkReceiptStatement>,
    pub reveals: Vec<String>,
}

/// Execute `archie repo receipt create`. With `--quiet` (the hook) every failure is one line
/// on stderr and the command still succeeds: a receipt never stands between a user and a push.
pub fn run_create_command(
    args: CreateArgs,
    db_path: Option<PathBuf>,
    ui: &crate::ui::Ui,
) -> Result<()> {
    if std::env::var_os(PUSH_GUARD_ENV).is_some() {
        return Ok(());
    }
    let quiet = args.quiet;
    match run_create(args, db_path, ui) {
        Ok(()) => Ok(()),
        Err(e) if quiet => {
            eprintln!("archie: receipt not created ({e:#}); the push goes ahead");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

fn run_create(args: CreateArgs, db_path: Option<PathBuf>, ui: &crate::ui::Ui) -> Result<()> {
    let key_path = default_key_path()?;
    let Some(key) = load_key(&key_path)? else {
        let msg = "no receipt key yet: run `archie repo receipt init` (prints the setup) or \
                   `archie repo receipt init --write`";
        if args.json {
            print_json(&CreateReport::skipped("no_key", args.transport))?;
            return Ok(());
        }
        if args.quiet {
            eprintln!("archie: {msg}");
            return Ok(());
        }
        bail!("{msg}");
    };
    let signer = key.public_key();

    let storage = Arc::new(match db_path {
        Some(p) => Storage::open_path(&p)?,
        None => Storage::open_default()?,
    });
    let opts = BuildOptions {
        query: SuspectQuery {
            repo: args.repo.clone().unwrap_or_else(|| PathBuf::from(".")),
            base: args.base.clone(),
            ..SuspectQuery::default()
        },
        include_spend: args.include_spend,
        now: Utc::now(),
    };
    let loader = ScannerTraceLoader::new(storage.clone());
    let built = if args.quiet || args.json {
        build_receipt(&storage, &loader, &signer, &opts)?
    } else {
        crate::ui::with_status(ui, "building receipt", || {
            build_receipt(&storage, &loader, &signer, &opts)
        })?
    };
    let Some(built) = built else {
        if args.json {
            print_json(&CreateReport::skipped("empty_range", args.transport))?;
        } else if !args.quiet {
            println!("Nothing to sign: no commits in the range.");
        }
        return Ok(());
    };
    let reveal = reveals(&built.statement, &signer);
    let preview_id = built
        .statement
        .receipt_id()
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let tty = interactive();
    let show_preview = tty && !args.quiet && !args.json;
    if show_preview {
        print_preview(&built.statement, &reveal)?;
    }

    let cfg = crate::app::config::load_config()?;
    if !has_consent(&cfg, &built.root) {
        if args.json {
            let mut r = CreateReport::skipped("consent_missing", args.transport);
            r.receipt_id = Some(preview_id);
            r.statement = Some(built.statement.clone());
            r.reveals = reveal;
            r.highest_attested_rung = built.highest_attested_rung();
            print_json(&r)?;
            return Ok(());
        }
        if !show_preview {
            // The hook path, or a pipe: never ask, never export.
            eprintln!(
                "archie: receipt not signed: no consent recorded for this repository (run \
                 `archie repo receipt create` in a terminal, or `archie repo receipt init --write`)"
            );
            return Ok(());
        }
        print!("Sign and export Work Receipts for this repository from now on? [y/N] ");
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !matches!(answer.trim(), "y" | "Y" | "yes" | "Yes") {
            println!("Nothing signed.");
            return Ok(());
        }
        record_consent(&crate::app::config::config_file_path()?, &built.root)?;
    }

    let signed = sign_statement(&built.statement, &key).map_err(|e| anyhow::anyhow!("{e}"))?;
    let (written_to, push) = match args.transport {
        Transport::Notes => {
            write_note(&built.root, &built.head_sha, &signed.envelope)?;
            let push = if args.push {
                let remote = args.remote.as_deref().unwrap_or("origin");
                Some(push_notes(&built.root, remote)?)
            } else {
                None
            };
            (built.head_sha.clone(), push)
        }
        Transport::File => {
            let path = write_receipt_file(&built.root, &signed.receipt_id, &signed.envelope)?;
            let rel = path
                .strip_prefix(&built.root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            (rel, None)
        }
    };

    if args.json {
        print_json(&CreateReport {
            receipt_id: Some(signed.receipt_id.clone()),
            signed: true,
            skipped: None,
            transport: args.transport,
            written_to: Some(written_to),
            push,
            highest_attested_rung: built.highest_attested_rung(),
            statement: Some(signed.statement),
            reveals: reveal,
        })?;
        return Ok(());
    }

    let mut line = summary_line(&signed.receipt_id, &built);
    match (&args.transport, &push) {
        (Transport::Notes, Some(p)) if p.pushed => {
            line.push_str(&format!(", pushed {NOTES_REF}"));
        }
        (Transport::Notes, Some(p)) => {
            line.push_str(&format!(
                ", note written but not pushed ({})",
                p.error.as_deref().unwrap_or("push failed")
            ));
        }
        (Transport::Notes, None) => {
            line.push_str(&format!(", note on {} (not pushed)", short(&written_to)));
        }
        (Transport::File, _) => {
            line.push_str(&format!(", written to {written_to}; commit it to publish"));
        }
    }
    if args.quiet {
        eprintln!("archie: {line}");
    } else {
        println!("{}", ui.paint(crate::ui::Role::Verified, &line));
        println!("receipt_id {}", signed.receipt_id);
    }
    Ok(())
}

impl CreateReport {
    fn skipped(reason: &str, transport: Transport) -> Self {
        Self {
            receipt_id: None,
            signed: false,
            skipped: Some(reason.to_string()),
            transport,
            written_to: None,
            push: None,
            highest_attested_rung: None,
            statement: None,
            reveals: Vec::new(),
        }
    }
}

fn print_json<T: Serialize>(v: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

fn print_preview(statement: &WorkReceiptStatement, reveal: &[String]) -> Result<()> {
    println!("The exact receipt (decoded payload) that would be signed:");
    println!("{}", serde_json::to_string_pretty(statement)?);
    println!();
    println!("What this reveals:");
    for line in reveal {
        println!("  · {line}");
    }
    println!();
    Ok(())
}

fn short(sha: &str) -> String {
    sha.chars().take(8).collect()
}

// ---------------------------------------------------------------------------------------------
// `archie repo receipt show`
// ---------------------------------------------------------------------------------------------

/// One receipt as `show` reports it.
#[derive(Debug, Clone, Serialize)]
pub struct ShownReceipt {
    pub receipt_id: Option<String>,
    /// `key_not_linked` or `invalid: <reason>`. `show` has no GitHub keys, so a valid
    /// signature is never more than "key not linked" here.
    pub signature: String,
    pub keyid: Option<String>,
    /// True when the signer is this machine's receipt key.
    pub signed_by_this_machine: bool,
    pub source: String,
    pub statement: Option<WorkReceiptStatement>,
    /// Subjects that exist as commits in this checkout.
    pub commits_present: usize,
    pub commits_total: usize,
    /// Session ref → local session id, for refs this machine can map back.
    pub sessions: BTreeMap<String, Option<String>>,
}

fn is_hex_prefix(s: &str) -> bool {
    (4..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Every envelope `target` names: a receipt-id prefix (files, then every note), or a commit.
fn find_envelopes(root: &Path, target: &str) -> Result<Vec<(String, Envelope)>> {
    suspect::validate_ref(target, "receipt id or commit")?;
    let mut found = Vec::new();
    if is_hex_prefix(target) {
        let lower = target.to_ascii_lowercase();
        if let Ok(entries) = std::fs::read_dir(root.join(RECEIPT_DIR)) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(&lower) && name.ends_with(".dsse.json") {
                    if let Ok(bytes) = std::fs::read(entry.path()) {
                        for env in parse_envelope_lines(&bytes) {
                            found.push((format!("{RECEIPT_DIR}/{name}"), env));
                        }
                    }
                }
            }
        }
        if let Ok(list) = suspect::git_stdout(root, &["notes", "--ref", NOTES_REF_NAME, "list"]) {
            for line in list.lines() {
                let mut parts = line.split_whitespace();
                let (Some(blob), Some(commit)) = (parts.next(), parts.next()) else {
                    continue;
                };
                let Ok(out) = suspect::git(root, &["cat-file", "-p", blob]) else {
                    continue;
                };
                for env in parse_envelope_lines(&out.stdout) {
                    if envelope_id(&env).is_some_and(|id| id.starts_with(&lower)) {
                        found.push((format!("note on {}", short(commit)), env));
                    }
                }
            }
        }
        if !found.is_empty() {
            return Ok(found);
        }
    }
    if let Ok(sha) = rev_parse_commit(root, target) {
        for env in read_note(root, &sha)? {
            found.push((format!("note on {}", short(&sha)), env));
        }
    }
    Ok(found)
}

/// `receipt_id` of an envelope's payload, without trusting anything else in it.
fn envelope_id(env: &Envelope) -> Option<String> {
    use base64::Engine as _;
    let payload = base64::engine::general_purpose::STANDARD
        .decode(&env.payload)
        .ok()?;
    Some(agentworth_schema::work_receipt::receipt_id_of(&payload))
}

/// Decode, check the signature offline, re-check the subjects against this checkout, and map
/// session refs back to local session ids when this machine's key signed them.
pub fn show_receipts(
    storage: &Storage,
    root: &Path,
    target: &str,
    local_key: Option<&PublicKey>,
) -> Result<Vec<ShownReceipt>> {
    let envelopes = find_envelopes(root, target)?;
    let mut out = Vec::new();
    for (source, env) in envelopes {
        let v = verify_envelope(&env, &[]);
        let (signature, keyid) = match &v.verdict {
            SignatureVerdict::SignedBy { keyid, .. } | SignatureVerdict::KeyNotLinked { keyid } => {
                ("key_not_linked".to_string(), Some(keyid.clone()))
            }
            SignatureVerdict::Invalid(reason) => (format!("invalid: {reason:?}"), None),
        };
        let Some(receipt) = v.receipt else {
            out.push(ShownReceipt {
                receipt_id: envelope_id(&env),
                signature,
                keyid,
                signed_by_this_machine: false,
                source,
                statement: None,
                commits_present: 0,
                commits_total: 0,
                sessions: BTreeMap::new(),
            });
            continue;
        };
        let statement = receipt.statement;
        let commits_total = statement.subject.len();
        let commits_present = statement
            .subject
            .iter()
            .filter(|s| {
                let spec = format!("{}^{{commit}}", s.digest.git_commit);
                suspect::git(root, &["cat-file", "-e", &spec])
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            })
            .count();
        let mine = local_key.is_some_and(|k| *k == receipt.signer);
        let mut sessions: BTreeMap<String, Option<String>> = statement
            .predicate
            .sessions
            .iter()
            .map(|s| (s.session_ref.clone(), None))
            .collect();
        if mine {
            let raw = receipt.signer.raw();
            let all = storage.list_sessions_filtered(&agentworth_storage::SessionFilter {
                include_stubs: Some(true),
                ..Default::default()
            })?;
            for summary in all {
                let r = agentworth_schema::work_receipt::session_ref(&raw, &summary.session_id);
                if let Some(slot) = sessions.get_mut(&r) {
                    *slot = Some(summary.session_id.clone());
                }
            }
        }
        out.push(ShownReceipt {
            receipt_id: Some(receipt.receipt_id),
            signature,
            keyid,
            signed_by_this_machine: mine,
            source,
            statement: Some(statement),
            commits_present,
            commits_total,
            sessions,
        });
    }
    Ok(out)
}

/// Execute `archie repo receipt show <receipt-id|commit>`.
pub fn run_show_command(
    target: &str,
    repo: Option<PathBuf>,
    json: bool,
    db_path: Option<PathBuf>,
) -> Result<()> {
    let root = suspect::resolve_repo_root(&repo.unwrap_or_else(|| PathBuf::from(".")))?;
    let storage = match db_path {
        Some(p) => Storage::open_path(&p)?,
        None => Storage::open_default()?,
    };
    let local = read_public_key(&default_key_path()?)?;
    let shown = show_receipts(&storage, &root, target, local.as_ref())?;
    if json {
        return print_json(&shown);
    }
    if shown.is_empty() {
        bail!("no receipt matches '{target}' (looked in {RECEIPT_DIR}/ and {NOTES_REF})");
    }
    for r in &shown {
        println!(
            "receipt {}  ({})",
            r.receipt_id.as_deref().unwrap_or("?"),
            r.source
        );
        let who = if r.signed_by_this_machine {
            " · signed by this machine's key"
        } else {
            ""
        };
        println!(
            "  signature  {}{}{}",
            r.signature,
            r.keyid
                .as_deref()
                .map(|k| format!(" · {k}"))
                .unwrap_or_default(),
            who
        );
        let Some(st) = &r.statement else { continue };
        let p = &st.predicate;
        println!(
            "  created    {} · {} {}",
            p.created_at, p.generator.name, p.generator.version
        );
        println!(
            "  commits    {} of {} present in this checkout · range rule {:?}",
            r.commits_present, r.commits_total, p.range.rule
        );
        for s in &p.sessions {
            let local = r
                .sessions
                .get(&s.session_ref)
                .and_then(|o| o.as_deref())
                .unwrap_or("not on this machine");
            let rung = s
                .highest_rung
                .map(|h| format!("rung {h}"))
                .unwrap_or_else(|| "no rung".to_string());
            println!(
                "  session    {} → {}  · {} · {} · {} tokens",
                s.session_ref, local, s.adapter, rung, s.tokens_total
            );
        }
        if p.truncated {
            println!("  truncated  files[] dropped to fit the 64 KiB cap");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// `archie repo receipt key`
// ---------------------------------------------------------------------------------------------

/// The `gh` line that registers the receipt key as a GitHub SSH *signing* key.
pub fn github_signing_key_command(key_path: &Path) -> String {
    format!(
        "gh ssh-key add {} --type signing --title \"AgentWorth receipts\"",
        shell_quote(&pub_path(key_path).to_string_lossy())
    )
}

fn shell_quote(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-~".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Execute `archie repo receipt key [--github]`.
pub fn run_key_command(github: bool, json: bool) -> Result<()> {
    let path = default_key_path()?;
    let Some(public) = read_public_key(&path)? else {
        bail!(
            "no receipt key at {:?} yet: `archie repo receipt init --write` creates it",
            path
        );
    };
    if json {
        return print_json(&serde_json::json!({
            "public_key": public.to_openssh(),
            "fingerprint": public.fingerprint(),
            "path": path.to_string_lossy(),
            "github_command": github_signing_key_command(&path),
        }));
    }
    if github {
        println!("{}", github_signing_key_command(&path));
        return Ok(());
    }
    println!("{} {KEY_COMMENT}", public.to_openssh());
    println!("{}", public.fingerprint());
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// `archie repo receipt init`
// ---------------------------------------------------------------------------------------------

/// What `init --write` did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InitReport {
    pub workflow: String,
    pub hook: String,
    pub key_created: bool,
    pub fingerprint: String,
    pub consent_recorded: bool,
}

/// `<git dir>/hooks/pre-push`, honouring worktrees and `core.hooksPath`.
fn hook_path(root: &Path) -> Result<PathBuf> {
    let p = suspect::git_stdout(root, &["rev-parse", "--git-path", "hooks/pre-push"])?;
    let p = PathBuf::from(p);
    Ok(if p.is_absolute() { p } else { root.join(p) })
}

/// Write the workflow and the hook where absent, create the key, and record consent. Never
/// overwrites an existing workflow or hook: an existing hook without the receipt line is
/// reported so the user can add the line by hand.
pub fn init_write(root: &Path, key_path: &Path, config_path: &Path) -> Result<InitReport> {
    let workflow_path = root.join(WORKFLOW_PATH);
    let workflow = if workflow_path.exists() {
        format!("{WORKFLOW_PATH} already exists, left as is")
    } else {
        if let Some(dir) = workflow_path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&workflow_path, WORKFLOW_YAML)?;
        format!("wrote {WORKFLOW_PATH}")
    };

    let hook = hook_path(root)?;
    let hook_msg = match std::fs::read_to_string(&hook) {
        Ok(existing) if existing.contains(HOOK_MARKER) => {
            "pre-push hook already signs receipts".to_string()
        }
        Ok(_) => format!(
            "a pre-push hook already exists at {:?}; left as is. Add this line to it: \
             archie repo receipt create --push --quiet --remote \"${{1:-origin}}\" || true",
            hook
        ),
        Err(_) => {
            if let Some(dir) = hook.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&hook, suspect::PRE_PUSH_HOOK)?;
            set_mode(&hook, 0o755)?;
            "wrote the pre-push hook".to_string()
        }
    };

    let (key, key_created) = load_or_create_key(key_path)?;
    record_consent(config_path, root)?;
    Ok(InitReport {
        workflow,
        hook: hook_msg,
        key_created,
        fingerprint: key.public_key().fingerprint(),
        consent_recorded: true,
    })
}

/// Execute `archie repo receipt init [--write]`.
pub fn run_init_command(write: bool, repo: Option<PathBuf>, json: bool) -> Result<()> {
    let root = suspect::resolve_repo_root(&repo.unwrap_or_else(|| PathBuf::from(".")))?;
    if !write {
        if json {
            return print_json(&serde_json::json!({
                "workflow_path": WORKFLOW_PATH,
                "workflow": WORKFLOW_YAML,
                "hook": suspect::PRE_PUSH_HOOK,
            }));
        }
        println!("# {WORKFLOW_PATH}");
        print!("{WORKFLOW_YAML}");
        println!();
        println!("# .git/hooks/pre-push (chmod +x)");
        print!("{}", suspect::PRE_PUSH_HOOK);
        println!();
        println!(
            "# Nothing was written. `archie repo receipt init --write` writes both (never over an \
             existing file), creates your receipt key, and records that receipts may be signed \
             and pushed for this repository."
        );
        return Ok(());
    }
    let key_path = default_key_path()?;
    let report = init_write(&root, &key_path, &crate::app::config::config_file_path()?)?;
    if json {
        return print_json(&report);
    }
    println!("{}", report.workflow);
    println!("{}", report.hook);
    println!(
        "{} receipt key {}",
        if report.key_created {
            "created"
        } else {
            "using"
        },
        report.fingerprint
    );
    println!("recorded consent: receipts for this repository are signed and pushed as git notes");
    println!(
        "optional, for \"Signed by @you\": {}",
        github_signing_key_command(&key_path)
    );
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// MCP preview
// ---------------------------------------------------------------------------------------------

/// What the MCP `repo_receipt` tool returns: the receipt the next push would sign, and the
/// local per-commit prediction. Built read-only: nothing is signed, written or pushed.
#[derive(Debug, Clone, Serialize)]
pub struct ReceiptPreview {
    /// False when no receipt key exists yet; refs are then derived from a placeholder key and
    /// will change once `archie repo receipt init --write` creates the real one.
    pub key_present: bool,
    pub consent_recorded: bool,
    /// What the next `create` would be called (it changes with `created_at`'s minute).
    pub receipt_id: Option<String>,
    pub commits_in_range: usize,
    pub statement: Option<WorkReceiptStatement>,
    pub predicted: Vec<PredictedCommit>,
    /// Session ref → local session id, so an agent can open the session behind a rung.
    pub sessions_local: BTreeMap<String, String>,
    pub reveals: Vec<String>,
    pub summary: String,
}

/// Build the preview for the MCP tool. `key` is the public key to derive refs from.
pub fn preview_receipt(
    storage: &Storage,
    loader: &dyn TraceLoader,
    key: Option<PublicKey>,
    consent_for: &dyn Fn(&Path) -> bool,
    opts: &BuildOptions,
) -> Result<ReceiptPreview> {
    let key_present = key.is_some();
    let signer = match key {
        Some(k) => k,
        // A fixed placeholder (RFC 8032 TEST 1 public key, never a real signer).
        None => PublicKey::from_raw(agentworth_schema::work_receipt::example::SIGNER_PUBKEY)
            .map_err(|e| anyhow::anyhow!("{e}"))?,
    };
    let built = build_receipt(storage, loader, &signer, opts)?;
    let Some(built) = built else {
        return Ok(ReceiptPreview {
            key_present,
            consent_recorded: false,
            receipt_id: None,
            commits_in_range: 0,
            statement: None,
            predicted: Vec::new(),
            sessions_local: BTreeMap::new(),
            reveals: Vec::new(),
            summary: "No commits in the range: nothing to sign.".to_string(),
        });
    };
    let predicted = predict(&built.statement);
    let attested = predicted
        .iter()
        .filter_map(|p| p.highest_attested_rung)
        .max();
    let claimed = predicted
        .iter()
        .flat_map(|p| p.claimed_only_rungs.iter().copied())
        .max();
    let uncovered = predicted.iter().filter(|p| p.sessions.is_empty()).count();
    let mut summary = match attested {
        Some(r) => format!("This push would show rung {r} attested locally"),
        None => "This push would show no rung attested locally".to_string(),
    };
    if let Some(c) = claimed {
        summary.push_str(&format!(", rung {c} claimed only"));
    }
    summary.push_str(&format!(
        "; {uncovered} of {} commit(s) have no indexed session. Rungs 2, 4 and 5 are \
         re-derived from CI by the check itself.",
        predicted.len()
    ));
    if attested.is_none_or(|r| r < 3) {
        summary.push_str(" Run the tests before pushing to attest rung 3.");
    }
    Ok(ReceiptPreview {
        key_present,
        consent_recorded: consent_for(&built.root),
        receipt_id: built.statement.receipt_id().ok(),
        commits_in_range: built.statement.subject.len(),
        reveals: reveals(&built.statement, &signer),
        statement: Some(built.statement),
        predicted,
        sessions_local: built.session_ids_by_ref,
        summary,
    })
}

#[cfg(test)]
#[path = "work_receipt/tests.rs"]
mod tests;
