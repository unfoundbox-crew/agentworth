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
}

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
