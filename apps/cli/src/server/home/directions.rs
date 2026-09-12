//! Directions: the standing intents riders subscribe to (apps/home/DESIGN.md, "the Train").
//!
//! `home_directions` stores the fields a human edits (goal, area, done rung, budget, riders)
//! plus the last-materialized `state`/`exception`. `spentTokens` and `reached` are never
//! stored -- every call to [`to_wire`] recomputes them from `turn_usage` and `sessions` via
//! `agentworth_storage::Storage::home_direction_area_spend`/`home_direction_area_reached`
//! (see those functions for the exact SQL and the `agent_state.cwd` join).

use std::collections::HashMap;

use agentworth_storage::{HomeDirectionRow, Storage};
use anyhow::Result;
use chrono::Utc;

use super::protocol::{DirectionInput, Direction, DirectionState, Exception, Presence, Rung};

/// Builds the wire `Direction` for one stored row: fixed fields straight from the row, derived
/// fields (`reached`, `spentTokens`) freshly queried, `state`/`exception` from the row's last
/// materialized value (see [`recompute_state`] for how that value is produced).
pub fn to_wire(storage: &Storage, row: &HomeDirectionRow) -> Result<Direction> {
    let spend = storage.home_direction_area_spend(&row.area, row.created_at)?;
    let reached_name = storage.home_direction_area_reached(&row.area, row.created_at)?;
    let reached = reached_name.as_deref().and_then(Rung::from_storage_name);
    let done = Rung::parse_wire_str(&row.done_rung).unwrap_or(Rung::Said);

    let exception = match (&row.exception_reason, row.exception_since) {
        (Some(reason), Some(since)) => Some(Exception {
            reason: reason.clone(),
            since: since.to_rfc3339(),
        }),
        _ => None,
    };

    Ok(Direction {
        id: row.id.clone(),
        goal: row.goal.clone(),
        area: row.area.clone(),
        done,
        reached,
        budget_tokens: row.budget_tokens,
        spent_tokens: spend.tokens,
        riders: row.riders.clone(),
        state: DirectionState::parse_wire_str(&row.state).unwrap_or(DirectionState::Idle),
        exception,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
    })
}

pub fn list_wire(storage: &Storage) -> Result<Vec<Direction>> {
    storage
        .list_home_directions()?
        .iter()
        .map(|row| to_wire(storage, row))
        .collect()
}

/// Applies a `set_direction` client frame: creates the direction if `id` is new, or updates
/// its human-editable fields on an existing one. Never touches `state`/`exception` -- those
/// are the recompute loop's alone, so editing a direction can never silently clear a halt.
pub fn apply_set_direction(storage: &Storage, input: DirectionInput) -> Result<HomeDirectionRow> {
    let now = Utc::now();
    let existing = storage.get_home_direction(&input.id)?;
    let created_at = existing.as_ref().map(|r| r.created_at).unwrap_or(now);
    let row = HomeDirectionRow {
        id: input.id,
        goal: input.goal,
        area: input.area,
        done_rung: input.done.as_wire_str().to_string(),
        budget_tokens: input.budget_tokens,
        riders: input.riders,
        state: existing
            .as_ref()
            .map(|r| r.state.clone())
            .unwrap_or_else(|| DirectionState::Idle.as_wire_str().to_string()),
        exception_reason: existing.as_ref().and_then(|r| r.exception_reason.clone()),
        exception_since: existing.as_ref().and_then(|r| r.exception_since),
        created_at,
        updated_at: now,
    };
    storage.upsert_home_direction(&row)?;
    Ok(storage.get_home_direction(&row.id)?.unwrap_or(row))
}

/// The state rules (apps/home/DESIGN.md / the job brief), checked in this order -- the brief
/// lists `waiting`, `halted`, `done`, `idle`, `riding` without stating an explicit priority
/// among them, so this treats that as the priority: a rider blocked on you, or a halt/over
/// budget, is worth surfacing even over a direction that has technically reached its rung,
/// since a human still needs to look at it. Flagged as an assumption in this lane's report.
///
/// `presence_by_persona` is the live presence map from the gateway's `HomeRuntime` -- riders
/// are persona ids, and presence is not stored in SQLite, so this can't be answered from
/// `Storage` alone the way `reached`/`spentTokens` can.
///
/// `blocked_prompts` maps a blocked rider's persona id to the last ~12 lines the gateway read
/// off its pane (`herdr agent read`, box-drawing stripped) when it went blocked -- see
/// `HomeRuntime::set_blocked_prompt` in `gateway.rs`. When present for the first blocked rider,
/// it replaces the generic "waiting on X" reason with what the pane is actually asking, which is
/// what the alert plate renders verbatim.
pub fn recompute_state(
    storage: &Storage,
    row: &HomeDirectionRow,
    presence_by_persona: &HashMap<String, Presence>,
    blocked_prompts: &HashMap<String, String>,
) -> Result<(DirectionState, Option<Exception>)> {
    let done = Rung::parse_wire_str(&row.done_rung).unwrap_or(Rung::Said);
    let reached_name = storage.home_direction_area_reached(&row.area, row.created_at)?;
    let reached = reached_name.as_deref().and_then(Rung::from_storage_name);
    let spend = storage.home_direction_area_spend(&row.area, row.created_at)?;

    let blocked_riders: Vec<&String> = row
        .riders
        .iter()
        .filter(|id| presence_by_persona.get(*id) == Some(&Presence::Blocked))
        .collect();
    let any_working = row
        .riders
        .iter()
        .any(|id| presence_by_persona.get(id) == Some(&Presence::Working));

    let halt_reason = storage.home_direction_area_halt_reason(&row.area, row.created_at)?;
    let over_budget = row.budget_tokens > 0 && spend.tokens > row.budget_tokens;

    let now_iso = || Utc::now().to_rfc3339();
    let keep_or_now = |matches_prior: bool| -> String {
        if matches_prior {
            row.exception_since.map(|d| d.to_rfc3339()).unwrap_or_else(now_iso)
        } else {
            now_iso()
        }
    };

    if !blocked_riders.is_empty() {
        let reason = blocked_riders
            .first()
            .and_then(|id| blocked_prompts.get(id.as_str()))
            .cloned()
            .unwrap_or_else(|| {
                format!(
                    "waiting on {}",
                    blocked_riders
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            });
        let since = keep_or_now(row.state == "waiting");
        return Ok((
            DirectionState::Waiting,
            Some(Exception { reason, since }),
        ));
    }

    if let Some(reason) = halt_reason {
        let since = keep_or_now(row.state == "halted");
        return Ok((DirectionState::Halted, Some(Exception { reason, since })));
    }

    if over_budget {
        let reason = format!(
            "over budget: spent {} tokens against a budget of {}",
            spend.tokens, row.budget_tokens
        );
        let since = keep_or_now(row.state == "halted");
        return Ok((DirectionState::Halted, Some(Exception { reason, since })));
    }

    if reached.map(|r| r.rank() >= done.rank()).unwrap_or(false) {
        return Ok((DirectionState::Done, None));
    }

    if !any_working {
        return Ok((DirectionState::Idle, None));
    }

    Ok((DirectionState::Riding, None))
}

/// Runs `recompute_state` for every stored direction and persists+returns only the ones whose
/// state or exception actually changed, so the caller (the gateway's poll loop) broadcasts a
/// `direction` frame exactly when there's something to say.
pub fn recompute_all(
    storage: &Storage,
    presence_by_persona: &HashMap<String, Presence>,
    blocked_prompts: &HashMap<String, String>,
) -> Result<Vec<Direction>> {
    let mut changed = Vec::new();
    for row in storage.list_home_directions()? {
        let (state, exception) = recompute_state(storage, &row, presence_by_persona, blocked_prompts)?;
        let state_str = state.as_wire_str();
        let reason = exception.as_ref().map(|e| e.reason.as_str());
        let since = exception
            .as_ref()
            .map(|e| super::protocol::parse_rfc3339(&e.since));

        let prior_reason = row.exception_reason.as_deref();
        if row.state != state_str || prior_reason != reason {
            storage.set_home_direction_dynamic(&row.id, state_str, reason, since)?;
            let refreshed = storage
                .get_home_direction(&row.id)?
                .unwrap_or_else(|| row.clone());
            changed.push(to_wire(storage, &refreshed)?);
        }
    }
    Ok(changed)
}

/// Adds `persona_id` to a direction's riders, idempotently -- used when `start_rider` seats a
/// new rider on an already-existing direction. A no-op (not an error) if the persona already
/// rides it.
pub fn add_rider(storage: &Storage, direction_id: &str, persona_id: &str) -> Result<()> {
    let Some(mut row) = storage.get_home_direction(direction_id)? else {
        anyhow::bail!("no such direction: {direction_id}");
    };
    if row.riders.iter().any(|r| r == persona_id) {
        return Ok(());
    }
    row.riders.push(persona_id.to_string());
    row.updated_at = Utc::now();
    storage.upsert_home_direction(&row)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{AgentWorthTrace, Provenance};
    use agentworth_storage::{AgentStateRow, GovernorEventRow, Storage, TurnUsageRow};
    use chrono::Duration as ChronoDuration;

    /// A direction's area, one session under it, and one outside it -- so every assertion
    /// below that reads a derived field is also proving the join doesn't leak across areas.
    struct Fixture {
        storage: Storage,
        created_at: chrono::DateTime<Utc>,
    }

    fn seed_session(
        storage: &Storage,
        session_id: &str,
        cwd: &str,
        pane_id: &str,
        started_at: chrono::DateTime<Utc>,
        primary_outcome: Option<&str>,
        turns: &[(i64, i64, f64)], // (input, output, usd)
    ) {
        let provenance = Provenance::new(
            format!("/fixtures/{session_id}.jsonl"),
            "claude_code",
            100,
            0,
            format!("fp-{session_id}"),
        );
        let mut trace = AgentWorthTrace::new(session_id, "claude_code", provenance, started_at);
        trace.stats.total_events = 2;
        storage
            .upsert_session(&trace, primary_outcome, None, 1)
            .expect("upsert session");

        storage
            .upsert_agent_state(&AgentStateRow {
                session_id: session_id.to_string(),
                state: "working".to_string(),
                since: started_at,
                pane_id: Some(pane_id.to_string()),
                cwd: Some(cwd.to_string()),
                git_head: None,
                last_seq: 0,
                updated_at: started_at,
            })
            .expect("upsert agent state");

        let rows: Vec<TurnUsageRow> = turns
            .iter()
            .enumerate()
            .map(|(i, (input, output, usd))| TurnUsageRow {
                session_id: session_id.to_string(),
                seq: i as i64,
                at: started_at + ChronoDuration::minutes(i as i64),
                model: "claude-fable-5-1".to_string(),
                input: Some(*input),
                output: Some(*output),
                cache_read: Some(0),
                cache_creation: Some(0),
                usd: *usd,
            })
            .collect();
        storage.insert_turn_usage(&rows).expect("insert turn usage");
    }

    fn fixture() -> Fixture {
        let storage = Storage::open_in_memory().expect("open in-memory storage");
        let created_at = Utc::now() - ChronoDuration::hours(1);

        // In the direction's area (`/repo/proj`), started after `created_at`: counts.
        seed_session(
            &storage,
            "sess-in-area",
            "/repo/proj/sub",
            "pane-a",
            created_at + ChronoDuration::minutes(10),
            Some("artifact_changed"),
            &[(1000, 200, 0.05), (500, 100, 0.02)],
        );
        // A sibling directory that merely shares a prefix -- must NOT match `/repo/proj`.
        seed_session(
            &storage,
            "sess-sibling",
            "/repo/proj-other/sub",
            "pane-b",
            created_at + ChronoDuration::minutes(10),
            Some("ci_or_deployment_verified"),
            &[(9_999_999, 0, 500.0)],
        );

        Fixture { storage, created_at }
    }

    fn make_direction(id: &str, done_rung: &str, budget_tokens: i64, created_at: chrono::DateTime<Utc>) -> HomeDirectionRow {
        HomeDirectionRow {
            id: id.to_string(),
            goal: "ship the thing".to_string(),
            area: "/repo/proj".to_string(),
            done_rung: done_rung.to_string(),
            budget_tokens,
            riders: vec!["persona-a".to_string()],
            state: DirectionState::Idle.as_wire_str().to_string(),
            exception_reason: None,
            exception_since: None,
            created_at,
            updated_at: created_at,
        }
    }

    #[test]
    fn area_spend_joins_on_agent_state_cwd_and_excludes_siblings() {
        let fx = fixture();
        let spend = fx
            .storage
            .home_direction_area_spend("/repo/proj", fx.created_at)
            .expect("area spend");
        // (1000+200) + (500+100) = 1800, matching the two turns on sess-in-area only.
        assert_eq!(spend.tokens, 1800);
        assert!((spend.usd - 0.07).abs() < 1e-9);
        assert_eq!(spend.turns, 2, "the sibling session's turn must not be counted");
    }

    #[test]
    fn area_reached_is_the_highest_rung_among_matching_sessions() {
        let fx = fixture();
        let reached = fx
            .storage
            .home_direction_area_reached("/repo/proj", fx.created_at)
            .expect("area reached");
        assert_eq!(reached.as_deref(), Some("artifact_changed"), "the sibling's ci-rung session is out of area");
    }

    #[test]
    fn to_wire_derives_spent_and_reached_never_reading_them_from_the_row() {
        let fx = fixture();
        let row = make_direction("d1", "test", 10_000, fx.created_at);
        fx.storage.upsert_home_direction(&row).expect("upsert direction");

        let direction = to_wire(&fx.storage, &row).expect("to_wire");
        assert_eq!(direction.spent_tokens, 1800);
        assert_eq!(direction.reached, Some(Rung::Artifact));
        assert_eq!(direction.done, Rung::Test);
    }

    #[test]
    fn recompute_state_is_riding_when_a_rider_is_working_and_under_budget() {
        let fx = fixture();
        let row = make_direction("d1", "ci", 10_000, fx.created_at);
        let mut presence = HashMap::new();
        presence.insert("persona-a".to_string(), Presence::Working);

        let (state, exception) = recompute_state(&fx.storage, &row, &presence, &HashMap::new()).expect("recompute");
        assert_eq!(state, DirectionState::Riding);
        assert!(exception.is_none());
    }

    #[test]
    fn recompute_state_is_idle_when_no_rider_is_working() {
        let fx = fixture();
        let row = make_direction("d1", "ci", 10_000, fx.created_at);
        let mut presence = HashMap::new();
        presence.insert("persona-a".to_string(), Presence::Idle);

        let (state, _) = recompute_state(&fx.storage, &row, &presence, &HashMap::new()).expect("recompute");
        assert_eq!(state, DirectionState::Idle);
    }

    #[test]
    fn recompute_state_is_waiting_when_a_rider_is_blocked() {
        let fx = fixture();
        let row = make_direction("d1", "ci", 10_000, fx.created_at);
        let mut presence = HashMap::new();
        presence.insert("persona-a".to_string(), Presence::Blocked);

        let (state, exception) = recompute_state(&fx.storage, &row, &presence, &HashMap::new()).expect("recompute");
        assert_eq!(state, DirectionState::Waiting);
        assert_eq!(exception.unwrap().reason, "waiting on persona-a");
    }

    #[test]
    fn recompute_state_prefers_the_blocked_pane_prompt_over_the_generic_reason() {
        let fx = fixture();
        let row = make_direction("d1", "ci", 10_000, fx.created_at);
        let mut presence = HashMap::new();
        presence.insert("persona-a".to_string(), Presence::Blocked);
        let mut blocked_prompts = HashMap::new();
        blocked_prompts.insert(
            "persona-a".to_string(),
            "Do you want to proceed?\n1. Yes\n2. Yes, and don't ask again for: archie session *\n3. No".to_string(),
        );

        let (state, exception) = recompute_state(&fx.storage, &row, &presence, &blocked_prompts).expect("recompute");
        assert_eq!(state, DirectionState::Waiting);
        assert!(exception.unwrap().reason.contains("Do you want to proceed?"));
    }

    #[test]
    fn recompute_state_is_halted_over_budget() {
        let fx = fixture();
        // Spent is 1800; a budget of 100 puts it well over.
        let row = make_direction("d1", "ci", 100, fx.created_at);
        let mut presence = HashMap::new();
        presence.insert("persona-a".to_string(), Presence::Working);

        let (state, exception) = recompute_state(&fx.storage, &row, &presence, &HashMap::new()).expect("recompute");
        assert_eq!(state, DirectionState::Halted);
        assert!(exception.unwrap().reason.contains("over budget"));
    }

    #[test]
    fn recompute_state_is_halted_on_a_governor_thrash_halt_in_the_area() {
        let fx = fixture();
        let row = make_direction("d1", "ci", 10_000, fx.created_at);
        fx.storage
            .insert_governor_event(&GovernorEventRow {
                id: None,
                session_id: "sess-in-area".to_string(),
                at: fx.created_at + ChronoDuration::minutes(20),
                seq: Some(4),
                rule: "thrash".to_string(),
                action: "halt".to_string(),
                reason: "src/lib.rs has been edited 3 times".to_string(),
                evidence: None,
            })
            .expect("insert governor event");
        let mut presence = HashMap::new();
        presence.insert("persona-a".to_string(), Presence::Working);

        let (state, exception) = recompute_state(&fx.storage, &row, &presence, &HashMap::new()).expect("recompute");
        assert_eq!(state, DirectionState::Halted);
        assert_eq!(exception.unwrap().reason, "src/lib.rs has been edited 3 times");
    }

    #[test]
    fn recompute_state_is_done_when_reached_meets_the_done_rung() {
        let fx = fixture();
        let row = make_direction("d1", "artifact", 10_000, fx.created_at);
        let mut presence = HashMap::new();
        presence.insert("persona-a".to_string(), Presence::Idle);

        let (state, _) = recompute_state(&fx.storage, &row, &presence, &HashMap::new()).expect("recompute");
        assert_eq!(state, DirectionState::Done, "artifact_changed reached >= an artifact-rung goal");
    }

    #[test]
    fn recompute_all_persists_and_returns_only_changed_directions() {
        let fx = fixture();
        let row = make_direction("d1", "ci", 10_000, fx.created_at);
        fx.storage.upsert_home_direction(&row).expect("upsert direction");
        let mut presence = HashMap::new();
        presence.insert("persona-a".to_string(), Presence::Working);

        let changed = recompute_all(&fx.storage, &presence, &HashMap::new()).expect("recompute_all");
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].state, DirectionState::Riding);

        let stored = fx.storage.get_home_direction("d1").expect("get").unwrap();
        assert_eq!(stored.state, "riding");

        // A second pass with the same presence sees no change and returns nothing.
        let again = recompute_all(&fx.storage, &presence, &HashMap::new()).expect("recompute_all again");
        assert!(again.is_empty());
    }

    #[test]
    fn apply_set_direction_creates_then_updates_without_touching_state() {
        let fx = fixture();
        let input = DirectionInput {
            id: "d1".to_string(),
            goal: "ship it".to_string(),
            area: "/repo/proj".to_string(),
            done: Rung::Ci,
            budget_tokens: 5_000,
            riders: vec!["persona-a".to_string()],
        };
        let created = apply_set_direction(&fx.storage, input).expect("create");
        assert_eq!(created.state, "idle");
        let created_at = created.created_at;

        // Simulate the recompute loop having moved it to "riding" before a second edit.
        fx.storage
            .set_home_direction_dynamic("d1", "riding", None, None)
            .expect("set dynamic");

        let edit = DirectionInput {
            id: "d1".to_string(),
            goal: "ship it faster".to_string(),
            area: "/repo/proj".to_string(),
            done: Rung::Ci,
            budget_tokens: 8_000,
            riders: vec!["persona-a".to_string()],
        };
        let updated = apply_set_direction(&fx.storage, edit).expect("edit");
        assert_eq!(updated.goal, "ship it faster");
        assert_eq!(updated.budget_tokens, 8_000);
        assert_eq!(updated.created_at, created_at, "created_at is stamped once and kept");
        assert_eq!(updated.state, "riding", "editing fields never resets the materialized state");
    }

    #[test]
    fn add_rider_appends_once_and_is_idempotent() {
        let fx = fixture();
        let input = DirectionInput {
            id: "d1".to_string(),
            goal: "ship it".to_string(),
            area: "/repo/proj".to_string(),
            done: Rung::Test,
            budget_tokens: 5_000,
            riders: vec!["persona-a".to_string()],
        };
        apply_set_direction(&fx.storage, input).expect("create");

        add_rider(&fx.storage, "d1", "persona-b").expect("add new rider");
        let row = fx.storage.get_home_direction("d1").expect("get").expect("row");
        assert_eq!(row.riders, vec!["persona-a", "persona-b"]);

        add_rider(&fx.storage, "d1", "persona-b").expect("add again, idempotent");
        let row = fx.storage.get_home_direction("d1").expect("get").expect("row");
        assert_eq!(row.riders, vec!["persona-a", "persona-b"], "adding the same rider twice does not duplicate it");
    }

    #[test]
    fn add_rider_errors_on_unknown_direction() {
        let fx = fixture();
        let err = add_rider(&fx.storage, "no-such-direction", "persona-a").unwrap_err();
        assert!(err.to_string().contains("no-such-direction"));
    }
}
