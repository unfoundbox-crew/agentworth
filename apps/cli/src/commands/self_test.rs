//! `archie doctor --self-test`: runs Saurabh's real release workflow, in order,
//! against the real index and real sources on this machine, with no network, and times
//! every step.
//!
//! Every step below invokes the *actual installed binary* (`std::env::current_exe()`)
//! exactly the way a person types it -- `archie scan --json`, `archie stats
//! --json`, and so on -- rather than calling the underlying library functions directly.
//! That's deliberate: this is a release smoke test standing in for a person clicking
//! through the whole workflow by hand, so it should break the same way a person's
//! afternoon would if the binary itself were broken, not just if some internal function
//! regressed.
//!
//! Never print transcript content here. Every receipt below is built from a session id
//! already known to the caller, a row count, or one or two named numeric fields pulled
//! out of a step's own `--json` output -- never that output's own text/content fields.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use agentworth_storage::{SessionFilter, SessionOrderBy, Storage};
use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::ui::truncate;
use crate::ui::views::{self_test as render_self_test, SelfTestStatus, SelfTestStepView};
use crate::ui::Ui;

/// `scan` is the one step allowed to take real time -- a machine with ~10k sessions.
const SCAN_BUDGET: Duration = Duration::from_secs(60);
/// `stats` loads and scores every indexed session to build its verdict breakdown.
const STATS_BUDGET: Duration = Duration::from_secs(2);
/// Everything else: a handful of rows, or one already-resolved session.
const DEFAULT_BUDGET: Duration = Duration::from_secs(1);

/// How many of the newest sessions to scan, in Rust, looking for one with a compaction
/// round. Bounded rather than exhaustive: a machine with tens of thousands of sessions
/// should not have `doctor --self-test` itself become the slow step of the workflow it's
/// timing.
const COMPACTED_SESSION_SEARCH_WINDOW: usize = 2000;

/// Confines the `scan` step to one root, for the test suite.
///
/// The real command's `scan` step takes no path, which is the point: it is a release smoke
/// test of the whole workflow, and the workflow starts by discovering this machine's own
/// agent directories. That made `apps/cli/tests/doctor_self_test.rs` unrunnable on a
/// developer machine: the test hands the binary a fixture `--db-path`, but the self-test's
/// own `scan` then indexed the developer's entire real `$HOME` history into that fixture db,
/// so the `forgotten` step found a genuinely compacted session and passed where the test
/// asserts `skip`. Found on lenovo.
///
/// Set this to a directory and the `scan` step scans only that. Unset -- which is every real
/// invocation -- and nothing changes.
const SCAN_ROOT_ENV: &str = "AGENTWORTH_SELF_TEST_SCAN_ROOT";

struct Step {
    name: &'static str,
    status: SelfTestStatus,
    elapsed: Duration,
    receipt: String,
}

fn open_storage(db_path: Option<&Path>) -> Result<Storage> {
    match db_path {
        Some(p) => Storage::open_path(p),
        None => Storage::open_default(),
    }
    .context("opening the local SQLite index")
}

/// Output of spawning one subcommand of the running binary.
struct CliOutput {
    elapsed: Duration,
    success: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    /// Set only when the process could not even be launched (binary missing, exec bit
    /// stripped, etc.) -- distinct from the subcommand itself exiting non-zero.
    launch_error: Option<String>,
}

fn run_cli(exe: &Path, db_path: Option<&Path>, args: &[&str]) -> CliOutput {
    let mut cmd = Command::new(exe);
    if let Some(p) = db_path {
        cmd.arg("--db-path").arg(p);
    }
    cmd.args(args);
    cmd.stdin(Stdio::null());
    let start = Instant::now();
    match cmd.output() {
        Ok(out) => CliOutput {
            elapsed: start.elapsed(),
            success: out.status.success(),
            stdout: out.stdout,
            stderr: out.stderr,
            launch_error: None,
        },
        Err(e) => CliOutput {
            elapsed: start.elapsed(),
            success: false,
            stdout: Vec::new(),
            stderr: Vec::new(),
            launch_error: Some(e.to_string()),
        },
    }
}

fn status_for(elapsed: Duration, budget: Duration) -> SelfTestStatus {
    if elapsed > budget {
        SelfTestStatus::Slow
    } else {
        SelfTestStatus::Pass
    }
}

/// Runs `args` as a step of the workflow, requires it to exit 0 and print valid JSON on
/// stdout, and hands that parsed JSON to `receipt` to build the one-line summary. Never
/// echoes the subcommand's own stdout -- only what `receipt` chooses to extract from it.
fn run_step(
    exe: &Path,
    db_path: Option<&Path>,
    name: &'static str,
    args: &[&str],
    budget: Duration,
    receipt: impl FnOnce(&Value) -> String,
) -> Step {
    let out = run_cli(exe, db_path, args);
    if let Some(err) = out.launch_error {
        return Step {
            name,
            status: SelfTestStatus::Fail,
            elapsed: out.elapsed,
            receipt: format!("could not launch `agentworth {}`: {err}", args.join(" ")),
        };
    }
    if !out.success {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Step {
            name,
            status: SelfTestStatus::Fail,
            elapsed: out.elapsed,
            receipt: format!("exited with a failure: {}", truncate(stderr.trim(), 160)),
        };
    }
    match serde_json::from_slice::<Value>(&out.stdout) {
        Ok(v) => Step {
            name,
            status: status_for(out.elapsed, budget),
            elapsed: out.elapsed,
            receipt: receipt(&v),
        },
        Err(e) => Step {
            name,
            status: SelfTestStatus::Fail,
            elapsed: out.elapsed,
            receipt: format!("--json output did not parse: {e}"),
        },
    }
}

fn skip(name: &'static str, receipt: impl Into<String>) -> Step {
    Step {
        name,
        status: SelfTestStatus::Skip,
        elapsed: Duration::ZERO,
        receipt: receipt.into(),
    }
}

fn array_len_receipt(noun: &str) -> impl Fn(&Value) -> String + '_ {
    move |v: &Value| format!("{} {noun}", v.as_array().map(Vec::len).unwrap_or(0))
}

/// Whether `session asks` is registered on the running binary. `--help` on a
/// real subcommand always exits 0; clap exits non-zero on an unrecognized one. `asks`
/// landed on main in #97; this check means a binary built from a commit before that
/// still skips the step gracefully instead of failing on a missing subcommand, and any
/// future removal or rename degrades the same way rather than as a hard failure.
fn asks_subcommand_exists(exe: &Path) -> bool {
    Command::new(exe)
        .args(["session", "asks", "--help"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Spawns `archie mcp` as a real child process on stdio and drives it as a genuine
/// MCP client would: `tools/list`, then `session_show` (capped at 500 events) against
/// `session_id` if one was resolved. Returns a one-line receipt; never the tool results
/// themselves.
async fn mcp_roundtrip(exe: &Path, db_path: Option<&Path>, session_id: Option<&str>) -> Result<String> {
    use rmcp::model::CallToolRequestParams;
    use rmcp::transport::TokioChildProcess;
    use rmcp::ServiceExt;

    let mut cmd = tokio::process::Command::new(exe);
    cmd.arg("mcp");
    if let Some(p) = db_path {
        cmd.arg("--db-path").arg(p);
    }
    let transport = TokioChildProcess::new(cmd).context("spawning `archie mcp`")?;
    let client = ()
        .serve(transport)
        .await
        .context("MCP initialize handshake with `archie mcp` failed")?;

    let tools = client
        .list_tools(Default::default())
        .await
        .context("tools/list failed")?;
    let mut receipt = format!("tools/list: {} tools", tools.tools.len());

    if let Some(sid) = session_id {
        let call_result = client
            .call_tool(CallToolRequestParams::new("session_show").with_arguments(
                json!({ "session_id": sid, "events_limit": 500 })
                    .as_object()
                    .unwrap()
                    .clone(),
            ))
            .await
            .context("tools/call session_show failed")?;
        if call_result.is_error == Some(true) {
            anyhow::bail!("session_show returned an error result");
        }
        let text = call_result
            .content
            .first()
            .and_then(|c| c.as_text())
            .map(|t| t.text.clone())
            .context("session_show result carried no text content block")?;
        let _: Value =
            serde_json::from_str(&text).context("session_show result did not parse as JSON")?;
        receipt.push_str(&format!(", session_show(session={sid}): parsed ok"));
    } else {
        receipt.push_str(", session_show: skipped (no session indexed)");
    }

    let _ = client.cancel().await;
    Ok(receipt)
}

pub fn run_self_test_command(json_output: bool, db_path: Option<PathBuf>, ui: &Ui) -> Result<()> {
    let exe = std::env::current_exe().context("resolving the running agentworth binary")?;
    let db_path_ref = db_path.as_deref();
    let overall_start = Instant::now();
    let mut steps: Vec<Step> = Vec::new();

    let scan_root = std::env::var(SCAN_ROOT_ENV).ok();
    let scan_args: Vec<&str> = match scan_root.as_deref() {
        Some(root) => vec!["scan", root, "--json"],
        None => vec!["scan", "--json"],
    };
    steps.push(run_step(
        &exe,
        db_path_ref,
        "scan",
        &scan_args,
        SCAN_BUDGET,
        |v| {
            let scanned = v.get("scanned_sessions").and_then(Value::as_u64).unwrap_or(0);
            let indexed = v
                .get("total_indexed_sessions")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let errors = v
                .get("errors_encountered")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            format!("{scanned} scanned, {indexed} indexed total, {errors} errors")
        },
    ));

    steps.push(run_step(
        &exe,
        db_path_ref,
        "stats",
        &["stats", "--json"],
        STATS_BUDGET,
        |v| {
            let sessions = v.get("total_sessions").and_then(Value::as_u64).unwrap_or(0);
            let events = v.get("total_events").and_then(Value::as_u64).unwrap_or(0);
            format!("{sessions} sessions, {events} events")
        },
    ));

    steps.push(run_step(
        &exe,
        db_path_ref,
        "stats usage --period week",
        &["stats", "usage", "--period", "week", "--json"],
        DEFAULT_BUDGET,
        array_len_receipt("rows"),
    ));

    steps.push(run_step(
        &exe,
        db_path_ref,
        "stats ladder --period month",
        &["stats", "ladder", "--period", "month", "--json"],
        DEFAULT_BUDGET,
        |v| {
            let sessions = v.get("total_sessions").and_then(Value::as_u64).unwrap_or(0);
            let below = v
                .get("below_line_cost_share")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let groups = v.get("groups").and_then(Value::as_array).map_or(0, Vec::len);
            format!("{sessions} sessions, {groups} groups, {:.0}% of spend below the line", below * 100.0)
        },
    ));

    steps.push(run_step(
        &exe,
        db_path_ref,
        "session list --limit 5",
        &["session", "list", "--limit", "5", "--json"],
        DEFAULT_BUDGET,
        array_len_receipt("rows"),
    ));

    // One storage open, reused to pick the sessions `inspect`, `handoff`, and `forgotten`
    // below each need -- rather than re-deriving a session id from each subcommand's own
    // JSON, which would mean parsing (and risk echoing) more of that output than the
    // one or two fields each receipt actually needs.
    let storage = open_storage(db_path_ref);
    let newest_session_id: Option<String> = storage
        .as_ref()
        .ok()
        .and_then(|s| {
            s.list_sessions_filtered(&SessionFilter {
                limit: Some(1),
                order_by: Some(SessionOrderBy::StartedAtDesc),
                ..Default::default()
            })
            .ok()
        })
        .and_then(|mut rows| rows.pop())
        .map(|s| s.session_id);

    let newest_compacted_session_id: Option<String> = storage
        .as_ref()
        .ok()
        .and_then(|s| {
            s.list_sessions_filtered(&SessionFilter {
                limit: Some(COMPACTED_SESSION_SEARCH_WINDOW),
                order_by: Some(SessionOrderBy::StartedAtDesc),
                ..Default::default()
            })
            .ok()
        })
        .and_then(|rows| rows.into_iter().find(|s| s.compaction_count > 0))
        .map(|s| s.session_id);

    match &newest_session_id {
        Some(id) => {
            let id_owned = id.clone();
            steps.push(run_step(
                &exe,
                db_path_ref,
                "session show <newest non-stub session>",
                &["session", "show", id_owned.as_str(), "--json"],
                DEFAULT_BUDGET,
                |v| {
                    let events = v
                        .get("stats")
                        .and_then(|s| s.get("total_events"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    format!("session {id_owned}, {events} events")
                },
            ));
        }
        None => steps.push(skip(
            "session show <newest non-stub session>",
            "no non-stub session indexed yet",
        )),
    }

    match &newest_session_id {
        Some(_) => steps.push(run_step(
            &exe,
            db_path_ref,
            "session handoff --last",
            &["session", "handoff", "--last", "--json"],
            DEFAULT_BUDGET,
            |v| {
                let id = v
                    .get("receipt")
                    .and_then(|r| r.get("session_id"))
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                let events = v.get("total_events").and_then(Value::as_u64).unwrap_or(0);
                format!("session {id}, {events} events")
            },
        )),
        None => steps.push(skip("session handoff --last", "no session indexed yet")),
    }

    match &newest_compacted_session_id {
        Some(id) => {
            let id_owned = id.clone();
            steps.push(run_step(
                &exe,
                db_path_ref,
                "session forgotten <newest compacted session>",
                &["session", "forgotten", id_owned.as_str(), "--json"],
                DEFAULT_BUDGET,
                |v| {
                    let compactions = v.get("compactions").and_then(Value::as_u64).unwrap_or(0);
                    let forgotten = v.get("forgotten_total").and_then(Value::as_u64).unwrap_or(0);
                    format!("session {id_owned}, {compactions} compactions, {forgotten} forgotten statements")
                },
            ));
        }
        None => steps.push(skip(
            "session forgotten <newest compacted session>",
            "no session has compactions yet",
        )),
    }

    if asks_subcommand_exists(&exe) {
        steps.push(run_step(
            &exe,
            db_path_ref,
            "session asks --current",
            &["session", "asks", "--current", "--json"],
            DEFAULT_BUDGET,
            |v| {
                let id = v
                    .get("receipt")
                    .and_then(|r| r.get("session_id"))
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                let total = v.get("total_questions").and_then(Value::as_u64).unwrap_or(0);
                let returned = v.get("returned").and_then(Value::as_u64).unwrap_or(0);
                format!("session {id}, {total} questions, {returned} returned")
            },
        ));
    } else {
        steps.push(skip("session asks --current", "not on main yet"));
    }

    {
        let mcp_start = Instant::now();
        let result = tokio::runtime::Runtime::new()
            .context("starting a tokio runtime for the MCP round trip")
            .and_then(|rt| rt.block_on(mcp_roundtrip(&exe, db_path_ref, newest_session_id.as_deref())));
        let elapsed = mcp_start.elapsed();
        steps.push(match result {
            Ok(receipt) => Step {
                name: "mcp round trip",
                status: status_for(elapsed, DEFAULT_BUDGET),
                elapsed,
                receipt,
            },
            Err(e) => Step {
                name: "mcp round trip",
                status: SelfTestStatus::Fail,
                elapsed,
                receipt: e.chain().map(|c| c.to_string()).collect::<Vec<_>>().join(": "),
            },
        });
    }

    let total_elapsed = overall_start.elapsed();
    let ok = !steps.iter().any(|s| s.status == SelfTestStatus::Fail);

    if json_output {
        let report = json!({
            "ok": ok,
            "version": env!("CARGO_PKG_VERSION"),
            "total_ms": total_elapsed.as_millis() as u64,
            "steps": steps.iter().map(|s| json!({
                "name": s.name,
                "status": match s.status {
                    SelfTestStatus::Pass => "pass",
                    SelfTestStatus::Slow => "slow",
                    SelfTestStatus::Fail => "fail",
                    SelfTestStatus::Skip => "skip",
                },
                "elapsed_ms": s.elapsed.as_millis() as u64,
                "receipt": s.receipt,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        let views: Vec<SelfTestStepView> = steps
            .iter()
            .map(|s| SelfTestStepView {
                name: s.name.to_string(),
                status: s.status,
                elapsed_ms: s.elapsed.as_millis(),
                receipt: s.receipt.clone(),
            })
            .collect();
        print!(
            "{}",
            render_self_test(ui, env!("CARGO_PKG_VERSION"), &views, total_elapsed.as_millis(), ok)
        );
    }

    if !ok {
        std::process::exit(1);
    }
    Ok(())
}
