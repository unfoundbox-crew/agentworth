//! Work Receipt v1: the signed, privacy-safe summary of what a laptop's index saw happen on a
//! range of commits (spec `docs/specs/evidence-check.md`, draft §2).
//!
//! A receipt is an [in-toto Statement v1] whose `predicateType` is
//! [`WORK_RECEIPT_PREDICATE_TYPE`]. Its subjects are the covered commits (`gitCommit` digests);
//! its predicate carries only derived integers, enums, timestamps rounded to the minute, opaque
//! session refs, and repo-relative paths. No floats anywhere: confidence is basis points and
//! money is micro-USD, which is what lets [`canonical_json`] stay RFC 8785 (JCS) compatible
//! without a float serializer.
//!
//! This module owns the *format*: types, validation, canonical bytes, `receipt_id`, the 64 KiB
//! size cap, and the session-ref derivation. Signing lives in `agentworth-attest` so the crypto
//! dependency never reaches the scanner or adapter crates.
//!
//! [in-toto Statement v1]: https://github.com/in-toto/attestation/blob/main/spec/v1/statement.md

use std::fmt;

use chrono::{DateTime, Timelike, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::event::OutcomeKind;

/// `_type` of every in-toto Statement v1.
pub const IN_TOTO_STATEMENT_V1: &str = "https://in-toto.io/Statement/v1";
/// `predicateType` of a Work Receipt v1.
pub const WORK_RECEIPT_PREDICATE_TYPE: &str = "https://agentworth.dev/work-receipt/v1";
/// DSSE `payloadType` for an in-toto Statement.
pub const IN_TOTO_PAYLOAD_TYPE: &str = "application/vnd.in-toto+json";
/// `predicate.receipt_version` this module writes and accepts.
pub const RECEIPT_VERSION: u32 = 1;
/// Upper bound on the canonical Statement bytes (spec §2.3).
pub const MAX_RECEIPT_BYTES: usize = 64 * 1024;
/// `subject[].name` for a commit subject.
pub const COMMIT_SUBJECT_NAME: &str = "commit";
/// Domain separator for [`session_ref`] (spec §2.5).
pub const SESSION_REF_DOMAIN: &str = "agentworth-session-ref/v1";
/// Length of a session ref in hex characters.
pub const SESSION_REF_LEN: usize = 16;
/// Largest legal `confidence_bp` (100.00%).
pub const MAX_CONFIDENCE_BP: u16 = 10_000;

// ---------------------------------------------------------------------------------------------
// Statement
// ---------------------------------------------------------------------------------------------

/// The in-toto Statement v1 a Work Receipt is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkReceiptStatement {
    #[serde(rename = "_type")]
    pub statement_type: String,
    pub subject: Vec<Subject>,
    #[serde(rename = "predicateType")]
    pub predicate_type: String,
    pub predicate: WorkReceiptPredicate,
}

/// One covered commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    pub name: String,
    pub digest: SubjectDigest,
}

/// The in-toto `DigestSet` for a commit. Only `gitCommit` is used by v1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectDigest {
    #[serde(rename = "gitCommit")]
    pub git_commit: String,
}

impl Subject {
    /// A `{"name":"commit","digest":{"gitCommit":sha}}` subject.
    pub fn commit(sha: impl Into<String>) -> Self {
        Self {
            name: COMMIT_SUBJECT_NAME.to_string(),
            digest: SubjectDigest {
                git_commit: sha.into(),
            },
        }
    }
}

/// `predicate` of a Work Receipt v1 (spec §2.1, field sources §2.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkReceiptPredicate {
    pub receipt_version: u32,
    pub generator: Generator,
    /// Who signed. Not in the spec's §2.1 example: without the public key inside the signed
    /// payload a verifier cannot check a signature from a key GitHub does not know, so
    /// "Signed, key not linked" (§2.4) would be unreachable. It is also the `signer_pubkey`
    /// every [`session_ref`] in this receipt was derived from.
    pub signer: Signer,
    pub created_at: ReceiptTimestamp,
    pub range: CommitRange,
    pub attribution: Attribution,
    pub sessions: Vec<WorkSession>,
    pub commits: Vec<CommitAttribution>,
    pub index: IndexInfo,
    pub redaction: RedactionInfo,
    /// True when the size cap forced `commits[].files` to be dropped (spec §2.3).
    #[serde(default)]
    pub truncated: bool,
}

/// The tool that built the receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generator {
    pub name: String,
    pub version: String,
}

/// The signing key, as an OpenSSH `ssh-ed25519 <base64>` public key line with no comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signer {
    pub public_key: String,
}

/// Which commits the receipt covers and which rule picked them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitRange {
    pub base: Option<String>,
    pub head: String,
    pub commits: u32,
    pub rule: RangeRule,
}

/// How the range was resolved, mirroring `resolve_range` in `apps/cli/src/commands/suspect.rs`.
///
/// An enum rather than `repo suspect`'s prose description on purpose: that description embeds
/// the upstream and branch names, which spec §2.5 forbids in a receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RangeRule {
    /// `<upstream>..HEAD`, the branch's own upstream (the pre-push default).
    #[serde(rename = "upstream..HEAD")]
    Upstream,
    /// `origin/main..HEAD` or `origin/master..HEAD`.
    #[serde(rename = "default-branch..HEAD")]
    DefaultBranch,
    /// An explicit `--base`.
    #[serde(rename = "base..HEAD")]
    ExplicitBase,
    /// `--since <time>`.
    #[serde(rename = "since")]
    Since,
    /// No upstream and no default branch: the last N commits.
    #[serde(rename = "last-n")]
    LastN,
}

/// Which session-to-commit join produced `commits[]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attribution {
    /// e.g. `anchored-blame/v1`.
    pub rule: String,
    pub window_hours: u32,
    /// Blame rows that could not be anchored to this repository (counted, never used).
    pub unanchored_rows: u32,
}

/// One agent session, projected to what may leave the machine (spec §2.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkSession {
    /// [`session_ref`] of the raw session id. Never the raw id.
    #[serde(rename = "ref")]
    pub session_ref: String,
    pub adapter: String,
    pub models: Vec<String>,
    pub started_at: Option<ReceiptTimestamp>,
    pub ended_at: Option<ReceiptTimestamp>,
    pub tokens_total: u64,
    /// API-equivalent spend. `None` (serialized `null`) unless the user opted in
    /// (`--include-spend`, decision row 4 COS default: tokens only).
    pub spend_micro_usd: Option<u64>,
    /// Highest `rungs[].rung`, or `null` when the session has no outcome evidence.
    pub highest_rung: Option<u8>,
    pub rungs: Vec<RungEvidence>,
    pub risk: SessionRisk,
}

/// One piece of outcome evidence on the 1..=5 ladder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RungEvidence {
    /// Must equal [`evidence_rung`] of `kind`.
    pub rung: u8,
    pub kind: OutcomeKind,
    pub source: EvidenceSource,
    pub command_class: Option<CommandClass>,
    pub confidence_bp: u16,
    /// `null` for non-test evidence; `unknown` for a test pass without provenance.
    pub test_origin: Option<ReceiptTestOrigin>,
    pub event_seq: u64,
    pub verified_locally: bool,
}

/// What kind of observation produced a rung. A closed vocabulary so no free text can ride in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    /// A shell command observed exiting 0.
    #[serde(rename = "shell_exit_0")]
    ShellExit0,
    /// A tool result reporting success.
    ToolResult,
    /// A file write or edit observed.
    FileChange,
    /// A git commit observed.
    GitCommit,
    /// The assistant said so. Never evidence on its own.
    AssistantClaim,
    /// Anything the projection cannot name more precisely.
    #[serde(other)]
    Other,
}

/// Allowlisted command classes (spec §2.5). Anything else is [`CommandClass::OtherTestCommand`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandClass {
    #[serde(rename = "cargo test")]
    CargoTest,
    #[serde(rename = "cargo build")]
    CargoBuild,
    #[serde(rename = "npm test")]
    NpmTest,
    #[serde(rename = "pnpm test")]
    PnpmTest,
    #[serde(rename = "pytest")]
    Pytest,
    #[serde(rename = "go test")]
    GoTest,
    #[serde(rename = "make test")]
    MakeTest,
    #[serde(rename = "gh pr create")]
    GhPrCreate,
    /// Not in the allowlist. Also what an older reader maps a newer allowlist entry to.
    #[serde(rename = "other_test_command", other)]
    OtherTestCommand,
}

impl CommandClass {
    /// Classify a raw command line by its leading words. The raw line never leaves this
    /// function; only the class goes in a receipt.
    pub fn classify(command: &str) -> Self {
        let words: Vec<&str> = command.split_whitespace().take(3).collect();
        match words.as_slice() {
            ["cargo", "test", ..] => Self::CargoTest,
            ["cargo", "build", ..] => Self::CargoBuild,
            ["npm", "test", ..] | ["npm", "run", "test", ..] => Self::NpmTest,
            ["pnpm", "test", ..] | ["pnpm", "run", "test", ..] => Self::PnpmTest,
            ["pytest", ..] => Self::Pytest,
            ["python" | "python3", "-m", "pytest"] => Self::Pytest,
            ["go", "test", ..] => Self::GoTest,
            ["make", "test", ..] => Self::MakeTest,
            ["gh", "pr", "create", ..] => Self::GhPrCreate,
            _ => Self::OtherTestCommand,
        }
    }
}

/// Test provenance as it appears in a receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptTestOrigin {
    AgentChosen,
    HeldOut,
    /// A pass observed without authorship information (fails closed at the held-out gate).
    Unknown,
}

impl From<Option<crate::event::TestOrigin>> for ReceiptTestOrigin {
    fn from(origin: Option<crate::event::TestOrigin>) -> Self {
        match origin {
            Some(crate::event::TestOrigin::AgentChosen) => Self::AgentChosen,
            Some(crate::event::TestOrigin::HeldOut) => Self::HeldOut,
            None => Self::Unknown,
        }
    }
}

/// Integer risk counters from `session_risk` and `governor_events`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRisk {
    pub demoted_claims: u32,
    pub loop_alerts: u32,
    pub unresolved_loops: u32,
    pub recoveries: u32,
    pub governor_halts: u32,
    /// False when the index had no risk row for the session (unknown, never clean).
    pub known: bool,
}

/// One covered commit and the sessions the anchored blame join tied to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitAttribution {
    pub sha: String,
    /// Session refs; each must name a `sessions[].ref` of this receipt.
    pub sessions: Vec<String>,
    /// Repo-relative paths, only those in this commit's own diff. First thing dropped by the cap.
    pub files: Vec<String>,
    pub anchor: Option<CommitAnchor>,
}

/// Why the attribution is trusted (`crates/storage/src/anchoring.rs`). Only anchored rows are
/// evidence, so only the two accepting variants exist here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitAnchor {
    Absolute,
    RelativeToSessionRepo,
}

/// Freshness of the local index the receipt was built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexInfo {
    pub last_session_at: Option<ReceiptTimestamp>,
    pub parser_version_min: u32,
}

/// Which redaction policy was applied and which fields it removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactionInfo {
    pub policy: String,
    pub fields_dropped: Vec<String>,
}

/// Rung number of an outcome kind. Must agree with `agentworth_outcomes::outcome_rank`
/// (pinned by `work_receipt_rung_tests` in `crates/outcomes`; schema cannot depend on it).
pub fn evidence_rung(kind: OutcomeKind) -> u8 {
    match kind {
        OutcomeKind::DoneClaimed => 1,
        OutcomeKind::ArtifactChanged => 2,
        OutcomeKind::TestOrBuildPassed => 3,
        OutcomeKind::CommitObserved => 4,
        OutcomeKind::CiOrDeploymentVerified => 5,
    }
}

// ---------------------------------------------------------------------------------------------
// Timestamps
// ---------------------------------------------------------------------------------------------

/// A UTC timestamp floored to the minute, written `YYYY-MM-DDTHH:MM:00Z` (spec §2.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReceiptTimestamp(DateTime<Utc>);

impl ReceiptTimestamp {
    /// Floor `t` to the minute.
    pub fn from_datetime(t: DateTime<Utc>) -> Self {
        let floored = t
            .with_nanosecond(0)
            .and_then(|t| t.with_second(0))
            .unwrap_or(t);
        Self(floored)
    }

    pub fn as_datetime(&self) -> DateTime<Utc> {
        self.0
    }

    /// Parse the exact receipt form; anything with seconds, fractions or an offset is rejected.
    pub fn parse(s: &str) -> Result<Self, WorkReceiptError> {
        let bad = || WorkReceiptError::BadTimestamp(s.to_string());
        if s.len() != 20 || s.as_bytes()[10] != b'T' || !s.ends_with(":00Z") {
            return Err(bad());
        }
        let t = DateTime::parse_from_rfc3339(s).map_err(|_| bad())?;
        Ok(Self(t.with_timezone(&Utc)))
    }
}

impl fmt::Display for ReceiptTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.format("%Y-%m-%dT%H:%M:00Z"))
    }
}

impl Serialize for ReceiptTimestamp {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ReceiptTimestamp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------------------------
// Errors and validation
// ---------------------------------------------------------------------------------------------

/// Everything that can be wrong with a Work Receipt.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkReceiptError {
    #[error("floats are not allowed in a receipt (found {0})")]
    FloatInReceipt(String),
    #[error("receipt is not valid JSON for Work Receipt v1: {0}")]
    Json(String),
    #[error("_type must be {IN_TOTO_STATEMENT_V1}, got {0}")]
    WrongStatementType(String),
    #[error("predicateType must be {WORK_RECEIPT_PREDICATE_TYPE}, got {0}")]
    WrongPredicateType(String),
    #[error("unsupported receipt_version {0}")]
    UnsupportedVersion(u32),
    #[error("signer.public_key must be a bare `ssh-ed25519 <base64>` line with no comment")]
    BadSigner,
    #[error("subject {0} is not a commit subject with a lowercase hex gitCommit digest")]
    BadSubject(usize),
    #[error("duplicate subject {0}")]
    DuplicateSubject(String),
    #[error("commit {0} is not one of the receipt's subjects")]
    CommitNotInSubjects(String),
    #[error("session ref {0} is not {SESSION_REF_LEN} lowercase hex characters")]
    BadSessionRef(String),
    #[error("commit {sha} names session {session_ref} that the receipt does not carry")]
    UnknownSessionRef { sha: String, session_ref: String },
    #[error("rung {rung} does not match kind {kind:?}")]
    RungKindMismatch { rung: u8, kind: OutcomeKind },
    #[error("confidence_bp {0} exceeds {MAX_CONFIDENCE_BP}")]
    BadConfidence(u16),
    #[error("highest_rung of session {0} does not equal the max of its rungs")]
    BadHighestRung(String),
    #[error("path {0} is not repo-relative")]
    NonRelativePath(String),
    #[error("timestamp {0} is not YYYY-MM-DDTHH:MM:00Z")]
    BadTimestamp(String),
    #[error(
        "receipt is {size} bytes after dropping files[], over the {MAX_RECEIPT_BYTES}-byte cap"
    )]
    TooLarge { size: usize },
}

fn is_lower_hex(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// SHA-1 (40) or SHA-256 (64) object id, lowercase hex.
fn is_git_oid(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && is_lower_hex(s)
}

fn is_session_ref(s: &str) -> bool {
    s.len() == SESSION_REF_LEN && is_lower_hex(s)
}

fn is_repo_relative(p: &str) -> bool {
    let rejected = p.is_empty()
        || p.starts_with('/')
        || p.starts_with('~')
        || p.contains('\\')
        || p.split('/').any(|seg| seg == ".." || seg.is_empty())
        // A Windows drive letter (`C:`).
        || p.as_bytes().get(1) == Some(&b':');
    !rejected
}

impl WorkReceiptStatement {
    /// Wrap a predicate and its subjects in a Statement v1.
    pub fn new(subject: Vec<Subject>, predicate: WorkReceiptPredicate) -> Self {
        Self {
            statement_type: IN_TOTO_STATEMENT_V1.to_string(),
            subject,
            predicate_type: WORK_RECEIPT_PREDICATE_TYPE.to_string(),
            predicate,
        }
    }

    /// Structural checks every reader runs before trusting a single field.
    pub fn validate(&self) -> Result<(), WorkReceiptError> {
        if self.statement_type != IN_TOTO_STATEMENT_V1 {
            return Err(WorkReceiptError::WrongStatementType(
                self.statement_type.clone(),
            ));
        }
        if self.predicate_type != WORK_RECEIPT_PREDICATE_TYPE {
            return Err(WorkReceiptError::WrongPredicateType(
                self.predicate_type.clone(),
            ));
        }
        let p = &self.predicate;
        if p.receipt_version != RECEIPT_VERSION {
            return Err(WorkReceiptError::UnsupportedVersion(p.receipt_version));
        }
        let key = &p.signer.public_key;
        let body = key.strip_prefix("ssh-ed25519 ").unwrap_or("");
        if body.is_empty() || body.contains(char::is_whitespace) {
            return Err(WorkReceiptError::BadSigner);
        }
        let mut subjects = std::collections::BTreeSet::new();
        for (i, s) in self.subject.iter().enumerate() {
            if s.name != COMMIT_SUBJECT_NAME || !is_git_oid(&s.digest.git_commit) {
                return Err(WorkReceiptError::BadSubject(i));
            }
            if !subjects.insert(s.digest.git_commit.as_str()) {
                return Err(WorkReceiptError::DuplicateSubject(
                    s.digest.git_commit.clone(),
                ));
            }
        }
        let mut refs = std::collections::BTreeSet::new();
        for session in &p.sessions {
            if !is_session_ref(&session.session_ref) {
                return Err(WorkReceiptError::BadSessionRef(session.session_ref.clone()));
            }
            refs.insert(session.session_ref.as_str());
            for r in &session.rungs {
                if r.rung != evidence_rung(r.kind) {
                    return Err(WorkReceiptError::RungKindMismatch {
                        rung: r.rung,
                        kind: r.kind,
                    });
                }
                if r.confidence_bp > MAX_CONFIDENCE_BP {
                    return Err(WorkReceiptError::BadConfidence(r.confidence_bp));
                }
            }
            if session.highest_rung != session.rungs.iter().map(|r| r.rung).max() {
                return Err(WorkReceiptError::BadHighestRung(
                    session.session_ref.clone(),
                ));
            }
        }
        for c in &p.commits {
            if !subjects.contains(c.sha.as_str()) {
                return Err(WorkReceiptError::CommitNotInSubjects(c.sha.clone()));
            }
            for r in &c.sessions {
                if !refs.contains(r.as_str()) {
                    return Err(WorkReceiptError::UnknownSessionRef {
                        sha: c.sha.clone(),
                        session_ref: r.clone(),
                    });
                }
            }
            if let Some(bad) = c.files.iter().find(|f| !is_repo_relative(f)) {
                return Err(WorkReceiptError::NonRelativePath(bad.clone()));
            }
        }
        Ok(())
    }

    /// Whether `sha` is one of this receipt's subjects.
    pub fn covers_commit(&self, sha: &str) -> bool {
        self.subject.iter().any(|s| s.digest.git_commit == sha)
    }

    /// Canonical bytes (§2.3): sorted keys, no whitespace, UTF-8, integers only.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, WorkReceiptError> {
        let value =
            serde_json::to_value(self).map_err(|e| WorkReceiptError::Json(e.to_string()))?;
        canonical_json(&value)
    }

    /// `hex(sha256(canonical bytes))`.
    pub fn receipt_id(&self) -> Result<String, WorkReceiptError> {
        Ok(receipt_id_of(&self.canonical_bytes()?))
    }

    /// Enforce the 64 KiB cap: when over it, drop every `commits[].files` and set
    /// `truncated: true`. Returns the canonical bytes that fit. Errors if even that is too
    /// large; nothing else is dropped silently.
    pub fn fit_to_size_cap(&mut self) -> Result<Vec<u8>, WorkReceiptError> {
        let bytes = self.canonical_bytes()?;
        if bytes.len() <= MAX_RECEIPT_BYTES {
            return Ok(bytes);
        }
        for c in &mut self.predicate.commits {
            c.files.clear();
        }
        self.predicate.truncated = true;
        let bytes = self.canonical_bytes()?;
        if bytes.len() > MAX_RECEIPT_BYTES {
            return Err(WorkReceiptError::TooLarge { size: bytes.len() });
        }
        Ok(bytes)
    }

    /// Parse and validate payload bytes (e.g. a verified DSSE payload).
    pub fn from_payload(bytes: &[u8]) -> Result<Self, WorkReceiptError> {
        let statement: Self =
            serde_json::from_slice(bytes).map_err(|e| WorkReceiptError::Json(e.to_string()))?;
        statement.validate()?;
        Ok(statement)
    }
}

/// `hex(sha256(bytes))`, the receipt id of already-canonical bytes.
pub fn receipt_id_of(canonical: &[u8]) -> String {
    hex::encode(Sha256::digest(canonical))
}

/// Session ref (spec §2.5):
/// `hex(sha256("agentworth-session-ref/v1" || signer_pubkey || session_id))[:16]`.
///
/// `signer_pubkey` is the raw 32-byte ed25519 public key (not the OpenSSH blob); `session_id`
/// is the raw id's UTF-8 bytes. The domain string and the key are fixed-length, so the plain
/// concatenation is unambiguous.
pub fn session_ref(signer_pubkey: &[u8; 32], session_id: &str) -> String {
    let mut h = Sha256::new();
    h.update(SESSION_REF_DOMAIN.as_bytes());
    h.update(signer_pubkey);
    h.update(session_id.as_bytes());
    let digest = hex::encode(h.finalize());
    // hex is ASCII, so the first 16 chars are the first 16 bytes; char-safe anyway.
    digest.chars().take(SESSION_REF_LEN).collect()
}

// ---------------------------------------------------------------------------------------------
// Canonical JSON
// ---------------------------------------------------------------------------------------------

/// Serialize `value` canonically: object keys sorted by UTF-16 code units (RFC 8785 §3.2.3; the
/// same as byte order for the ASCII keys a receipt uses), no insignificant whitespace, strings
/// escaped per RFC 8785 §3.2.2.2, integers in plain decimal. Floats are rejected rather than
/// formatted, which is what keeps this JCS-compatible without a float serializer.
///
/// Sorting is done here, not by `serde_json::Map`, so the output cannot change if some crate
/// in the build turns on serde_json's `preserve_order` feature.
pub fn canonical_json(value: &serde_json::Value) -> Result<Vec<u8>, WorkReceiptError> {
    let mut out = Vec::new();
    write_canonical(value, &mut out)?;
    Ok(out)
}

fn write_canonical(value: &serde_json::Value, out: &mut Vec<u8>) -> Result<(), WorkReceiptError> {
    use serde_json::Value;
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                out.extend_from_slice(u.to_string().as_bytes());
            } else if let Some(i) = n.as_i64() {
                out.extend_from_slice(i.to_string().as_bytes());
            } else {
                return Err(WorkReceiptError::FloatInReceipt(n.to_string()));
            }
        }
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_canonical(item, out)?;
            }
            out.push(b']');
        }
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
            out.push(b'{');
            for (i, (k, v)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_string(k, out);
                out.push(b':');
                write_canonical(v, out)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}

fn write_string(s: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for ch in s.chars() {
        match ch {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\u{08}' => out.extend_from_slice(b"\\b"),
            '\u{09}' => out.extend_from_slice(b"\\t"),
            '\u{0A}' => out.extend_from_slice(b"\\n"),
            '\u{0C}' => out.extend_from_slice(b"\\f"),
            '\u{0D}' => out.extend_from_slice(b"\\r"),
            c if (c as u32) < 0x20 => {
                out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes());
            }
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(b'"');
}

/// Deterministic EXAMPLE DATA receipts for golden tests here and in `agentworth-attest`.
///
/// Built from fake identities only: the signer is the RFC 8032 §7.1 TEST 1 public key, session
/// ids are made-up UUIDs, and every SHA is synthetic.
pub mod example {
    use super::*;

    /// RFC 8032 §7.1 TEST 1 public key (its secret is public too: never a real signer).
    pub const SIGNER_PUBKEY: [u8; 32] = [
        0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07,
        0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07,
        0x51, 0x1a,
    ];
    /// [`SIGNER_PUBKEY`] as an OpenSSH line (cross-checked in `agentworth-attest`).
    pub const SIGNER_OPENSSH: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINdamAGCsQq31Uv+08lkBzoO4XLz2qYjJa8CGmj3B1Ea";
    /// Fake raw session ids; only their refs appear in a receipt.
    pub const SESSION_IDS: [&str; 2] = [
        "11111111-2222-4333-8444-555555555555",
        "66666666-7777-4888-9999-aaaaaaaaaaaa",
    ];
    pub const HEAD: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f9001122334";
    pub const MIDDLE: &str = "9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d1e0f";
    pub const FIRST: &str = "5d4c3b2a1f0e9d8c7b6a59483726150f0e0d0c0b";
    pub const BASE: &str = "0123456789abcdef0123456789abcdef01234567";

    fn ts(s: &str) -> ReceiptTimestamp {
        ReceiptTimestamp::parse(s).expect("fixture timestamp")
    }

    /// The main fixture: the spec §2.1 example with full SHAs and a second (Codex) session.
    pub fn statement() -> WorkReceiptStatement {
        let claude = session_ref(&SIGNER_PUBKEY, SESSION_IDS[0]);
        let codex = session_ref(&SIGNER_PUBKEY, SESSION_IDS[1]);
        let predicate = WorkReceiptPredicate {
            receipt_version: RECEIPT_VERSION,
            generator: Generator {
                name: "archie".into(),
                version: "0.1.30".into(),
            },
            signer: Signer {
                public_key: SIGNER_OPENSSH.into(),
            },
            created_at: ts("2026-10-20T09:41:00Z"),
            range: CommitRange {
                base: Some(BASE.into()),
                head: HEAD.into(),
                commits: 3,
                rule: RangeRule::Upstream,
            },
            attribution: Attribution {
                rule: "anchored-blame/v1".into(),
                window_hours: 24,
                unanchored_rows: 0,
            },
            sessions: vec![
                WorkSession {
                    session_ref: claude.clone(),
                    adapter: "claude_code".into(),
                    models: vec!["claude-opus-4-5".into()],
                    started_at: Some(ts("2026-10-20T08:02:00Z")),
                    ended_at: Some(ts("2026-10-20T09:30:00Z")),
                    tokens_total: 1_840_000,
                    spend_micro_usd: None,
                    highest_rung: Some(3),
                    rungs: vec![
                        RungEvidence {
                            rung: 3,
                            kind: OutcomeKind::TestOrBuildPassed,
                            source: EvidenceSource::ShellExit0,
                            command_class: Some(CommandClass::CargoTest),
                            confidence_bp: 8500,
                            test_origin: Some(ReceiptTestOrigin::Unknown),
                            event_seq: 412,
                            verified_locally: true,
                        },
                        RungEvidence {
                            rung: 1,
                            kind: OutcomeKind::DoneClaimed,
                            source: EvidenceSource::AssistantClaim,
                            command_class: None,
                            confidence_bp: 3500,
                            test_origin: None,
                            event_seq: 455,
                            verified_locally: false,
                        },
                    ],
                    risk: SessionRisk {
                        demoted_claims: 0,
                        loop_alerts: 1,
                        unresolved_loops: 0,
                        recoveries: 2,
                        governor_halts: 0,
                        known: true,
                    },
                },
                WorkSession {
                    session_ref: codex.clone(),
                    adapter: "codex".into(),
                    models: vec!["gpt-5-codex".into()],
                    started_at: Some(ts("2026-10-20T07:15:00Z")),
                    ended_at: Some(ts("2026-10-20T07:58:00Z")),
                    tokens_total: 412_000,
                    spend_micro_usd: None,
                    highest_rung: Some(2),
                    rungs: vec![RungEvidence {
                        rung: 2,
                        kind: OutcomeKind::ArtifactChanged,
                        source: EvidenceSource::FileChange,
                        command_class: None,
                        confidence_bp: 9000,
                        test_origin: None,
                        event_seq: 88,
                        verified_locally: true,
                    }],
                    risk: SessionRisk {
                        known: false,
                        ..SessionRisk::default()
                    },
                },
            ],
            commits: vec![
                CommitAttribution {
                    sha: HEAD.into(),
                    sessions: vec![claude],
                    files: vec!["crates/core/src/turns.rs".into()],
                    anchor: Some(CommitAnchor::Absolute),
                },
                CommitAttribution {
                    sha: MIDDLE.into(),
                    sessions: vec![codex],
                    files: vec![
                        "crates/schema/src/work_receipt.rs".into(),
                        "docs/schemas/work-receipt-v1.json".into(),
                    ],
                    anchor: Some(CommitAnchor::RelativeToSessionRepo),
                },
                CommitAttribution {
                    sha: FIRST.into(),
                    sessions: vec![],
                    files: vec![],
                    anchor: None,
                },
            ],
            index: IndexInfo {
                last_session_at: Some(ts("2026-10-20T09:30:00Z")),
                parser_version_min: 7,
            },
            redaction: RedactionInfo {
                policy: "publication/v1".into(),
                fields_dropped: vec![
                    "source_path".into(),
                    "db_path".into(),
                    "prompt_preview".into(),
                    "commands".into(),
                    "quotes".into(),
                ],
            },
            truncated: false,
        };
        WorkReceiptStatement::new(
            vec![
                Subject::commit(HEAD),
                Subject::commit(MIDDLE),
                Subject::commit(FIRST),
            ],
            predicate,
        )
    }
}

#[cfg(test)]
#[path = "work_receipt_tests.rs"]
mod tests;
