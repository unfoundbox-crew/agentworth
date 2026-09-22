//! The three loop verbs: `archie agent status`, `archie session drift`, `archie session
//! anchors` (docs/specs/loop.md section 3).
//!
//! Each has one builder returning JSON and one renderer over it, so the CLI and the
//! `agent_status` / `session_drift` MCP tools cannot answer differently. Nothing here scans,
//! and nothing here writes: drift re-hashes files on disk and reads rows, and that is all.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agentworth_loop::{drift, SupportEntry, Writer};
use agentworth_storage::Storage;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::ui::Ui;

/// How many live sessions `agent status` lists. More than the number of panes anyone has open,
/// and the table is sorted newest-first anyway.
const STATUS_LIMIT: usize = 50;

/// What to say when the index has never seen a hook.
pub const NO_AGENTS: &str =
    "no agent has reported in; `archie hook print claude` shows how to register the hook";

fn short(id: &str) -> &str {
    agentworth_schema::text::truncate_chars(id, 8)
}

fn short_hash(hash: Option<&String>) -> String {
    match hash {
        Some(hash) => agentworth_schema::text::truncate_chars(hash, 12).to_string(),
        None => "unhashed".to_string(),
    }
}

/// The live loop states, newest first.
pub fn agent_status_json(storage: &Storage) -> Result<Value> {
    let rows = storage.list_agent_states(STATUS_LIMIT)?;
    let sessions: Vec<Value> = rows
        .iter()
        .map(|row| {
            let repo = row
                .cwd
                .as_deref()
                .map(agentworth_schema::extract_repository_or_workspace);
            json!({
                "session_id": row.session_id,
                "session_short": short(&row.session_id),
                "state": row.state,
                "since": row.since,
                "updated_at": row.updated_at,
                "repo": repo,
                "cwd": row.cwd,
                "pane_id": row.pane_id,
                "git_head": row.git_head,
                "last_seq": row.last_seq,
            })
        })
        .collect();
    Ok(json!({ "sessions": sessions, "count": rows.len() }))
}

/// What moved under this session since it read it.
pub fn session_drift_json(storage: &Storage, session_id: &str) -> Result<Value> {
    let rows = storage.support_for_session(session_id)?;
    let unhashed = rows.iter().filter(|row| row.sha256.is_none()).count();
    let entries: Vec<SupportEntry> = rows
        .iter()
        .map(|row| SupportEntry {
            path: PathBuf::from(&row.path),
            sha256: row.sha256.clone(),
            size: row.size.unwrap_or(0) as u64,
            read_seq: row.read_seq as u64,
            read_at: row.read_at,
        })
        .collect();

    // Keyed by the moment this session read the path: a writer only counts if it wrote after
    // the read, and `seq` cannot answer that across two sessions (see
    // `Storage::writers_of_path_since`).
    let read_at_of: BTreeMap<&Path, DateTime<Utc>> = rows
        .iter()
        .map(|row| (Path::new(row.path.as_str()), row.read_at))
        .collect();
    let writers = |path: &Path| -> Option<Writer> {
        let since = read_at_of
            .get(path)
            .copied()
            .unwrap_or(DateTime::<Utc>::MIN_UTC);
        storage
            .writers_of_path_since(&path.to_string_lossy(), since, Some(session_id))
            .ok()?
            .into_iter()
            .next()
            .map(|(session, seq)| Writer {
                session_id: session,
                seq: seq as u64,
            })
    };

    let drifted = drift(&entries, &writers);
    let items: Vec<Value> = drifted
        .iter()
        .map(|item| {
            json!({
                "path": item.path,
                "then": item.then,
                "now": item.now,
                "then_short": short_hash(item.then.as_ref()),
                "now_short": short_hash(item.now.as_ref()),
                "writer": item.writer.as_ref().map(|w| json!({
                    "session_id": w.session_id,
                    "session_short": short(&w.session_id),
                    "seq": w.seq,
                })),
            })
        })
        .collect();

    Ok(json!({
        "session_id": session_id,
        "entries_checked": entries.len(),
        "unhashed": unhashed,
        "drift": items,
    }))
}

/// The join keys this session's own trace carried, and who else carried them.
pub fn session_anchors_json(storage: &Storage, session_id: &str) -> Result<Value> {
    let rows = storage.anchors_for_session(session_id)?;
    let mut by_kind: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for row in &rows {
        let shared: Vec<Value> = storage
            .sessions_for_anchor(&row.kind, &row.value)?
            .into_iter()
            .filter(|(other, _)| other != session_id)
            .map(|(other, seq)| {
                json!({"session_id": other, "session_short": short(&other), "seq": seq})
            })
            .collect();
        by_kind.entry(row.kind.clone()).or_default().push(json!({
            "value": row.value,
            "value_short": agentworth_schema::text::truncate_chars(&row.value, 16),
            "seq": row.seq,
            "shared_with": shared,
        }));
    }
    Ok(json!({
        "session_id": session_id,
        "count": rows.len(),
        "kinds": by_kind,
    }))
}

/// Resolves what the caller typed to one session the loop knows about.
///
/// Deliberately not `crate::ui::picker`: a session that has only ever spoken through hooks has
/// no row in `sessions` yet, so the picker would refuse an id the loop can answer for.
pub fn resolve_loop_session(storage: &Storage, session_id: Option<&str>) -> Result<String> {
    let rows = storage.list_agent_states(STATUS_LIMIT)?;
    match session_id {
        Some(wanted) => {
            if rows.iter().any(|row| row.session_id == wanted) {
                return Ok(wanted.to_string());
            }
            let matches: Vec<&str> = rows
                .iter()
                .map(|row| row.session_id.as_str())
                .filter(|id| id.starts_with(wanted))
                .collect();
            match matches.as_slice() {
                [one] => Ok((*one).to_string()),
                [] => Err(anyhow!(
                    "no session in the loop index starts with {wanted}. `archie agent status` lists them"
                )),
                many => Err(anyhow!(
                    "{wanted} matches {} sessions; give more of the id",
                    many.len()
                )),
            }
        }
        // Newest by `updated_at`, which is what `--last` means for a live index.
        None => rows
            .first()
            .map(|row| row.session_id.clone())
            .ok_or_else(|| anyhow!("{NO_AGENTS}")),
    }
}

/// Environment variables a harness sets to name the session it is running, in the order we
/// trust them. One entry today; the list exists so a second harness is an addition here
/// rather than a restructure. Only variables whose exact name has been verified belong in it.
const SELF_SESSION_ENV_VARS: &[&str] = &["CLAUDE_CODE_SESSION_ID"];

/// The session id the harness named for this process, if any.
fn self_session_from_env() -> Option<String> {
    SELF_SESSION_ENV_VARS.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

/// Resolves the session for a verb whose contract is *this* session -- `session burn`,
/// `session drift` -- rather than "the newest one on the machine".
///
/// `resolve_loop_session`'s `None` arm answers with the newest row by `updated_at`, which on a
/// machine running several agents at once is whichever unrelated session last fired a hook.
/// Correct for `session anchors`, whose `--help` promises exactly that; wrong for a verb whose
/// help says "this session", which is how three consecutive self-lookups returned three
/// different ids, none of them the caller.
///
/// So: an explicit id behaves exactly as before, and otherwise the harness's own variable
/// wins when it names a session the loop index knows. Failing both, we fall back to
/// newest-by-`updated_at` -- archie run outside a harness, or before this session's first hook
/// event has landed, must keep answering as it does today rather than hard-failing.
pub fn resolve_self_session(storage: &Storage, session_id: Option<&str>) -> Result<String> {
    resolve_self_session_with(storage, session_id, self_session_from_env().as_deref())
}

/// `resolve_self_session` with the harness variable passed in, so tests can exercise both
/// arms without mutating the process environment under a threaded test runner.
pub(crate) fn resolve_self_session_with(
    storage: &Storage,
    session_id: Option<&str>,
    env_session: Option<&str>,
) -> Result<String> {
    if session_id.is_some() {
        return resolve_loop_session(storage, session_id);
    }
    if let Some(wanted) = env_session {
        let rows = storage.list_agent_states(STATUS_LIMIT)?;
        if let Some(row) = rows.iter().find(|row| row.session_id == wanted) {
            return Ok(row.session_id.clone());
        }
        // Prefix tolerance, for a harness that exports a shortened id.
        let matches: Vec<&str> = rows
            .iter()
            .map(|row| row.session_id.as_str())
            .filter(|id| id.starts_with(wanted))
            .collect();
        if let [one] = matches.as_slice() {
            return Ok((*one).to_string());
        }
    }
    resolve_loop_session(storage, None)
}

fn open_storage(db_path: Option<PathBuf>) -> Result<Arc<Storage>> {
    match db_path {
        Some(path) => Ok(Arc::new(Storage::open_path(&path)?)),
        None => Ok(Arc::new(Storage::open_default()?)),
    }
}

pub fn run_agent_status_command(json_out: bool, db_path: Option<PathBuf>, _ui: &Ui) -> Result<()> {
    let storage = open_storage(db_path)?;
    let value = agent_status_json(&storage)?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    let sessions = value["sessions"].as_array().cloned().unwrap_or_default();
    if sessions.is_empty() {
        println!("{NO_AGENTS}");
        return Ok(());
    }
    for session in &sessions {
        let since = session["since"]
            .as_str()
            .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
            .map(|at| at.with_timezone(&Utc).format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let mut parts = vec![
            session["state"].as_str().unwrap_or("?").to_string(),
            format!("since {since}"),
            session["session_short"].as_str().unwrap_or("?").to_string(),
        ];
        if let Some(repo) = session["repo"].as_str() {
            parts.push(repo.to_string());
        }
        if let Some(pane) = session["pane_id"].as_str() {
            parts.push(format!("pane {pane}"));
        }
        println!("{}", parts.join(" · "));
    }
    Ok(())
}

pub fn run_session_drift_command(
    session_id: Option<String>,
    newest: bool,
    json_out: bool,
    db_path: Option<PathBuf>,
    _ui: &Ui,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    // `--last` asks for the machine's newest session by name, so it keeps the old resolver;
    // the bare default means "me", which is what `resolve_self_session` answers.
    let session = if newest {
        resolve_loop_session(&storage, None)?
    } else {
        resolve_self_session(&storage, session_id.as_deref())?
    };
    let value = session_drift_json(&storage, &session)?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    let items = value["drift"].as_array().cloned().unwrap_or_default();
    let checked = value["entries_checked"].as_u64().unwrap_or(0);
    let unhashed = value["unhashed"].as_u64().unwrap_or(0);
    if items.is_empty() {
        println!(
            "nothing drifted · {checked} entr{} checked · {unhashed} unhashed (over the size cap)",
            if checked == 1 { "y" } else { "ies" }
        );
        return Ok(());
    }
    for item in &items {
        let writer = match item["writer"].as_object() {
            Some(writer) => format!(
                "{} at seq {}",
                writer["session_short"].as_str().unwrap_or("?"),
                writer["seq"]
            ),
            None => "not an agent on this machine".to_string(),
        };
        println!(
            "{} · {}→{} · {}",
            item["path"].as_str().unwrap_or("?"),
            item["then_short"].as_str().unwrap_or("?"),
            item["now_short"].as_str().unwrap_or("?"),
            writer
        );
    }
    println!("{} of {checked} entries drifted · {unhashed} unhashed", items.len());
    Ok(())
}

pub fn run_session_anchors_command(
    session_id: Option<String>,
    json_out: bool,
    db_path: Option<PathBuf>,
    _ui: &Ui,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let session = resolve_loop_session(&storage, session_id.as_deref())?;
    let value = session_anchors_json(&storage, &session)?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    let kinds = value["kinds"].as_object().cloned().unwrap_or_default();
    if kinds.is_empty() {
        println!("no anchors recorded for {}", short(&session));
        return Ok(());
    }
    for (kind, anchors) in &kinds {
        println!("{kind}");
        for anchor in anchors.as_array().cloned().unwrap_or_default() {
            let shared = anchor["shared_with"].as_array().cloned().unwrap_or_default();
            let also = if shared.is_empty() {
                "this session only".to_string()
            } else {
                format!(
                    "also {}",
                    shared
                        .iter()
                        .map(|s| s["session_short"].as_str().unwrap_or("?").to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            println!(
                "  {} · seq {} · {}",
                anchor["value"].as_str().unwrap_or("?"),
                anchor["seq"],
                also
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_storage::AgentStateRow;
    use chrono::Duration;

    const OLDER: &str = "aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa";
    const NEWER: &str = "bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb";

    /// Two sessions, `NEWER` more recently active than `OLDER` -- the concurrent-fleet shape
    /// that made the bug: an unrelated session fires a hook and becomes "the newest".
    fn two_sessions() -> Storage {
        let storage = Storage::open_in_memory().expect("open storage");
        let now = Utc::now();
        for (id, ago) in [(OLDER, 600i64), (NEWER, 5)] {
            storage
                .upsert_agent_state(&AgentStateRow {
                    session_id: id.to_string(),
                    state: "idle".to_string(),
                    since: now - Duration::seconds(ago),
                    pane_id: None,
                    cwd: Some("/tmp/repo".to_string()),
                    git_head: None,
                    last_seq: 1,
                    updated_at: now - Duration::seconds(ago),
                })
                .expect("upsert agent state");
        }
        storage
    }

    #[test]
    fn the_harness_variable_beats_a_newer_unrelated_session() {
        let storage = two_sessions();
        assert_eq!(
            resolve_self_session_with(&storage, None, Some(OLDER)).expect("resolve"),
            OLDER,
            "the caller's own id must win over a session that merely reported in later"
        );
    }

    #[test]
    fn the_harness_variable_is_honoured_as_a_prefix_too() {
        let storage = two_sessions();
        assert_eq!(
            resolve_self_session_with(&storage, None, Some(&OLDER[..8])).expect("resolve"),
            OLDER
        );
    }

    #[test]
    fn no_harness_variable_falls_back_to_newest_by_updated_at() {
        let storage = two_sessions();
        assert_eq!(
            resolve_self_session_with(&storage, None, None).expect("resolve"),
            NEWER,
            "with nothing naming the caller, today's behaviour has to survive unchanged"
        );
    }

    #[test]
    fn a_harness_variable_the_index_has_never_seen_falls_back_rather_than_failing() {
        let storage = two_sessions();
        assert_eq!(
            resolve_self_session_with(&storage, None, Some("cccccccc-3333-4333-8333-cccccccccccc"))
                .expect("resolve"),
            NEWER,
            "archie before this session's first hook lands must still answer, not error"
        );
    }

    #[test]
    fn an_explicit_id_still_wins_over_the_harness_variable() {
        let storage = two_sessions();
        assert_eq!(
            resolve_self_session_with(&storage, Some(NEWER), Some(OLDER)).expect("resolve"),
            NEWER
        );
        // And the explicit arm keeps `resolve_loop_session`'s errors word for word.
        let err = resolve_self_session_with(&storage, Some("zz"), Some(OLDER))
            .expect_err("no session starts with zz");
        assert!(err
            .to_string()
            .contains("no session in the loop index starts with zz"));
    }

    #[test]
    fn the_real_env_var_is_wired_through() {
        let storage = two_sessions();
        std::env::set_var("CLAUDE_CODE_SESSION_ID", OLDER);
        let got = resolve_self_session(&storage, None);
        std::env::remove_var("CLAUDE_CODE_SESSION_ID");
        assert_eq!(got.expect("resolve"), OLDER);
    }

    /// `anchors` keeps the old resolver, and the old resolver keeps its old answer: the
    /// newest session, whoever it belongs to. That is `anchors`'s documented contract, not
    /// the bug.
    #[test]
    fn anchors_resolver_still_answers_newest_first() {
        let storage = two_sessions();
        assert_eq!(resolve_loop_session(&storage, None).expect("resolve"), NEWER);
    }
}
