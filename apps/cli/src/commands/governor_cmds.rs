//! The governor's verbs: `archie session burn` and the four `archie policy` ones
//! (docs/specs/governor.md).
//!
//! Every one of these reads. The gate writes rows; nothing here does, except `policy lift`,
//! which is a person clearing a suspension they were told about. Each has a builder returning
//! JSON and a renderer over it, so the CLI and the `session_burn` MCP tool cannot answer
//! differently.
//!
//! The number this file must never print is a "remaining". AgentWorth's count is its own count
//! at the pricing table's rates; the provider's quota is the provider's, and it appears only
//! when the transcript carries the provider's own line, labelled as theirs.

use std::path::PathBuf;
use std::sync::Arc;

use agentworth_loop::{EditLedger, Governor, Policy, SessionSpend};
use agentworth_storage::Storage;
use anyhow::{anyhow, Result};
use chrono::{Duration, Utc};
use std::collections::BTreeMap;
use serde_json::{json, Value};

use crate::loop_governor::{repo_policy_file, turn_from_row};
use crate::ui::Ui;

/// The window `burn` reports a rate over. Long enough that one slow turn does not dominate it,
/// short enough that it describes now rather than the session.
const RATE_WINDOW_MINUTES: i64 = 10;

/// What one session has spent, from `turn_usage`. The CLI reads rows and nothing else: a
/// running `archie serve` may be a few turns ahead of what is stored, which is why the
/// rendered line says as of when.
pub fn session_burn_json(storage: &Storage, session_id: &str) -> Result<Value> {
    let spend = storage.session_spend(session_id)?;
    let recent = storage.turn_usage_for_session(
        session_id,
        Some(Utc::now() - Duration::minutes(RATE_WINDOW_MINUTES)),
    )?;
    let recent_tokens: i64 = recent
        .iter()
        .map(|row| {
            row.input.unwrap_or(0)
                + row.output.unwrap_or(0)
                + row.cache_read.unwrap_or(0)
                + row.cache_creation.unwrap_or(0)
        })
        .sum();
    let tokens_per_minute = recent_tokens as f64 / RATE_WINDOW_MINUTES as f64;

    // The provider's own accounting, and only when the harness wrote it down. Codex does in
    // its rollout log; Claude Code does not, and an absent line stays absent rather than
    // becoming a number we derived.
    let provider = storage
        .get_transcript_cursor(session_id)
        .ok()
        .flatten()
        .map(|(path, _)| path)
        .and_then(|path| provider_limit(&path));

    Ok(json!({
        "session_id": session_id,
        "tokens": spend.tokens,
        "usd": spend.usd,
        "turns": spend.turns,
        "cache_read_share": spend.cache_read_share,
        "tokens_per_minute": tokens_per_minute,
        "rate_window_minutes": RATE_WINDOW_MINUTES,
        "first_at": spend.first_at,
        "last_at": spend.last_at,
        "provider_limit": provider,
        "note": "AgentWorth's own count at the pricing table's rates, as of the last serve update",
    }))
}

/// Re-reads a Codex rollout for the provider's newest rate-limit line. A Claude Code transcript
/// has none and yields `None`, which is the honest answer rather than a computed one.
fn provider_limit(transcript_path: &str) -> Option<Value> {
    let mut tail = agentworth_loop::TranscriptTail::new(transcript_path);
    tail.read_new(&crate::loop_governor::rates_for).ok()?;
    let limit = tail.latest_limit()?;
    Some(json!({
        "used_percent": limit.used_percent,
        "window_minutes": limit.window_minutes,
        "resets_at": limit.resets_at,
        "plan_type": limit.plan_type,
        "spend_control_reached": limit.spend_control_reached,
        "secondary": tail.secondary_limit().map(|w| json!({
            "used_percent": w.used_percent,
            "window_minutes": w.window_minutes,
            "resets_at": w.resets_at,
        })),
    }))
}

fn open_storage(db_path: Option<PathBuf>) -> Result<Arc<Storage>> {
    match db_path {
        Some(path) => Ok(Arc::new(Storage::open_path(&path)?)),
        None => Ok(Arc::new(Storage::open_default()?)),
    }
}

fn window_name(minutes: Option<i64>) -> String {
    match minutes {
        Some(m) if m % 10080 == 0 => "weekly".to_string(),
        Some(m) if m % 1440 == 0 => format!("{}-day", m / 1440),
        Some(m) if m % 60 == 0 => format!("{}-hour", m / 60),
        Some(m) => format!("{m}-minute"),
        None => "unnamed".to_string(),
    }
}

pub fn run_session_burn_command(
    session_id: Option<String>,
    json_out: bool,
    db_path: Option<PathBuf>,
    _ui: &Ui,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let session = crate::commands::loop_cmds::resolve_loop_session(&storage, session_id.as_deref())?;
    let value = session_burn_json(&storage, &session)?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    let tokens = value["tokens"].as_i64().unwrap_or(0);
    if tokens == 0 && value["turns"].as_i64().unwrap_or(0) == 0 {
        println!(
            "nothing metered for {session}: the meter runs only when a policy.toml exists \
             (`archie policy show`)"
        );
        return Ok(());
    }
    println!(
        "{tokens} tokens · ${:.2} · {} turns · {:.0}% cache reads · {:.0} tokens/min over the \
         last {RATE_WINDOW_MINUTES} minutes — as of last serve update",
        value["usd"].as_f64().unwrap_or(0.0),
        value["turns"],
        value["cache_read_share"].as_f64().unwrap_or(0.0) * 100.0,
        value["tokens_per_minute"].as_f64().unwrap_or(0.0),
    );
    if let Some(limit) = value["provider_limit"].as_object() {
        let resets = limit["resets_at"].as_str().unwrap_or("an unstated time");
        println!(
            "provider says: {:.0}% of the {} window used, resets {resets}",
            limit["used_percent"].as_f64().unwrap_or(0.0),
            window_name(limit["window_minutes"].as_i64()),
        );
    }
    Ok(())
}

/// The two files that decide whether anything is governed, and what they say.
pub fn policy_show_json(cwd: Option<&str>) -> Result<Value> {
    let home = agentworth_storage::default_db_dir()?.join("policy.toml");
    let repo = repo_policy_file(cwd);
    let policy = Policy::load(&home, &repo)?;
    Ok(json!({
        "home_file": home,
        "home_file_present": home.exists(),
        "repo_file": repo,
        "repo_file_present": repo.exists(),
        "governed": !policy.is_empty(),
        "rules": policy.describe(),
        "policy": policy,
    }))
}

pub fn run_policy_show_command(json_out: bool, _ui: &Ui) -> Result<()> {
    let cwd = std::env::current_dir().ok().map(|c| c.to_string_lossy().to_string());
    let value = policy_show_json(cwd.as_deref())?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    for key in ["home_file", "repo_file"] {
        let present = value[format!("{key}_present").as_str()].as_bool().unwrap_or(false);
        println!(
            "{}: {}",
            value[key].as_str().unwrap_or("?"),
            if present { "read" } else { "not there" }
        );
    }
    for rule in value["rules"].as_array().cloned().unwrap_or_default() {
        println!("{}", rule.as_str().unwrap_or_default());
    }
    Ok(())
}

/// Parses both files and says what is wrong with them, naming the file and the line. A
/// `[spend]` section with neither cap is the one shape that parses and means nothing.
pub fn run_policy_check_command(_ui: &Ui) -> Result<()> {
    let cwd = std::env::current_dir().ok().map(|c| c.to_string_lossy().to_string());
    let home = agentworth_storage::default_db_dir()?.join("policy.toml");
    let repo = repo_policy_file(cwd.as_deref());
    let policy = Policy::load(&home, &repo)?;
    if let Some(spend) = policy.spend {
        if spend.tokens.is_none() && spend.usd.is_none() {
            return Err(anyhow!(
                "[spend] sets neither `tokens` nor `usd`, so it caps nothing. Write one of them, \
                 or delete the section"
            ));
        }
    }
    if policy.is_empty() {
        println!("nothing is governed: no rule in either file");
        return Ok(());
    }
    println!("both files parse · {} rule(s) in force", policy.describe().len());
    Ok(())
}

pub fn run_policy_lift_command(session_id: String, db_path: Option<PathBuf>, _ui: &Ui) -> Result<()> {
    let storage = open_storage(db_path)?;
    let active = storage.active_suspension(&session_id)?;
    if !storage.lift_session(&session_id)? {
        return Err(anyhow!(
            "nothing to lift: {session_id} is not suspended. `archie session burn {session_id}` \
             says what it has spent"
        ));
    }
    match active {
        Some(row) => println!(
            "lifted {session_id}: {} (suspended since {})",
            row.reason,
            row.since.format("%Y-%m-%d %H:%M")
        ),
        None => println!("lifted {session_id}"),
    }
    Ok(())
}

/// Replays the current policy over the sessions the index already holds: how often each rule
/// would trip, and how many tokens each session went on to spend after its first trip. A fresh
/// ledger per session -- a threshold is a per-session question, and carrying one session's
/// edits into the next makes every number after the first one wrong. Read-only, and a tuning
/// tool rather than the product.
pub fn policy_replay_json(storage: &Storage, since_days: i64, cwd: Option<&str>) -> Result<Value> {
    let since = Utc::now() - Duration::days(since_days.max(0));
    let home = agentworth_storage::default_db_dir()?.join("policy.toml");
    let policy = Policy::load(&home, &repo_policy_file(cwd))?;

    let mut trips_by_rule: BTreeMap<String, i64> = BTreeMap::new();
    let mut tokens_after = 0u64;
    let mut sessions = Vec::new();
    for state in storage.list_agent_states(1_000)? {
        if state.updated_at < since {
            continue;
        }
        let mut ledger = EditLedger::new();
        let mut first_trip: Option<u64> = None;
        let mut tripped: BTreeMap<String, i64> = BTreeMap::new();
        let turns: Vec<_> = storage
            .turn_usage_for_session(&state.session_id, None)?
            .iter()
            .map(turn_from_row)
            .collect();
        for intent in storage.intents_for_session(&state.session_id)? {
            let seq = intent.seq.max(0) as u64;
            match intent.command.as_deref() {
                Some(command) if agentworth_loop::is_verification_command(command) => {
                    match intent.result.as_deref() {
                        Some("ok") => ledger.record_verification(None, true, seq, command, ""),
                        Some("error") => ledger.record_verification(None, false, seq, command, ""),
                        _ => continue,
                    }
                }
                _ => {
                    for path in &intent.predicted_paths {
                        ledger.record_edit(path, seq);
                    }
                }
            }
            let spend_so_far = SessionSpend::from_turns(
                &turns
                    .iter()
                    .filter(|turn| turn.seq <= seq)
                    .cloned()
                    .collect::<Vec<_>>(),
            );
            for decision in Governor::evaluate(&policy, &ledger, &spend_so_far, 0, None) {
                *tripped.entry(decision.rule.as_str().to_string()).or_insert(0) += 1;
                first_trip.get_or_insert(seq);
            }
        }
        if tripped.is_empty() {
            continue;
        }
        let after: u64 = turns
            .iter()
            .filter(|turn| Some(turn.seq) > first_trip)
            .map(|turn| turn.tokens())
            .sum();
        tokens_after += after;
        for (rule, count) in &tripped {
            *trips_by_rule.entry(rule.clone()).or_insert(0) += count;
        }
        sessions.push(json!({
            "session_id": state.session_id,
            "first_trip_seq": first_trip,
            "trips": tripped,
            "tokens_after": after,
        }));
    }
    Ok(json!({
        "since_days": since_days,
        "governed": !policy.is_empty(),
        "rules": policy.describe(),
        "sessions": sessions,
        "trips_by_rule": trips_by_rule,
        "tokens_after_first_trip": tokens_after,
    }))
}

pub fn run_policy_replay_command(
    since_days: i64,
    json_out: bool,
    db_path: Option<PathBuf>,
    _ui: &Ui,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let cwd = std::env::current_dir().ok().map(|c| c.to_string_lossy().to_string());
    let value = policy_replay_json(&storage, since_days, cwd.as_deref())?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    let trips = value["trips_by_rule"].as_object().cloned().unwrap_or_default();
    if trips.is_empty() {
        println!("no rule has tripped in the last {since_days} days");
        return Ok(());
    }
    for (rule, count) in &trips {
        println!("{rule}: {count} trip(s)");
    }
    println!(
        "{} session(s) tripped a rule · {} tokens spent after the first trip",
        value["sessions"].as_array().map(Vec::len).unwrap_or(0),
        value["tokens_after_first_trip"],
    );
    Ok(())
}
