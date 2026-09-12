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
    json_out: bool,
    db_path: Option<PathBuf>,
    _ui: &Ui,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let session = resolve_loop_session(&storage, session_id.as_deref())?;
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
