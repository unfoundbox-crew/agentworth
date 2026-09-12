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
use agentworth_storage::{extract_repository_or_workspace, Percentiles, Storage};
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
pub fn run_policy_check_command(db_path: Option<PathBuf>, _ui: &Ui) -> Result<()> {
    let cwd = std::env::current_dir().ok().map(|c| c.to_string_lossy().to_string());
    let home = agentworth_storage::default_db_dir()?.join("policy.toml");
    let repo_file = repo_policy_file(cwd.as_deref());
    let policy = Policy::load(&home, &repo_file)?;
    if let Some(spend) = policy.spend {
        if spend.tokens.is_none() && spend.usd.is_none() {
            return Err(anyhow!(
                "[spend] sets neither `tokens` nor `usd`, so it caps nothing. Write one of them, \
                 or delete the section"
            ));
        }
        if let Some(cap) = spend.tokens {
            let storage = open_storage(db_path)?;
            let repo_name = cwd.as_deref().map(extract_repository_or_workspace);
            for warning in spend_cap_warnings(&storage, cap, repo_name.as_deref())? {
                println!("warning: {warning}");
            }
        }
    }
    if policy.is_empty() {
        println!("nothing is governed: no rule in either file");
        return Ok(());
    }
    println!("both files parse · {} rule(s) in force", policy.describe().len());
    Ok(())
}

/// One line per population (machine-wide, and repo-wide when a repo file exists) where `cap`
/// sits below that population's own p99 -- the exact shape of a cap that would halt sessions
/// this machine has already run, not a hypothetical one.
fn spend_cap_warnings(storage: &Storage, cap: u64, repo: Option<&str>) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    let machine = storage.session_token_percentiles(None)?;
    if machine.n > 0 && cap < machine.p99 {
        let tripped = storage.count_primary_sessions_over_tokens(cap, None)?;
        warnings.push(format!(
            "[spend] tokens = {cap} is below this machine's own p99 of {} across {} primary \
             sessions ({tripped} would have tripped it)",
            machine.p99, machine.n,
        ));
    }
    if let Some(repo) = repo {
        let repo_stats = storage.session_token_percentiles(Some(repo))?;
        if repo_stats.n > 0 && cap < repo_stats.p99 {
            let tripped = storage.count_primary_sessions_over_tokens(cap, Some(repo))?;
            warnings.push(format!(
                "[spend] tokens = {cap} is below {repo}'s own p99 of {} across {} primary \
                 sessions ({tripped} would have tripped it)",
                repo_stats.p99, repo_stats.n,
            ));
        }
    }
    Ok(warnings)
}

/// Writes a starter `policy.toml` with the thrash and loop rules on, and a `[spend]` block
/// left commented out -- carrying this machine's own token percentiles as comments, so the
/// number a person eventually uncomments is theirs, not a guess (AGENTS.md, "Dogfood before
/// you propose": a cap picked without looking at this machine's own sessions is how a 20M
/// example would have halted a 728M primary session).
pub fn run_policy_init_command(
    repo: bool,
    force: bool,
    db_path: Option<PathBuf>,
    _ui: &Ui,
) -> Result<()> {
    let cwd = std::env::current_dir().ok().map(|c| c.to_string_lossy().to_string());
    let target = if repo {
        let file = repo_policy_file(cwd.as_deref());
        if file.starts_with("/nonexistent") {
            return Err(anyhow!(
                "--repo needs a git repository; run this from inside one"
            ));
        }
        file
    } else {
        agentworth_storage::default_db_dir()?.join("policy.toml")
    };
    if target.exists() && !force {
        return Err(anyhow!(
            "{} already exists; pass --force to overwrite it",
            target.display()
        ));
    }

    let storage = open_storage(db_path)?;
    let machine = storage.session_token_percentiles(None)?;
    let repo_name = cwd.as_deref().map(extract_repository_or_workspace);
    let repo_stats = repo_name
        .as_deref()
        .map(|r| storage.session_token_percentiles(Some(r)))
        .transpose()?;

    let contents = render_policy_init_toml(repo_name.as_deref(), &machine, repo_stats.as_ref());
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&target, &contents)?;
    println!("wrote {}", target.display());
    println!(
        "[thrash] and [loop] are on now. [spend] is commented out -- read the numbers in the \
         file, then uncomment and set your own cap."
    );
    Ok(())
}

fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M tokens", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K tokens", n as f64 / 1_000.0)
    } else {
        format!("{n} tokens")
    }
}

fn percentile_comment_lines(label: &str, p: &Percentiles) -> String {
    if p.n == 0 {
        return format!("# {label}: no indexed primary sessions yet");
    }
    format!(
        "# {label} (n={}): p50 = {}, p90 = {}, p99 = {}, max = {}",
        p.n,
        fmt_tokens(p.p50),
        fmt_tokens(p.p90),
        fmt_tokens(p.p99),
        fmt_tokens(p.max),
    )
}

fn render_policy_init_toml(
    repo_name: Option<&str>,
    machine: &Percentiles,
    repo_stats: Option<&Percentiles>,
) -> String {
    let mut out = String::new();
    out.push_str("[thrash]\nedits = 3\naction = \"halt\"\n\n");
    out.push_str("[loop]\nrepeats = 3\naction = \"note\"\n\n");
    out.push_str("# [spend] tokens = ... usd = ... action = \"halt\"\n");
    out.push_str(
        "# These numbers are this machine's own primary-session token totals, read from the \
         index\n# by `archie policy init` (docs/specs/governor.md, AGENTS.md \"Dogfood before \
         you propose\"):\n",
    );
    out.push_str(&percentile_comment_lines("machine-wide, primary sessions", machine));
    out.push('\n');
    if let (Some(repo), Some(stats)) = (repo_name, repo_stats) {
        out.push_str(&percentile_comment_lines(&format!("this repo ({repo})"), stats));
        out.push('\n');
    }
    out.push_str("# a cap below your p99 halts your own long sessions\n");
    out
}

pub fn run_policy_lift_command(session_id: String, db_path: Option<PathBuf>, _ui: &Ui) -> Result<()> {
    let storage = open_storage(db_path)?;
    let active = storage.active_suspension(&session_id)?;
    // What the session has spent right now becomes the baseline the cap is measured from, so
    // the lift buys another cap's worth of work rather than a single prompt.
    let spend = storage.session_spend(&session_id)?;
    if !storage.lift_session(&session_id, spend.tokens, spend.usd)? {
        return Err(anyhow!(
            "nothing to lift: {session_id} is not suspended. `archie session burn {session_id}` \
             says what it has spent"
        ));
    }
    match active {
        Some(row) => println!(
            "lifted {session_id} at {} tokens: {} (suspended since {})",
            spend.tokens,
            row.reason,
            row.since.format("%Y-%m-%d %H:%M")
        ),
        None => println!("lifted {session_id} at {} tokens", spend.tokens),
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

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{AgentWorthTrace, Provenance, TokenUsage};
    use agentworth_storage::Percentiles;
    use chrono::Utc;

    fn seed(storage: &Storage, path: &str, id: &str, tokens: u64) {
        let prov = Provenance::new(path, "claude_code", 100, 100, format!("fp-{id}"));
        let mut trace = AgentWorthTrace::new(id, "claude_code", prov, Utc::now());
        trace.stats.token_usage = TokenUsage::new(tokens, 0, 0, 0);
        trace.stats.total_events = 5;
        storage.upsert_trace(&trace).expect("upsert");
    }

    #[test]
    fn render_policy_init_toml_carries_thrash_and_loop_on_and_spend_commented_with_percentiles() {
        let machine = Percentiles { n: 4, p50: 100, p90: 300, p99: 390, max: 400 };
        let out = render_policy_init_toml(None, &machine, None);

        assert!(out.contains("[thrash]"));
        assert!(out.contains("action = \"halt\""));
        assert!(out.contains("[loop]"));
        assert!(out.contains("# [spend]"), "spend must ship commented out, not a live cap");
        assert!(!out.lines().any(|l| l.trim_start().starts_with("[spend]")));
        assert!(out.contains("p99 = 390"));
        assert!(out.contains("n=4"));
    }

    #[test]
    fn render_policy_init_toml_adds_a_second_block_for_the_repo() {
        let machine = Percentiles { n: 10, p50: 100, p90: 900, p99: 990, max: 1000 };
        let repo_stats = Percentiles { n: 3, p50: 50, p90: 90, p99: 99, max: 100 };
        let out = render_policy_init_toml(Some("unfoundbox/agentworth"), &machine, Some(&repo_stats));

        assert!(out.contains("unfoundbox/agentworth"));
        assert!(out.contains("n=10"));
        assert!(out.contains("n=3"));
    }

    #[test]
    fn spend_cap_warnings_fires_when_cap_is_below_machine_p99() {
        let storage = Storage::open_in_memory().expect("open storage");
        for (i, tokens) in [10u64, 20, 30, 728_000_000].into_iter().enumerate() {
            seed(
                &storage,
                &format!("/Users/dev/.claude/projects/-Users-dev-code-unfoundbox-agentworth/s{i}.jsonl"),
                &format!("sess_{i}"),
                tokens,
            );
        }

        let warnings = spend_cap_warnings(&storage, 20_000_000, None).expect("warnings");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("20000000"));
        assert!(warnings[0].contains("1 would have tripped it"));
    }

    #[test]
    fn spend_cap_warnings_is_silent_when_cap_is_above_p99() {
        let storage = Storage::open_in_memory().expect("open storage");
        for (i, tokens) in [10u64, 20, 30].into_iter().enumerate() {
            seed(
                &storage,
                &format!("/Users/dev/.claude/projects/-Users-dev-code-unfoundbox-agentworth/s{i}.jsonl"),
                &format!("sess_{i}"),
                tokens,
            );
        }

        let warnings = spend_cap_warnings(&storage, 20_000_000, None).expect("warnings");
        assert!(warnings.is_empty());
    }
}
