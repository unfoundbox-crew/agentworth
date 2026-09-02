//! Grid tests for every redesigned human screen.
//!
//! Each command is rendered against one small fixture index at 80 and 120 columns, in
//! plain and in colour, and checked against the four rules the design system actually
//! makes falsifiable: nothing wraps, colour never moves a column, no glyph falls outside
//! the allowed set, and no emoji ships.

use assert_cmd::Command;
use std::fs::{self, File};
use std::io::Write;
use tempfile::{tempdir, TempDir};

/// Two sessions on different rungs so the ladder, the meter and the "still a claim"
/// closing line all have something to say.
fn fixture() -> (TempDir, std::path::PathBuf) {
    let temp = tempdir().unwrap();
    let dir = temp.path().join(".claude").join("projects").join("proj");
    fs::create_dir_all(&dir).unwrap();

    // Rung 3: an edit plus a passing test run.
    let mut f = File::create(dir.join("session_tested.jsonl")).unwrap();
    writeln!(f, r#"{{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"fix the vector index"}}"#).unwrap();
    writeln!(f, r#"{{"type":"assistant","timestamp":"2026-08-29T10:00:02Z","model":"claude-3-5-sonnet-20241022","usage":{{"input_tokens":50000,"output_tokens":12000,"cache_read_input_tokens":900000,"cache_creation_input_tokens":4000}},"content":[{{"type":"tool_use","id":"t1","name":"FileEdit","input":{{"file_path":"crates/storage/src/vector.rs","diff":"+ fn cosine() {{}}"}}}}]}}"#).unwrap();
    writeln!(f, r#"{{"type":"tool_result","timestamp":"2026-08-29T10:00:04Z","tool_use_id":"t1","content":"File modified successfully","is_error":false}}"#).unwrap();
    writeln!(f, r#"{{"type":"assistant","timestamp":"2026-08-29T10:00:05Z","content":[{{"type":"tool_use","id":"t2","name":"Bash","input":{{"command":"cargo test"}}}}],"usage":{{"input_tokens":200,"output_tokens":30,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}"#).unwrap();
    writeln!(f, r#"{{"type":"tool_result","timestamp":"2026-08-29T10:00:07Z","tool_use_id":"t2","content":"test result: ok. 8 passed; 0 failed","is_error":false}}"#).unwrap();

    // Rung 0: a long claim and nothing else.
    let mut g = File::create(dir.join("session_claimed.jsonl")).unwrap();
    writeln!(g, r#"{{"type":"user","timestamp":"2026-08-30T11:00:00Z","content":"tidy the readme"}}"#).unwrap();
    writeln!(g, r#"{{"type":"assistant","timestamp":"2026-08-30T11:04:00Z","model":"claude-3-5-haiku-20241022","usage":{{"input_tokens":900,"output_tokens":250,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"content":[{{"type":"text","text":"All done, the readme is tidy now."}}]}}"#).unwrap();

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

/// Render one command. `color` picks the palette; `unicode` picks the glyph set. The two
/// are separate axes: the spec keeps Unicode under NO_COLOR and drops to ASCII only when
/// stdout is not a terminal.
fn render_with(
    db: &std::path::Path,
    args: &[&str],
    columns: usize,
    color: bool,
    unicode: bool,
) -> String {
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    cmd.arg("--db-path").arg(db);
    for a in args {
        cmd.arg(a);
    }
    cmd.env("COLUMNS", columns.to_string())
        .env_remove("NO_COLOR")
        .env_remove("COLORTERM")
        .env_remove("CLICOLOR_FORCE");
    // The test harness is never a TTY, so CLICOLOR_FORCE stands in for one.
    if color || unicode {
        cmd.env("CLICOLOR_FORCE", "1");
    }
    if !color {
        cmd.env("NO_COLOR", "1");
    }
    let out = cmd.assert().get_output().stdout.clone();
    String::from_utf8(out).unwrap()
}

/// The default: a piped stream, which is no colour and ASCII.
fn render(db: &std::path::Path, args: &[&str], columns: usize, color: bool) -> String {
    render_with(db, args, columns, color, color)
}

fn session_id(db: &std::path::Path) -> String {
    let json = render(db, &["traces", "--json"], 80, false);
    let v: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    v[0]["session_id"].as_str().unwrap().to_string()
}

/// Every screen, in the form the tests iterate over.
fn screens() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("stats", vec!["stats"]),
        ("traces", vec!["traces", "--limit", "5"]),
        ("usage", vec!["usage", "--period", "day"]),
        ("usage-by-model", vec!["usage", "--period", "day", "--by-model"]),
        ("usage-pacing", vec!["usage", "--pacing"]),
        ("blame", vec!["blame", "crates/storage/src/vector.rs"]),
        ("scan", vec!["scan"]),
        ("doctor", vec!["doctor"]),
        ("matrix", vec!["matrix"]),
    ]
}

fn is_allowed(c: char) -> bool {
    c.is_ascii()
        || matches!(c as u32, 0x2500..=0x259F)
        || matches!(c, '●' | '○' | '·' | '—' | '→')
}

#[test]
fn no_screen_wraps_at_80_or_120_columns() {
    let (_t, db) = fixture();
    for (name, args) in screens() {
        for columns in [80usize, 120] {
            let out = render(&db, &args, columns, false);
            assert!(!out.trim().is_empty(), "{} rendered nothing", name);
            for line in out.lines() {
                let w = console::measure_text_width(line);
                assert!(
                    w <= columns.min(78),
                    "{} at {} columns: a line is {} wide\n{}",
                    name,
                    columns,
                    w,
                    line
                );
            }
        }
    }
}

#[test]
fn colour_never_moves_a_column() {
    // The whole point of the palette: awk and cut must work on the human output, not
    // only on --json. Stripping the escapes has to give back the plain rendering byte
    // for byte.
    let (_t, db) = fixture();
    for (name, args) in screens() {
        // `scan` reports elapsed-dependent counts on a second run; skip its re-render.
        if name == "scan" {
            continue;
        }
        for columns in [80usize, 120] {
            // Same glyph set on both arms, so only the palette differs.
            let plain = render_with(&db, &args, columns, false, true);
            let colour = render_with(&db, &args, columns, true, true);
            assert!(
                colour.contains('\x1b'),
                "{} at {} columns emitted no colour under CLICOLOR_FORCE",
                name,
                columns
            );
            assert_eq!(
                console::strip_ansi_codes(&colour),
                plain,
                "{} at {} columns: colour and plain disagree on column positions",
                name,
                columns
            );
        }
    }
}

#[test]
fn no_screen_ships_a_glyph_outside_the_allowed_set() {
    let (_t, db) = fixture();
    for (name, args) in screens() {
        let out = render(&db, &args, 80, false);
        for c in out.chars() {
            assert!(
                is_allowed(c),
                "{} ships U+{:04X} ({}), outside the box-drawing / block / ● ○ · — → set",
                name,
                c as u32,
                c
            );
        }
    }
}

#[test]
fn the_receipt_holds_its_frame() {
    let (_t, db) = fixture();
    let id = session_id(&db);
    let out = render(&db, &["receipt", &id], 80, false);
    let framed: Vec<&str> = out
        .lines()
        .filter(|l| l.trim_start().starts_with('|'))
        .collect();
    assert!(framed.len() > 10, "receipt drew only {} rows", framed.len());
    let width = console::measure_text_width(framed[0]);
    for line in &framed {
        assert_eq!(
            console::measure_text_width(line),
            width,
            "the receipt box does not close:\n{}",
            line
        );
        assert!(line.trim_end().ends_with('|'), "unclosed row: {}", line);
    }
    for c in out.chars() {
        assert!(is_allowed(c), "receipt ships U+{:04X}", c as u32);
    }
}

#[test]
fn the_error_screen_names_the_noun_and_the_way_out() {
    let (_t, db) = fixture();
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    let out = cmd
        .arg("--db-path")
        .arg(&db)
        .arg("inspect")
        .arg("9f21aa")
        .env("COLUMNS", "80")
        .env("NO_COLOR", "1")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).unwrap();

    assert!(out.contains("No indexed session starts with 9f21aa."));
    assert!(out.contains("agentworth traces --limit 20"));
    assert!(out.contains("agentworth scan"));
    assert!(!out.contains("panicked"), "an error screen is not a stack trace");
    for line in out.lines() {
        assert!(console::measure_text_width(line) <= 78, "{}", line);
    }
}

/// The fixture's two sessions are `session_tested` and `session_claimed` -- both file
/// stems, so both share the `session_` prefix and diverge right after it. Dropping just
/// the last character off either id keeps far more than that shared prefix, so it stays
/// unique to its own session without hardcoding which one `session_id()` happens to pick.
#[test]
fn inspect_resolves_a_unique_prefix() {
    let (_t, db) = fixture();
    let full_id = session_id(&db);
    #[allow(clippy::string_slice, reason = "full_id is an ASCII test fixture id (e.g. session_tested)")]
    let prefix = &full_id[..full_id.len() - 1];

    let out = render(&db, &["inspect", prefix, "--json"], 80, false);
    let trace: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(trace["session_id"].as_str().unwrap(), full_id);
}

/// `session_` matches both fixture sessions, so it must fall through to the same
/// not-found/nearest-matches screen an unknown id gets -- not resolve to either one.
#[test]
fn inspect_lists_candidates_for_an_ambiguous_prefix() {
    let (_t, db) = fixture();
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    let out = cmd
        .arg("--db-path")
        .arg(&db)
        .arg("inspect")
        .arg("session_")
        .env("COLUMNS", "80")
        .env("NO_COLOR", "1")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).unwrap();

    assert!(out.contains("No indexed session starts with session_."));
    assert!(out.contains("session_tested"));
    assert!(out.contains("session_claimed"));
}

#[test]
fn stats_draws_the_evidence_line_between_rung_3_and_rung_2() {
    let (_t, db) = fixture();
    let out = render(&db, &["stats"], 80, false);
    let lines: Vec<&str> = out.lines().collect();

    let line_at = lines
        .iter()
        .position(|l| l.contains("the evidence line"))
        .expect("stats draws the evidence line");
    let rung3 = lines.iter().position(|l| l.contains("tests passed")).unwrap();
    let rung2 = lines.iter().position(|l| l.contains("files changed")).unwrap();
    assert!(
        rung3 < line_at && line_at < rung2,
        "the evidence line must sit between rung 3 and rung 2"
    );

    assert!(out.contains("VERIFIED"), "stats names the verified total");
    assert!(out.contains("EVIDENCE LADDER"));
    assert!(out.contains("TOKENS"));
    assert!(out.starts_with("agentworth stats"), "the command echoes at column 0");
}

#[test]
fn traces_leads_with_a_five_cell_meter_and_ends_on_the_finding() {
    let (_t, db) = fixture();
    let out = render(&db, &["traces", "--limit", "5"], 80, false);
    assert!(out.contains("EVIDENCE"));
    assert!(!out.contains("[UNVERIFIED]"), "the bracketed badge is gone");
    assert!(!out.contains("EVENTS"), "EVENTS answers no question anyone asked");

    let meters: Vec<&str> = out
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|t| t.len() == 5 && t.chars().all(|c| c == '#' || c == '.'))
        .collect();
    assert_eq!(meters.len(), 2, "one meter per row, five cells each");
    assert!(meters.contains(&"###.."), "the tested session sits on rung 3");
    assert!(meters.contains(&"....."), "the claim-only session sits on rung 0");
    assert!(out.contains("agentworth inspect "), "the screen ends with the next command");
}

#[test]
fn usage_prints_a_dash_not_a_zero_for_unmeasured_adapters() {
    let (_t, db) = fixture();
    let out = render(&db, &["usage", "--period", "day"], 80, false);
    assert!(out.contains("SESSIONS"));
    assert!(out.contains("CACHE RD"));
    assert!(!out.contains("Ledger"), "no decorated title");
}

#[test]
fn plain_flag_matches_a_piped_stream() {
    let (_t, db) = fixture();
    let piped = render(&db, &["stats"], 80, false);
    let flagged = render(&db, &["stats", "--plain"], 80, false);
    assert_eq!(piped, flagged, "--plain is what a pipe already gets");
}

/// `suspect` needs a real git checkout, so it cannot ride the shared fixture's `screens()`
/// list. It gets the same three grid rules here instead: nothing wraps, colour never moves a
/// column, no glyph outside the allowed set.
#[test]
fn the_suspect_screen_holds_the_grid() {
    let (_t, db) = fixture();
    let repo = tempdir().unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(args)
            .output()
            .expect("run git");
        assert!(out.status.success(), "git {args:?} failed");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "test@example.invalid"]);
    git(&["config", "user.name", "Test"]);
    git(&["config", "commit.gpgsign", "false"]);
    fs::write(repo.path().join("a.rs"), "fn a() {}\n").unwrap();
    git(&["add", "a.rs"]);
    git(&["commit", "-q", "-m", "feat: a"]);

    let repo_arg = repo.path().to_string_lossy().to_string();
    let args = vec!["suspect", "--repo", repo_arg.as_str()];

    for columns in [80usize, 120] {
        let plain = render(&db, &args, columns, false);
        assert!(!plain.trim().is_empty(), "suspect rendered nothing");
        for line in plain.lines() {
            let w = console::measure_text_width(line);
            assert!(w <= columns.min(78), "suspect at {columns}: line is {w} wide\n{line}");
        }

        let plain_unicode = render_with(&db, &args, columns, false, true);
        let colour = render_with(&db, &args, columns, true, true);
        assert_eq!(
            console::strip_ansi_codes(&colour),
            plain_unicode,
            "suspect at {columns}: colour and plain disagree on column positions"
        );
    }

    for c in render(&db, &args, 80, false).chars() {
        assert!(is_allowed(c), "suspect ships U+{:04X} ({})", c as u32, c);
    }

    // The honest empty case says what it does not know, rather than reading as clean.
    let out = render(&db, &args, 80, false);
    assert!(out.contains("Unknown, not clean"), "{out}");
    assert!(out.contains("could not be placed"), "{out}");

    // `--hook` prints a script that exits 0 no matter what, and blocks nothing.
    let hook = render(&db, &["suspect", "--hook"], 80, false);
    assert!(hook.starts_with("#!/bin/sh"), "{hook}");
    assert!(hook.contains("exit 0"), "{hook}");
    assert!(!hook.contains("exit 1"), "a pre-push note must never block a push: {hook}");
}

#[test]
fn json_payloads_are_untouched_by_the_redesign() {
    let (_t, db) = fixture();
    for args in [
        vec!["stats", "--json"],
        vec!["traces", "--json"],
        vec!["usage", "--period", "day", "--json"],
        vec!["blame", "crates/storage/src/vector.rs", "--json"],
        vec!["doctor", "--json"],
        vec!["matrix", "--json"],
    ] {
        let out = render(&db, &args, 80, false);
        serde_json::from_str::<serde_json::Value>(out.trim())
            .unwrap_or_else(|e| panic!("{:?} is no longer valid JSON: {}", args, e));
        assert!(!out.contains('\x1b'), "{:?} leaked an escape into --json", args);
    }
}
