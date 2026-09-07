//! Wire protocol between the home UI and the gateway in `archie serve`, protocol 2.
//! Rust mirrors `apps/home/src/protocol.ts` field for field -- change both together or
//! neither. `rename_all_fields` (the container-level attribute that renames every variant's
//! fields at once) is not applied to the tagged enums here on purpose: it did not rename
//! anything in practice against serde 1.0.229 in this workspace when combined with internal
//! tagging (`tag = "t"`) -- caught by the literal-JSON tests in `gateway.rs`, which is exactly
//! the case they exist for. Each struct variant gets its own `rename_all = "camelCase"`
//! instead, which is unambiguous and is what the tests verify.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Wire protocol version. Bump alongside `PROTOCOL` in `apps/home/src/protocol.ts`.
pub const PROTOCOL: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    Idle,
    Working,
    Blocked,
    Done,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    ChiefOfStaff,
    Senior,
    Associate,
    Executor,
    Controller,
    Guest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpaceKind {
    Office,
    Room,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageKind {
    Speech,
    Work,
    System,
}

/// Archie's evidence ladder, lowest rung first -- `agentworth_outcomes::outcome_rank`'s scale
/// (1..5) under its protocol name. `Rung::from_outcome_rank`/`rank` are the one place that
/// mapping is written down for this module; `agentworth_storage::rung_outcome_name` is the
/// same mapping under the storage crate's own (longer) `OutcomeKind` names -- both must move
/// together if the ladder ever grows a rung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rung {
    Said,
    Artifact,
    Test,
    Commit,
    Ci,
}

impl Rung {
    /// `agentworth_outcomes::outcome_rank`'s 1..5 scale -> a `Rung`. `None` for 0 (no
    /// evidence, "unflown" in `agentworth_storage::rung_outcome_name`) and anything outside
    /// the known range.
    pub fn from_outcome_rank(rank: u8) -> Option<Self> {
        match rank {
            1 => Some(Self::Said),
            2 => Some(Self::Artifact),
            3 => Some(Self::Test),
            4 => Some(Self::Commit),
            5 => Some(Self::Ci),
            _ => None,
        }
    }

    /// `agentworth_storage`'s snake_case `OutcomeKind` name -> a `Rung`, for reading back
    /// `sessions.primary_outcome` / `home_directions.done_rung` values.
    pub fn from_storage_name(name: &str) -> Option<Self> {
        match name {
            "done_claimed" => Some(Self::Said),
            "artifact_changed" => Some(Self::Artifact),
            "test_or_build_passed" => Some(Self::Test),
            "commit_observed" => Some(Self::Commit),
            "ci_or_deployment_verified" => Some(Self::Ci),
            _ => None,
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Self::Said => 1,
            Self::Artifact => 2,
            Self::Test => 3,
            Self::Commit => 4,
            Self::Ci => 5,
        }
    }

    pub fn as_wire_str(self) -> &'static str {
        match self {
            Self::Said => "said",
            Self::Artifact => "artifact",
            Self::Test => "test",
            Self::Commit => "commit",
            Self::Ci => "ci",
        }
    }

    pub fn parse_wire_str(s: &str) -> Option<Self> {
        match s {
            "said" => Some(Self::Said),
            "artifact" => Some(Self::Artifact),
            "test" => Some(Self::Test),
            "commit" => Some(Self::Commit),
            "ci" => Some(Self::Ci),
            _ => None,
        }
    }
}

/// Whether a steer interrupts the rider now or lands after its current step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SteerMode {
    Now,
    After,
}

/// A key press for `answer` -- the numbered choices a CLI permission prompt offers, `esc` to
/// cancel, or a plain yes/no. Wire strings match `herdr agent send-keys` verbatim (including the
/// bare digits, which is why the variants are spelled out and renamed rather than derived).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnswerKey {
    #[serde(rename = "1")]
    One,
    #[serde(rename = "2")]
    Two,
    #[serde(rename = "3")]
    Three,
    #[serde(rename = "esc")]
    Esc,
    #[serde(rename = "y")]
    Y,
    #[serde(rename = "n")]
    N,
}

impl AnswerKey {
    pub fn as_wire_str(self) -> &'static str {
        match self {
            Self::One => "1",
            Self::Two => "2",
            Self::Three => "3",
            Self::Esc => "esc",
            Self::Y => "y",
            Self::N => "n",
        }
    }

    /// Whether this key is the standing-permission ("don't ask again") choice -- widens what a
    /// pane will do without asking again, so the gateway logs it distinctly. TODO(archie policy):
    /// route this through `archie policy` once that surface exists; this lane only logs it.
    pub fn is_standing_permission_change(self) -> bool {
        matches!(self, Self::Two)
    }
}

/// Whether herdr, required to seat any rider, is usable from here. Distinguishes "not
/// installed" from "installed but never run" (no socket yet) -- see `home_cmd::herdr_reachable`
/// for the same check made at `archie home` startup; this is the wire-typed twin used by
/// `detect_env` for the deck's first-run screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HerdrStatus {
    Ok,
    Missing,
    NoSocket,
}

/// A harness this machine can seat a rider on -- `id` matches `herdr agent start --kind`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Harness {
    pub id: String,
    pub label: String,
    pub bin: String,
}

/// What the server already knows about where it runs, detected once at `archie home` startup
/// (see [`detect_env`]) and handed to every connecting client in `hello`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeEnv {
    pub cwd: String,
    pub repo: Option<String>,
    pub harnesses: Vec<Harness>,
    pub herdr: HerdrStatus,
    /// A new direction's starting budget, before the human edits it. The same number
    /// `archie policy init` writes into its starter `policy.toml` comments --
    /// `Storage::session_token_percentiles(None)`'s p90 across this machine's own indexed
    /// sessions (AGENTS.md's "dogfood before you propose": a fleet here has run primary
    /// sessions past 700M tokens, so a guessed cap is not safe). Falls back to a plain 5,000,000
    /// when this machine has no indexed sessions yet (`n == 0`) -- set by `gateway::spawn`, not
    /// [`detect_env`], since it needs `Storage`.
    pub budget_default_tokens: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectionState {
    Riding,
    Idle,
    Done,
    Waiting,
    Halted,
}

impl DirectionState {
    pub fn as_wire_str(self) -> &'static str {
        match self {
            Self::Riding => "riding",
            Self::Idle => "idle",
            Self::Done => "done",
            Self::Waiting => "waiting",
            Self::Halted => "halted",
        }
    }

    pub fn parse_wire_str(s: &str) -> Option<Self> {
        match s {
            "riding" => Some(Self::Riding),
            "idle" => Some(Self::Idle),
            "done" => Some(Self::Done),
            "waiting" => Some(Self::Waiting),
            "halted" => Some(Self::Halted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Exception {
    pub reason: String,
    pub since: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Direction {
    pub id: String,
    pub goal: String,
    pub area: String,
    pub done: Rung,
    pub reached: Option<Rung>,
    pub budget_tokens: i64,
    pub spent_tokens: i64,
    pub riders: Vec<String>,
    pub state: DirectionState,
    pub exception: Option<Exception>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stop {
    pub id: String,
    pub direction_id: String,
    pub from: String,
    pub rung: Rung,
    pub artifact_id: Option<String>,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Persona {
    pub id: String,
    pub role: Role,
    pub kind: String,
    pub agent_name: String,
    pub pane_id: String,
    pub workspace_id: String,
    pub cwd: String,
    pub presence: Presence,
    pub title: String,
    pub revision: u64,
}

impl Persona {
    /// What gets passed as `<TARGET>` to `herdr agent prompt`/`herdr agent read`. herdr's own
    /// agent name when herdr assigned one; the pane id is the fallback, since that always
    /// exists and always resolves (confirmed via `herdr agent list`, see the report this
    /// module shipped with).
    pub fn target_ref(&self) -> &str {
        &self.agent_name
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Space {
    pub id: String,
    pub kind: SpaceKind,
    pub label: String,
    pub members: Vec<String>,
    pub unread: u32,
    pub last_summary: Option<String>,
    pub last_activity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub space_id: String,
    pub from: String,
    pub kind: MessageKind,
    pub text: String,
    pub at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mentions: Option<Vec<String>>,
    /// Set on a `you` message so the deck can dedupe its own optimistic local echo against this
    /// module's own record of the same steer -- see `apps/home/src/protocol.ts`'s `Message.clientId`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
    Diff,
    File,
    Command,
    Link,
    Note,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub id: String,
    pub space_id: String,
    pub from: String,
    pub kind: ArtifactKind,
    pub title: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub body: Option<String>,
    pub at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "t")]
pub enum ServerFrame {
    #[serde(rename = "hello", rename_all = "camelCase")]
    Hello {
        protocol: u32,
        personas: Vec<Persona>,
        spaces: Vec<Space>,
        directions: Vec<Direction>,
        env: HomeEnv,
    },
    #[serde(rename = "direction")]
    Direction {
        direction: Direction,
    },
    #[serde(rename = "stop")]
    Stop {
        stop: Stop,
    },
    #[serde(rename = "presence", rename_all = "camelCase")]
    Presence {
        persona_id: String,
        presence: Presence,
        title: String,
        revision: u64,
    },
    /// A persona the gateway just discovered off `herdr agent list` -- e.g. a rider
    /// `start_rider` just seated. Existing clients otherwise only learn personas from `hello`.
    #[serde(rename = "persona")]
    Persona {
        persona: Persona,
    },
    #[serde(rename = "message")]
    Message {
        message: Message,
    },
    #[serde(rename = "artifact")]
    #[allow(dead_code)]
    Artifact {
        artifact: Artifact,
    },
    #[serde(rename = "space")]
    #[allow(dead_code)]
    Space {
        space: Space,
    },
    #[serde(rename = "backfill", rename_all = "camelCase")]
    Backfill {
        space_id: String,
        messages: Vec<Message>,
        artifacts: Vec<Artifact>,
    },
    #[serde(rename = "error")]
    Error {
        code: String,
        detail: String,
    },
}

/// The client-editable subset of `Direction` -- mirrors protocol.ts's
/// `Omit<Direction, 'reached' | 'spentTokens' | 'state' | 'exception' | 'createdAt' | 'updatedAt'>`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectionInput {
    pub id: String,
    pub goal: String,
    pub area: String,
    pub done: Rung,
    pub budget_tokens: i64,
    pub riders: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "t")]
pub enum ClientFrame {
    #[serde(rename = "open", rename_all = "camelCase")]
    Open {
        space_id: String,
    },
    #[serde(rename = "set_direction", rename_all = "camelCase")]
    SetDirection {
        direction: DirectionInput,
    },
    #[serde(rename = "steer", rename_all = "camelCase")]
    Steer {
        direction_id: String,
        text: String,
        mode: SteerMode,
        #[serde(default)]
        mentions: Vec<String>,
        #[serde(default)]
        client_id: Option<String>,
    },
    #[serde(rename = "prompt", rename_all = "camelCase")]
    Prompt {
        space_id: String,
        text: String,
        #[serde(default)]
        mentions: Vec<String>,
    },
    #[serde(rename = "seen", rename_all = "camelCase")]
    Seen {
        #[allow(dead_code)]
        space_id: String,
        #[allow(dead_code)]
        upto: String,
    },
    #[serde(rename = "fetch", rename_all = "camelCase")]
    Fetch {
        #[allow(dead_code)]
        artifact_id: String,
    },
    /// Answers a blocked pane's prompt from the alert plate. Refused unless `persona_id` is
    /// presently `blocked` -- see `dispatch_answer` in `gateway.rs`.
    #[serde(rename = "answer", rename_all = "camelCase")]
    Answer {
        #[allow(dead_code)]
        direction_id: String,
        persona_id: String,
        key: AnswerKey,
        #[serde(default)]
        #[allow(dead_code)]
        client_id: Option<String>,
    },
    /// Seats a rider on a direction: a fresh herdr pane, the harness started in it, the
    /// direction's goal sent as its first prompt. See `dispatch_start_rider` in `gateway.rs`.
    #[serde(rename = "start_rider", rename_all = "camelCase")]
    StartRider {
        direction_id: String,
        harness: String,
        /// Passed through to `herdr agent start ... -- <args>` verbatim -- not surfaced in the
        /// first-run UI, which never picks a harness's own flags for the human; exists for
        /// scripted/test seating (e.g. `["--model", "haiku"]`).
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        #[allow(dead_code)]
        client_id: Option<String>,
    },
}

/// Candidate harnesses this deck knows how to seat, `(id, label, bin)`. `id` is what
/// `herdr agent start --kind` and the wire protocol both use; `bin` is the executable actually
/// looked up on PATH -- `cursor-agent`, not `cursor` (the Cursor.app CLI launcher, a different
/// binary present on this machine that answers `--version` but is not an agent herdr can seat).
const HARNESS_CANDIDATES: [(&str, &str, &str); 6] = [
    ("claude", "Claude", "claude"),
    ("codex", "Codex", "codex"),
    ("gemini", "Gemini", "gemini"),
    ("agy", "Antigravity", "agy"),
    ("opencode", "opencode", "opencode"),
    ("cursor", "Cursor", "cursor-agent"),
];

/// Whether `bin` resolves on `PATH` -- a `which`-style lookup with no subprocess spawn, so
/// detection never runs an untrusted or slow binary just to see if it exists.
fn on_path(bin: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(bin);
        candidate.is_file() && is_executable(&candidate)
    })
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_path: &std::path::Path) -> bool {
    true
}

/// Which harnesses this machine can seat a rider on right now, in `HARNESS_CANDIDATES` order.
pub fn detect_harnesses() -> Vec<Harness> {
    HARNESS_CANDIDATES
        .iter()
        .filter(|(_, _, bin)| on_path(bin))
        .map(|(id, label, bin)| Harness { id: id.to_string(), label: label.to_string(), bin: bin.to_string() })
        .collect()
}

/// Whether herdr can seat a rider: on PATH at all, and its socket actually present (an
/// installed-but-never-run herdr looks identical to a missing one otherwise). Mirrors
/// `home_cmd::herdr_reachable`'s two-step check, kept separate because that one returns a
/// bool for a println and this one returns the wire-typed three-way status `hello` carries.
pub fn detect_herdr_status() -> HerdrStatus {
    if !on_path("herdr") {
        return HerdrStatus::Missing;
    }
    match herdr_socket_path_for_env() {
        Some(p) if p.exists() => HerdrStatus::Ok,
        _ => HerdrStatus::NoSocket,
    }
}

fn herdr_socket_path_for_env() -> Option<std::path::PathBuf> {
    let base = directories::BaseDirs::new()?;
    Some(base.home_dir().join(".config").join("herdr").join("herdr.sock"))
}

/// The git toplevel of `cwd`, or `None` outside a repo (`git rev-parse --show-toplevel`, run
/// once at startup -- cheap enough not to need caching beyond the `HomeEnv` it lands in).
pub fn git_toplevel(cwd: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Builds the `HomeEnv` `archie home` hands every connecting client, detected once at startup:
/// where it runs (decision 4, "auto-fill the working directory ... never ask what the
/// environment already knows"), which harnesses can seat a rider (decision 2), and whether
/// herdr itself is usable (decision 3, no PTY fallback).
pub fn detect_env(cwd: std::path::PathBuf) -> HomeEnv {
    let repo = git_toplevel(&cwd);
    HomeEnv {
        cwd: cwd.to_string_lossy().to_string(),
        repo,
        harnesses: detect_harnesses(),
        herdr: detect_herdr_status(),
        // Filled in by `gateway::spawn`, which has `Storage`; a plain placeholder here so
        // `detect_env` alone still returns a usable value for tests and callers without one.
        budget_default_tokens: PLAIN_BUDGET_DEFAULT_TOKENS,
    }
}

/// The plain default when this machine has no indexed sessions to draw a percentile from yet.
pub const PLAIN_BUDGET_DEFAULT_TOKENS: i64 = 5_000_000;

pub fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// `~/.agentworth/home.toml`: `[roles]` maps a herdr agent name to a `Role`. Unmapped agents
/// get `Role::Guest`.
#[derive(Debug, Default, Deserialize)]
pub struct HomeConfig {
    #[serde(default)]
    pub roles: HashMap<String, Role>,
}

pub fn load_role_map() -> HashMap<String, Role> {
    let path = match agentworth_storage::default_db_dir() {
        Ok(dir) => dir.join("home.toml"),
        Err(_) => return HashMap::new(),
    };
    match std::fs::read_to_string(&path) {
        Ok(raw) => toml::from_str::<HomeConfig>(&raw)
            .map(|c| c.roles)
            .unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

/// Turns an agent name (or any other free-form label) into a stable, URL-safe persona id.
pub fn slugify(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_was_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            out.push('-');
            last_was_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "persona".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Whether `AgentSessionInfo::value` is the harness's own session id, or a path to one.
/// herdr's `AgentSessionRefKind` (`herdr api schema --json`, `$defs.AgentSessionRefKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionRefKind {
    Id,
    Path,
}

/// herdr's own idea of which harness session a pane is running, straight off `herdr agent
/// list`/`herdr api schema --json`'s `AgentSessionInfo`. Verified live on 2026-09-07: `value`
/// matches this index's `sessions.session_id` exactly for claude_code, codex and antigravity
/// panes when `kind == "id"` (e.g. claude `61eef7db-...`, codex `01a07081-...`, agy
/// `219114fe-...`) -- so it's a more direct rider->session join than going through
/// `agent_state.pane_id`, when it resolves. Grok's adapter names sessions after files
/// ("events", "chat_history") rather than an id herdr would recognize, so a Grok pane's
/// `agent_session.value` does not resolve against this index either way; that's a gap in
/// Grok's own adapter, not something to route around here.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentSessionInfo {
    #[allow(dead_code)]
    pub agent: String,
    pub kind: AgentSessionRefKind,
    #[allow(dead_code)]
    pub source: String,
    pub value: String,
}

/// One row of `herdr agent list`'s `result.agents` array. Only the fields this module needs;
/// herdr's own schema (`herdr api schema --json`) has more.
#[derive(Debug, Clone, Deserialize)]
pub struct HerdrAgent {
    pub agent: String,
    #[serde(default)]
    pub name: Option<String>,
    pub pane_id: String,
    pub workspace_id: String,
    pub cwd: String,
    pub agent_status: Presence,
    #[serde(default)]
    pub terminal_title_stripped: Option<String>,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub agent_session: Option<AgentSessionInfo>,
}

/// herdr's every CLI response is the same envelope: `{"id": ..., "result": {...}}` or
/// `{"id": ..., "error": {"code", "message"}}`. Parsed as a bare `Value` rather than a generic
/// `HerdrEnvelope<T>` -- serde's derive adds a `T: Default` bound to any generic field marked
/// `#[serde(default)]` regardless of nesting, which `Option<T>` doesn't actually need but
/// which every non-`Default` `T` (this module's response types included) then fails.
pub async fn herdr_agent_list() -> anyhow::Result<Vec<HerdrAgent>> {
    let output = tokio::process::Command::new("herdr")
        .args(["agent", "list"])
        .output()
        .await?;
    if !output.status.success() {
        anyhow::bail!(
            "herdr agent list exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    if let Some(message) = envelope.pointer("/error/message").and_then(|v| v.as_str()) {
        anyhow::bail!("herdr agent list: {message}");
    }
    let agents = envelope
        .pointer("/result/agents")
        .cloned()
        .unwrap_or(serde_json::Value::Array(Vec::new()));
    Ok(serde_json::from_value(agents)?)
}

pub fn parse_rfc3339(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
