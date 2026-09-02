//! Live values for shell completion: session ids, repositories, models, and the fixed
//! lists that are not worth a `value_parser`.
//!
//! Everything here runs inside a Tab press, which is a hard constraint rather than a
//! preference. Three rules, and the tests at the bottom of this file are what keep them
//! honest:
//!
//! 1. **One bounded `SELECT`, read-only.** `SQLITE_OPEN_READ_ONLY` plus a zero busy timeout,
//!    so a scan holding the write lock makes Tab return nothing instead of hanging on it.
//! 2. **No index, no candidates.** A missing, locked, corrupt or empty database is an empty
//!    completion list, never an error and never a stall — a Tab that offers nothing is a
//!    great deal better than a Tab that blocks the shell.
//! 3. **Never the network.** Nothing here loads an adapter, a model, or an embedder.

use std::path::{Path, PathBuf};

use clap_complete::engine::CompletionCandidate;
use rusqlite::{Connection, OpenFlags};

/// Sessions offered for a session-id argument. The spec's number: enough to cover "the one I
/// was just in" without turning a Tab into a pager.
const SESSION_CANDIDATES: usize = 50;

/// Rows read when deriving repositories and models. `repo` is not a stored column and
/// `models_used` is a JSON array, so both are derived in Rust from a bounded recent slice
/// rather than by scanning every row in the index.
const DERIVED_SCAN_ROWS: usize = 400;

/// The index a completion request reads. `--db-path` is not visible to a completer (the
/// shell asks for values for one argument, not for a parsed command line), so this is always
/// the default index.
fn default_db_path() -> Option<PathBuf> {
    agentworth_storage::default_db_dir()
        .ok()
        .map(|dir| dir.join("agentworth.db"))
}

/// A read-only connection that gives up immediately rather than waiting on a lock.
fn open_read_only(db: &Path) -> Option<Connection> {
    if !db.exists() {
        return None;
    }
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    // Zero, not a small number: a Tab that waits is the failure mode this budget exists to
    // prevent, and a scan can hold the write lock for minutes.
    conn.busy_timeout(std::time::Duration::from_millis(0)).ok()?;
    Some(conn)
}

/// Newest sessions, each labelled with its repository and the start of its first prompt.
pub fn session_candidates() -> Vec<CompletionCandidate> {
    default_db_path()
        .map(|db| session_candidates_for(&db))
        .unwrap_or_default()
}

/// The body of [`session_candidates`], against a named index. Public so the budget and the
/// missing-database behaviour can be tested against a fixture.
pub fn session_candidates_for(db: &Path) -> Vec<CompletionCandidate> {
    let Some(conn) = open_read_only(db) else {
        return Vec::new();
    };
    // `idx_sessions_started_at` covers the ORDER BY, so this reads 50 rows off the index
    // rather than sorting the table.
    let sql = "SELECT session_id, source_path, prompt_preview FROM sessions \
               ORDER BY started_at DESC LIMIT ?1";
    let Ok(mut stmt) = conn.prepare(sql) else {
        return Vec::new();
    };
    let rows = stmt.query_map([SESSION_CANDIDATES], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    });
    let Ok(rows) = rows else {
        return Vec::new();
    };

    rows.filter_map(Result::ok)
        .map(|(session_id, source_path, preview)| {
            let repo = agentworth_schema::extract_repository_or_workspace(&source_path);
            let help = match preview.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
                Some(preview) => format!("{repo} — {}", agentworth_schema::text::truncate_chars(preview, 48)),
                None => repo,
            };
            CompletionCandidate::new(session_id).help(Some(help.into()))
        })
        .collect()
}

/// Distinct repositories, newest activity first.
pub fn repo_candidates() -> Vec<CompletionCandidate> {
    default_db_path()
        .map(|db| repo_candidates_for(&db))
        .unwrap_or_default()
}

pub fn repo_candidates_for(db: &Path) -> Vec<CompletionCandidate> {
    derived_column(db, "source_path", |value| {
        Some(agentworth_schema::extract_repository_or_workspace(value))
    })
}

/// Distinct models seen in the index.
pub fn model_candidates() -> Vec<CompletionCandidate> {
    default_db_path()
        .map(|db| model_candidates_for(&db))
        .unwrap_or_default()
}

pub fn model_candidates_for(db: &Path) -> Vec<CompletionCandidate> {
    let Some(conn) = open_read_only(db) else {
        return Vec::new();
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT models_used FROM sessions ORDER BY started_at DESC LIMIT ?1",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([DERIVED_SCAN_ROWS], |row| row.get::<_, String>(0)) else {
        return Vec::new();
    };

    let mut seen: Vec<String> = Vec::new();
    for raw in rows.filter_map(Result::ok) {
        let models: Vec<String> = serde_json::from_str(&raw).unwrap_or_default();
        for model in models {
            if !model.is_empty() && !seen.contains(&model) {
                seen.push(model);
            }
        }
    }
    seen.sort();
    seen.into_iter().map(CompletionCandidate::new).collect()
}

/// One bounded read of a column, mapped to a distinct sorted candidate list.
fn derived_column(
    db: &Path,
    column: &str,
    map: impl Fn(&str) -> Option<String>,
) -> Vec<CompletionCandidate> {
    let Some(conn) = open_read_only(db) else {
        return Vec::new();
    };
    // `column` is a literal from this module, never caller input -- there is no user string
    // anywhere in this statement.
    let sql = format!("SELECT {column} FROM sessions ORDER BY started_at DESC LIMIT ?1");
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([DERIVED_SCAN_ROWS], |row| row.get::<_, String>(0)) else {
        return Vec::new();
    };

    let mut seen: Vec<String> = Vec::new();
    for value in rows.filter_map(Result::ok) {
        if let Some(mapped) = map(&value) {
            if !mapped.is_empty() && !seen.contains(&mapped) {
                seen.push(mapped);
            }
        }
    }
    seen.sort();
    seen.into_iter().map(CompletionCandidate::new).collect()
}

/// The adapter registry: static, no query, and complete by construction.
pub fn adapter_candidates() -> Vec<CompletionCandidate> {
    agentworth_adapters::all_adapters()
        .iter()
        .map(|a| CompletionCandidate::new(a.name()))
        .collect()
}

fn fixed(values: &'static [(&'static str, &'static str)]) -> Vec<CompletionCandidate> {
    values
        .iter()
        .map(|(value, help)| CompletionCandidate::new(*value).help(Some((*help).into())))
        .collect()
}

/// `session search --kind`.
pub fn chunk_kind_candidates() -> Vec<CompletionCandidate> {
    fixed(&[
        ("summary", "the session's own summary turns"),
        ("error_recovery", "a failure followed by a fix"),
        ("tool_invocation", "a tool call and its result"),
        ("apology_panic", "the agent apologising or thrashing"),
        ("code_lineage", "a chunk carrying code provenance"),
    ])
}

/// `session forgotten --class`.
pub fn forgotten_class_candidates() -> Vec<CompletionCandidate> {
    fixed(&[
        ("decision", "what the session decided"),
        ("rejected", "what it considered and rejected"),
        ("reason", "why it decided as it did"),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_database_completes_to_nothing() {
        let missing = Path::new("/nonexistent/agentworth/does-not-exist.db");
        assert!(session_candidates_for(missing).is_empty());
        assert!(repo_candidates_for(missing).is_empty());
        assert!(model_candidates_for(missing).is_empty());
    }

    #[test]
    fn the_adapter_registry_completes_without_a_database() {
        assert!(!adapter_candidates().is_empty());
    }
}
