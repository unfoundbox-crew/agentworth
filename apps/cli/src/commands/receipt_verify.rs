//! `archie repo receipt verify`: the offline half of the AgentWorth / Evidence check
//! (spec `docs/specs/evidence-check.md`, draft §3 and §4 row 25).
//!
//! Given a PR range (`base..head`) in a local clone, this module
//!
//! 1. reads every Work Receipt it can find for that range: DSSE envelopes in git notes under
//!    [`NOTES_REF`] on PR commits (transport A), committed `*.dsse.json` files under
//!    [`RECEIPT_DIR`] in the head tree (transport B), and any file passed explicitly;
//! 2. verifies each envelope with `agentworth-attest` against the GitHub signing keys the caller
//!    supplies (`keys.json`), landing in one of the three signature states of spec §2.4;
//! 3. re-derives every rung CI can see (§3.1): rung 4 (the receipt's SHA is in this PR), rung 2
//!    (receipt `files[]` against `git diff-tree`), rung 5 (head CI from `ci.json`). Rung 3 can
//!    only be *attested* (signed and `verified_locally`); rung 1 is never evidence;
//! 4. applies the conclusion policy of §3.2 (neutral by default, `success` only when fully
//!    covered by linked keys with head CI green, `failure` only through opt-in `fail-on`);
//! 5. renders the check as markdown or as the `POST /check-runs` body ([`render`]).
//!
//! There is no network code here. CI results and signing keys arrive as files the Action
//! collects with `gh api` (PR 4); this binary never fetches them. Every display string is built
//! from enums, integers, validated hex and allowlisted names, never copied from a receipt.
//!
//! The pure core is [`verify`], which takes already-collected inputs, so the rung matrix and
//! golden render tests do not depend on git. [`collect_pr_commits`] and [`collect_receipts`]
//! are the thin git layer; [`run_receipt_verify_command`] wires files and git together.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use agentworth_attest::{
    verify_envelope, Envelope, InvalidReason, KeyLink, PublicKey, SignatureVerdict,
};
use agentworth_schema::work_receipt::{
    receipt_id_of, CommandClass, EvidenceSource, ReceiptTestOrigin, WorkReceiptStatement,
    MAX_RECEIPT_BYTES,
};
use agentworth_schema::OutcomeKind;
use anyhow::{bail, Context, Result};
use base64::Engine as _;
use serde_json::Value;

pub mod render;

pub use render::{render_check_json, render_markdown, render_title};

/// Name of the check run the Action posts, and the one CI entry [`CiInput`] always ignores
/// (the check must never count itself as CI).
pub const CHECK_NAME: &str = "AgentWorth / Evidence";
/// Git notes ref of transport A.
pub const NOTES_REF: &str = "refs/notes/agentworth";
/// Directory of transport B, relative to the repository root.
pub const RECEIPT_DIR: &str = ".agentworth/receipts";
/// Most envelopes read for one PR; the rest are ignored (and counted).
pub const MAX_ENVELOPES: usize = 64;
/// Most GitHub logins whose signing keys are considered (spec §2.4: "cap 10 logins").
pub const MAX_KEY_LOGINS: usize = 10;
/// Most PR commits examined. GitHub's own PR commit listing stops at 250.
pub const MAX_PR_COMMITS: usize = 250;
/// Largest note or receipt file read (a valid envelope is ~4/3 of the 64 KiB cap).
const MAX_SOURCE_BYTES: usize = 4 * MAX_RECEIPT_BYTES;

// ---------------------------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------------------------

/// One commit of the PR, as git on the runner sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrCommit {
    /// Full lowercase SHA.
    pub sha: String,
    /// First line of the commit message (escaped by the renderer, never interpreted).
    pub subject: String,
    /// Repo-relative paths in this commit's own diff (first parent for a merge).
    pub files: BTreeSet<String>,
}

/// Where a receipt came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Transport {
    /// A git note under [`NOTES_REF`] on a PR commit.
    Note,
    /// A committed file under [`RECEIPT_DIR`].
    File,
    /// A path passed on the command line.
    Explicit,
}

/// Raw receipt bytes before parsing: one note, one file. A note can hold several envelopes,
/// one per line, after `git notes merge -s cat_sort_uniq`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptSource {
    pub transport: Transport,
    pub bytes: Vec<u8>,
}

/// One CI entry on the head SHA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiEntryState {
    Passed,
    Failed,
    Pending,
}

/// Check runs and commit statuses on the head SHA (`ci.json`).
///
/// Accepted shapes, so the Action can pass `gh api` output through `jq -s` unchanged:
///
/// ```json
/// { "head_sha": "<sha>",
///   "check_runs": [ { "name": "test", "status": "completed", "conclusion": "success" } ]
///                 | { "total_count": 1, "check_runs": [ ... ] },
///   "statuses":   [ { "context": "ci/x", "state": "success" } ]
///                 | { "state": "success", "statuses": [ ... ] } }
/// ```
///
/// Entries named [`CHECK_NAME`] are dropped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CiInput {
    /// The SHA the data was collected for. When present and not the PR head, the data is
    /// ignored (CI unknown) rather than credited to the wrong commit.
    pub head_sha: Option<String>,
    pub entries: Vec<CiEntryState>,
}

impl CiInput {
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let v: Value = serde_json::from_slice(bytes).context("ci.json is not JSON")?;
        let obj = v.as_object().context("ci.json must be a JSON object")?;
        let head_sha = obj
            .get("head_sha")
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase);
        let mut entries = Vec::new();
        for run in list_field(obj.get("check_runs"), "check_runs") {
            let name = run.get("name").and_then(Value::as_str).unwrap_or("");
            if name == CHECK_NAME {
                continue;
            }
            let status = run.get("status").and_then(Value::as_str).unwrap_or("");
            let conclusion = run.get("conclusion").and_then(Value::as_str).unwrap_or("");
            entries.push(check_run_state(status, conclusion));
        }
        for st in list_field(obj.get("statuses"), "statuses") {
            let context = st.get("context").and_then(Value::as_str).unwrap_or("");
            if context == CHECK_NAME {
                continue;
            }
            let state = st.get("state").and_then(Value::as_str).unwrap_or("");
            entries.push(match state {
                "success" => CiEntryState::Passed,
                "failure" | "error" => CiEntryState::Failed,
                _ => CiEntryState::Pending,
            });
        }
        Ok(Self { head_sha, entries })
    }
}

/// A list, or an object wrapping the list under `key` (the raw `gh api` response).
fn list_field<'a>(v: Option<&'a Value>, key: &str) -> Vec<&'a serde_json::Map<String, Value>> {
    let list = match v {
        Some(Value::Array(a)) => Some(a),
        Some(Value::Object(o)) => o.get(key).and_then(Value::as_array),
        _ => None,
    };
    list.map(|a| a.iter().filter_map(Value::as_object).collect())
        .unwrap_or_default()
}

fn check_run_state(status: &str, conclusion: &str) -> CiEntryState {
    if status != "completed" {
        return CiEntryState::Pending;
    }
    match conclusion {
        "success" | "neutral" | "skipped" => CiEntryState::Passed,
        "" => CiEntryState::Pending,
        _ => CiEntryState::Failed,
    }
}

/// Head CI as the check reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiState {
    /// No `ci.json`, or it describes another SHA.
    Unknown,
    /// `ci.json` present but no check run or status other than this check.
    NoChecks,
    Pending {
        done: usize,
        total: usize,
    },
    Red {
        failed: usize,
        total: usize,
    },
    Green {
        passed: usize,
        total: usize,
    },
}

impl CiState {
    pub fn of(ci: Option<&CiInput>, head: &str) -> Self {
        let Some(ci) = ci else {
            return Self::Unknown;
        };
        if ci.head_sha.as_deref().is_some_and(|s| s != head) {
            return Self::Unknown;
        }
        let total = ci.entries.len();
        let count = |want| ci.entries.iter().filter(|e| **e == want).count();
        let (passed, failed, pending) = (
            count(CiEntryState::Passed),
            count(CiEntryState::Failed),
            count(CiEntryState::Pending),
        );
        if total == 0 {
            Self::NoChecks
        } else if failed > 0 {
            Self::Red { failed, total }
        } else if pending > 0 {
            Self::Pending {
                done: total - pending,
                total,
            }
        } else {
            Self::Green { passed, total }
        }
    }

    pub fn is_green(self) -> bool {
        matches!(self, Self::Green { .. })
    }
}

/// Parse `keys.json`: the public GitHub signing keys of the PR author and commit authors.
///
/// Accepted shapes: `[{"login": "octocat", "keys": ["ssh-ed25519 AAAA… title", {"key": "…"}]}]`
/// or `{"octocat": [ … ]}` where each key is a string or a `GET /users/{u}/ssh_signing_keys`
/// item. Logins that are not valid GitHub logins and keys that are not `ssh-ed25519` are
/// skipped. At most [`MAX_KEY_LOGINS`] logins are kept, in input order.
pub fn parse_keys_json(bytes: &[u8]) -> Result<Vec<KeyLink>> {
    let v: Value = serde_json::from_slice(bytes).context("keys.json is not JSON")?;
    let mut pairs: Vec<(String, &Value)> = Vec::new();
    match &v {
        Value::Array(items) => {
            for item in items {
                let login = item.get("login").and_then(Value::as_str).unwrap_or("");
                if let Some(keys) = item.get("keys") {
                    pairs.push((login.to_string(), keys));
                }
            }
        }
        Value::Object(map) => {
            for (login, keys) in map {
                pairs.push((login.clone(), keys));
            }
        }
        _ => bail!("keys.json must be a JSON array or object"),
    }
    let mut links = Vec::new();
    let mut logins = Vec::new();
    for (login, keys) in pairs {
        if !is_github_login(&login) {
            continue;
        }
        if !logins.contains(&login) {
            if logins.len() == MAX_KEY_LOGINS {
                continue;
            }
            logins.push(login.clone());
        }
        let Some(keys) = keys.as_array() else {
            continue;
        };
        for k in keys {
            let line = k
                .as_str()
                .or_else(|| k.get("key").and_then(Value::as_str))
                .unwrap_or("");
            if let Ok(key) = PublicKey::parse_openssh(line) {
                links.push(KeyLink {
                    login: login.clone(),
                    key,
                });
            }
        }
    }
    Ok(links)
}

/// GitHub login rules: 1–39 ASCII alphanumerics or single hyphens, not starting or ending with
/// a hyphen. Anything else is never displayed.
pub fn is_github_login(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 39
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Opt-in failure conditions (spec §3.2). Without any, the conclusion is never `failure`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailOn {
    /// Any receipt for this PR has an invalid signature (or is unreadable).
    InvalidSignature,
    /// The highest rung (verified here or attested) is below N, or there is none.
    BelowRung(u8),
    /// Any PR commit is covered by no receipt at all.
    Unattributed,
}

impl FailOn {
    /// Parse the Action's `fail-on` input: comma- or whitespace-separated tokens.
    pub fn parse_list(s: &str) -> Result<Vec<Self>> {
        let mut out = Vec::new();
        for tok in s.split(|c: char| c == ',' || c.is_whitespace()) {
            let tok = tok.trim();
            if tok.is_empty() {
                continue;
            }
            let item = match tok {
                "invalid-signature" => Self::InvalidSignature,
                "unattributed" => Self::Unattributed,
                _ => match tok.strip_prefix("below-rung:").map(str::parse::<u8>) {
                    Some(Ok(n)) if (1..=5).contains(&n) => Self::BelowRung(n),
                    _ => bail!(
                        "unknown fail-on value {tok:?}: expected invalid-signature, below-rung:1..5, or unattributed"
                    ),
                },
            };
            if !out.contains(&item) {
                out.push(item);
            }
        }
        Ok(out)
    }
}

/// Everything [`verify`] needs, already collected.
#[derive(Debug, Clone, Default)]
pub struct VerifyInput {
    pub base: String,
    pub head: String,
    /// PR commits, newest first (the order `git log` prints and the check shows).
    pub commits: Vec<PrCommit>,
    pub receipts: Vec<ReceiptSource>,
    pub ci: Option<CiInput>,
    pub keys: Vec<KeyLink>,
    pub fail_on: Vec<FailOn>,
}

// ---------------------------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------------------------

/// Why an envelope is invalid, reduced to what the check prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidKind {
    /// Not a DSSE envelope at all (bad JSON in a note or receipt file).
    Unreadable,
    UnsupportedPayloadType,
    BadEncoding,
    TooLarge,
    MalformedStatement,
    BadSignerKey,
    SignatureMismatch,
    NonCanonicalPayload,
}

impl From<&InvalidReason> for InvalidKind {
    fn from(r: &InvalidReason) -> Self {
        match r {
            InvalidReason::UnsupportedPayloadType => Self::UnsupportedPayloadType,
            InvalidReason::BadBase64 => Self::BadEncoding,
            InvalidReason::TooLarge => Self::TooLarge,
            InvalidReason::MalformedStatement(_) => Self::MalformedStatement,
            InvalidReason::BadSignerKey => Self::BadSignerKey,
            InvalidReason::SignatureMismatch => Self::SignatureMismatch,
            InvalidReason::NonCanonicalPayload => Self::NonCanonicalPayload,
        }
    }
}

/// Signature state of one receipt (spec §2.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureState {
    /// "Signed by @login". `login` passed [`is_github_login`].
    SignedBy { login: String, keyid: String },
    /// "Signed, key not linked".
    KeyNotLinked { keyid: String },
    /// "Signature invalid": every rung it names is claimed only.
    Invalid(InvalidKind),
}

impl SignatureState {
    pub fn is_valid(&self) -> bool {
        !matches!(self, Self::Invalid(_))
    }
}

/// One receipt found for this PR.
#[derive(Debug, Clone)]
pub struct ReceiptReport {
    /// `hex(sha256(payload))`, when the payload decoded at all.
    pub receipt_id: Option<String>,
    pub transport: Transport,
    pub signature: SignatureState,
    /// The Statement. For an invalid receipt this is the *unverified* decode (used only to show
    /// which commits it claims), and `None` when even that failed.
    pub statement: Option<WorkReceiptStatement>,
    /// Subjects that are PR commits.
    pub subjects_in_pr: usize,
    /// Subjects that are not (rebased or squashed SHAs, or another branch).
    pub subjects_outside_pr: usize,
}

/// An adapter name from the receipt, shown only when it is one AgentWorth ships.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AdapterLabel(&'static str);

impl AdapterLabel {
    const KNOWN: &'static [&'static str] = &[
        "aider",
        "antigravity",
        "claude_code",
        "cline",
        "codex",
        "cursor",
        "deepseek",
        "gemini",
        "goose",
        "grok",
        "herdr",
        "hermes",
        "kimi",
        "manus",
        "minimax",
        "openclaw",
        "opencode",
        "pi",
        "qwen",
        "windsurf",
        "zhipu",
    ];

    pub fn of(adapter: &str) -> Self {
        Self(
            Self::KNOWN
                .iter()
                .find(|k| **k == adapter)
                .copied()
                .unwrap_or("other"),
        )
    }

    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

/// A session that worked on a commit: adapter and the first 8 hex chars of its ref.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SessionTag {
    pub adapter: AdapterLabel,
    pub short_ref: String,
}

/// A fact this runner re-derived ("Verified here").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifiedFact {
    /// Rung 5: head CI green on this SHA.
    Ci { passed: usize, total: usize },
    /// Rung 4: a valid receipt names this SHA and it is in `base..head`.
    CommitInPr,
    /// Rung 2: `matched` of the receipt's `claimed` files appear in this commit's diff.
    FilesMatch { matched: usize, claimed: usize },
    /// Not a rung: head CI is green and covers this (earlier) commit's final tree.
    HeadCiCoversFinalTree,
}

impl VerifiedFact {
    pub fn rung(&self) -> Option<u8> {
        match self {
            Self::Ci { .. } => Some(5),
            Self::CommitInPr => Some(4),
            Self::FilesMatch { .. } => Some(2),
            Self::HeadCiCoversFinalTree => None,
        }
    }
}

/// A rung-3 run that a valid signature attests ("Attested locally (signed)").
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AttestedRun {
    pub command_class: Option<CommandClassKey>,
    pub source: SourceKey,
    pub test_origin: Option<OriginKey>,
    pub count: usize,
}

/// Orderable mirrors of the schema enums, so attested runs group and sort deterministically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CommandClassKey(pub u8);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceKey(pub u8);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct OriginKey(pub u8);

impl CommandClassKey {
    const ALL: [CommandClass; 9] = [
        CommandClass::CargoTest,
        CommandClass::CargoBuild,
        CommandClass::NpmTest,
        CommandClass::PnpmTest,
        CommandClass::Pytest,
        CommandClass::GoTest,
        CommandClass::MakeTest,
        CommandClass::GhPrCreate,
        CommandClass::OtherTestCommand,
    ];
    pub fn of(c: CommandClass) -> Self {
        Self(Self::ALL.iter().position(|x| *x == c).unwrap_or(8) as u8)
    }
    pub fn class(self) -> CommandClass {
        Self::ALL[usize::from(self.0).min(8)]
    }
}

impl SourceKey {
    const ALL: [EvidenceSource; 6] = [
        EvidenceSource::ShellExit0,
        EvidenceSource::ToolResult,
        EvidenceSource::FileChange,
        EvidenceSource::GitCommit,
        EvidenceSource::AssistantClaim,
        EvidenceSource::Other,
    ];
    pub fn of(s: EvidenceSource) -> Self {
        Self(Self::ALL.iter().position(|x| *x == s).unwrap_or(5) as u8)
    }
    pub fn source(self) -> EvidenceSource {
        Self::ALL[usize::from(self.0).min(5)]
    }
}

impl OriginKey {
    const ALL: [ReceiptTestOrigin; 3] = [
        ReceiptTestOrigin::AgentChosen,
        ReceiptTestOrigin::HeldOut,
        ReceiptTestOrigin::Unknown,
    ];
    pub fn of(o: ReceiptTestOrigin) -> Self {
        Self(Self::ALL.iter().position(|x| *x == o).unwrap_or(2) as u8)
    }
    pub fn origin(self) -> ReceiptTestOrigin {
        Self::ALL[usize::from(self.0).min(2)]
    }
}

/// Something only claimed ("Claimed only").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    /// Rung-1 completion claims (never evidence).
    CompletionClaims(usize),
    /// Rungs 2..=5 the receipt itself marks `verified_locally: false`.
    NotVerifiedLocally { rung: u8, count: usize },
    /// Claimed files that are not in this commit's diff.
    FilesNotInDiff(usize),
    /// Only invalid receipts cover this commit; `rungs` is how many rungs they claimed for it.
    InvalidSignature { rungs: usize, highest: Option<u8> },
}

/// How a commit is covered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// At least one valid receipt names it; `linked` when one of them is "Signed by @login".
    Signed { linked: bool },
    /// Only receipts with invalid signatures name it.
    InvalidOnly,
    /// No receipt names it: "unattributed", unknown, never clean.
    None,
}

/// One row of the check.
#[derive(Debug, Clone)]
pub struct CommitReport {
    pub sha: String,
    pub subject: String,
    pub is_head: bool,
    pub coverage: Coverage,
    pub sessions: Vec<SessionTag>,
    pub verified: Vec<VerifiedFact>,
    pub attested: Vec<AttestedRun>,
    pub claimed: Vec<Claim>,
}

impl CommitReport {
    pub fn highest_verified(&self) -> Option<u8> {
        self.verified.iter().filter_map(VerifiedFact::rung).max()
    }
    pub fn highest_attested(&self) -> Option<u8> {
        (!self.attested.is_empty()).then_some(3)
    }
}

/// Integer risk totals across the sessions of valid receipts that touch this PR.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RiskTotals {
    pub sessions: usize,
    pub unknown_sessions: usize,
    pub demoted_claims: u64,
    pub loop_alerts: u64,
    pub unresolved_loops: u64,
    pub recoveries: u64,
    pub governor_halts: u64,
}

/// The check conclusion (spec §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conclusion {
    Success,
    Neutral,
    Failure,
}

impl Conclusion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Neutral => "neutral",
            Self::Failure => "failure",
        }
    }
}

/// Everything the renderer needs.
#[derive(Debug, Clone)]
pub struct VerifyReport {
    pub base: String,
    pub head: String,
    pub commits: Vec<CommitReport>,
    /// Receipts that name at least one PR commit, or could not be read at all.
    pub receipts: Vec<ReceiptReport>,
    /// Valid receipts none of whose subjects is a PR commit.
    pub stale_receipts: usize,
    /// Envelopes beyond [`MAX_ENVELOPES`], not examined.
    pub skipped_envelopes: usize,
    /// PR commits beyond [`MAX_PR_COMMITS`], not examined.
    pub skipped_commits: usize,
    pub ci: CiState,
    pub risk: RiskTotals,
    pub conclusion: Conclusion,
    /// Which `fail-on` conditions tripped (empty unless the conclusion is `failure`).
    pub failed_on: Vec<FailOn>,
}

impl VerifyReport {
    /// Headline: highest rung in the "Verified here" column across receipt-covered commits.
    pub fn highest_verified(&self) -> Option<u8> {
        self.commits
            .iter()
            .filter(|c| matches!(c.coverage, Coverage::Signed { .. }))
            .filter_map(CommitReport::highest_verified)
            .max()
    }

    pub fn highest_attested(&self) -> Option<u8> {
        self.commits
            .iter()
            .filter_map(CommitReport::highest_attested)
            .max()
    }

    pub fn signed_commits(&self) -> usize {
        self.commits
            .iter()
            .filter(|c| matches!(c.coverage, Coverage::Signed { .. }))
            .count()
    }

    pub fn any_invalid(&self) -> bool {
        self.receipts.iter().any(|r| !r.signature.is_valid())
    }
}

// ---------------------------------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------------------------------

/// Split a note or file into envelopes: the whole thing as one JSON value, else one per line.
/// Lines that are not envelopes come back as `Err(())` so they can be counted as unreadable.
fn split_envelopes(bytes: &[u8]) -> Vec<Result<Envelope, ()>> {
    if bytes.len() > MAX_SOURCE_BYTES {
        return vec![Err(())];
    }
    if let Ok(e) = Envelope::from_json(bytes) {
        return vec![Ok(e)];
    }
    let text = String::from_utf8_lossy(bytes);
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| Envelope::from_json(l.as_bytes()).map_err(|_| ()))
        .collect()
}

/// Best-effort decode of an envelope that did not verify, so the check can say which commits
/// it *claims*. Nothing decoded here is ever trusted.
fn unverified_statement(envelope: &Envelope) -> (Option<String>, Option<WorkReceiptStatement>) {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    let payload = [STANDARD, URL_SAFE, STANDARD_NO_PAD, URL_SAFE_NO_PAD]
        .iter()
        .find_map(|e| e.decode(&envelope.payload).ok());
    let Some(payload) = payload else {
        return (None, None);
    };
    if payload.len() > MAX_RECEIPT_BYTES {
        return (Some(receipt_id_of(&payload)), None);
    }
    (
        Some(receipt_id_of(&payload)),
        WorkReceiptStatement::from_payload(&payload).ok(),
    )
}

/// Verify every receipt and re-derive every rung for the PR. Pure: no git, no network.
pub fn verify(input: &VerifyInput) -> VerifyReport {
    let skipped_commits = input.commits.len().saturating_sub(MAX_PR_COMMITS);
    let commits = &input.commits[..input.commits.len().min(MAX_PR_COMMITS)];
    let pr_shas: BTreeSet<&str> = commits.iter().map(|c| c.sha.as_str()).collect();
    let ci = CiState::of(input.ci.as_ref(), &input.head);

    // 1. Envelopes, deduplicated by their exact bytes, in a stable order.
    let mut seen = BTreeSet::new();
    let mut parsed: Vec<(Transport, Result<Envelope, ()>)> = Vec::new();
    let mut sources = input.receipts.clone();
    sources.sort_by(|a, b| a.transport.cmp(&b.transport).then(a.bytes.cmp(&b.bytes)));
    for src in &sources {
        for env in split_envelopes(&src.bytes) {
            if let Ok(e) = &env {
                if !seen.insert(e.to_json()) {
                    continue;
                }
            }
            parsed.push((src.transport, env));
        }
    }
    let skipped_envelopes = parsed.len().saturating_sub(MAX_ENVELOPES);
    parsed.truncate(MAX_ENVELOPES);

    // 2. Signatures.
    let mut receipts = Vec::new();
    let mut stale_receipts = 0;
    for (transport, env) in parsed {
        let report = match env {
            Err(()) => ReceiptReport {
                receipt_id: None,
                transport,
                signature: SignatureState::Invalid(InvalidKind::Unreadable),
                statement: None,
                subjects_in_pr: 0,
                subjects_outside_pr: 0,
            },
            Ok(env) => receipt_report(transport, &env, &input.keys, &pr_shas),
        };
        if report.signature.is_valid() && report.subjects_in_pr == 0 {
            stale_receipts += 1;
            continue;
        }
        receipts.push(report);
    }

    // 3. Rungs, per PR commit.
    let mut risk = RiskTotals::default();
    let mut risk_seen = BTreeSet::new();
    let mut rows = Vec::with_capacity(commits.len());
    for commit in commits {
        let is_head = commit.sha == input.head;
        let row = commit_report(commit, is_head, &receipts, ci, &mut risk, &mut risk_seen);
        rows.push(row);
    }

    let mut report = VerifyReport {
        base: input.base.clone(),
        head: input.head.clone(),
        commits: rows,
        receipts,
        stale_receipts,
        skipped_envelopes,
        skipped_commits,
        ci,
        risk,
        conclusion: Conclusion::Neutral,
        failed_on: Vec::new(),
    };
    let (conclusion, failed_on) = conclude(&report, &input.fail_on);
    report.conclusion = conclusion;
    report.failed_on = failed_on;
    report
}

fn receipt_report(
    transport: Transport,
    env: &Envelope,
    keys: &[KeyLink],
    pr_shas: &BTreeSet<&str>,
) -> ReceiptReport {
    let v = verify_envelope(env, keys);
    let (signature, receipt_id, statement) = match (v.verdict, v.receipt) {
        (SignatureVerdict::SignedBy { login, keyid }, Some(r)) => {
            let state = if is_github_login(&login) {
                SignatureState::SignedBy { login, keyid }
            } else {
                SignatureState::KeyNotLinked { keyid }
            };
            (state, Some(r.receipt_id), Some(r.statement))
        }
        (SignatureVerdict::KeyNotLinked { keyid }, Some(r)) => (
            SignatureState::KeyNotLinked { keyid },
            Some(r.receipt_id),
            Some(r.statement),
        ),
        (verdict, _) => {
            let kind = match &verdict {
                SignatureVerdict::Invalid(reason) => InvalidKind::from(reason),
                // A valid verdict always carries its receipt; fail closed if it ever does not.
                _ => InvalidKind::SignatureMismatch,
            };
            let (id, st) = unverified_statement(env);
            (SignatureState::Invalid(kind), id, st)
        }
    };
    let (subjects_in_pr, subjects_outside_pr) = statement
        .as_ref()
        .map(|s| {
            let n = s
                .subject
                .iter()
                .filter(|x| pr_shas.contains(x.digest.git_commit.as_str()))
                .count();
            (n, s.subject.len() - n)
        })
        .unwrap_or((0, 0));
    ReceiptReport {
        receipt_id,
        transport,
        signature,
        statement,
        subjects_in_pr,
        subjects_outside_pr,
    }
}

fn commit_report(
    commit: &PrCommit,
    is_head: bool,
    receipts: &[ReceiptReport],
    ci: CiState,
    risk: &mut RiskTotals,
    risk_seen: &mut BTreeSet<(String, String)>,
) -> CommitReport {
    let covering: Vec<&ReceiptReport> = receipts
        .iter()
        .filter(|r| {
            r.statement
                .as_ref()
                .is_some_and(|s| s.covers_commit(&commit.sha))
        })
        .collect();
    let valid: Vec<&ReceiptReport> = covering
        .iter()
        .copied()
        .filter(|r| r.signature.is_valid())
        .collect();
    let coverage = if !valid.is_empty() {
        Coverage::Signed {
            linked: valid
                .iter()
                .any(|r| matches!(r.signature, SignatureState::SignedBy { .. })),
        }
    } else if !covering.is_empty() {
        Coverage::InvalidOnly
    } else {
        Coverage::None
    };

    let mut row = CommitReport {
        sha: commit.sha.clone(),
        subject: commit.subject.clone(),
        is_head,
        coverage,
        sessions: Vec::new(),
        verified: Vec::new(),
        attested: Vec::new(),
        claimed: Vec::new(),
    };

    match coverage {
        Coverage::None => {}
        Coverage::InvalidOnly => {
            // Every rung an invalid receipt names drops to "claimed only".
            let mut rungs = 0;
            let mut highest = None;
            let mut tags = BTreeSet::new();
            for r in &covering {
                let st = r.statement.as_ref().expect("covering implies a statement");
                for s in sessions_for(st, &commit.sha) {
                    rungs += s.rungs.len();
                    highest = highest.max(s.highest_rung);
                    tags.insert(tag_of(s));
                }
            }
            row.sessions = tags.into_iter().collect();
            row.claimed.push(Claim::InvalidSignature { rungs, highest });
        }
        Coverage::Signed { .. } => {
            let mut tags = BTreeSet::new();
            let mut claimed_files = BTreeSet::new();
            let mut attested: BTreeMap<
                (Option<CommandClassKey>, SourceKey, Option<OriginKey>),
                usize,
            > = BTreeMap::new();
            let mut completion_claims = 0;
            let mut not_local: BTreeMap<u8, usize> = BTreeMap::new();
            for r in &valid {
                let st = r.statement.as_ref().expect("valid implies a statement");
                let id = r.receipt_id.clone().unwrap_or_default();
                for c in st.predicate.commits.iter().filter(|c| c.sha == commit.sha) {
                    claimed_files.extend(c.files.iter().cloned());
                }
                for s in sessions_for(st, &commit.sha) {
                    tags.insert(tag_of(s));
                    if risk_seen.insert((id.clone(), s.session_ref.clone())) {
                        risk.sessions += 1;
                        if !s.risk.known {
                            risk.unknown_sessions += 1;
                        }
                        risk.demoted_claims += u64::from(s.risk.demoted_claims);
                        risk.loop_alerts += u64::from(s.risk.loop_alerts);
                        risk.unresolved_loops += u64::from(s.risk.unresolved_loops);
                        risk.recoveries += u64::from(s.risk.recoveries);
                        risk.governor_halts += u64::from(s.risk.governor_halts);
                    }
                    for e in &s.rungs {
                        match (e.kind, e.verified_locally) {
                            (OutcomeKind::DoneClaimed, _) => completion_claims += 1,
                            (OutcomeKind::TestOrBuildPassed, true) => {
                                *attested
                                    .entry((
                                        e.command_class.map(CommandClassKey::of),
                                        SourceKey::of(e.source),
                                        e.test_origin.map(OriginKey::of),
                                    ))
                                    .or_default() += 1;
                            }
                            // Rungs 2, 4 and 5 are re-derived on the runner below; the local
                            // copy adds nothing a reader can trust more.
                            (_, true) => {}
                            (_, false) => *not_local.entry(e.rung).or_default() += 1,
                        }
                    }
                }
            }
            row.sessions = tags.into_iter().collect();

            if is_head {
                if let CiState::Green { passed, total } = ci {
                    row.verified.push(VerifiedFact::Ci { passed, total });
                }
            }
            row.verified.push(VerifiedFact::CommitInPr);
            if !claimed_files.is_empty() {
                let matched = claimed_files
                    .iter()
                    .filter(|f| commit.files.contains(*f))
                    .count();
                if matched > 0 {
                    row.verified.push(VerifiedFact::FilesMatch {
                        matched,
                        claimed: claimed_files.len(),
                    });
                }
                let missing = claimed_files.len() - matched;
                if missing > 0 {
                    row.claimed.push(Claim::FilesNotInDiff(missing));
                }
            }
            if !is_head && ci.is_green() {
                row.verified.push(VerifiedFact::HeadCiCoversFinalTree);
            }

            row.attested = attested
                .into_iter()
                .map(
                    |((command_class, source, test_origin), count)| AttestedRun {
                        command_class,
                        source,
                        test_origin,
                        count,
                    },
                )
                .collect();
            if completion_claims > 0 {
                row.claimed.push(Claim::CompletionClaims(completion_claims));
            }
            for (rung, count) in not_local.into_iter().rev() {
                row.claimed.push(Claim::NotVerifiedLocally { rung, count });
            }
        }
    }
    row
}

/// Sessions a receipt ties to `sha` through its anchored `commits[]` join.
fn sessions_for<'a>(
    st: &'a WorkReceiptStatement,
    sha: &str,
) -> impl Iterator<Item = &'a agentworth_schema::work_receipt::WorkSession> {
    let refs: BTreeSet<&str> = st
        .predicate
        .commits
        .iter()
        .filter(|c| c.sha == sha)
        .flat_map(|c| c.sessions.iter().map(String::as_str))
        .collect();
    st.predicate
        .sessions
        .iter()
        .filter(move |s| refs.contains(s.session_ref.as_str()))
}

fn tag_of(s: &agentworth_schema::work_receipt::WorkSession) -> SessionTag {
    SessionTag {
        adapter: AdapterLabel::of(&s.adapter),
        // Validated as 16 lowercase hex chars by the schema.
        short_ref: s.session_ref.chars().take(8).collect(),
    }
}

/// Spec §3.2.
fn conclude(report: &VerifyReport, fail_on: &[FailOn]) -> (Conclusion, Vec<FailOn>) {
    let mut failed = Vec::new();
    for f in fail_on {
        let tripped = match *f {
            FailOn::InvalidSignature => report.any_invalid(),
            FailOn::BelowRung(n) => {
                let best = report.highest_verified().max(report.highest_attested());
                best.is_none_or(|b| b < n)
            }
            FailOn::Unattributed => report
                .commits
                .iter()
                .any(|c| matches!(c.coverage, Coverage::None)),
        };
        if tripped {
            failed.push(*f);
        }
    }
    if !failed.is_empty() {
        return (Conclusion::Failure, failed);
    }
    let fully_linked = !report.commits.is_empty()
        && report.skipped_commits == 0
        && report
            .commits
            .iter()
            .all(|c| c.coverage == Coverage::Signed { linked: true });
    if fully_linked && report.ci.is_green() && !report.any_invalid() {
        (Conclusion::Success, failed)
    } else {
        (Conclusion::Neutral, failed)
    }
}

// ---------------------------------------------------------------------------------------------
// Git layer
// ---------------------------------------------------------------------------------------------

fn git_raw(repo: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "core.quotePath=false"])
        .args(args)
        .output()
        .context("failed to run git")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out.stdout)
}

fn git_text(repo: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8_lossy(&git_raw(repo, args)?)
        .trim()
        .to_string())
}

/// Resolve a revision to a full commit SHA. Rejects anything that looks like an option.
pub fn resolve_commit(repo: &Path, rev: &str) -> Result<String> {
    if rev.is_empty() || rev.starts_with('-') || rev.contains(char::is_whitespace) {
        bail!("not a revision: {rev:?}");
    }
    let spec = format!("{rev}^{{commit}}");
    git_text(repo, &["rev-parse", "--verify", "--quiet", &spec])
        .with_context(|| format!("{rev} is not a commit in this repository"))
}

/// PR commits `base..head`, newest first, each with the paths in its own diff.
pub fn collect_pr_commits(repo: &Path, base: &str, head: &str) -> Result<Vec<PrCommit>> {
    let range = format!("{base}..{head}");
    let limit = format!("--max-count={}", MAX_PR_COMMITS + 1);
    let log = git_raw(
        repo,
        &["log", "-z", &limit, "--format=%H%x1f%P%x1f%s", &range],
    )?;
    let mut out = Vec::new();
    for rec in log.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let rec = String::from_utf8_lossy(rec);
        let mut parts = rec.trim_start_matches('\n').splitn(3, '\u{1f}');
        let sha = parts.next().unwrap_or("").to_string();
        let parents: Vec<&str> = parts.next().unwrap_or("").split_whitespace().collect();
        let subject = parts.next().unwrap_or("").to_string();
        let names = match parents.first() {
            Some(first) if parents.len() > 1 => git_raw(
                repo,
                &["diff", "-z", "--name-only", "--no-renames", first, &sha],
            )?,
            _ => git_raw(
                repo,
                &[
                    "diff-tree",
                    "-z",
                    "-r",
                    "--root",
                    "--no-commit-id",
                    "--name-only",
                    "--no-renames",
                    &sha,
                ],
            )?,
        };
        let files = names
            .split(|b| *b == 0)
            .filter(|n| !n.is_empty())
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .collect();
        out.push(PrCommit {
            sha,
            subject,
            files,
        });
    }
    Ok(out)
}

/// Receipts for the PR: notes on PR commits (transport A) and committed receipt files in the
/// head tree (transport B). A missing notes ref or receipt directory is not an error.
pub fn collect_receipts(
    repo: &Path,
    head: &str,
    pr_shas: &BTreeSet<String>,
) -> Result<Vec<ReceiptSource>> {
    let mut out = Vec::new();
    let listing =
        git_text(repo, &["notes", &format!("--ref={NOTES_REF}"), "list"]).unwrap_or_default();
    for line in listing.lines() {
        let mut it = line.split_whitespace();
        let (Some(blob), Some(obj)) = (it.next(), it.next()) else {
            continue;
        };
        if !pr_shas.contains(obj) {
            continue;
        }
        if let Ok(bytes) = git_raw(repo, &["cat-file", "blob", blob]) {
            out.push(ReceiptSource {
                transport: Transport::Note,
                bytes,
            });
        }
    }
    let tree = git_raw(
        repo,
        &[
            "ls-tree",
            "-r",
            "-z",
            "--name-only",
            head,
            "--",
            RECEIPT_DIR,
        ],
    )
    .unwrap_or_default();
    for name in tree.split(|b| *b == 0).filter(|n| !n.is_empty()) {
        let name = String::from_utf8_lossy(name);
        if !name.ends_with(".dsse.json") {
            continue;
        }
        let spec = format!("{head}:{name}");
        if let Ok(bytes) = git_raw(repo, &["cat-file", "blob", &spec]) {
            out.push(ReceiptSource {
                transport: Transport::File,
                bytes,
            });
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Command
// ---------------------------------------------------------------------------------------------

/// Output format of `archie repo receipt verify`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VerifyFormat {
    /// The check summary as markdown (also what goes to `$GITHUB_STEP_SUMMARY`).
    #[default]
    Markdown,
    /// The `POST /repos/{o}/{r}/check-runs` body.
    CheckJson,
}

/// Arguments of `archie repo receipt verify --base SHA --head SHA [--ci ci.json]
/// [--keys keys.json] [--receipt FILE]... [--fail-on LIST] [--format md|check-json]`.
#[derive(Debug, Clone, Default)]
pub struct VerifyArgs {
    pub repo: PathBuf,
    pub base: String,
    pub head: String,
    pub ci: Option<PathBuf>,
    pub keys: Option<PathBuf>,
    pub receipts: Vec<PathBuf>,
    pub fail_on: Vec<FailOn>,
    pub format: VerifyFormat,
}

/// Collect from git and files, verify, and return the report. Offline.
pub fn verify_repo(args: &VerifyArgs) -> Result<VerifyReport> {
    let base = resolve_commit(&args.repo, &args.base)?;
    let head = resolve_commit(&args.repo, &args.head)?;
    let commits = collect_pr_commits(&args.repo, &base, &head)?;
    let pr_shas: BTreeSet<String> = commits.iter().map(|c| c.sha.clone()).collect();
    let mut receipts = collect_receipts(&args.repo, &head, &pr_shas)?;
    for path in &args.receipts {
        let bytes =
            std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
        receipts.push(ReceiptSource {
            transport: Transport::Explicit,
            bytes,
        });
    }
    let ci = match &args.ci {
        Some(p) => Some(CiInput::from_json(
            &std::fs::read(p).with_context(|| format!("cannot read {}", p.display()))?,
        )?),
        None => None,
    };
    let keys = match &args.keys {
        Some(p) => parse_keys_json(
            &std::fs::read(p).with_context(|| format!("cannot read {}", p.display()))?,
        )?,
        None => Vec::new(),
    };
    Ok(verify(&VerifyInput {
        base,
        head,
        commits,
        receipts,
        ci,
        keys,
        fail_on: args.fail_on.clone(),
    }))
}

/// Run the verifier and render it in the requested format. The conclusion lives in the output,
/// never in the exit status: the Action step always exits 0 (spec §3.2).
pub fn run_receipt_verify_command(args: &VerifyArgs) -> Result<String> {
    let report = verify_repo(args)?;
    Ok(match args.format {
        VerifyFormat::Markdown => render_markdown(&report),
        VerifyFormat::CheckJson => render_check_json(&report),
    })
}

#[cfg(test)]
mod tests;
