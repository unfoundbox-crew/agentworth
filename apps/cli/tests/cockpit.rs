//! The cockpit's contract, from outside the binary.
//!
//! Three things this file holds that no unit test can: the non-TTY path really prints the
//! overview and exits 0, the spec's one-rendering-path rule really holds across the source
//! tree, and opening really is bounded rather than a scan.
//!
//! The key handling itself is unit-tested on the state machine in
//! `apps/cli/src/ui/cockpit.rs`, with no terminal involved.

use assert_cmd::Command;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};
use tempfile::{tempdir, TempDir};

/// Opening the cockpit must not read the index the way a scan does. Generous against a
/// fixture, and it includes process start -- the point is to catch a first screen that
/// walks every session, not to time this machine.
const OPEN_BUDGET: Duration = Duration::from_millis(agentworth_cli::ui::cockpit::OPEN_BUDGET_MS as u64);

/// Enough sessions that an unbounded query would show up as time, built the cheap way: one
/// small transcript each, scanned once through the real `scan` command.
fn fixture(sessions: usize) -> (TempDir, std::path::PathBuf) {
    let temp = tempdir().unwrap();
    let dir = temp.path().join(".claude").join("projects").join("proj");
    fs::create_dir_all(&dir).unwrap();

    for n in 0..sessions {
        let mut f = File::create(dir.join(format!("session_{n:05}.jsonl"))).unwrap();
        writeln!(f, r#"{{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"fix the vector index"}}"#).unwrap();
        writeln!(f, r#"{{"type":"assistant","timestamp":"2026-08-29T10:00:02Z","model":"claude-3-5-sonnet-20241022","usage":{{"input_tokens":5000,"output_tokens":1200,"cache_read_input_tokens":9000,"cache_creation_input_tokens":400}},"content":[{{"type":"tool_use","id":"t1","name":"FileEdit","input":{{"file_path":"crates/storage/src/vector.rs","diff":"+ fn cosine() {{}}"}}}}]}}"#).unwrap();
        writeln!(f, r#"{{"type":"tool_result","timestamp":"2026-08-29T10:00:04Z","tool_use_id":"t1","content":"File modified successfully","is_error":false}}"#).unwrap();
        writeln!(f, r#"{{"type":"assistant","timestamp":"2026-08-29T10:00:05Z","content":[{{"type":"tool_use","id":"t2","name":"Bash","input":{{"command":"cargo test"}}}}],"usage":{{"input_tokens":200,"output_tokens":30,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"tool_result","timestamp":"2026-08-29T10:00:07Z","tool_use_id":"t2","content":"test result: ok. 8 passed; 0 failed","is_error":false}}"#).unwrap();
    }

    let db = temp.path().join("index.db");
    Command::cargo_bin("agentworth")
        .unwrap()
        .arg("--db-path")
        .arg(&db)
        .arg("scan")
        .arg(temp.path())
        .arg("--json")
        .assert()
        .success();

    (temp, db)
}

fn run(db: &Path, args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    cmd.arg("--db-path").arg(db);
    for a in args {
        cmd.arg(a);
    }
    cmd.env("COLUMNS", "80")
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR_FORCE")
        .env_remove("COLORTERM");
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

// -----------------------------------------------------------------------------
// the non-TTY path
// -----------------------------------------------------------------------------

/// The spec's rule for a bare `archie`: off a terminal it prints the overview and exits 0.
/// The test harness is never a TTY, so this is the path every script and every CI job gets.
#[test]
fn a_bare_archie_off_a_terminal_prints_the_overview_and_exits_zero() {
    let (_t, db) = fixture(3);
    let out = run(&db, &[], &[]);
    assert!(out.status.success(), "bare archie exited {:?}", out.status);
    let text = stdout(&out);
    assert!(text.contains("archie stats"), "no stats screen:\n{text}");
    assert!(text.contains("WINDOW"), "the current window is missing:\n{text}");
    assert!(text.contains("archie tui"), "no way onwards:\n{text}");
}

/// `archie tui` is the explicit spelling of the same thing, and behaves identically where
/// there is no terminal to take over.
#[test]
fn the_tui_verb_matches_the_bare_invocation() {
    let (_t, db) = fixture(3);
    let bare = stdout(&run(&db, &[], &[]));
    let verb = stdout(&run(&db, &["tui"], &[]));
    // Compared up to the window head, which carries a wall-clock span: two invocations a
    // few milliseconds apart can land either side of a minute boundary.
    assert_eq!(bare.split("WINDOW").next(), verb.split("WINDOW").next());
    assert!(bare.contains("WINDOW") && verb.contains("WINDOW"));
}

/// `--plain`, `TERM=dumb` and a JSON default are the other three ways out. None of them
/// may open anything, and all three must exit 0.
#[test]
fn plain_dumb_and_json_all_stay_out_of_the_terminal() {
    let (_t, db) = fixture(3);

    let plain = run(&db, &["--plain"], &[]);
    assert!(plain.status.success());
    assert!(stdout(&plain).contains("WINDOW"));

    let dumb = run(&db, &[], &[("TERM", "dumb")]);
    assert!(dumb.status.success());
    assert!(stdout(&dumb).contains("WINDOW"));

    let json = run(&db, &["tui", "--json"], &[]);
    assert!(json.status.success());
    let v: serde_json::Value = serde_json::from_str(stdout(&json).trim()).unwrap();
    assert_eq!(v["total_sessions"].as_u64(), Some(3));
    assert!(v["window"].is_object(), "the window is missing from --json");
}

/// An index with nothing in it says so, in the same words `session list` says it, and
/// still exits 0 rather than looking like a failure.
#[test]
fn an_empty_index_shows_the_scan_line() {
    let temp = tempdir().unwrap();
    let db = temp.path().join("empty.db");
    Command::cargo_bin("agentworth")
        .unwrap()
        .arg("--db-path")
        .arg(&db)
        .arg("scan")
        .arg(temp.path())
        .arg("--json")
        .assert()
        .success();

    let out = run(&db, &[], &[]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("No sessions found in index."), "{text}");
    assert!(text.contains("archie scan"), "{text}");
}

/// The four rules every other screen keeps (`output_snapshots.rs`), applied to the two
/// spellings the cockpit adds.
#[test]
fn the_overview_holds_the_grid_and_the_glyph_set() {
    let (_t, db) = fixture(3);
    for args in [vec![], vec!["tui"]] {
        for columns in [80usize, 120] {
            let mut cmd = Command::cargo_bin("agentworth").unwrap();
            cmd.arg("--db-path").arg(&db);
            for a in &args {
                cmd.arg(a);
            }
            let text = String::from_utf8(
                cmd.env("COLUMNS", columns.to_string())
                    .env("NO_COLOR", "1")
                    .assert()
                    .success()
                    .get_output()
                    .stdout
                    .clone(),
            )
            .unwrap();
            for line in text.lines() {
                assert!(
                    console::measure_text_width(line) <= columns.min(78),
                    "{args:?} at {columns}: {line}"
                );
            }
            for c in text.chars() {
                let ok = c.is_ascii()
                    || matches!(c as u32, 0x2500..=0x259F)
                    || matches!(c, '●' | '○' | '·' | '—' | '→');
                assert!(ok, "{args:?} ships U+{:04X} ({c})", c as u32);
            }
        }
    }
}

// -----------------------------------------------------------------------------
// the one-rendering-path rule
// -----------------------------------------------------------------------------

/// The spec's binding rule, made mechanical: **no view function may exist that only the
/// cockpit calls.**
///
/// Every `pub fn` in `ui/views.rs` has to be called from somewhere that is not
/// `ui/cockpit.rs` -- another view, a command module, or `app.rs`. A view added for a
/// cockpit screen and given no printed surface fails here, which is the whole point.
#[test]
fn no_view_exists_that_only_the_cockpit_calls() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let views_path = src.join("ui/views.rs");
    let cockpit_path = src.join("ui/cockpit.rs");
    let views = fs::read_to_string(&views_path).unwrap();

    let names: Vec<String> = views
        .lines()
        .filter_map(|l| l.strip_prefix("pub fn "))
        .filter_map(|l| l.split('(').next())
        .filter_map(|l| l.split('<').next())
        .map(|n| n.trim().to_string())
        .collect();
    assert!(names.len() > 20, "only found {} views", names.len());

    let mut elsewhere = String::new();
    for file in rust_files(&src) {
        if file == views_path || file == cockpit_path {
            continue;
        }
        elsewhere.push_str(&fs::read_to_string(&file).unwrap());
    }
    // A view calling another view keeps it reachable too: `overview` renders `stats`, and
    // `stats` is what `archie stats` prints.
    let inside: String = views
        .lines()
        .filter(|l| !l.starts_with("pub fn "))
        .collect::<Vec<_>>()
        .join("\n");

    for name in &names {
        let call = format!("{name}(");
        let qualified = format!("views::{name}");
        let imported = format!("{name} as ");
        let referenced = elsewhere.contains(&qualified)
            || elsewhere.contains(&imported)
            || inside.contains(&call);
        assert!(
            referenced,
            "`views::{name}` is called from nowhere but the cockpit (or from nowhere at \
             all). Every cockpit screen must also be something `archie` can print."
        );
    }
}

fn rust_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(rust_files(&p));
        } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
            out.push(p);
        }
    }
    out
}

// -----------------------------------------------------------------------------
// the opening budget
// -----------------------------------------------------------------------------

/// Opening must not scan.
///
/// Measured as the *marginal* cost: the same binary printing `--help`, which touches no
/// index at all, against the same binary drawing the first screen. Subtracting the
/// baseline takes process start, dynamic linking and clap out of the number, so this
/// measures the index work and stays honest on a slow CI runner.
///
/// A ten-thousand-session fixture is not built here -- scanning ten thousand transcripts
/// would dominate the test's own runtime and measure the scanner. What actually makes a
/// large index open fast is the bound asserted below this, and this is the timing that
/// catches a screen that stops honouring it.
#[test]
fn the_cockpit_opens_without_reading_the_index() {
    let (_t, db) = fixture(400);

    // Warm the page cache and the binary the way a second run would find them.
    let _ = run(&db, &["--help"], &[]);
    let _ = run(&db, &[], &[]);

    let start = Instant::now();
    let _ = run(&db, &["--help"], &[]);
    let baseline = start.elapsed();

    let start = Instant::now();
    let out = run(&db, &[], &[]);
    let total = start.elapsed();
    assert!(out.status.success());

    let marginal = total.saturating_sub(baseline);
    assert!(
        marginal < OPEN_BUDGET,
        "the first screen cost {marginal:?} of index work ({total:?} total against a \
         {baseline:?} baseline), over the {OPEN_BUDGET:?} budget"
    );
}

/// The sessions screen reads a bounded slice, whatever the index holds.
#[test]
fn the_sessions_screen_is_limit_bounded() {
    let (_t, db) = fixture(400);
    let out = run(&db, &["session", "list", "--limit", "50", "--json"], &[]);
    let rows: serde_json::Value = serde_json::from_str(stdout(&out).trim()).unwrap();
    assert_eq!(
        rows.as_array().unwrap().len(),
        agentworth_cli::ui::cockpit::LIST_LIMIT,
        "the cockpit's list limit and `session list`'s have drifted apart"
    );
}
