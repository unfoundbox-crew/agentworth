//! The gateway behind `apps/home`: a WebSocket at `/ws` that turns herdr's live pane state
//! into the persona/space/message wire protocol `apps/home/src/protocol.ts` defines. Rust
//! mirrors that file's types field for field -- change both together or neither.
//!
//! herdr is the only source of truth for who is running and what state they are in. This
//! module never talks to herdr's socket protocol to send a prompt -- it shells out to the
//! `herdr` CLI (`herdr agent prompt`, `herdr agent read`) the same way a person would, so a
//! herdr release can change its socket wire format without this module noticing.
//!
//! Presence streams over herdr's own `events.subscribe` (protocol 20, confirmed live on
//! 2026-09-07 -- see the report this module shipped with for captured frames; docs/specs/loop.md
//! is stale on this point, it predates the subscription API). A snapshot poll every 5s
//! (`herdr agent list`) is the fallback and the only way new agents are discovered, since
//! subscriptions are per-pane-id and a pane that doesn't exist yet has no id to subscribe to.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command;
use tokio::sync::{broadcast, Mutex};

use super::routes::AppState;

/// Wire protocol version. Bump alongside `PROTOCOL` in `apps/home/src/protocol.ts`.
pub const PROTOCOL: u32 = 1;

/// How many messages a space's in-memory log keeps. Older ones are dropped; there is no
/// SQLite-backed history for this lane (see the module doc and the report this shipped with).
const RING_CAPACITY: usize = 500;

/// How often the persona/space list is refreshed from `herdr agent list`. This is also the
/// only path by which a newly-opened pane becomes a persona -- see `refresh_from_agents`.
const SNAPSHOT_POLL: Duration = Duration::from_secs(5);

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
    /// exists and always resolves (confirmed via `herdr agent list`, see the report).
    fn target_ref(&self) -> &str {
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

/// `work`/`artifact` frames are out of scope for this lane (see the job brief). This type
/// exists only so `ServerFrame::Artifact` and the `backfill` frame's `artifacts` field type-check;
/// nothing in this module ever constructs one. TODO(archie loop): feed these from
/// `server/loop_socket.rs`'s hook stream once a turn's tool calls are available there.
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

// `rename_all_fields` (the container-level attribute that renames every variant's fields at
// once) is not applied here on purpose: it did not rename anything in practice against serde
// 1.0.229 in this workspace when combined with internal tagging (`tag = "t"`) -- caught by the
// literal-JSON tests below, which is exactly the case they exist for. Each struct variant gets
// its own `rename_all = "camelCase"` instead, which is unambiguous and is what the tests verify.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "t")]
pub enum ServerFrame {
    #[serde(rename = "hello")]
    Hello {
        protocol: u32,
        personas: Vec<Persona>,
        spaces: Vec<Space>,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "t")]
pub enum ClientFrame {
    #[serde(rename = "open", rename_all = "camelCase")]
    Open {
        space_id: String,
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

/// The three standing rooms every persona belongs to, beside each persona's own office.
const ROOMS: [(&str, &str); 3] = [
    ("incidents", "Incidents"),
    ("lounge", "Lounge"),
    ("standup", "Standup"),
];

fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Turns an agent name (or any other free-form label) into a stable, URL-safe persona id.
fn slugify(raw: &str) -> String {
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

/// `~/.agentworth/home.toml`: `[roles]` maps a herdr agent name to a `Role`. Unmapped agents
/// get `Role::Guest`.
#[derive(Debug, Default, Deserialize)]
struct HomeConfig {
    #[serde(default)]
    roles: HashMap<String, Role>,
}

fn load_role_map() -> HashMap<String, Role> {
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

/// One row of `herdr agent list`'s `result.agents` array. Only the fields this module needs;
/// herdr's own schema (`herdr api schema --json`) has more.
#[derive(Debug, Clone, Deserialize)]
struct HerdrAgent {
    agent: String,
    #[serde(default)]
    name: Option<String>,
    pane_id: String,
    workspace_id: String,
    cwd: String,
    agent_status: Presence,
    #[serde(default)]
    terminal_title_stripped: Option<String>,
    #[serde(default)]
    revision: u64,
}

/// herdr's every CLI response is the same envelope: `{"id": ..., "result": {...}}` or
/// `{"id": ..., "error": {"code", "message"}}`. Parsed as a bare `Value` rather than a generic
/// `HerdrEnvelope<T>` -- serde's derive adds a `T: Default` bound to any generic field marked
/// `#[serde(default)]` regardless of nesting, which `Option<T>` doesn't actually need but
/// which every non-`Default` `T` (this module's response types included) then fails.
async fn herdr_agent_list() -> anyhow::Result<Vec<HerdrAgent>> {
    let output = Command::new("herdr")
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

/// State one connected client's `hello` and `backfill` frames are built from, and the target
/// of every `presence`/`message` update this module broadcasts.
struct HomeRuntime {
    personas: HashMap<String, Persona>,
    pane_to_persona: HashMap<String, String>,
    spaces: HashMap<String, Space>,
    messages: HashMap<String, VecDeque<Message>>,
    role_map: HashMap<String, Role>,
}

impl HomeRuntime {
    fn new(role_map: HashMap<String, Role>) -> Self {
        let mut spaces = HashMap::new();
        for (id, label) in ROOMS {
            spaces.insert(
                id.to_string(),
                Space {
                    id: id.to_string(),
                    kind: SpaceKind::Room,
                    label: label.to_string(),
                    members: Vec::new(),
                    unread: 0,
                    last_summary: None,
                    last_activity: None,
                },
            );
        }
        Self {
            personas: HashMap::new(),
            pane_to_persona: HashMap::new(),
            spaces,
            messages: HashMap::new(),
            role_map,
        }
    }

    fn snapshot_hello(&self) -> (Vec<Persona>, Vec<Space>) {
        let mut personas: Vec<Persona> = self.personas.values().cloned().collect();
        personas.sort_by(|a, b| a.id.cmp(&b.id));
        let mut spaces: Vec<Space> = self.spaces.values().cloned().collect();
        spaces.sort_by(|a, b| a.id.cmp(&b.id));
        (personas, spaces)
    }

    /// Rebuilds personas and offices from a fresh `herdr agent list`. Returns the pane ids
    /// that are new since the last refresh, so the caller can subscribe to their status
    /// changes. Existing personas keep their `id` (a slug of their agent name) even if their
    /// pane's other fields (cwd, revision, title) changed underneath them.
    fn refresh_from_agents(&mut self, agents: &[HerdrAgent]) -> Vec<String> {
        let mut new_pane_ids = Vec::new();
        let all_room_members: Vec<String> = agents
            .iter()
            .map(|a| slugify(a.name.as_deref().unwrap_or(&a.pane_id)))
            .collect();

        for agent in agents {
            let agent_name = agent
                .name
                .clone()
                .unwrap_or_else(|| agent.pane_id.clone());
            let id = slugify(&agent_name);
            let role = self
                .role_map
                .get(&agent_name)
                .copied()
                .unwrap_or(Role::Guest);
            let persona = Persona {
                id: id.clone(),
                role,
                kind: agent.agent.clone(),
                agent_name: agent_name.clone(),
                pane_id: agent.pane_id.clone(),
                workspace_id: agent.workspace_id.clone(),
                cwd: agent.cwd.clone(),
                presence: agent.agent_status,
                title: agent.terminal_title_stripped.clone().unwrap_or_default(),
                revision: agent.revision,
            };
            if !self.pane_to_persona.contains_key(&agent.pane_id) {
                new_pane_ids.push(agent.pane_id.clone());
            }
            self.pane_to_persona.insert(agent.pane_id.clone(), id.clone());
            self.personas.insert(id.clone(), persona);

            self.spaces
                .entry(format!("office-{id}"))
                .and_modify(|s| s.members = vec![id.clone()])
                .or_insert_with(|| Space {
                    id: format!("office-{id}"),
                    kind: SpaceKind::Office,
                    label: agent_name.clone(),
                    members: vec![id.clone()],
                    unread: 0,
                    last_summary: None,
                    last_activity: None,
                });
        }

        for (_, label) in ROOMS {
            if let Some(room) = self.spaces.values_mut().find(|s| s.label == label) {
                all_room_members.clone_into(&mut room.members);
            }
        }

        new_pane_ids
    }

    fn apply_status_change(
        &mut self,
        pane_id: &str,
        presence: Presence,
        title: Option<String>,
    ) -> Option<ServerFrame> {
        let persona_id = self.pane_to_persona.get(pane_id)?.clone();
        let persona = self.personas.get_mut(&persona_id)?;
        persona.presence = presence;
        persona.revision += 1;
        if let Some(t) = title {
            persona.title = t;
        }
        Some(ServerFrame::Presence {
            persona_id,
            presence,
            title: persona.title.clone(),
            revision: persona.revision,
        })
    }

    fn push_message(&mut self, message: Message) {
        let ring = self
            .messages
            .entry(message.space_id.clone())
            .or_insert_with(|| VecDeque::with_capacity(RING_CAPACITY));
        if ring.len() >= RING_CAPACITY {
            ring.pop_front();
        }
        ring.push_back(message);
    }

    fn backfill(&self, space_id: &str) -> Vec<Message> {
        self.messages
            .get(space_id)
            .map(|ring| ring.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Office -> its one member. Room -> the mentioned personas, or the first idle member
    /// when no mention was given. Returns an error string (not a `ServerFrame::Error` directly,
    /// since the caller may want to attach it to a specific client rather than broadcast it).
    fn resolve_targets(&self, space_id: &str, mentions: &[String]) -> Result<Vec<Persona>, String> {
        let space = self
            .spaces
            .get(space_id)
            .ok_or_else(|| format!("no such space: {space_id}"))?;
        match space.kind {
            SpaceKind::Office => space
                .members
                .first()
                .and_then(|id| self.personas.get(id))
                .cloned()
                .map(|p| vec![p])
                .ok_or_else(|| format!("office {space_id} has no persona")),
            SpaceKind::Room => {
                if mentions.is_empty() {
                    let first_idle = space
                        .members
                        .iter()
                        .filter_map(|id| self.personas.get(id))
                        .find(|p| p.presence == Presence::Idle)
                        .cloned();
                    first_idle
                        .map(|p| vec![p])
                        .ok_or_else(|| "no idle persona in this room".to_string())
                } else {
                    let targets: Vec<Persona> = mentions
                        .iter()
                        .filter_map(|id| self.personas.get(id))
                        .cloned()
                        .collect();
                    if targets.is_empty() {
                        Err("no mentioned persona is a member of this room".to_string())
                    } else {
                        Ok(targets)
                    }
                }
            }
        }
    }
}

/// Shared handle passed into `AppState` and cloned once per connection/spawned prompt task.
#[derive(Clone)]
pub struct HomeHandle {
    runtime: Arc<Mutex<HomeRuntime>>,
    tx: broadcast::Sender<ServerFrame>,
}

const BROADCAST_CAPACITY: usize = 256;

/// Starts the herdr snapshot-poll and subscription tasks and returns the handle `start_server`
/// wires into `AppState`. Never fails: if herdr's CLI or socket is unreachable, the tasks log
/// a warning and keep retrying, and a connecting client just gets an empty `hello`.
pub fn spawn() -> HomeHandle {
    let (tx, _rx) = broadcast::channel(BROADCAST_CAPACITY);
    let runtime = Arc::new(Mutex::new(HomeRuntime::new(load_role_map())));
    // Carries newly-discovered pane ids from the poll loop to the subscription loop. Built
    // here and moved into both tasks at spawn time, rather than reached for via a shared
    // static, so there's no window where the poll loop's first tick could fire before the
    // subscription loop exists to receive from it.
    let (new_panes_tx, new_panes_rx) = tokio::sync::mpsc::unbounded_channel();

    let handle = HomeHandle {
        runtime: runtime.clone(),
        tx: tx.clone(),
    };

    tokio::spawn(snapshot_poll_loop(runtime.clone(), new_panes_tx));
    tokio::spawn(subscription_loop(runtime, tx, new_panes_rx));

    handle
}

/// Refreshes personas/spaces from `herdr agent list` every `SNAPSHOT_POLL` and hands each new
/// pane id it discovers to the subscription loop over `new_panes_tx`.
async fn snapshot_poll_loop(
    runtime: Arc<Mutex<HomeRuntime>>,
    new_panes_tx: tokio::sync::mpsc::UnboundedSender<Vec<String>>,
) {
    let mut interval = tokio::time::interval(SNAPSHOT_POLL);
    loop {
        interval.tick().await;
        match herdr_agent_list().await {
            Ok(agents) => {
                let mut rt = runtime.lock().await;
                let new_panes = rt.refresh_from_agents(&agents);
                drop(rt);
                if !new_panes.is_empty() {
                    let _ = new_panes_tx.send(new_panes);
                }
            }
            Err(e) => {
                tracing::warn!("home gateway: herdr agent list failed: {e:#}");
            }
        }
    }
}

/// Owns the one connection to herdr's socket, subscribes to `pane.agent_status_changed` for
/// every pane the snapshot loop has found, and turns each event herdr sends into a `presence`
/// broadcast. Reconnects with a fixed backoff if herdr's socket isn't there yet or drops.
async fn subscription_loop(
    runtime: Arc<Mutex<HomeRuntime>>,
    tx: broadcast::Sender<ServerFrame>,
    mut new_panes_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<String>>,
) {
    loop {
        match herdr_socket_path() {
            Some(path) => match UnixStream::connect(&path).await {
                Ok(stream) => {
                    if let Err(e) =
                        run_subscription(stream, &runtime, &tx, &mut new_panes_rx).await
                    {
                        tracing::warn!("home gateway: herdr subscription ended: {e:#}");
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "home gateway: could not connect to herdr socket at {}: {e}",
                        path.display()
                    );
                }
            },
            None => {
                tracing::warn!("home gateway: no herdr config dir; presence will only update on the 5s snapshot poll");
            }
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

fn herdr_socket_path() -> Option<std::path::PathBuf> {
    let base = directories::BaseDirs::new()?;
    Some(base.home_dir().join(".config").join("herdr").join("herdr.sock"))
}

/// Runs one herdr socket connection until it errors or closes: subscribes to every pane
/// already known at connect time, adds newly-discovered panes to the same subscription as
/// they arrive from the poll loop, and turns each `pane_agent_status_changed` event into a
/// broadcast `presence` frame.
async fn run_subscription(
    stream: UnixStream,
    runtime: &Arc<Mutex<HomeRuntime>>,
    tx: &broadcast::Sender<ServerFrame>,
    new_panes_rx: &mut tokio::sync::mpsc::UnboundedReceiver<Vec<String>>,
) -> anyhow::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let initial_panes: Vec<String> = {
        let rt = runtime.lock().await;
        rt.pane_to_persona.keys().cloned().collect()
    };
    if !initial_panes.is_empty() {
        subscribe(&mut write_half, &initial_panes).await?;
    }

    let mut line = String::new();
    loop {
        tokio::select! {
            read = reader.read_line(&mut line) => {
                let n = read?;
                if n == 0 {
                    anyhow::bail!("herdr socket closed");
                }
                let text = std::mem::take(&mut line);
                if let Some(frame) = parse_subscription_event(&text) {
                    let mut rt = runtime.lock().await;
                    if let Some(server_frame) =
                        rt.apply_status_change(&frame.pane_id, frame.agent_status, frame.title)
                    {
                        let _ = tx.send(server_frame);
                    }
                }
            }
            Some(new_panes) = new_panes_rx.recv() => {
                if let Err(e) = subscribe(&mut write_half, &new_panes).await {
                    tracing::warn!("home gateway: could not subscribe to new panes: {e:#}");
                }
            }
        }
    }
}

async fn subscribe(
    write_half: &mut tokio::net::unix::OwnedWriteHalf,
    pane_ids: &[String],
) -> anyhow::Result<()> {
    let subscriptions: Vec<serde_json::Value> = pane_ids
        .iter()
        .map(|pane_id| {
            serde_json::json!({"type": "pane.agent_status_changed", "pane_id": pane_id})
        })
        .collect();
    let request = serde_json::json!({
        "id": format!("home-gateway:{}", new_id()),
        "method": "events.subscribe",
        "params": {"subscriptions": subscriptions},
    });
    let mut line = serde_json::to_vec(&request)?;
    line.push(b'\n');
    write_half.write_all(&line).await?;
    Ok(())
}

struct StatusChangedFrame {
    pane_id: String,
    agent_status: Presence,
    title: Option<String>,
}

/// Parses one JSON line from herdr's socket. Ignores anything that isn't a
/// `pane_agent_status_changed` subscription event -- the ack for `events.subscribe` itself,
/// and any other subscription this connection didn't ask for, land here too and are dropped.
fn parse_subscription_event(line: &str) -> Option<StatusChangedFrame> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if value.get("event")?.as_str()? != "pane_agent_status_changed" {
        return None;
    }
    let data = value.get("data")?;
    Some(StatusChangedFrame {
        pane_id: data.get("pane_id")?.as_str()?.to_string(),
        agent_status: serde_json::from_value(data.get("agent_status")?.clone()).ok()?,
        title: data.get("title").and_then(|v| v.as_str()).map(String::from),
    })
}

pub fn router() -> Router<AppState> {
    Router::new().route("/ws", get(ws_handler))
}

async fn ws_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    match state.home.clone() {
        Some(home) => ws.on_upgrade(move |socket| handle_socket(socket, home)),
        None => (
            StatusCode::NOT_FOUND,
            "home gateway not enabled; start `archie serve --home`",
        )
            .into_response(),
    }
}

async fn send_frame(socket: &mut WebSocket, frame: &ServerFrame) -> anyhow::Result<()> {
    let text = serde_json::to_string(frame)?;
    socket.send(WsMessage::Text(text)).await?;
    Ok(())
}

async fn handle_socket(mut socket: WebSocket, home: HomeHandle) {
    let (personas, spaces) = {
        let rt = home.runtime.lock().await;
        rt.snapshot_hello()
    };
    if send_frame(
        &mut socket,
        &ServerFrame::Hello {
            protocol: PROTOCOL,
            personas,
            spaces,
        },
    )
    .await
    .is_err()
    {
        return;
    }

    let mut rx = home.tx.subscribe();
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(WsMessage::Text(text))) => {
                        match serde_json::from_str::<ClientFrame>(&text) {
                            Ok(frame) => handle_client_frame(&home, &mut socket, frame).await,
                            Err(e) => {
                                let _ = send_frame(
                                    &mut socket,
                                    &ServerFrame::Error { code: "bad_frame".into(), detail: e.to_string() },
                                )
                                .await;
                            }
                        }
                    }
                    Some(Ok(WsMessage::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            broadcast_msg = rx.recv() => {
                match broadcast_msg {
                    Ok(frame) => {
                        if send_frame(&mut socket, &frame).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

async fn handle_client_frame(home: &HomeHandle, socket: &mut WebSocket, frame: ClientFrame) {
    match frame {
        ClientFrame::Open { space_id } => {
            let messages = {
                let rt = home.runtime.lock().await;
                rt.backfill(&space_id)
            };
            let _ = send_frame(
                socket,
                &ServerFrame::Backfill {
                    space_id,
                    messages,
                    artifacts: Vec::new(),
                },
            )
            .await;
        }
        // Unread counters and artifact bodies need a store this lane doesn't build (see the
        // module doc's TODO); nothing to do with either frame yet.
        ClientFrame::Seen { .. } | ClientFrame::Fetch { .. } => {}
        ClientFrame::Prompt {
            space_id,
            text,
            mentions,
        } => dispatch_prompt(home, socket, space_id, text, mentions).await,
    }
}

async fn dispatch_prompt(
    home: &HomeHandle,
    socket: &mut WebSocket,
    space_id: String,
    text: String,
    mentions: Vec<String>,
) {
    let targets = {
        let rt = home.runtime.lock().await;
        rt.resolve_targets(&space_id, &mentions)
    };
    let targets = match targets {
        Ok(t) => t,
        Err(detail) => {
            let _ = send_frame(
                socket,
                &ServerFrame::Error { code: "no_target".into(), detail },
            )
            .await;
            return;
        }
    };

    let blocked: Vec<&Persona> = targets.iter().filter(|p| p.presence == Presence::Blocked).collect();
    for persona in &blocked {
        let _ = send_frame(
            socket,
            &ServerFrame::Error {
                code: "blocked".into(),
                detail: format!("{} is blocked and can't take a prompt right now", persona.agent_name),
            },
        )
        .await;
    }
    let runnable: Vec<Persona> = targets
        .into_iter()
        .filter(|p| p.presence != Presence::Blocked)
        .collect();
    if runnable.is_empty() {
        return;
    }

    let you_message = Message {
        id: new_id(),
        space_id: space_id.clone(),
        from: "you".to_string(),
        kind: MessageKind::Speech,
        text: text.clone(),
        at: now_iso(),
        artifacts: None,
        mentions: if mentions.is_empty() { None } else { Some(mentions) },
    };
    {
        let mut rt = home.runtime.lock().await;
        rt.push_message(you_message.clone());
    }
    let _ = home.tx.send(ServerFrame::Message { message: you_message });

    for persona in runnable {
        let home = home.clone();
        let space_id = space_id.clone();
        let text = text.clone();
        tokio::spawn(async move {
            run_prompt_and_reply(home, space_id, persona, text).await;
        });
    }
}

/// Sends `text` to `persona` via the `herdr` CLI and waits for it to settle, then makes a
/// best-effort attempt at recovering the reply text and turns it into a `speech` message.
///
/// Reliability of the reply extraction: LOW. `herdr agent read` returns a plain-text terminal
/// snapshot (box-drawing borders, the input box, a status footer with the model/branch/PR),
/// not a transcript -- there is no message-boundary marker anywhere in it. This takes a
/// snapshot immediately before sending the prompt and one immediately after `--wait` settles,
/// and reports the lines present in the second that weren't in the first. That is enough to
/// mostly work on a short reply and will visibly mis-extract on anything that scrolls the
/// pane, redraws its footer, or wraps differently between the two reads. See the report this
/// module shipped with for the exact captured output `herdr agent read` gave during dev.
async fn run_prompt_and_reply(home: HomeHandle, space_id: String, persona: Persona, text: String) {
    let target = persona.target_ref();
    let before = read_agent_snapshot(target).await.unwrap_or_default();

    let output = Command::new("herdr")
        .args(["agent", "prompt", target, &text, "--wait"])
        .output()
        .await;

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            let _ = home.tx.send(ServerFrame::Error {
                code: "herdr_unavailable".into(),
                detail: e.to_string(),
            });
            return;
        }
    };

    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if detail.is_empty() {
            String::from_utf8_lossy(&output.stderr).trim().to_string()
        } else {
            detail
        };
        let _ = home.tx.send(ServerFrame::Error {
            code: "prompt_failed".into(),
            detail: if detail.is_empty() {
                format!("herdr agent prompt {target} failed")
            } else {
                detail
            },
        });
        return;
    }

    let after = read_agent_snapshot(target).await.unwrap_or_default();
    let reply = diff_new_lines(&before, &after);
    if reply.trim().is_empty() {
        return;
    }

    let message = Message {
        id: new_id(),
        space_id,
        from: persona.id.clone(),
        kind: MessageKind::Speech,
        text: reply,
        at: now_iso(),
        artifacts: None,
        mentions: None,
    };
    {
        let mut rt = home.runtime.lock().await;
        rt.push_message(message.clone());
    }
    let _ = home.tx.send(ServerFrame::Message { message });
}

async fn read_agent_snapshot(target: &str) -> anyhow::Result<String> {
    let output = Command::new("herdr")
        .args(["agent", "read", target, "--source", "recent-unwrapped", "--lines", "60"])
        .output()
        .await?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Lines in `after` that aren't in `before`, in order, skipping herdr's own box-drawing
/// borders and the cwd/status footer line -- both constant chrome that would otherwise show
/// up as "new" on almost every read. Heuristic; see `run_prompt_and_reply`'s doc comment.
fn diff_new_lines(before: &str, after: &str) -> String {
    let mut before_counts: HashMap<&str, usize> = HashMap::new();
    for line in before.lines() {
        *before_counts.entry(line).or_insert(0) += 1;
    }
    let mut out = Vec::new();
    for line in after.lines() {
        let trimmed = line.trim();
        if trimmed.chars().all(|c| c == '─' || c.is_whitespace()) {
            continue;
        }
        if let Some(count) = before_counts.get_mut(line) {
            if *count > 0 {
                *count -= 1;
                continue;
            }
        }
        out.push(line.trim_end());
    }
    out.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_agent(pane_id: &str, agent: &str, name: Option<&str>, status: Presence) -> HerdrAgent {
        HerdrAgent {
            agent: agent.to_string(),
            name: name.map(String::from),
            pane_id: pane_id.to_string(),
            workspace_id: "w1".to_string(),
            cwd: "/repo".to_string(),
            agent_status: status,
            terminal_title_stripped: Some("doing a thing".to_string()),
            revision: 1,
        }
    }

    #[test]
    fn hello_frame_matches_protocol_ts_shape() {
        let frame = ServerFrame::Hello {
            protocol: 1,
            personas: vec![Persona {
                id: "partner-harvey".into(),
                role: Role::Senior,
                kind: "claude".into(),
                agent_name: "partner-harvey".into(),
                pane_id: "w9:pA".into(),
                workspace_id: "w9".into(),
                cwd: "/repo".into(),
                presence: Presence::Idle,
                title: "mvec mission director handoff".into(),
                revision: 10,
            }],
            spaces: vec![Space {
                id: "office-partner-harvey".into(),
                kind: SpaceKind::Office,
                label: "partner-harvey".into(),
                members: vec!["partner-harvey".into()],
                unread: 0,
                last_summary: None,
                last_activity: None,
            }],
        };

        let expected = serde_json::json!({
            "t": "hello",
            "protocol": 1,
            "personas": [{
                "id": "partner-harvey",
                "role": "senior",
                "kind": "claude",
                "agentName": "partner-harvey",
                "paneId": "w9:pA",
                "workspaceId": "w9",
                "cwd": "/repo",
                "presence": "idle",
                "title": "mvec mission director handoff",
                "revision": 10
            }],
            "spaces": [{
                "id": "office-partner-harvey",
                "kind": "office",
                "label": "partner-harvey",
                "members": ["partner-harvey"],
                "unread": 0,
                "lastSummary": null,
                "lastActivity": null
            }]
        });
        assert_eq!(serde_json::to_value(&frame).unwrap(), expected);
    }

    #[test]
    fn presence_frame_matches_protocol_ts_shape() {
        let frame = ServerFrame::Presence {
            persona_id: "partner-harvey".into(),
            presence: Presence::Working,
            title: "reviewing #196".into(),
            revision: 11,
        };
        let expected = serde_json::json!({
            "t": "presence",
            "personaId": "partner-harvey",
            "presence": "working",
            "title": "reviewing #196",
            "revision": 11
        });
        assert_eq!(serde_json::to_value(&frame).unwrap(), expected);
    }

    #[test]
    fn message_frame_matches_protocol_ts_shape() {
        let frame = ServerFrame::Message {
            message: Message {
                id: "m1".into(),
                space_id: "office-partner-harvey".into(),
                from: "you".into(),
                kind: MessageKind::Speech,
                text: "ship it".into(),
                at: "2026-09-07T00:00:00+00:00".into(),
                artifacts: None,
                mentions: None,
            },
        };
        let expected = serde_json::json!({
            "t": "message",
            "message": {
                "id": "m1",
                "spaceId": "office-partner-harvey",
                "from": "you",
                "kind": "speech",
                "text": "ship it",
                "at": "2026-09-07T00:00:00+00:00"
            }
        });
        assert_eq!(serde_json::to_value(&frame).unwrap(), expected);
    }

    #[test]
    fn error_frame_matches_protocol_ts_shape() {
        let frame = ServerFrame::Error { code: "blocked".into(), detail: "nope".into() };
        let expected = serde_json::json!({"t": "error", "code": "blocked", "detail": "nope"});
        assert_eq!(serde_json::to_value(&frame).unwrap(), expected);
    }

    #[test]
    fn backfill_frame_matches_protocol_ts_shape() {
        let frame = ServerFrame::Backfill {
            space_id: "lounge".into(),
            messages: Vec::new(),
            artifacts: Vec::new(),
        };
        let expected = serde_json::json!({
            "t": "backfill",
            "spaceId": "lounge",
            "messages": [],
            "artifacts": []
        });
        assert_eq!(serde_json::to_value(&frame).unwrap(), expected);
    }

    #[test]
    fn client_open_frame_parses() {
        let frame: ClientFrame = serde_json::from_str(r#"{"t":"open","spaceId":"lounge"}"#).unwrap();
        matches!(frame, ClientFrame::Open { space_id } if space_id == "lounge");
    }

    #[test]
    fn client_prompt_frame_parses_with_default_mentions() {
        let frame: ClientFrame =
            serde_json::from_str(r#"{"t":"prompt","spaceId":"lounge","text":"hi"}"#).unwrap();
        match frame {
            ClientFrame::Prompt { space_id, text, mentions } => {
                assert_eq!(space_id, "lounge");
                assert_eq!(text, "hi");
                assert!(mentions.is_empty());
            }
            _ => panic!("expected Prompt"),
        }
    }

    /// Fixture shaped like `herdr agent list`'s real output (captured 2026-09-07, paths and
    /// pane/workspace ids redacted). Exercises role assignment (mapped vs. unmapped -> guest)
    /// and the office/room layout `refresh_from_agents` builds.
    #[test]
    fn refresh_from_agents_builds_offices_and_rooms_with_roles() {
        let agents = vec![
            sample_agent("w1:pA", "claude", Some("partner-harvey"), Presence::Idle),
            sample_agent("w1:pB", "grok", Some("mike2-grok4.6"), Presence::Working),
            // No `name`: falls back to the pane id for both agentName and the slug.
            sample_agent("w1:pC", "codex", None, Presence::Idle),
        ];

        let mut role_map = HashMap::new();
        role_map.insert("partner-harvey".to_string(), Role::Senior);

        let mut runtime = HomeRuntime::new(role_map);
        let new_panes = runtime.refresh_from_agents(&agents);

        assert_eq!(new_panes.len(), 3, "all three panes are new on first refresh");

        let harvey = runtime.personas.get("partner-harvey").expect("harvey persona");
        assert_eq!(harvey.role, Role::Senior);
        assert_eq!(harvey.presence, Presence::Idle);

        let mike = runtime.personas.get("mike2-grok4-6").expect("mike persona");
        assert_eq!(mike.role, Role::Guest, "unmapped agent name defaults to guest");
        assert_eq!(mike.presence, Presence::Working);

        let fallback_id = slugify("w1:pC");
        let fallback = runtime.personas.get(&fallback_id).expect("fallback persona");
        assert_eq!(fallback.agent_name, "w1:pC");

        assert!(runtime.spaces.contains_key("office-partner-harvey"));
        assert!(runtime.spaces.contains_key("office-mike2-grok4-6"));
        assert!(runtime.spaces.contains_key(&format!("office-{fallback_id}")));

        for room_id in ["incidents", "lounge", "standup"] {
            let room = runtime.spaces.get(room_id).unwrap_or_else(|| panic!("missing room {room_id}"));
            assert_eq!(room.kind, SpaceKind::Room);
            assert_eq!(room.members.len(), 3, "every persona is a member of every room");
        }

        // A second refresh with the same panes reports no new pane ids.
        let no_new = runtime.refresh_from_agents(&agents);
        assert!(no_new.is_empty());
    }

    #[test]
    fn resolve_targets_office_returns_its_one_member() {
        let mut runtime = HomeRuntime::new(HashMap::new());
        runtime.refresh_from_agents(&[sample_agent("w1:pA", "claude", Some("harvey"), Presence::Idle)]);
        let targets = runtime.resolve_targets("office-harvey", &[]).unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].id, "harvey");
    }

    #[test]
    fn resolve_targets_room_without_mentions_picks_first_idle() {
        let mut runtime = HomeRuntime::new(HashMap::new());
        runtime.refresh_from_agents(&[
            sample_agent("w1:pA", "claude", Some("harvey"), Presence::Working),
            sample_agent("w1:pB", "grok", Some("mike"), Presence::Idle),
        ]);
        let targets = runtime.resolve_targets("lounge", &[]).unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].id, "mike");
    }

    #[test]
    fn resolve_targets_blocked_persona_is_still_returned_for_dispatch_to_reject() {
        // resolve_targets itself doesn't filter blocked personas out -- dispatch_prompt does,
        // so it can send a specific `error` frame naming who's blocked.
        let mut runtime = HomeRuntime::new(HashMap::new());
        runtime.refresh_from_agents(&[sample_agent("w1:pA", "claude", Some("harvey"), Presence::Blocked)]);
        let targets = runtime.resolve_targets("office-harvey", &[]).unwrap();
        assert_eq!(targets[0].presence, Presence::Blocked);
    }

    #[test]
    fn apply_status_change_bumps_revision_and_updates_title() {
        let mut runtime = HomeRuntime::new(HashMap::new());
        runtime.refresh_from_agents(&[sample_agent("w1:pA", "claude", Some("harvey"), Presence::Idle)]);
        let frame = runtime
            .apply_status_change("w1:pA", Presence::Working, Some("reviewing #196".into()))
            .expect("frame");
        match frame {
            ServerFrame::Presence { persona_id, presence, title, revision } => {
                assert_eq!(persona_id, "harvey");
                assert_eq!(presence, Presence::Working);
                assert_eq!(title, "reviewing #196");
                assert_eq!(revision, 2);
            }
            _ => panic!("expected Presence frame"),
        }
    }

    #[test]
    fn diff_new_lines_drops_borders_and_repeated_lines() {
        let before = "line one\n───────\nfooter";
        let after = "line one\n───────\nfooter\nnew reply text";
        assert_eq!(diff_new_lines(before, after), "new reply text");
    }

    #[test]
    fn slugify_handles_dots_and_spaces() {
        assert_eq!(slugify("mike2-grok4.6"), "mike2-grok4-6");
        assert_eq!(slugify("w9:pA"), "w9-pa");
    }
}
