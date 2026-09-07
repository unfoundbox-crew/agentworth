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
//! fixed as of this lane to no longer say the socket is request/response only). Two live probes
//! the same day found: `pane.updated` fires constantly (hundreds of times in tens of seconds)
//! and each one carries the pane's current `agent_status`, so this module subscribes to it and
//! derives presence from it, deduped per pane in `apply_status_change` so that volume doesn't
//! turn into a noisy client; `pane.agent_status_changed` does also arrive, just rarely, and
//! under a wire spelling ("pane.agent_status_changed", dotted, matching its own subscription
//! type string) that doesn't match herdr's own `EventKind` enum name -- the first probe's match
//! against the underscored name alone is why it looked like this event never fired at all (see
//! `parse_subscription_event`). A snapshot poll every 5s (`herdr agent list`) is the fallback and
//! the only way new agents are discovered, since subscriptions are per-pane-id and
//! a pane that doesn't exist yet has no id to subscribe to.

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
    /// `agent_session.value` -> `pane_id`, for panes whose `agent_session.kind == "id"`.
    /// Verified live (2026-09-07): this `value` equals this index's `sessions.session_id`
    /// exactly for claude_code, codex, and antigravity panes -- a more direct rider->session
    /// join than `agent_state.pane_id`, when it resolves. See `pane_for_session`.
    session_id_to_pane: HashMap<String, String>,
    /// `agent_session.value` -> `pane_id`, for panes whose `agent_session.kind == "path"`.
    /// Matched against a session's own indexed `source_path` instead of its `session_id`.
    source_path_to_pane: HashMap<String, String>,
    /// Persona id -> the last ~12 non-empty lines read off its pane (`herdr agent read`,
    /// box-drawing stripped) at the moment it went `blocked`. Populated by
    /// `run_subscription`'s spawned fetch when `apply_status_change` reports a fresh transition
    /// into `blocked`; cleared the moment that persona leaves `blocked`. `directions::recompute_state`
    /// reads this to put what a pane is actually asking into `exception.reason`, instead of the
    /// generic "waiting on X".
    blocked_prompts: HashMap<String, String>,
    /// Area (a direction's `area` field) -> `(workspace_id, most-recently-created pane_id in
    /// it)`. `start_rider` creates one herdr workspace per area on first use and splits a fresh
    /// pane off the last one for every rider after that, so seating five riders on one
    /// direction opens one workspace, not five.
    area_workspaces: HashMap<String, (String, String)>,
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
            session_id_to_pane: HashMap::new(),
            source_path_to_pane: HashMap::new(),
            blocked_prompts: HashMap::new(),
            area_workspaces: HashMap::new(),
        }
    }

    /// The pane running `session_id` (whose indexed source lives at `source_path`), preferring
    /// herdr's own `agent_session` join over the caller's `agent_state.pane_id` fallback.
    /// `fallback_pane_id` is `Storage::get_agent_state(session_id)`'s `pane_id` -- the join this
    /// runtime used exclusively before `agent_session` was modeled -- so a session herdr's own
    /// join doesn't (yet) resolve, e.g. a Grok session named after a file rather than an id
    /// herdr recognizes, still finds its pane the old way.
    fn pane_for_session(
        &self,
        session_id: &str,
        source_path: &str,
        fallback_pane_id: Option<&str>,
    ) -> Option<String> {
        self.session_id_to_pane
            .get(session_id)
            .or_else(|| self.source_path_to_pane.get(source_path))
            .cloned()
            .or_else(|| fallback_pane_id.map(String::from))
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
    ///
    /// Also returns `(persona_id, target_ref)` for every persona this refresh observed
    /// transitioning into `blocked` -- the snapshot poll is a second path (besides the herdr
    /// socket subscription in `run_subscription`) that can first notice a block, so it needs the
    /// same "fetch the pane text" hook `apply_status_change` provides there.
    fn refresh_from_agents(&mut self, agents: &[HerdrAgent]) -> (Vec<String>, Vec<(String, String)>) {
        let mut new_pane_ids = Vec::new();
        let mut newly_blocked = Vec::new();
        let all_room_members: Vec<String> = agents
            .iter()
            .map(|a| slugify(a.name.as_deref().unwrap_or(&a.pane_id)))
            .collect();

        for agent in agents {
            let agent_name = agent.name.clone().unwrap_or_else(|| agent.pane_id.clone());
            let id = slugify(&agent_name);
            let role = self.role_map.get(&agent_name).copied().unwrap_or(Role::Guest);
            let was_blocked = self.personas.get(&id).map(|p| p.presence) == Some(Presence::Blocked);
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
            if persona.presence == Presence::Blocked && !was_blocked {
                newly_blocked.push((id.clone(), persona.target_ref().to_string()));
            }
            if persona.presence != Presence::Blocked {
                self.blocked_prompts.remove(&id);
            }
            self.pane_to_persona.insert(agent.pane_id.clone(), id.clone());
            if let Some(session) = &agent.agent_session {
                match session.kind {
                    AgentSessionRefKind::Id => {
                        self.session_id_to_pane.insert(session.value.clone(), agent.pane_id.clone());
                    }
                    AgentSessionRefKind::Path => {
                        self.source_path_to_pane
                            .insert(session.value.clone(), agent.pane_id.clone());
                    }
                }
            }
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
                room.members.clone_from(&all_room_members);
            }
        }

        (new_pane_ids, newly_blocked)
    }

    /// Applies an observed `(presence, title)` for `pane_id` and returns the frame to broadcast
    /// -- or `None` if nothing actually changed. The `None` case matters now that presence can
    /// arrive from `pane_updated` (fires on every pane change: scroll, tokens, title, agent
    /// status -- 1,818 of them in 25s in the live probe that found this), not just
    /// `pane_agent_status_changed` (which only ever fires on a real transition): without this
    /// check, every `pane_updated` tick for an unrelated field would still bump `revision` and
    /// broadcast a `presence` frame, and the client would see noise instead of "the client only
    /// sees changes".
    /// Returns the broadcast frame (or `None` if nothing changed, see the doc comment above)
    /// plus, when this call is the transition *into* `blocked`, that persona's `target_ref` --
    /// the caller uses it to spawn a `herdr agent read` fetch for `set_blocked_prompt` without
    /// holding the runtime lock across the process call. A transition *out of* `blocked` clears
    /// any stored prompt immediately, inline, since that's just a map removal.
    fn apply_status_change(
        &mut self,
        pane_id: &str,
        presence: Presence,
        title: Option<String>,
    ) -> Option<(ServerFrame, Option<(String, String)>)> {
        let persona_id = self.pane_to_persona.get(pane_id)?.clone();
        let persona = self.personas.get_mut(&persona_id)?;
        let title_changed = title.as_deref().is_some_and(|t| t != persona.title);
        if persona.presence == presence && !title_changed {
            return None;
        }
        let became_blocked = presence == Presence::Blocked && persona.presence != Presence::Blocked;
        let target_ref = persona.target_ref().to_string();
        persona.presence = presence;
        persona.revision += 1;
        if let Some(t) = title {
            persona.title = t;
        }
        let frame = ServerFrame::Presence {
            persona_id: persona_id.clone(),
            presence,
            title: persona.title.clone(),
            revision: persona.revision,
        };
        if presence != Presence::Blocked {
            self.blocked_prompts.remove(&persona_id);
        }
        let fetch = became_blocked.then_some((persona_id, target_ref));
        Some((frame, fetch))
    }

    /// Stores the pane text read for a persona that went `blocked` (see `apply_status_change`).
    /// A no-op if the persona has since left `blocked` -- the fetch that produced `prompt` is a
    /// background task that can land after the pane already unblocked.
    fn set_blocked_prompt(&mut self, persona_id: &str, prompt: String) {
        if self.personas.get(persona_id).map(|p| p.presence) == Some(Presence::Blocked) {
            self.blocked_prompts.insert(persona_id.to_string(), prompt);
        }
    }

    fn blocked_prompts(&self) -> HashMap<String, String> {
        self.blocked_prompts.clone()
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
    /// Detected once at startup (decision 4: "auto-fill the working directory ... never ask
    /// what the environment already knows"). Never changes for the life of the process.
    env: HomeEnv,
    /// Forwards a freshly-discovered pane id to `subscription_loop` so `start_rider`'s new
    /// pane gets live presence events immediately, the same path `snapshot_poll_loop` uses.
    new_panes_tx: tokio::sync::mpsc::UnboundedSender<Vec<String>>,
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

    let mut env = std::env::current_dir()
        .map(detect_env)
        .unwrap_or_else(|_| detect_env(std::path::PathBuf::from(".")));
    // The same number `archie policy init` writes into its starter policy.toml (AGENTS.md's
    // "dogfood before you propose"): this machine's own p90 across indexed primary sessions,
    // or the plain fallback `detect_env` already filled in when there's nothing indexed yet.
    if let Ok(percentiles) = storage.session_token_percentiles(None) {
        if percentiles.n > 0 {
            env.budget_default_tokens = percentiles.p90 as i64;
        }
    }

    let handle = HomeHandle {
        runtime: runtime.clone(),
        tx: tx.clone(),
        storage: storage.clone(),
        env,
        new_panes_tx: new_panes_tx.clone(),
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
                let (new_panes, newly_blocked) = rt.refresh_from_agents(&agents);
                drop(rt);
                if !new_panes.is_empty() {
                    let _ = new_panes_tx.send(new_panes);
                }
                for (persona_id, target_ref) in newly_blocked {
                    let runtime = runtime.clone();
                    tokio::spawn(async move {
                        if let Some(prompt) = fetch_blocked_prompt(&target_ref).await {
                            let mut rt = runtime.lock().await;
                            rt.set_blocked_prompt(&persona_id, prompt);
                        }
                    });
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
        let (presence, blocked_prompts) = {
            let rt = runtime.lock().await;
            (rt.presence_by_persona(), rt.blocked_prompts())
        };
        match directions::recompute_all(&storage, &presence, &blocked_prompts) {
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
        // `agent_state.pane_id` (set by the Claude Code hook loop at session start) is the
        // fallback join, kept for sessions herdr's own `agent_session` doesn't (yet) resolve --
        // e.g. Grok, whose adapter names sessions after files rather than an id herdr
        // recognizes. `pane_for_session` prefers the more direct `agent_session.value` join
        // when it's available.
        let fallback_pane_id = storage.get_agent_state(&session_id).ok().flatten().and_then(|s| s.pane_id);

        let (space_id, from_persona) = {
            let rt = runtime.lock().await;
            let pane_id = rt.pane_for_session(&session_id, &path_str, fallback_pane_id.as_deref());
            match pane_id.as_deref().and_then(|p| rt.pane_to_persona.get(p)) {
                Some(persona_id) => (format!("office-{persona_id}"), persona_id.clone()),
                None => continue,
            }
        };

        let output = {
            let mut rt = runtime.lock().await;
            let is_first_sighting = !rt.session_cursors.contains_key(&session_id);
            let cursor = rt.session_cursors.entry(session_id.clone()).or_default();
            if is_first_sighting {
                // Baseline to the trace's current end instead of zero, so a session already
                // hundreds of turns deep doesn't replay its entire history as a burst the
                // moment the feed starts watching it -- only activity from here on is a frame.
                transcript_feed::seed_cursor(&trace, cursor);
            }
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

/// Unicode box-drawing characters a terminal snapshot uses for borders and separators (the
/// horizontal/vertical/corner/junction/double-line ranges) -- stripped so `exception.reason`
/// carries the prompt's actual words, not the rule the CLI drew around them.
fn strip_box_drawing(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(*c, '\u{2500}'..='\u{257F}'))
        .collect()
}

/// The last `n` non-blank lines of `text`, trimmed. `herdr agent read`'s tail is where a live
/// permission prompt sits; blank lines are dropped so the budget of lines goes to content, not
/// to the terminal's own spacing.
fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// Reads `target`'s pane with `herdr agent read` and returns its last ~12 non-blank lines,
/// box-drawing stripped -- what a blocked pane is actually asking, for `exception.reason`.
/// `None` on any failure; the caller falls back to the generic "waiting on X" reason, which is
/// the pre-existing behavior, not a regression.
async fn fetch_blocked_prompt(target: &str) -> Option<String> {
    let output = Command::new("herdr").args(["agent", "read", target]).output().await.ok()?;
    if !output.status.success() {
        tracing::warn!(
            "home gateway: herdr agent read {target} exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    let stripped = strip_box_drawing(&raw);
    let text = last_lines(&stripped, 12);
    if text.is_empty() {
        None
    } else {
        Some(text)
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
                tracing::debug!("home gateway: herdr socket line: {}", text.trim());
                if let Some(frame) = parse_subscription_event(&text) {
                    tracing::debug!(
                        "home gateway: parsed pane_id={} status={:?}",
                        frame.pane_id, frame.agent_status
                    );
                    let outcome = {
                        let mut rt = runtime.lock().await;
                        rt.apply_status_change(&frame.pane_id, frame.agent_status, frame.title)
                    };
                    if let Some((server_frame, fetch)) = outcome {
                        let _ = tx.send(server_frame);
                        if let Some((persona_id, target_ref)) = fetch {
                            let runtime = runtime.clone();
                            tokio::spawn(async move {
                                if let Some(prompt) = fetch_blocked_prompt(&target_ref).await {
                                    let mut rt = runtime.lock().await;
                                    rt.set_blocked_prompt(&persona_id, prompt);
                                }
                            });
                        }
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

/// Subscribes to both `pane.agent_status_changed` and `pane.updated` for every pane in
/// `pane_ids`. `pane.updated` is the workhorse (hundreds of events in tens of seconds on a real
/// fleet) and is what `apply_status_change`'s per-pane dedup is built for; `pane.agent_status_changed`
/// arrives too, just rarely, and its `event` field is spelled with a dot
/// ("pane.agent_status_changed") rather than herdr's own underscored `EventKind` name -- see
/// `parse_subscription_event`, which normalizes both.
async fn subscribe(
    write_half: &mut tokio::net::unix::OwnedWriteHalf,
    pane_ids: &[String],
) -> anyhow::Result<()> {
    let subscriptions: Vec<serde_json::Value> = pane_ids
        .iter()
        .flat_map(|pane_id| {
            [
                serde_json::json!({"type": "pane.agent_status_changed", "pane_id": pane_id}),
                serde_json::json!({"type": "pane.updated", "pane_id": pane_id}),
            ]
        })
        .collect();
    let request = serde_json::json!({
        "id": format!("home-gateway:{}", new_id()),
        "method": "events.subscribe",
        "params": {"subscriptions": subscriptions},
    });
    tracing::debug!("home gateway: subscribe request: {}", request);
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

/// Parses one line of herdr's `events.subscribe` stream. Handles both event shapes: a
/// `pane_agent_status_changed` envelope carries its fields flat on `data`; a `pane_updated`
/// envelope nests the whole `PaneInfo` under `data.pane` (herdr's own JSON schema, `herdr api
/// schema --json`, `$defs.PaneInfo`) -- this reads `pane_id`, `agent_status`, and
/// `terminal_title_stripped` off of that nested object instead.
///
/// The `event` field's own notation is not consistent between the two: a live capture on
/// 2026-09-07 against a real fleet found `"event":"pane_updated"` (underscored, matching the
/// `EventKind` enum name in herdr's schema) but `"event":"pane.agent_status_changed"` (dotted,
/// matching the *subscription* type string instead) -- so a match against the underscored form
/// alone silently drops every `pane_agent_status_changed` event, which is why the probe that
/// found this module's presence bug reported seeing zero of them: they were arriving, just
/// under the other spelling. Normalizing dots to underscores before matching covers both.
fn parse_subscription_event(line: &str) -> Option<StatusChangedFrame> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let event = value.get("event")?.as_str()?.replace('.', "_");
    let data = value.get("data")?;
    match event.as_str() {
        "pane_agent_status_changed" => Some(StatusChangedFrame {
            pane_id: data.get("pane_id")?.as_str()?.to_string(),
            agent_status: serde_json::from_value(data.get("agent_status")?.clone()).ok()?,
            title: data.get("title").and_then(|v| v.as_str()).map(String::from),
        }),
        "pane_updated" => {
            let pane = data.get("pane")?;
            Some(StatusChangedFrame {
                pane_id: pane.get("pane_id")?.as_str()?.to_string(),
                agent_status: serde_json::from_value(pane.get("agent_status")?.clone()).ok()?,
                title: pane
                    .get("terminal_title_stripped")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            })
        }
        _ => None,
    }
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
        &ServerFrame::Hello { protocol: PROTOCOL, personas, spaces, directions, env: home.env.clone() },
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
        ClientFrame::Steer { direction_id, text, mode, mentions, client_id } => {
            dispatch_steer(home, socket, direction_id, text, mode, mentions, client_id).await
        }
        ClientFrame::Answer { direction_id, persona_id, key, .. } => {
            dispatch_answer(home, socket, direction_id, persona_id, key).await
        }
        ClientFrame::StartRider { direction_id, harness, args, .. } => {
            dispatch_start_rider(home, socket, direction_id, harness, args).await
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

    post_you_message(home, &space_id, &text, &mentions, None).await;

    for persona in runnable {
        let target = persona.target_ref().to_string();
        let text = text.clone();
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
    client_id: Option<String>,
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
        post_you_message(home, &format!("office-{}", persona.id), &text, &[], client_id.clone()).await;
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

/// `client_id`, when given, mirrors back the id the deck's own optimistic local echo carried, so
/// `apps/home/src/model/store.ts` can replace that echo in place instead of appending a second
/// copy of the same steer -- see `Message.clientId` in `apps/home/src/protocol.ts`.
async fn post_you_message(
    home: &HomeHandle,
    space_id: &str,
    text: &str,
    mentions: &[String],
    client_id: Option<String>,
) {
    let message = Message {
        id: new_id(),
        space_id: space_id.to_string(),
        from: "you".to_string(),
        kind: MessageKind::Speech,
        text: text.to_string(),
        at: now_iso(),
        artifacts: None,
        mentions: if mentions.is_empty() { None } else { Some(mentions.to_vec()) },
        client_id,
    };
    {
        let mut rt = home.runtime.lock().await;
        rt.push_message(message.clone());
    }
    let _ = home.tx.send(ServerFrame::Message { message });
}

/// Appends a `system` message to a direction's office log -- used to record an `answer` (`"you
/// answered 1 on probe-haiku"`) so Archie's own transcript of the direction shows who resolved
/// what, the same way any other stop or steer does.
async fn post_system_message(home: &HomeHandle, space_id: &str, text: &str) {
    let message = Message {
        id: new_id(),
        space_id: space_id.to_string(),
        from: "you".to_string(),
        kind: MessageKind::System,
        text: text.to_string(),
        at: now_iso(),
        artifacts: None,
        mentions: None,
        client_id: None,
    };
    {
        let mut rt = home.runtime.lock().await;
        rt.push_message(message.clone());
    }
    let _ = home.tx.send(ServerFrame::Message { message });
}

/// Whether `presence` is answerable at all -- pulled out of `dispatch_answer` so it has a unit
/// test independent of the socket/process plumbing around it (the job brief: "a unit test that a
/// non-blocked target is refused").
fn validate_answerable(presence: Presence) -> Result<(), String> {
    if presence == Presence::Blocked {
        Ok(())
    } else {
        Err(format!("persona is {presence:?}, not blocked; nothing to answer"))
    }
}

/// Resolves `persona_id`'s herdr agent name and runs `herdr agent send-keys <name> <key>` --
/// verified working live on 2026-09-07 against `probe-haiku`: `herdr agent send-keys probe-haiku
/// 1` took it from `blocked` to `working`. Refuses (an `error` frame, no process spawned) unless
/// the persona is presently `blocked`, per the job brief. `direction_id` is only used to name the
/// office the system message lands in when the persona can't be resolved to one via `personas`
/// (which shouldn't happen in practice, but the frame carries it either way).
async fn dispatch_answer(
    home: &HomeHandle,
    socket: &mut WebSocket,
    direction_id: String,
    persona_id: String,
    key: AnswerKey,
) {
    let persona = {
        let rt = home.runtime.lock().await;
        rt.personas.get(&persona_id).cloned()
    };
    let Some(persona) = persona else {
        let _ = send_frame(
            socket,
            &ServerFrame::Error { code: "no_target".into(), detail: format!("no such persona: {persona_id}") },
        )
        .await;
        return;
    };

    if let Err(detail) = validate_answerable(persona.presence) {
        let _ = send_frame(socket, &ServerFrame::Error { code: "not_blocked".into(), detail }).await;
        return;
    }

    let target = persona.target_ref().to_string();
    let key_str = key.as_wire_str();
    let output = Command::new("herdr").args(["agent", "send-keys", &target, key_str]).output().await;
    match output {
        Ok(o) if o.status.success() => {
            // Policy note: key "2" is a standing-permission change ("don't ask again"), which
            // widens what `target` will do without asking again. This lane only logs it as a
            // system message so Archie's record shows who widened what; it is not routed through
            // the governor here.
            // TODO(archie policy): route "don't ask again" through `archie policy` once that
            // surface exists, instead of only logging it.
            let text = if key.is_standing_permission_change() {
                format!("you answered {key_str} (don't ask again) on {target}")
            } else {
                format!("you answered {key_str} on {target}")
            };
            post_system_message(home, &format!("office-{}", persona.id), &text).await;
        }
        Ok(o) => {
            let _ = send_frame(
                socket,
                &ServerFrame::Error {
                    code: "send_keys_failed".into(),
                    detail: format!(
                        "herdr agent send-keys {target} {key_str} exited {}: {}",
                        o.status,
                        String::from_utf8_lossy(&o.stderr)
                    ),
                },
            )
            .await;
        }
        Err(e) => {
            let _ = send_frame(
                socket,
                &ServerFrame::Error {
                    code: "send_keys_failed".into(),
                    detail: format!("could not run herdr agent send-keys {target} {key_str}: {e}"),
                },
            )
            .await;
        }
    }
    let _ = direction_id; // carried on the frame for the client's own bookkeeping; not needed here.
}

/// The label a herdr workspace gets for `area` -- the directory's own name, or `"home"` when
/// that can't be determined (an empty or root path).
fn workspace_label_for_area(area: &str) -> String {
    std::path::Path::new(area)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("home")
        .to_string()
}

/// Reserves a herdr pane in `area`'s workspace for a new rider: creates the workspace on first
/// use for that area (`herdr workspace create --label <dir name> --no-focus --cwd <area>`),
/// and splits a fresh pane off the last one created there on every call after that, so seating
/// several riders on one direction opens one workspace, not one per rider. Never focuses the
/// new pane -- riders are watched, not stared at, per `apps/home/DESIGN.md`.
async fn reserve_pane_for_area(home: &HomeHandle, area: &str) -> Result<String, String> {
    let existing = {
        let rt = home.runtime.lock().await;
        rt.area_workspaces.get(area).cloned()
    };

    if let Some((workspace_id, last_pane_id)) = existing {
        let output = Command::new("herdr")
            .args(["pane", "split", &last_pane_id, "--direction", "down", "--cwd", area, "--no-focus"])
            .output()
            .await
            .map_err(|e| format!("herdr pane split: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "herdr pane split exited {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("herdr pane split: bad JSON: {e}"))?;
        let pane_id = value
            .pointer("/result/pane_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "herdr pane split: no result.pane_id in response".to_string())?
            .to_string();
        let mut rt = home.runtime.lock().await;
        rt.area_workspaces.insert(area.to_string(), (workspace_id, pane_id.clone()));
        return Ok(pane_id);
    }

    let label = workspace_label_for_area(area);
    let output = Command::new("herdr")
        .args(["workspace", "create", "--label", &label, "--no-focus", "--cwd", area])
        .output()
        .await
        .map_err(|e| format!("herdr workspace create: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "herdr workspace create exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("herdr workspace create: bad JSON: {e}"))?;
    let workspace_id = value
        .pointer("/result/workspace/workspace_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "herdr workspace create: no result.workspace.workspace_id in response".to_string())?
        .to_string();
    let pane_id = value
        .pointer("/result/root_pane/pane_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "herdr workspace create: no result.root_pane.pane_id in response".to_string())?
        .to_string();
    let mut rt = home.runtime.lock().await;
    rt.area_workspaces.insert(area.to_string(), (workspace_id, pane_id.clone()));
    Ok(pane_id)
}

/// Seats a rider on a direction (decisions 2-4 of the first-run brief): reserves a herdr pane
/// in the direction's area, starts `harness` in it, adds the resulting persona to the
/// direction's riders, and sends the direction's goal as that rider's first prompt, verbatim
/// and unprefixed -- "send my exact prompts, not your inference" (docs/specs/home.md). Refuses
/// with an `error` frame, no process spawned, when herdr is not `ok`.
async fn dispatch_start_rider(
    home: &HomeHandle,
    socket: &mut WebSocket,
    direction_id: String,
    harness: String,
    args: Vec<String>,
) {
    if home.env.herdr != HerdrStatus::Ok {
        let _ = send_frame(
            socket,
            &ServerFrame::Error {
                code: "herdr_unavailable".into(),
                detail: format!("herdr is not usable here ({:?}); install it to seat riders", home.env.herdr),
            },
        )
        .await;
        return;
    }

    let direction_row = match home.storage.get_home_direction(&direction_id) {
        Ok(Some(row)) => row,
        Ok(None) => {
            let _ = send_frame(
                socket,
                &ServerFrame::Error { code: "no_such_direction".into(), detail: direction_id },
            )
            .await;
            return;
        }
        Err(e) => {
            let _ = send_frame(socket, &ServerFrame::Error { code: "direction_error".into(), detail: e.to_string() }).await;
            return;
        }
    };

    let pane_id = match reserve_pane_for_area(home, &direction_row.area).await {
        Ok(p) => p,
        Err(detail) => {
            let _ = send_frame(socket, &ServerFrame::Error { code: "herdr_error".into(), detail }).await;
            return;
        }
    };

    let short_id: String = direction_id.chars().filter(|c| *c != '-').take(8).collect();
    let name = format!("{harness}-{short_id}");

    let mut start_args: Vec<&str> = vec!["agent", "start", &name, "--kind", &harness, "--pane", &pane_id];
    if !args.is_empty() {
        start_args.push("--");
        start_args.extend(args.iter().map(String::as_str));
    }
    let start = Command::new("herdr").args(&start_args).output().await;
    match start {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            let _ = send_frame(
                socket,
                &ServerFrame::Error {
                    code: "herdr_error".into(),
                    detail: format!(
                        "herdr agent start {name} --kind {harness} exited {}: {}",
                        o.status,
                        String::from_utf8_lossy(&o.stderr)
                    ),
                },
            )
            .await;
            return;
        }
        Err(e) => {
            let _ = send_frame(
                socket,
                &ServerFrame::Error { code: "herdr_error".into(), detail: format!("could not run herdr agent start: {e}") },
            )
            .await;
            return;
        }
    }

    // Refresh personas so the new one exists in the runtime, then broadcast it -- new personas
    // otherwise only reach a client via `hello`, and this one just seated after every connected
    // client's `hello` already went out.
    let persona_id = slugify(&name);
    let new_pane_ids = match herdr_agent_list().await {
        Ok(agents) => {
            let mut rt = home.runtime.lock().await;
            let (new_pane_ids, _) = rt.refresh_from_agents(&agents);
            new_pane_ids
        }
        Err(e) => {
            tracing::warn!("home gateway: start_rider herdr agent list failed: {e:#}");
            Vec::new()
        }
    };
    if !new_pane_ids.is_empty() {
        let _ = home.new_panes_tx.send(new_pane_ids);
    }
    let new_persona = {
        let rt = home.runtime.lock().await;
        rt.personas.get(&persona_id).cloned()
    };
    if let Some(persona) = new_persona {
        let _ = home.tx.send(ServerFrame::Persona { persona });
    }

    if let Err(e) = directions::add_rider(&home.storage, &direction_id, &persona_id) {
        let _ = send_frame(socket, &ServerFrame::Error { code: "direction_error".into(), detail: e.to_string() }).await;
        return;
    }
    if let Ok(Some(row)) = home.storage.get_home_direction(&direction_id) {
        if let Ok(direction) = directions::to_wire(&home.storage, &row) {
            let _ = home.tx.send(ServerFrame::Direction { direction });
        }
    }

    // The goal, verbatim, unprefixed, as the rider's first prompt.
    let goal = direction_row.goal.clone();
    let target = name.clone();
    tokio::spawn(async move {
        let _ = Command::new("herdr").args(["agent", "prompt", &target, &goal]).output().await;
    });
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
            agent_session: None,
        }
    }

    fn sample_agent_with_session(
        pane_id: &str,
        agent: &str,
        name: Option<&str>,
        status: Presence,
        session: AgentSessionInfo,
    ) -> HerdrAgent {
        HerdrAgent { agent_session: Some(session), ..sample_agent(pane_id, agent, name, status) }
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
            env: HomeEnv {
                cwd: "/repo".into(),
                repo: Some("/repo".into()),
                harnesses: vec![Harness { id: "claude".into(), label: "Claude".into(), bin: "claude".into() }],
                herdr: HerdrStatus::Ok,
                budget_default_tokens: 5_000_000,
            },
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
            }],
            "env": {
                "cwd": "/repo",
                "repo": "/repo",
                "harnesses": [{ "id": "claude", "label": "Claude", "bin": "claude" }],
                "herdr": "ok",
                "budgetDefaultTokens": 5000000
            }
        });
        assert_eq!(serde_json::to_value(&frame).unwrap(), expected);
    }

    #[test]
    fn persona_frame_matches_protocol_ts_shape() {
        let frame = ServerFrame::Persona {
            persona: Persona {
                id: "claude-abcd1234".into(),
                role: Role::Executor,
                kind: "claude".into(),
                agent_name: "claude-abcd1234".into(),
                pane_id: "wC:p2".into(),
                workspace_id: "wC".into(),
                cwd: "/repo".into(),
                presence: Presence::Idle,
                title: String::new(),
                revision: 0,
            },
        };
        let expected = serde_json::json!({
            "t": "persona",
            "persona": {
                "id": "claude-abcd1234",
                "role": "executor",
                "kind": "claude",
                "agentName": "claude-abcd1234",
                "paneId": "wC:p2",
                "workspaceId": "wC",
                "cwd": "/repo",
                "presence": "idle",
                "title": "",
                "revision": 0
            }
        });
        assert_eq!(serde_json::to_value(&frame).unwrap(), expected);
    }

    #[test]
    fn client_start_rider_frame_parses() {
        let frame: ClientFrame = serde_json::from_str(
            r#"{"t":"start_rider","directionId":"d1","harness":"claude","clientId":"c-1"}"#,
        )
        .unwrap();
        match frame {
            ClientFrame::StartRider { direction_id, harness, args, client_id } => {
                assert_eq!(direction_id, "d1");
                assert_eq!(harness, "claude");
                assert!(args.is_empty(), "args defaults to empty when omitted");
                assert_eq!(client_id.as_deref(), Some("c-1"));
            }
            other => panic!("expected StartRider, got {other:?}"),
        }
    }

    #[test]
    fn client_start_rider_frame_parses_args() {
        let frame: ClientFrame = serde_json::from_str(
            r#"{"t":"start_rider","directionId":"d1","harness":"claude","args":["--model","haiku"]}"#,
        )
        .unwrap();
        match frame {
            ClientFrame::StartRider { args, .. } => assert_eq!(args, vec!["--model", "haiku"]),
            other => panic!("expected StartRider, got {other:?}"),
        }
    }

    #[test]
    fn workspace_label_for_area_uses_dir_name_or_home() {
        assert_eq!(workspace_label_for_area("/Users/saurabh/code/unfoundbox/agentworth"), "agentworth");
        assert_eq!(workspace_label_for_area(""), "home");
        assert_eq!(workspace_label_for_area("/"), "home");
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
                client_id: None,
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
            ClientFrame::Steer { direction_id, text, mode, mentions, client_id } => {
                assert_eq!(direction_id, "d1");
                assert_eq!(text, "ship it now");
                assert_eq!(mode, SteerMode::Now);
                assert_eq!(mentions, vec!["harvey".to_string()]);
                assert_eq!(client_id, None);
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
    fn client_steer_frame_parses_client_id() {
        let raw = r#"{"t":"steer","directionId":"d1","text":"go","mode":"now","clientId":"c-123"}"#;
        let frame: ClientFrame = serde_json::from_str(raw).unwrap();
        match frame {
            ClientFrame::Steer { client_id, .. } => assert_eq!(client_id.as_deref(), Some("c-123")),
            _ => panic!("expected Steer"),
        }
    }

    #[test]
    fn message_with_client_id_serializes_it() {
        let message = Message {
            id: "m1".into(),
            space_id: "office-harvey".into(),
            from: "you".into(),
            kind: MessageKind::Speech,
            text: "ship it".into(),
            at: "2026-09-07T00:00:00+00:00".into(),
            artifacts: None,
            mentions: None,
            client_id: Some("c-123".into()),
        };
        let value = serde_json::to_value(&message).unwrap();
        assert_eq!(value.get("clientId").and_then(|v| v.as_str()), Some("c-123"));
    }

    #[test]
    fn client_answer_frame_parses() {
        let raw = r#"{"t":"answer","directionId":"d1","personaId":"probe-haiku","key":"1"}"#;
        let frame: ClientFrame = serde_json::from_str(raw).unwrap();
        match frame {
            ClientFrame::Answer { direction_id, persona_id, key, client_id } => {
                assert_eq!(direction_id, "d1");
                assert_eq!(persona_id, "probe-haiku");
                assert_eq!(key, AnswerKey::One);
                assert_eq!(client_id, None);
            }
            _ => panic!("expected Answer"),
        }
    }

    #[test]
    fn client_answer_frame_parses_every_key() {
        for (wire, expected) in [
            ("1", AnswerKey::One),
            ("2", AnswerKey::Two),
            ("3", AnswerKey::Three),
            ("esc", AnswerKey::Esc),
            ("y", AnswerKey::Y),
            ("n", AnswerKey::N),
        ] {
            let raw = format!(r#"{{"t":"answer","directionId":"d1","personaId":"p1","key":"{wire}"}}"#);
            let frame: ClientFrame = serde_json::from_str(&raw).unwrap();
            match frame {
                ClientFrame::Answer { key, .. } => assert_eq!(key, expected, "wire key {wire}"),
                _ => panic!("expected Answer"),
            }
        }
    }

    #[test]
    fn validate_answerable_accepts_only_blocked() {
        assert!(validate_answerable(Presence::Blocked).is_ok());
        assert!(validate_answerable(Presence::Idle).is_err());
        assert!(validate_answerable(Presence::Working).is_err());
        assert!(validate_answerable(Presence::Done).is_err());
        assert!(validate_answerable(Presence::Unknown).is_err());
    }

    #[test]
    fn strip_box_drawing_removes_the_rule_but_keeps_the_words() {
        let raw = "───────\n Do you want to proceed?\n ❯ 1. Yes\n───────";
        let stripped = strip_box_drawing(raw);
        assert!(stripped.contains("Do you want to proceed?"));
        assert!(!stripped.contains('─'));
    }

    #[test]
    fn last_lines_drops_blank_lines_and_caps_the_count() {
        let raw = "a\n\nb\nc\n\n\nd\ne";
        assert_eq!(last_lines(raw, 2), "d\ne");
        assert_eq!(last_lines(raw, 10), "a\nb\nc\nd\ne");
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
        let (new_panes, newly_blocked) = runtime.refresh_from_agents(&agents);

        assert_eq!(new_panes.len(), 3, "all three panes are new on first refresh");
        assert!(newly_blocked.is_empty(), "none of these agents start blocked");

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

        let (no_new, _) = runtime.refresh_from_agents(&agents);
        assert!(no_new.is_empty());
    }

    #[test]
    fn refresh_from_agents_reports_a_fresh_transition_into_blocked() {
        let mut runtime = HomeRuntime::new(HashMap::new());
        runtime.refresh_from_agents(&[sample_agent("w1:pA", "claude", Some("probe-haiku"), Presence::Idle)]);

        let (_, newly_blocked) =
            runtime.refresh_from_agents(&[sample_agent("w1:pA", "claude", Some("probe-haiku"), Presence::Blocked)]);
        assert_eq!(newly_blocked, vec![("probe-haiku".to_string(), "probe-haiku".to_string())]);

        // Staying blocked on a later poll is not a fresh transition -- no repeat fetch.
        let (_, again) =
            runtime.refresh_from_agents(&[sample_agent("w1:pA", "claude", Some("probe-haiku"), Presence::Blocked)]);
        assert!(again.is_empty());
    }

    #[test]
    fn pane_for_session_prefers_agent_session_id_join_over_fallback() {
        let mut runtime = HomeRuntime::new(HashMap::new());
        runtime.refresh_from_agents(&[sample_agent_with_session(
            "w1:pA",
            "claude",
            Some("harvey"),
            Presence::Idle,
            AgentSessionInfo {
                agent: "claude".into(),
                kind: AgentSessionRefKind::Id,
                source: "herdr:claude".into(),
                value: "sess-abc".into(),
            },
        )]);

        let resolved = runtime.pane_for_session("sess-abc", "/does/not/matter.jsonl", None);
        assert_eq!(resolved.as_deref(), Some("w1:pA"));

        // A fallback pane id is ignored once the agent_session join resolves -- it must never
        // override the more direct join.
        let still_preferred = runtime.pane_for_session("sess-abc", "/irrelevant", Some("w9:pZ"));
        assert_eq!(still_preferred.as_deref(), Some("w1:pA"));
    }

    #[test]
    fn pane_for_session_matches_path_kind_against_source_path() {
        let mut runtime = HomeRuntime::new(HashMap::new());
        runtime.refresh_from_agents(&[sample_agent_with_session(
            "w1:pB",
            "opencode",
            Some("opencode-1"),
            Presence::Idle,
            AgentSessionInfo {
                agent: "opencode".into(),
                kind: AgentSessionRefKind::Path,
                source: "herdr:opencode".into(),
                value: "/repo/.opencode/session-abc.db".into(),
            },
        )]);

        let resolved =
            runtime.pane_for_session("unrelated-session-id", "/repo/.opencode/session-abc.db", None);
        assert_eq!(resolved.as_deref(), Some("w1:pB"));
    }

    #[test]
    fn pane_for_session_falls_back_when_agent_session_does_not_resolve() {
        // Models Grok: no agent_session join resolves (its adapter names sessions after files
        // rather than an id/path herdr recognizes), so the caller's own `agent_state.pane_id`
        // fallback is what finds the pane.
        let mut runtime = HomeRuntime::new(HashMap::new());
        runtime.refresh_from_agents(&[sample_agent("w1:pC", "grok", Some("mike"), Presence::Idle)]);

        let resolved = runtime.pane_for_session("chat_history", "/repo/events.jsonl", Some("w1:pC"));
        assert_eq!(resolved.as_deref(), Some("w1:pC"));

        let unresolved = runtime.pane_for_session("chat_history", "/repo/events.jsonl", None);
        assert!(unresolved.is_none());
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
        let (frame, fetch) = runtime
            .apply_status_change("w1:pA", Presence::Working, Some("reviewing #196".into()))
            .expect("frame");
        assert!(fetch.is_none(), "idle -> working is not a transition into blocked");
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
    fn apply_status_change_reports_a_fetch_target_on_transition_into_blocked() {
        let mut runtime = HomeRuntime::new(HashMap::new());
        runtime.refresh_from_agents(&[sample_agent("w1:pA", "claude", Some("probe-haiku"), Presence::Idle)]);
        let (_, fetch) = runtime
            .apply_status_change("w1:pA", Presence::Blocked, None)
            .expect("frame");
        assert_eq!(fetch, Some(("probe-haiku".to_string(), "probe-haiku".to_string())));

        // Once blocked, a repeat "still blocked" observation is not a fresh transition.
        let repeat = runtime.apply_status_change("w1:pA", Presence::Blocked, None);
        assert!(repeat.is_none(), "same presence, no title change -- nothing to report");
    }

    #[test]
    fn set_blocked_prompt_and_leaving_blocked_clears_it() {
        let mut runtime = HomeRuntime::new(HashMap::new());
        runtime.refresh_from_agents(&[sample_agent("w1:pA", "claude", Some("probe-haiku"), Presence::Blocked)]);
        runtime.set_blocked_prompt("probe-haiku", "Do you want to proceed?".to_string());
        assert_eq!(
            runtime.blocked_prompts().get("probe-haiku").map(String::as_str),
            Some("Do you want to proceed?")
        );

        runtime.apply_status_change("w1:pA", Presence::Working, None);
        assert!(!runtime.blocked_prompts().contains_key("probe-haiku"), "leaving blocked clears the stored prompt");
    }

    #[test]
    fn apply_status_change_returns_none_when_nothing_actually_changed() {
        // The bug this pins down: `pane.updated` fires on every pane change (scroll, tokens,
        // title, agent status -- 1,818 of them in 25s in the live probe that found this), so
        // most of those calls must produce no frame at all, or "presence" becomes noise.
        let mut runtime = HomeRuntime::new(HashMap::new());
        runtime.refresh_from_agents(&[sample_agent("w1:pA", "claude", Some("harvey"), Presence::Idle)]);
        runtime.apply_status_change("w1:pA", Presence::Working, None).expect("first change: a frame");

        let repeat = runtime.apply_status_change("w1:pA", Presence::Working, None);
        assert!(repeat.is_none(), "same presence, no title -- nothing changed, no frame");

        let same_title = runtime.apply_status_change(
            "w1:pA",
            Presence::Working,
            Some("doing a thing".into()), // sample_agent's own default title, unchanged
        );
        assert!(same_title.is_none(), "same presence and same title -- still nothing changed");

        let (title_only, _) = runtime
            .apply_status_change("w1:pA", Presence::Working, Some("new title".into()))
            .expect("title alone changing is still a change");
        match title_only {
            ServerFrame::Presence { title, .. } => assert_eq!(title, "new title"),
            _ => panic!("expected Presence frame"),
        }
    }

    #[test]
    fn parse_subscription_event_reads_pane_agent_status_changed_flat_fields() {
        let line = r#"{"event":"pane_agent_status_changed","data":{"pane_id":"w1:pA","agent_status":"working","title":"reviewing #196"}}"#;
        let frame = parse_subscription_event(line).expect("parses");
        assert_eq!(frame.pane_id, "w1:pA");
        assert_eq!(frame.agent_status, Presence::Working);
        assert_eq!(frame.title.as_deref(), Some("reviewing #196"));
    }

    #[test]
    fn parse_subscription_event_reads_the_dotted_event_spelling_herdr_actually_sends() {
        // The bug this pins down: a second live probe on 2026-09-07 (after fixing presence to
        // read `pane_updated`) found herdr spells this event's `event` field
        // "pane.agent_status_changed" (dotted, matching the *subscription* type string) rather
        // than the underscored `EventKind` name -- 3 of them arrived in a run where the
        // underscored-only match silently dropped every one, which is what made the first
        // probe conclude this event "never fires" at all.
        let line = r#"{"event":"pane.agent_status_changed","data":{"pane_id":"w1:pA","agent_status":"working"}}"#;
        let frame = parse_subscription_event(line).expect("parses despite the dotted spelling");
        assert_eq!(frame.pane_id, "w1:pA");
        assert_eq!(frame.agent_status, Presence::Working);
    }

    #[test]
    fn parse_subscription_event_reads_pane_updated_nested_pane_info() {
        // Live probe on 2026-09-07: this is the event that actually arrives over herdr's
        // socket most often. Shape follows `herdr api schema --json`'s `PaneInfo` def: the
        // whole pane snapshot nested under `data.pane`, not flat on `data`.
        let line = r#"{"event":"pane_updated","data":{"pane":{"pane_id":"w1:pA","terminal_id":"t1","workspace_id":"w1","tab_id":"tab1","focused":true,"agent_status":"working","revision":7,"terminal_title_stripped":"reviewing #196"}}}"#;
        let frame = parse_subscription_event(line).expect("parses");
        assert_eq!(frame.pane_id, "w1:pA");
        assert_eq!(frame.agent_status, Presence::Working);
        assert_eq!(frame.title.as_deref(), Some("reviewing #196"));
    }

    #[test]
    fn parse_subscription_event_ignores_unrelated_event_types() {
        let line = r#"{"event":"pane_focused","data":{"pane_id":"w1:pA","workspace_id":"w1"}}"#;
        assert!(parse_subscription_event(line).is_none());
    }

    #[test]
    fn slugify_handles_dots_and_spaces() {
        assert_eq!(slugify("mike2-grok4.6"), "mike2-grok4-6");
        assert_eq!(slugify("w9:pA"), "w9-pa");
    }
}
