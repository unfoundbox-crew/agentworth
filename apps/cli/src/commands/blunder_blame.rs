//! Blunder-to-Blame Bridge for AgentWorth.
//!
//! Subcommand: `agwt blunder-blame [--session ID | --file PATH] [--top N] [--json]`
//!
//! AI Code Blame (`agwt blame` / `agwt pr-blame`, backed by
//! `Storage::find_sessions_for_blame`) and the Hall of Blunders (`agwt blunder`, backed
//! by `evaluate_trace_for_blunder`) are both fully built but never talk to each other,
//! even though they already share the same `session_id` and `file_path` identifiers in
//! the SQLite index. This module is the glue, in both directions:
//!
//! - **Blunder -> blame** (default, or `--session <ID>`): take a recorded blunder and
//!   resolve it forward to the exact files AI Code Blame attributes to that session.
//! - **Blame -> blunder** (`--file <PATH>`): take a file's blame history and check
//!   backward whether any of the sessions blamed for it also carry a real recorded
//!   blunder.
//!
//! No new identifiers or storage tables: this reuses `BlameMatch` and `BlunderExhibit`
//! exactly as the two existing systems already define them, joined on `session_id`
//! (see `Storage::find_files_for_session`, the reverse of `find_sessions_for_blame`).

use std::path::PathBuf;
use std::sync::Arc;

use crate::commands::blunder::{discover_blunders, evaluate_trace_for_blunder, BlunderExhibit};
use agentworth_core::Scanner;
use agentworth_storage::{extract_repository_or_workspace, BlameMatch, Storage};
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// One recorded blunder, resolved forward to the files AI Code Blame attributes to that
/// same session -- "here's a blunder, here's exactly what it touched."
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlunderBlameTrail {
    pub blunder: BlunderExhibit,
    pub blamed_files: Vec<BlameMatch>,
}

/// One session AI Code Blame attributes a file to, resolved backward to whether that
/// session's own trace also carries a real recorded blunder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileBlunderMatch {
    pub blame: BlameMatch,
    /// `None` when the session's blunder evaluation found nothing beyond the nominal
    /// "TRAJECTORY_RECEIPT" / INFO placeholder every session gets -- see `is_real_blunder`.
    pub blunder: Option<BlunderExhibit>,
}

/// A file's blame history cross-checked against recorded blunders in each blamed session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileBlunderReport {
    pub file_path: String,
    pub matches: Vec<FileBlunderMatch>,
}

/// `evaluate_trace_for_blunder` always returns `Some`, even for sessions with nothing
/// wrong -- it doubles as the ranking function `agwt blunder` sorts by, using rule_id
/// "TRAJECTORY_RECEIPT" / severity "INFO" as the do-nothing floor (see its doc comment
/// in `commands::blunder`). A bridge that reported every session as "carries a blunder"
/// would be noise, so this is the one place that draws the real line.
fn is_real_blunder(exhibit: &BlunderExhibit) -> bool {
    exhibit.severity != "INFO"
}

/// Evaluate a single session's trace for a blunder, by session ID rather than scanning
/// the whole index like `discover_blunders` does. `Ok(None)` covers both "session ID
/// isn't indexed" and "indexed, but its raw transcript file has since moved or been
/// deleted" (same graceful-degradation precedent as `pr_blame::annotate_pr_files`'s
/// spend fallback) -- either way there's nothing to evaluate. Otherwise follows
/// `evaluate_trace_for_blunder`'s own semantics: see `is_real_blunder` for what "a real
/// blunder" means versus the nominal floor every session gets.
pub fn blunder_for_session(
    storage: &Arc<Storage>,
    session_id: &str,
) -> Result<Option<BlunderExhibit>> {
    let session = match storage.get_session_by_id(session_id)? {
        Some(session) => session,
        None => return Ok(None),
    };

    let scanner = Scanner::new(storage.clone());
    let trace = match scanner.load_trace(session_id) {
        Ok(trace) => trace,
        Err(_) => return Ok(None),
    };

    let project = extract_repository_or_workspace(&session.source_path);
    Ok(evaluate_trace_for_blunder(&trace, &project))
}

/// Blunder -> blame: resolve one blunder exhibit forward to the files AI Code Blame
/// attributes to its session.
pub fn trace_blunder_to_blame(
    storage: &Arc<Storage>,
    exhibit: &BlunderExhibit,
) -> Result<BlunderBlameTrail> {
    let blamed_files = storage.find_files_for_session(&exhibit.session_id)?;
    Ok(BlunderBlameTrail {
        blunder: exhibit.clone(),
        blamed_files,
    })
}

/// Blunder -> blame, batch mode: the same top-N ranking `agwt blunder` uses, each
/// resolved forward to its blamed files.
pub fn discover_blunder_blame_trails(
    storage: &Arc<Storage>,
    top_n: usize,
) -> Result<Vec<BlunderBlameTrail>> {
    discover_blunders(storage, top_n)?
        .iter()
        .map(|exhibit| trace_blunder_to_blame(storage, exhibit))
        .collect()
}

/// Blame -> blunder: resolve a file's blame history backward to whether any of its
/// blamed sessions also carry a real recorded blunder.
pub fn trace_blame_to_blunder(storage: &Arc<Storage>, file_path: &str) -> Result<FileBlunderReport> {
    let blame_matches = storage.find_sessions_for_blame(file_path)?;
    let mut matches = Vec::with_capacity(blame_matches.len());

    for blame in blame_matches {
        let blunder = blunder_for_session(storage, &blame.session_id)?.filter(is_real_blunder);
        matches.push(FileBlunderMatch { blame, blunder });
    }

    Ok(FileBlunderReport {
        file_path: file_path.to_string(),
        matches,
    })
}

fn open_storage(db_path: Option<PathBuf>) -> Result<Arc<Storage>> {
    if let Some(path) = db_path {
        Ok(Arc::new(Storage::open_path(&path)?))
    } else {
        Ok(Arc::new(Storage::open_default()?))
    }
}

/// Execute the `agwt blunder-blame` subcommand.
#[allow(
    clippy::too_many_arguments,
    reason = "one CLI flag each (file/session/last/current/top/json/db_path) plus ui -- a \
              params struct would only move the count, not reduce it"
)]
pub fn run_blunder_blame_command(
    file: Option<String>,
    session: Option<String>,
    last: bool,
    current: bool,
    top: usize,
    json_output: bool,
    db_path: Option<PathBuf>,
    ui: &crate::ui::Ui,
) -> Result<()> {
    let storage = open_storage(db_path)?;

    // Blame -> blunder direction.
    if let Some(file_path) = file {
        let report = crate::ui::with_status(ui, "cross-checking blame history", || {
            trace_blame_to_blunder(&storage, &file_path)
        })?;
        if json_output {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            let rows: Vec<crate::ui::views::BlunderBlameFileMatchRow> = report
                .matches
                .iter()
                .map(|m| crate::ui::views::BlunderBlameFileMatchRow {
                    session_id: &m.blame.session_id,
                    model: m.blame.model.as_deref().unwrap_or("-"),
                    blunder: m.blunder.as_ref().map(|e| (e.severity.as_str(), e.title.as_str())),
                })
                .collect();
            print!("{}", crate::ui::views::blunder_blame_file_report(ui, &report.file_path, &rows));
        }
        return Ok(());
    }

    // Blunder -> blame direction, one specific session -- by id/prefix, or `--last`/
    // `--current` for the newest session in this repository.
    let session = match session {
        Some(input) => match crate::ui::picker::resolve_explicit(&storage, &input)? {
            crate::ui::picker::Resolved::Id(id) => Some(id),
            crate::ui::picker::Resolved::Ambiguous { input, candidates } => {
                crate::ui::picker::exit_ambiguous(ui, json, &input, &candidates)
            }
            crate::ui::picker::Resolved::NotFound(input) => Some(input),
        },
        None if last || current => crate::ui::picker::resolve_last(&storage)?,
        None => None,
    };
    if let Some(session_id) = session {
        let exhibit = match crate::ui::with_status(ui, "loading session", || {
            blunder_for_session(&storage, &session_id)
        })? {
            Some(exhibit) => exhibit,
            None => {
                if json_output {
                    println!("null");
                } else {
                    print!(
                        "{}",
                        crate::ui::views::error(
                            ui,
                            "agentworth blunder-blame",
                            &format!("No indexed session found matching '{}'.", session_id),
                            "",
                            &[],
                            &[("agentworth scan".to_string(), "index agent histories first".to_string())],
                        )
                    );
                }
                return Ok(());
            }
        };
        let trail = trace_blunder_to_blame(&storage, &exhibit)?;
        if json_output {
            println!("{}", serde_json::to_string_pretty(&trail)?);
        } else {
            print!("{}", crate::ui::views::blunder_blame_trails(ui, &[trail_row(&trail)]));
        }
        return Ok(());
    }

    // Default: blunder -> blame direction, batch mode over the top-N blunders.
    let top_limit = if top == 0 { 5 } else { top };
    let trails = crate::ui::with_status(ui, "scanning sessions for blunders", || {
        discover_blunder_blame_trails(&storage, top_limit)
    })?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&trails)?);
        return Ok(());
    }

    if trails.is_empty() {
        print!(
            "{}",
            crate::ui::views::error(
                ui,
                "agentworth blunder-blame",
                "No blunder exhibits found in the local index.",
                "",
                &[],
                &[("agentworth scan".to_string(), "index agent histories first".to_string())],
            )
        );
        return Ok(());
    }

    let rows: Vec<crate::ui::views::BlunderBlameTrailRow> = trails.iter().map(trail_row).collect();
    print!("{}", crate::ui::views::blunder_blame_trails(ui, &rows));
    Ok(())
}

fn trail_row(trail: &BlunderBlameTrail) -> crate::ui::views::BlunderBlameTrailRow<'_> {
    crate::ui::views::BlunderBlameTrailRow {
        severity: &trail.blunder.severity,
        title: &trail.blunder.title,
        session_id: &trail.blunder.session_id,
        model: &trail.blunder.model,
        blamed_files: trail
            .blamed_files
            .iter()
            .map(|bf| (bf.file_path.clone(), bf.action.clone()))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{AgentWorthTrace, EventPayload, NormalizedEvent, Provenance, ShellCommand};
    use chrono::Utc;

    fn leaked_var_trace(session_id: &str, files: &[(&str, agentworth_schema::FileActionType)]) -> AgentWorthTrace {
        let prov = Provenance::new(
            format!("/tmp/{}.jsonl", session_id),
            "claude_code",
            100,
            100,
            format!("fp_{}", session_id),
        );
        let start = Utc::now();
        let mut trace = AgentWorthTrace::new(session_id, "claude_code", prov, start);
        trace.stats.models_used = vec!["claude-opus-5".to_string()];

        trace.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::ShellCommand(ShellCommand {
                command: "for d in \"${PROTECTED_PATHS[@]}\"; do rm -rf \"$d\"; done".to_string(),
                cwd: Some("/Users/saurabh/code/katana".to_string()),
                exit_code: Some(0),
                output: None,
            }),
        ));

        for (i, (path, action)) in files.iter().enumerate() {
            trace.events.push(NormalizedEvent::new(
                (i + 2) as u64,
                start + chrono::Duration::seconds((i + 1) as i64),
                EventPayload::FileAction {
                    path: path.to_string(),
                    action: *action,
                    diff: None,
                    lines_changed: None,
                },
            ));
        }

        trace
    }

    fn benign_trace(session_id: &str, files: &[(&str, agentworth_schema::FileActionType)]) -> AgentWorthTrace {
        let prov = Provenance::new(
            format!("/tmp/{}.jsonl", session_id),
            "claude_code",
            100,
            100,
            format!("fp_{}", session_id),
        );
        let start = Utc::now();
        let mut trace = AgentWorthTrace::new(session_id, "claude_code", prov, start);
        trace.stats.models_used = vec!["claude-sonnet-5".to_string()];

        for (i, (path, action)) in files.iter().enumerate() {
            trace.events.push(NormalizedEvent::new(
                (i + 1) as u64,
                start + chrono::Duration::seconds((i + 1) as i64),
                EventPayload::FileAction {
                    path: path.to_string(),
                    action: *action,
                    diff: None,
                    lines_changed: None,
                },
            ));
        }

        trace
    }

    #[test]
    fn test_is_real_blunder_draws_the_info_line() {
        let mut exhibit = crate::commands::blunder::evaluate_trace_for_blunder(
            &AgentWorthTrace::new(
                "sess_benign",
                "claude_code",
                Provenance::new("/tmp/benign.jsonl", "claude_code", 10, 10, "fp"),
                Utc::now(),
            ),
            "proj",
        )
        .expect("always Some");
        assert_eq!(exhibit.severity, "INFO");
        assert!(!is_real_blunder(&exhibit));

        exhibit.severity = "CRITICAL".to_string();
        assert!(is_real_blunder(&exhibit));
    }

    /// Blunder -> blame direction: a session with a real (CRITICAL) blunder resolves
    /// forward to exactly the files AI Code Blame attributes to that session. This
    /// exercises `trace_blunder_to_blame` / `Storage::find_files_for_session` directly
    /// against a constructed fixture -- no Scanner/disk parsing involved, since
    /// `trace_blunder_to_blame` only ever needs the SQLite `file_modifications` index.
    #[test]
    fn test_trace_blunder_to_blame_finds_touched_files() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let trace = leaked_var_trace(
            "sess_blunder_1",
            &[
                ("crates/danger/src/sweep.rs", agentworth_schema::FileActionType::Write),
                ("crates/danger/src/lib.rs", agentworth_schema::FileActionType::Edit),
            ],
        );
        storage.upsert_trace(&trace).expect("upsert");

        let exhibit = evaluate_trace_for_blunder(&trace, "danger-project").expect("must detect blunder");
        assert_eq!(exhibit.rule_id, "LEAKED_SHELL_VARIABLE");
        assert!(is_real_blunder(&exhibit));

        let trail = trace_blunder_to_blame(&storage, &exhibit).expect("trace to blame");
        assert_eq!(trail.blunder.session_id, "sess_blunder_1");
        assert_eq!(trail.blamed_files.len(), 2);
        let paths: Vec<&str> = trail.blamed_files.iter().map(|f| f.file_path.as_str()).collect();
        assert!(paths.contains(&"crates/danger/src/sweep.rs"));
        assert!(paths.contains(&"crates/danger/src/lib.rs"));
        for f in &trail.blamed_files {
            assert_eq!(f.session_id, "sess_blunder_1");
            assert_eq!(f.model.as_deref(), None); // no ModelInvocation event pushed in this fixture
        }
    }

    /// Blunder -> blame direction, degenerate case: a blunder session that never
    /// recorded a FileAction still resolves cleanly to an empty (not missing/error)
    /// blamed-files list.
    #[test]
    fn test_trace_blunder_to_blame_no_files_touched() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let trace = leaked_var_trace("sess_blunder_no_files", &[]);
        storage.upsert_trace(&trace).expect("upsert");

        let exhibit = evaluate_trace_for_blunder(&trace, "proj").expect("must detect blunder");
        let trail = trace_blunder_to_blame(&storage, &exhibit).expect("trace to blame");
        assert!(trail.blamed_files.is_empty());
    }

    /// Blunder -> blame, batch mode: `discover_blunder_blame_trails` ranks the same way
    /// `agwt blunder --top` does, and each returned trail carries its blamed files.
    ///
    /// Unlike `trace_blunder_to_blame` (pure SQL, no Scanner), `discover_blunders`
    /// itself re-parses every session from disk via `Scanner::load_trace` to run
    /// blunder evaluation -- a hand-built trace with no backing file on disk (the
    /// style used by the tests above) would silently vanish from the ranking instead
    /// of failing loudly, the same graceful-skip behavior documented on
    /// `blunder_for_session`. So this test writes two real, minimal, adapter-parseable
    /// JSONL fixtures and points each trace's `provenance.source_path` at one of them;
    /// the SQL side (session row + file_modifications, via `upsert_trace`) is seeded
    /// independently of the file's own content, exactly like the real `agwt scan`
    /// pipeline keeps them in sync in production -- just done here by hand for two
    /// fixtures instead of a full scan.
    #[test]
    fn test_discover_blunder_blame_trails_ranks_and_attaches_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));

        let leaked_path = temp.path().join("sess_top_blunder.jsonl");
        std::fs::write(
            &leaked_path,
            concat!(
                r#"{"type":"user","timestamp":"2026-09-01T09:00:00Z","content":"Clean up"}"#,
                "\n",
                r#"{"type":"assistant","timestamp":"2026-09-01T09:00:02Z","model":"claude-3-5-sonnet-20241022","usage":{"input_tokens":100,"output_tokens":20,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"for d in \"${PROTECTED_PATHS[@]}\"; do rm -rf \"$d\"; done"}}]}"#,
                "\n",
            ),
        )
        .expect("write leaked fixture");

        let calm_path = temp.path().join("sess_calm.jsonl");
        std::fs::write(
            &calm_path,
            concat!(
                r#"{"type":"user","timestamp":"2026-09-01T09:00:00Z","content":"Please review the readme"}"#,
                "\n",
                r#"{"type":"assistant","timestamp":"2026-09-01T09:00:02Z","model":"claude-3-5-sonnet-20241022","usage":{"input_tokens":50,"output_tokens":10,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"The README looks fine, no changes needed."}]}"#,
                "\n",
            ),
        )
        .expect("write calm fixture");

        let mut blunder_trace = leaked_var_trace(
            "sess_top_blunder",
            &[("crates/x/src/bad.rs", agentworth_schema::FileActionType::Write)],
        );
        blunder_trace.provenance.source_path = leaked_path.to_string_lossy().to_string();
        storage.upsert_trace(&blunder_trace).expect("upsert blunder");

        let mut calm_trace = benign_trace(
            "sess_calm",
            &[("crates/x/src/fine.rs", agentworth_schema::FileActionType::Read)],
        );
        calm_trace.provenance.source_path = calm_path.to_string_lossy().to_string();
        storage.upsert_trace(&calm_trace).expect("upsert calm");

        let trails = discover_blunder_blame_trails(&storage, 5).expect("discover trails");
        assert!(!trails.is_empty());
        // The real blunder must outrank the benign trajectory-receipt session.
        assert_eq!(trails[0].blunder.session_id, "sess_top_blunder");
        assert_eq!(trails[0].blunder.rule_id, "LEAKED_SHELL_VARIABLE");
        assert_eq!(trails[0].blamed_files.len(), 1);
        assert_eq!(trails[0].blamed_files[0].file_path, "crates/x/src/bad.rs");
    }

    /// Blame -> blunder direction: a file blame-attributed to two sessions reports each
    /// session's real blunder status independently. Because `blunder_for_session` needs
    /// `Scanner::load_trace` to re-parse the original transcript from disk, and this
    /// fixture's `Provenance::source_path` intentionally doesn't exist on disk (same
    /// "constructed fixture, no real file" style as `pr_blame`'s own test), both sessions
    /// gracefully resolve to `blunder: None` here -- this test's job is to verify the
    /// blame-side fan-out and per-session shape, not blunder detection itself (already
    /// covered directly by the blunder -> blame tests above, which never call
    /// `load_trace`). The full path with a real on-disk fixture (blunder correctly
    /// detected via the actual adapter parse) is covered end-to-end by the
    /// `agwt blunder-blame --file` CLI integration test.
    #[test]
    fn test_trace_blame_to_blunder_reports_every_blamed_session() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let trace_a = benign_trace(
            "sess_file_a",
            &[("shared/src/hot.rs", agentworth_schema::FileActionType::Edit)],
        );
        let trace_b = benign_trace(
            "sess_file_b",
            &[("shared/src/hot.rs", agentworth_schema::FileActionType::Write)],
        );
        storage.upsert_trace(&trace_a).expect("upsert a");
        storage.upsert_trace(&trace_b).expect("upsert b");

        let report = trace_blame_to_blunder(&storage, "hot.rs").expect("trace to blunder");
        assert_eq!(report.file_path, "hot.rs");
        assert_eq!(report.matches.len(), 2);
        let session_ids: Vec<&str> = report.matches.iter().map(|m| m.blame.session_id.as_str()).collect();
        assert!(session_ids.contains(&"sess_file_a"));
        assert!(session_ids.contains(&"sess_file_b"));
        // Source file never existed on disk -> load_trace fails -> graceful None, not an error.
        for m in &report.matches {
            assert!(m.blunder.is_none());
        }
    }

    #[test]
    fn test_trace_blame_to_blunder_no_matches() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let report = trace_blame_to_blunder(&storage, "never_touched.rs").expect("trace to blunder");
        assert!(report.matches.is_empty());
    }

    #[test]
    fn test_blunder_for_session_unknown_session_is_none() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let result = blunder_for_session(&storage, "does_not_exist").expect("lookup");
        assert!(result.is_none());
    }
}
