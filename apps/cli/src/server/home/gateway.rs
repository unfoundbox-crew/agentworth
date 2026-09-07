//! The gateway behind `apps/home`: a WebSocket at `/ws` that turns herdr's live pane state,
//! this index's own directions table, and harness transcripts into the protocol-2 wire types
//! `protocol.rs` mirrors from `apps/home/src/protocol.ts`.
//!
//! herdr is the only source of truth for who is running and what state they are in. This
//! module never talks to herdr's socket protocol to send a prompt -- it shells out to the
//! `herdr` CLI (`herdr agent prompt`) the same way a person would, so a herdr release can
//! change its socket wire format without this module noticing.
//!
//! Presence streams over herdr's own `events.subscribe` (protocol 20, confirmed live on
//! 2026-09-07 -- see the `home-gateway` report for captured frames; docs/specs/loop.md is
//! fixed as of this lane to no longer say the socket is request/response only). A snapshot
//! poll every 5s (`herdr agent list`) is the fallback and the only way new agents are
//! discovered, since subscriptions are per-pane-id and a pane that doesn't exist yet has no
//! id to subscribe to.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use agentworth_core::Scanner;
use agentworth_storage::Storage;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command;
use tokio::sync::{broadcast, Mutex};

use super::directions;
use super::protocol::*;
use super::transcript_feed::{self, SessionCursor};
use super::super::live_tail::LiveTailEvent;
use super::super::routes::AppState;

/// How many messages a space's in-memory log keeps. Older ones are dropped; there is no
/// SQLite-backed history for the office/room chat stream (directions persist in SQLite;
/// their chat frames are still ephemeral, same tradeoff the first `home-gateway` lane made).
const RING_CAPACITY: usize = 500;

/// How often the persona/space list is refreshed from `herdr agent list`. This is also the
/// only path by which a newly-opened pane becomes a persona -- see `refresh_from_agents`.
const SNAPSHOT_POLL: Duration = Duration::from_secs(5);

/// How often directions are recomputed (`spentTokens`/`reached`/`state`) and, on change,
/// broadcast. Independent of `SNAPSHOT_POLL` -- direction state can change from a governor
/// halt or a spend cap crossing with no presence change at all.
const DIRECTION_POLL: Duration = Duration::from_secs(5);

/// The three standing rooms every persona belongs to, beside each persona's own office.
const ROOMS: [(&str, &str); 3] = [
    ("incidents", "Incidents"),
    ("lounge", "Lounge"),
    ("standup", "Standup"),
];

/// State one connected client's `hello`/`backfill` frames are built from, and the target of
/// every broadcast this module sends.
struct HomeRuntime {
    personas: HashMap<String, Persona>,
    pane_to_persona: HashMap<String, String>,
    spaces: HashMap<String, Space>,
    messages: HashMap<String, VecDeque<Message>>,
    role_map: HashMap<String, Role>,
    /// Per-session read position into that session's transcript, owned by the transcript feed
    /// (see `transcript_feed.rs`). Keyed by `sessions.session_id`.
    session_cursors: HashMap<String, SessionCursor>,
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
            session_cursors: HashMap::new(),
        }
    }

    fn snapshot_hello(&self) -> (Vec<Persona>, Vec<Space>) {
        let mut personas: Vec<Persona> = self.personas.values().cloned().collect();
        personas.sort_by(|a, b| a.id.cmp(&b.id));
        let mut spaces: Vec<Space> = self.spaces.values().cloned().collect();
        spaces.sort_by(|a, b| a.id.cmp(&b.id));
        (personas, spaces)
    }

    fn presence_by_persona(&self) -> HashMap<String, Presence> {
        self.personas.iter().map(|(id, p)| (id.clone(), p.presence)).collect()
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
            let agent_name = agent.name.clone().unwrap_or_else(|| agent.pane_id.clone());
            let id = slugify(&agent_name);
            let role = self.role_map.get(&agent_name).copied().unwrap_or(Role::Guest);
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
        self.messages.get(space_id).map(|ring| ring.iter().cloned().collect()).unwrap_or_default()
    }

    /// Office -> its one member. Room -> the mentioned personas, or the first idle member
    /// when no mention was given.
    fn resolve_targets(&self, space_id: &str, mentions: &[String]) -> Result<Vec<Persona>, String> {
        let space = self.spaces.get(space_id).ok_or_else(|| format!("no such space: {space_id}"))?;
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
                    space
                        .members
                        .iter()
                        .filter_map(|id| self.personas.get(id))
                        .find(|p| p.presence == Presence::Idle)
                        .cloned()
                        .map(|p| vec![p])
                        .ok_or_else(|| "no idle persona in this room".to_string())
                } else {
                    let targets: Vec<Persona> =
                        mentions.iter().filter_map(|id| self.personas.get(id)).cloned().collect();
                    if targets.is_empty() {
                        Err("no mentioned persona is a member of this room".to_string())
                    } else {
                        Ok(targets)
                    }
                }
            }
        }
    }

    /// Riders of a direction, resolved to personas the runtime currently knows about. Ids in
    /// `riders` that don't (yet) match a known persona are silently dropped -- the caller
    /// treats an empty result as "no reachable rider" the same way `resolve_targets` does.
    fn riders_for(&self, rider_ids: &[String], mentions: &[String]) -> Vec<Persona> {
        let ids: Vec<&String> = if mentions.is_empty() {
            rider_ids.iter().collect()
        } else {
            rider_ids.iter().filter(|id| mentions.contains(id)).collect()
        };
        ids.into_iter().filter_map(|id| self.personas.get(id)).cloned().collect()
    }
}

/// Shared handle passed into `AppState` and cloned once per connection/spawned prompt task.
#[derive(Clone)]
pub struct HomeHandle {
    runtime: Arc<Mutex<HomeRuntime>>,
    tx: broadcast::Sender<ServerFrame>,
    storage: Arc<Storage>,
    scanner: Arc<Scanner>,
}

const BROADCAST_CAPACITY: usize = 256;

/// Starts the herdr snapshot-poll/subscription tasks, the direction recompute loop, and the
/// transcript feed, and returns the handle `start_server` wires into `AppState`. Never fails:
/// if herdr's CLI or socket is unreachable, the tasks log a warning and keep retrying, and a
/// connecting client just gets an empty `hello`.
pub fn spawn(storage: Arc<Storage>, scanner: Arc<Scanner>, live_tail_tx: broadcast::Sender<LiveTailEvent>) -> HomeHandle {
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
        storage: storage.clone(),
        scanner: scanner.clone(),
    };

    tokio::spawn(snapshot_poll_loop(runtime.clone(), new_panes_tx));
    tokio::spawn(subscription_loop(runtime.clone(), tx.clone(), new_panes_rx));
    tokio::spawn(direction_poll_loop(runtime.clone(), storage.clone(), tx.clone()));
    tokio::spawn(transcript_feed_loop(runtime, storage, scanner, tx, live_tail_tx.subscribe()));

    handle
}

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
            Err(e) => tracing::warn!("home gateway: herdr agent list failed: {e:#}"),
        }
    }
}

/// Recomputes every direction's `state`/`exception` on `DIRECTION_POLL` and broadcasts a
/// `direction` frame for each one that actually changed (`directions::recompute_all` only
/// returns changed ones).
async fn direction_poll_loop(
    runtime: Arc<Mutex<HomeRuntime>>,
    storage: Arc<Storage>,
    tx: broadcast::Sender<ServerFrame>,
) {
    let mut interval = tokio::time::interval(DIRECTION_POLL);
    loop {
        interval.tick().await;
        let presence = {
            let rt = runtime.lock().await;
            rt.presence_by_persona()
        };
        match directions::recompute_all(&storage, &presence) {
            Ok(changed) => {
                for direction in changed {
                    let _ = tx.send(ServerFrame::Direction { direction });
                }
            }
            Err(e) => tracing::warn!("home gateway: direction recompute failed: {e:#}"),
        }
    }
}

/// Consumes the same `live_tail` filesystem-change feed `/api/live-tail` (SSE) reads, and for
/// every changed session turns newly-appended transcript events into `message`/`stop` frames
/// (see `transcript_feed.rs`). Replaces the old `herdr agent read` diff heuristic entirely.
async fn transcript_feed_loop(
    runtime: Arc<Mutex<HomeRuntime>>,
    storage: Arc<Storage>,
    scanner: Arc<Scanner>,
    tx: broadcast::Sender<ServerFrame>,
    mut live_tail_rx: broadcast::Receiver<LiveTailEvent>,
) {
    loop {
        let event = match live_tail_rx.recv().await {
            Ok(event) => event,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => return,
        };
        if event.adapter.is_none() {
            continue;
        }
        let path_str = event.path.to_string_lossy().to_string();
        let Ok(Some(session_id)) = storage.session_id_for_source_path(&path_str) else {
            // Not indexed yet -- a brand-new session file, or one the periodic scanner
            // hasn't reached. The transcript feed only follows sessions already in the
            // index; picking up a session on its very first write is left to the scanner.
            continue;
        };

        let trace = match scanner.load_trace(&session_id) {
            Ok(trace) => trace,
            Err(e) => {
                tracing::warn!("home gateway: could not load trace for {session_id}: {e:#}");
                continue;
            }
        };

        let Some(cwd) = storage.get_agent_state(&session_id).ok().flatten().and_then(|s| s.cwd) else {
            continue;
        };
        let pane_id = storage.get_agent_state(&session_id).ok().flatten().and_then(|s| s.pane_id);

        let (space_id, from_persona) = {
            let rt = runtime.lock().await;
            match pane_id.as_deref().and_then(|p| rt.pane_to_persona.get(p)) {
                Some(persona_id) => (format!("office-{persona_id}"), persona_id.clone()),
                None => continue,
            }
        };

        let output = {
            let mut rt = runtime.lock().await;
            let cursor = rt.session_cursors.entry(session_id.clone()).or_default();
            transcript_feed::feed_from_trace(&trace, cursor, &space_id, &from_persona)
        };

        for message in output.messages {
            {
                let mut rt = runtime.lock().await;
                rt.push_message(message.clone());
            }
            let _ = tx.send(ServerFrame::Message { message });
        }

        if let Some((rung, _summary)) = output.new_outcome {
            if let Ok(direction_ids) = directions_matching_area(&storage, &cwd) {
                for direction_id in direction_ids {
                    let stop = Stop {
                        id: new_id(),
                        direction_id,
                        from: from_persona.clone(),
                        rung,
                        artifact_id: None,
                        at: now_iso(),
                    };
                    let _ = tx.send(ServerFrame::Stop { stop });
                }
            }
        }
    }
}

/// Every stored direction whose `area` contains `cwd` (a session's cwd is a descendant of, or
/// equal to, the direction's area) -- the same membership test `home_direction_area_spend`
/// applies, just evaluated in Rust against already-loaded rows instead of round-tripping SQL
/// per session.
fn directions_matching_area(storage: &Storage, cwd: &str) -> anyhow::Result<Vec<String>> {
    Ok(storage
        .list_home_directions()?
        .into_iter()
        .filter(|d| cwd == d.area || cwd.starts_with(&format!("{}/", d.area)))
        .map(|d| d.id)
        .collect())
}

async fn subscription_loop(
    runtime: Arc<Mutex<HomeRuntime>>,
    tx: broadcast::Sender<ServerFrame>,
    mut new_panes_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<String>>,
) {
    loop {
        match herdr_socket_path() {
            Some(path) => match UnixStream::connect(&path).await {
                Ok(stream) => {
                    if let Err(e) = run_subscription(stream, &runtime, &tx, &mut new_panes_rx).await {
                        tracing::warn!("home gateway: herdr subscription ended: {e:#}");
                    }
                }
                Err(e) => tracing::warn!(
                    "home gateway: could not connect to herdr socket at {}: {e}",
                    path.display()
                ),
            },
            None => tracing::warn!(
                "home gateway: no herdr config dir; presence will only update on the 5s snapshot poll"
            ),
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

fn herdr_socket_path() -> Option<std::path::PathBuf> {
    let base = directories::BaseDirs::new()?;
    Some(base.home_dir().join(".config").join("herdr").join("herdr.sock"))
}

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
        .map(|pane_id| serde_json::json!({"type": "pane.agent_status_changed", "pane_id": pane_id}))
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
    let directions = directions::list_wire(&home.storage).unwrap_or_default();
    if send_frame(
        &mut socket,
        &ServerFrame::Hello { protocol: PROTOCOL, personas, spaces, directions },
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
                &ServerFrame::Backfill { space_id, messages, artifacts: Vec::new() },
            )
            .await;
        }
        ClientFrame::Seen { .. } | ClientFrame::Fetch { .. } => {}
        ClientFrame::Prompt { space_id, text, mentions } => {
            dispatch_prompt(home, socket, space_id, text, mentions).await
        }
        ClientFrame::SetDirection { direction } => dispatch_set_direction(home, socket, direction).await,
        ClientFrame::Steer { direction_id, text, mode, mentions } => {
            dispatch_steer(home, socket, direction_id, text, mode, mentions).await
        }
    }
}

async fn dispatch_set_direction(home: &HomeHandle, socket: &mut WebSocket, input: DirectionInput) {
    match directions::apply_set_direction(&home.storage, input) {
        Ok(row) => match directions::to_wire(&home.storage, &row) {
            Ok(direction) => {
                let _ = home.tx.send(ServerFrame::Direction { direction });
            }
            Err(e) => {
                let _ = send_frame(
                    socket,
                    &ServerFrame::Error { code: "direction_error".into(), detail: e.to_string() },
                )
                .await;
            }
        },
        Err(e) => {
            let _ = send_frame(
                socket,
                &ServerFrame::Error { code: "direction_error".into(), detail: e.to_string() },
            )
            .await;
        }
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
            let _ = send_frame(socket, &ServerFrame::Error { code: "no_target".into(), detail }).await;
            return;
        }
    };

    let blocked: Vec<&Persona> = targets.iter().filter(|p| p.presence == Presence::Blocked).collect();
    for persona in &blocked {
        let _ = send_frame(
            socket,
            &ServerFrame::Error {
                code: "blocked".into(),
                detail: format!("{} is blocked and can't take a prompt right now (herdr: agent_blocked)", persona.agent_name),
            },
        )
        .await;
    }
    let runnable: Vec<Persona> = targets.into_iter().filter(|p| p.presence != Presence::Blocked).collect();
    if runnable.is_empty() {
        return;
    }

    post_you_message(home, &space_id, &text, &mentions).await;

    for persona in runnable {
        let target = persona.target_ref().to_string();
        tokio::spawn(async move {
            let _ = Command::new("herdr").args(["agent", "prompt", &target, &text]).output().await;
        });
    }
}

/// Sends `text` verbatim to each of `direction.riders` (or the mentioned subset), honoring
/// `mode`. Speech is delivered exactly as typed -- never paraphrased -- per
/// docs/specs/home.md's "send my exact prompts, not your inference".
async fn dispatch_steer(
    home: &HomeHandle,
    socket: &mut WebSocket,
    direction_id: String,
    text: String,
    mode: SteerMode,
    mentions: Vec<String>,
) {
    let Ok(Some(direction_row)) = home.storage.get_home_direction(&direction_id) else {
        let _ = send_frame(
            socket,
            &ServerFrame::Error { code: "no_such_direction".into(), detail: direction_id },
        )
        .await;
        return;
    };

    let targets = {
        let rt = home.runtime.lock().await;
        rt.riders_for(&direction_row.riders, &mentions)
    };
    if targets.is_empty() {
        let _ = send_frame(
            socket,
            &ServerFrame::Error {
                code: "no_target".into(),
                detail: format!("no reachable rider for direction {direction_id}"),
            },
        )
        .await;
        return;
    }

    if mode == SteerMode::Now {
        let blocked: Vec<&Persona> = targets.iter().filter(|p| p.presence == Presence::Blocked).collect();
        if !blocked.is_empty() {
            let names: Vec<&str> = blocked.iter().map(|p| p.agent_name.as_str()).collect();
            let _ = send_frame(
                socket,
                &ServerFrame::Error {
                    code: "blocked".into(),
                    detail: format!(
                        "{} blocked; herdr rejects a prompt to a blocked agent (agent_blocked)",
                        names.join(", ")
                    ),
                },
            )
            .await;
            return;
        }
    }

    for persona in &targets {
        post_you_message(home, &format!("office-{}", persona.id), &text, &[]).await;
    }

    for persona in targets {
        let target = persona.target_ref().to_string();
        let text = text.clone();
        let home = home.clone();
        match mode {
            SteerMode::Now => {
                tokio::spawn(async move {
                    let _ = Command::new("herdr").args(["agent", "prompt", &target, &text]).output().await;
                });
            }
            SteerMode::After => {
                tokio::spawn(async move { steer_after_settle(home, persona.id, target, text).await });
            }
        }
    }
}

/// Bounded wait for a rider to settle (`docs/specs/home.md`: "once they are done, they report
/// back", no unbounded polling loops surfaced to the UI). Polls the runtime's own presence
/// map -- already kept current by the snapshot poll and the herdr subscription -- rather than
/// shelling out to herdr again.
async fn steer_after_settle(home: HomeHandle, persona_id: String, target: String, text: String) {
    const POLL: Duration = Duration::from_secs(2);
    const MAX_WAIT: Duration = Duration::from_secs(30 * 60);
    let mut waited = Duration::ZERO;
    loop {
        let presence = {
            let rt = home.runtime.lock().await;
            rt.personas.get(&persona_id).map(|p| p.presence)
        };
        match presence {
            Some(Presence::Idle) | Some(Presence::Done) | None => break,
            Some(Presence::Blocked) => {
                let _ = home.tx.send(ServerFrame::Error {
                    code: "blocked".into(),
                    detail: format!("{target} settled blocked; steer not delivered (agent_blocked)"),
                });
                return;
            }
            _ => {}
        }
        if waited >= MAX_WAIT {
            let _ = home.tx.send(ServerFrame::Error {
                code: "steer_timeout".into(),
                detail: format!("{target} never settled within {}s", MAX_WAIT.as_secs()),
            });
            return;
        }
        tokio::time::sleep(POLL).await;
        waited += POLL;
    }
    let _ = Command::new("herdr").args(["agent", "prompt", &target, &text]).output().await;
}

async fn post_you_message(home: &HomeHandle, space_id: &str, text: &str, mentions: &[String]) {
    let message = Message {
        id: new_id(),
        space_id: space_id.to_string(),
        from: "you".to_string(),
        kind: MessageKind::Speech,
        text: text.to_string(),
        at: now_iso(),
        artifacts: None,
        mentions: if mentions.is_empty() { None } else { Some(mentions.to_vec()) },
    };
    {
        let mut rt = home.runtime.lock().await;
        rt.push_message(message.clone());
    }
    let _ = home.tx.send(ServerFrame::Message { message });
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
            protocol: 2,
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
            directions: vec![Direction {
                id: "d1".into(),
                goal: "ship the thing".into(),
                area: "/repo".into(),
                done: Rung::Test,
                reached: Some(Rung::Artifact),
                budget_tokens: 1_000_000,
                spent_tokens: 42_000,
                riders: vec!["partner-harvey".into()],
                state: DirectionState::Riding,
                exception: None,
                created_at: "2026-09-07T00:00:00+00:00".into(),
                updated_at: "2026-09-07T00:00:00+00:00".into(),
            }],
        };

        let expected = serde_json::json!({
            "t": "hello",
            "protocol": 2,
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
            }],
            "directions": [{
                "id": "d1",
                "goal": "ship the thing",
                "area": "/repo",
                "done": "test",
                "reached": "artifact",
                "budgetTokens": 1000000,
                "spentTokens": 42000,
                "riders": ["partner-harvey"],
                "state": "riding",
                "exception": null,
                "createdAt": "2026-09-07T00:00:00+00:00",
                "updatedAt": "2026-09-07T00:00:00+00:00"
            }]
        });
        assert_eq!(serde_json::to_value(&frame).unwrap(), expected);
    }

    #[test]
    fn direction_frame_matches_protocol_ts_shape_with_exception() {
        let frame = ServerFrame::Direction {
            direction: Direction {
                id: "d1".into(),
                goal: "ship it".into(),
                area: "/repo".into(),
                done: Rung::Ci,
                reached: None,
                budget_tokens: 500_000,
                spent_tokens: 600_000,
                riders: vec!["harvey".into()],
                state: DirectionState::Halted,
                exception: Some(Exception {
                    reason: "over budget: spent 600000 tokens against a budget of 500000".into(),
                    since: "2026-09-07T01:00:00+00:00".into(),
                }),
                created_at: "2026-09-07T00:00:00+00:00".into(),
                updated_at: "2026-09-07T01:00:00+00:00".into(),
            },
        };
        let expected = serde_json::json!({
            "t": "direction",
            "direction": {
                "id": "d1",
                "goal": "ship it",
                "area": "/repo",
                "done": "ci",
                "reached": null,
                "budgetTokens": 500000,
                "spentTokens": 600000,
                "riders": ["harvey"],
                "state": "halted",
                "exception": {
                    "reason": "over budget: spent 600000 tokens against a budget of 500000",
                    "since": "2026-09-07T01:00:00+00:00"
                },
                "createdAt": "2026-09-07T00:00:00+00:00",
                "updatedAt": "2026-09-07T01:00:00+00:00"
            }
        });
        assert_eq!(serde_json::to_value(&frame).unwrap(), expected);
    }

    #[test]
    fn stop_frame_matches_protocol_ts_shape() {
        let frame = ServerFrame::Stop {
            stop: Stop {
                id: "s1".into(),
                direction_id: "d1".into(),
                from: "harvey".into(),
                rung: Rung::Test,
                artifact_id: None,
                at: "2026-09-07T00:00:00+00:00".into(),
            },
        };
        let expected = serde_json::json!({
            "t": "stop",
            "stop": {
                "id": "s1",
                "directionId": "d1",
                "from": "harvey",
                "rung": "test",
                "artifactId": null,
                "at": "2026-09-07T00:00:00+00:00"
            }
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
        let frame = ServerFrame::Backfill { space_id: "lounge".into(), messages: Vec::new(), artifacts: Vec::new() };
        let expected = serde_json::json!({"t": "backfill", "spaceId": "lounge", "messages": [], "artifacts": []});
        assert_eq!(serde_json::to_value(&frame).unwrap(), expected);
    }

    #[test]
    fn client_open_frame_parses() {
        let frame: ClientFrame = serde_json::from_str(r#"{"t":"open","spaceId":"lounge"}"#).unwrap();
        matches!(frame, ClientFrame::Open { space_id } if space_id == "lounge");
    }

    #[test]
    fn client_prompt_frame_parses_with_default_mentions() {
        let frame: ClientFrame = serde_json::from_str(r#"{"t":"prompt","spaceId":"lounge","text":"hi"}"#).unwrap();
        match frame {
            ClientFrame::Prompt { space_id, text, mentions } => {
                assert_eq!(space_id, "lounge");
                assert_eq!(text, "hi");
                assert!(mentions.is_empty());
            }
            _ => panic!("expected Prompt"),
        }
    }

    #[test]
    fn client_set_direction_frame_parses_camel_case() {
        let raw = r#"{"t":"set_direction","direction":{"id":"d1","goal":"ship it","area":"/repo","done":"test","budgetTokens":1000000,"riders":["harvey"]}}"#;
        let frame: ClientFrame = serde_json::from_str(raw).unwrap();
        match frame {
            ClientFrame::SetDirection { direction } => {
                assert_eq!(direction.id, "d1");
                assert_eq!(direction.done, Rung::Test);
                assert_eq!(direction.budget_tokens, 1_000_000);
                assert_eq!(direction.riders, vec!["harvey".to_string()]);
            }
            _ => panic!("expected SetDirection"),
        }
    }

    #[test]
    fn client_steer_frame_parses_mode_and_mentions() {
        let raw = r#"{"t":"steer","directionId":"d1","text":"ship it now","mode":"now","mentions":["harvey"]}"#;
        let frame: ClientFrame = serde_json::from_str(raw).unwrap();
        match frame {
            ClientFrame::Steer { direction_id, text, mode, mentions } => {
                assert_eq!(direction_id, "d1");
                assert_eq!(text, "ship it now");
                assert_eq!(mode, SteerMode::Now);
                assert_eq!(mentions, vec!["harvey".to_string()]);
            }
            _ => panic!("expected Steer"),
        }
    }

    #[test]
    fn client_steer_frame_defaults_mentions_to_empty() {
        let raw = r#"{"t":"steer","directionId":"d1","text":"go","mode":"after"}"#;
        let frame: ClientFrame = serde_json::from_str(raw).unwrap();
        match frame {
            ClientFrame::Steer { mode, mentions, .. } => {
                assert_eq!(mode, SteerMode::After);
                assert!(mentions.is_empty());
            }
            _ => panic!("expected Steer"),
        }
    }

    #[test]
    fn refresh_from_agents_builds_offices_and_rooms_with_roles() {
        let agents = vec![
            sample_agent("w1:pA", "claude", Some("partner-harvey"), Presence::Idle),
            sample_agent("w1:pB", "grok", Some("mike2-grok4.6"), Presence::Working),
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
    fn riders_for_filters_to_mentions_when_given() {
        let mut runtime = HomeRuntime::new(HashMap::new());
        runtime.refresh_from_agents(&[
            sample_agent("w1:pA", "claude", Some("harvey"), Presence::Idle),
            sample_agent("w1:pB", "grok", Some("mike"), Presence::Idle),
        ]);
        let riders = vec!["harvey".to_string(), "mike".to_string()];
        let all = runtime.riders_for(&riders, &[]);
        assert_eq!(all.len(), 2);
        let just_mike = runtime.riders_for(&riders, &["mike".to_string()]);
        assert_eq!(just_mike.len(), 1);
        assert_eq!(just_mike[0].id, "mike");
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
    fn slugify_handles_dots_and_spaces() {
        assert_eq!(slugify("mike2-grok4.6"), "mike2-grok4-6");
        assert_eq!(slugify("w9:pA"), "w9-pa");
    }
}
