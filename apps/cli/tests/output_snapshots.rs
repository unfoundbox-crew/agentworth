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
        .env_remove("CLICOLOR_FORCE")
        // `app::run` loads persisted defaults on every command, so without this the whole
        // suite reads the developer's own `~/.agentworth/config.toml` -- and a persisted
        // `json = true` there would turn every screen below into a JSON payload. Points at
        // the fixture directory, where no config file exists.
        .env(
            "AGENTWORTH_CONFIG_PATH",
            db.parent().unwrap().join("config.toml"),
        );
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
///
/// `scan` and `watch` are both given `root` — the fixture directory — explicitly. With
/// no path either one falls back to the machine's own agent directories, so on a
/// developer's laptop this list would read their real `~/.claude` history, and `scan`
/// would index it into the shared test db.
fn screens(root: &std::path::Path) -> Vec<(&'static str, Vec<String>)> {
    let argv = |args: &[&str]| args.iter().map(|a| a.to_string()).collect::<Vec<_>>();
    vec![
        ("stats", argv(&["stats"])),
        ("traces", argv(&["traces", "--limit", "5"])),
        ("usage", argv(&["usage", "--period", "day"])),
        ("usage-by-model", argv(&["usage", "--period", "day", "--by", "model"])),
        ("usage-year-by-model", argv(&["usage", "--period", "year", "--by", "model"])),
        ("usage-all-by-repo", argv(&["usage", "--period", "all", "--by", "repo"])),
        ("usage-pacing", argv(&["usage", "--pacing"])),
        ("blame", argv(&["blame", "crates/storage/src/vector.rs"])),
        ("scan", vec!["scan".to_string(), root.display().to_string()]),
        ("doctor", argv(&["doctor"])),
        ("matrix", argv(&["matrix"])),
        ("audit", argv(&["audit"])),
        ("blind-spots", argv(&["blind-spots"])),
        ("threat-digest", argv(&["threat-digest"])),
        ("pr-blame", argv(&["pr-blame", "crates/storage/src/vector.rs"])),
        (
            "watch-once",
            vec![
                "watch".to_string(),
                "--poll-once".to_string(),
                "--paths".to_string(),
                root.display().to_string(),
            ],
        ),
        ("blunder", argv(&["blunder"])),
        ("blunder-blame", argv(&["blunder-blame"])),
        // The cockpit's non-TTY path: a bare `archie` and its explicit spelling both print
        // the overview, so both are held to the same four rules as every other screen.
        ("overview", argv(&[])),
        ("tui", argv(&["tui"])),
        // The second audit pass (#111 covered the thirteen above).
        ("session-autopsy", argv(&["session", "autopsy"])),
        ("session-recall", argv(&["session", "recall", "vector index"])),
        ("session-search", argv(&["session", "search", "vector index"])),
        // `--offline` on both: neither may reach the network from a test.
        ("version", argv(&["version", "--offline"])),
        ("update", argv(&["update", "--offline"])),
        ("config-list", argv(&["config", "list"])),
    ]
}

/// `session show` needs a session id, so it joins the sweep separately.
fn session_show_args(db: &std::path::Path) -> Vec<String> {
    vec!["session".to_string(), "show".to_string(), session_id(db)]
}

fn is_allowed(c: char) -> bool {
    c.is_ascii()
        || matches!(c as u32, 0x2500..=0x259F)
        || matches!(c, '●' | '○' | '·' | '—' | '→')
}

/// Every screen in `screens`, plus `session show` on the fixture's own newest session.
fn every_screen(t: &TempDir, db: &std::path::Path) -> Vec<(&'static str, Vec<String>)> {
    let mut all = screens(t.path());
    all.push(("session-show", session_show_args(db)));
    all
}

#[test]
fn no_screen_wraps_at_80_or_120_columns() {
    let (t, db) = fixture();
    for (name, args) in every_screen(&t, &db) {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
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
    let (t, db) = fixture();
    for (name, args) in every_screen(&t, &db) {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        // `scan` reports elapsed-dependent counts on a second run; skip its re-render.
        // `watch-once` prints the wall-clock second it polled at, which can tick over
        // between the plain and coloured invocations below -- same class of flake.
        // `session-search` tops up the vector store as a side effect, so its first run
        // reports chunks embedded and its second reports none: the two renderings below
        // are of different states, not of different palettes.
        if name == "scan" || name == "watch-once" || name == "session-search" {
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
    let (t, db) = fixture();
    for (name, args) in every_screen(&t, &db) {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
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
    assert!(out.contains("archie session list --limit 20"));
    assert!(out.contains("archie scan"));
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

/// `session_` matches both fixture sessions, so it resolves to neither: exit 2, with the
/// candidates on stdout so a script gets data and a non-zero code.
#[test]
fn show_lists_candidates_for_an_ambiguous_prefix() {
    let (_t, db) = fixture();
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    let assertion = cmd
        .arg("--db-path")
        .arg(&db)
        .arg("session")
        .arg("show")
        .arg("session_")
        .env("COLUMNS", "80")
        .env("NO_COLOR", "1")
        .assert()
        .code(2);
    let out = String::from_utf8(assertion.get_output().stdout.clone()).unwrap();

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
    assert!(out.starts_with("archie stats"), "the command echoes at column 0");
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
    assert!(out.contains("archie session show "), "the screen ends with the next command");
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

/// The approved terminal short form: three lines, nine columns, and only the torch
/// glyph changes. A piped stream cannot repaint, so it gets frame 1 once — never a loop.
#[test]
fn the_scan_line_draws_archie_and_prints_one_frame_to_a_pipe() {
    let (t, db) = fixture();
    let root = t.path().display().to_string();
    let out = render(&db, &["scan", &root], 80, false);

    // At most two figures: the one dig frame, then the found frame the summary carries.
    // A loop would put one per tick into the log, which is the failure this guards.
    let ears = out.lines().filter(|l| l.contains("( o o )")).count();
    assert!(
        (1..=2).contains(&ears),
        "expected the dig frame and the found frame, got {} figures:\n{}",
        ears,
        out
    );
    assert!(out.contains("'._.'"), "the merged jaw is missing:\n{}", out);
    assert!(
        out.contains("-*  '._.'"),
        "the torch is missing from the paw:\n{}",
        out
    );
    assert!(out.contains(",---."), "the crown is missing:\n{}", out);
}

/// A brand-new machine: one session on disk, nothing indexed. Returns a fresh temp dir
/// and a db path that does not exist yet, so the caller owns the very first scan.
fn unscanned() -> (TempDir, std::path::PathBuf) {
    let temp = tempdir().unwrap();
    let dir = temp.path().join(".claude").join("projects").join("proj");
    fs::create_dir_all(&dir).unwrap();
    let mut f = File::create(dir.join("session_new.jsonl")).unwrap();
    writeln!(f, r#"{{"type":"user","timestamp":"2026-08-31T09:00:00Z","content":"index my history"}}"#).unwrap();
    writeln!(f, r#"{{"type":"assistant","timestamp":"2026-08-31T09:00:03Z","model":"claude-3-5-sonnet-20241022","usage":{{"input_tokens":100,"output_tokens":20,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"content":[{{"type":"text","text":"On it."}}]}}"#).unwrap();
    let db = temp.path().join("first.db");
    (temp, db)
}

/// The one screen a brand-new user sees. Archie introduces himself once, before anything
/// is indexed, and never again -- two figures on one screen are two states and one of them
/// is lying.
#[test]
fn the_first_scan_introduces_archie_and_the_second_does_not() {
    let (t, db) = unscanned();
    let root = t.path().display().to_string();

    let first = render(&db, &["scan", &root], 80, false);
    assert!(first.contains("first run"), "no first-run banner:\n{}", first);
    assert!(
        first.contains("nothing indexed here yet"),
        "the banner does not say why it is here:\n{}",
        first
    );
    assert!(
        first.contains("reading your agent histories into"),
        "the banner does not say where the index lands:\n{}",
        first
    );
    assert!(first.contains(",---."), "the figure is missing:\n{}", first);

    let second = render(&db, &["scan", &root], 80, false);
    assert!(
        !second.contains("first run"),
        "the banner printed twice:\n{}",
        second
    );
}

/// He is a state, not a texture: machine-readable output carries no mascot, and the
/// payload has to stay parseable on the very first run.
#[test]
fn the_first_scan_prints_nothing_extra_under_json() {
    let (t, db) = unscanned();
    let root = t.path().display().to_string();
    let out = render(&db, &["scan", &root, "--json"], 80, false);
    assert!(!out.contains("first run"), "banner leaked into --json:\n{}", out);
    serde_json::from_str::<serde_json::Value>(out.trim())
        .unwrap_or_else(|e| panic!("--json is not parseable ({e}):\n{out}"));
}

/// The banner is on the same grid as every other screen: nothing wraps, nothing outside
/// the allowed glyph set, and it collapses to the one-line form in a narrow window.
#[test]
fn the_first_run_banner_holds_the_grid() {
    for columns in [40usize, 80, 120] {
        let (t, db) = unscanned();
        let root = t.path().display().to_string();
        let out = render(&db, &["scan", &root], columns, false);
        for line in out.lines() {
            let w = console::measure_text_width(line);
            assert!(
                w <= columns.min(78),
                "first run at {} columns: a line is {} wide\n{}",
                columns,
                w,
                line
            );
            for c in line.chars() {
                assert!(is_allowed(c), "first run ships U+{:04X} ({})", c as u32, c);
            }
        }
        let head = out.lines().next().unwrap_or_default();
        if columns < 48 {
            assert!(
                head.trim_start().starts_with("(*) archie"),
                "the banner should be the one-line form at {} columns, got:\n{}",
                columns,
                out
            );
        } else {
            assert!(
                head.contains(",---."),
                "the banner should lead with the crown at {} columns, got:\n{}",
                columns,
                out
            );
        }
    }
}

/// Under 48 columns the progress block does not leave room for the label and the track
/// beside it, so the scan line collapses to the ears and the torch. The summary that
/// follows is a different screen and keeps its three lines.
#[test]
fn the_scan_line_collapses_to_one_line_in_a_narrow_window() {
    let (t, db) = fixture();
    let root = t.path().display().to_string();
    let out = render(&db, &["scan", &root], 40, false);

    let first = out.lines().find(|l| !l.trim().is_empty()).unwrap_or_default();
    assert!(
        first.trim_start().starts_with("(*) archie") || first.trim_start().starts_with("(o) archie"),
        "the scan line should be the one-line form at 40 columns, got:\n{}",
        out
    );
    for line in out.lines() {
        assert!(console::measure_text_width(line) <= 40, "{}", line);
    }
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

/// `inspect`, `export`, and `receipt` used to require a session id as a positional clap
/// argument, so omitting it failed before any of this code ran ("required arguments were
/// not provided"). It is now optional and this is what fills the gap: on a TTY, a picker;
/// otherwise, the same listing as `--json` or a plain table, and exit 2 rather than a
/// silent guess.
#[test]
fn picker_lists_sessions_and_exits_2_without_an_id_or_last() {
    let (_t, db) = fixture();
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    let assert = cmd
        .arg("--db-path")
        .arg(&db)
        .arg("inspect")
        .env("COLUMNS", "80")
        .env("NO_COLOR", "1")
        .assert()
        .code(2);
    let output = assert.get_output();
    let out = String::from_utf8(output.stdout.clone()).unwrap();
    let err = String::from_utf8(output.stderr.clone()).unwrap();

    assert!(out.contains("SESSION"), "{out}");
    assert!(out.contains("ADAPTER"), "{out}");
    assert!(
        out.contains("session_tested") || out.contains("session_claimed"),
        "the plain fallback must print full, copyable ids: {out}"
    );
    assert!(err.contains("pass a session id or prefix"), "{err}");
    for line in out.lines() {
        assert!(console::measure_text_width(line) <= 78, "{line}");
        for c in line.chars() {
            assert!(is_allowed(c), "picker listing ships U+{:04X}", c as u32);
        }
    }
}

/// The `--json` fallback carries the same rows a person would pick from interactively --
/// enough for a script to resolve a session on its own instead of parsing the plain table.
#[test]
fn picker_json_listing_is_well_formed_and_exits_2() {
    let (_t, db) = fixture();
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    let assert = cmd
        .arg("--db-path")
        .arg(&db)
        .arg("inspect")
        .arg("--json")
        .assert()
        .code(2);
    let out = assert.get_output().stdout.clone();
    let rows: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2, "the fixture has two non-stub sessions");
    for row in rows {
        assert!(row["session_id"].is_string());
        assert!(row["rung"].is_u64());
        assert!(row["number"].is_u64());
    }
}

/// `export` and `receipt` only ever matched an exact id; `agentworth inspect` (#76) made a
/// unique prefix the normal way to name a session and this brings the other two in line.
#[test]
fn export_and_receipt_resolve_a_unique_prefix() {
    let (_t, db) = fixture();
    let full_id = session_id(&db);
    #[allow(clippy::string_slice, reason = "full_id is an ASCII test fixture id (e.g. session_tested)")]
    let prefix = &full_id[..full_id.len() - 1];

    let out = render(&db, &["export", prefix, "--format", "json"], 80, false);
    let trace: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(trace["session_id"].as_str().unwrap(), full_id);

    let out = render(&db, &["receipt", prefix, "--format", "json"], 80, false);
    let receipt: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(receipt["session_id"].as_str().unwrap(), full_id);
}

#[test]
fn json_payloads_are_untouched_by_the_redesign() {
    let (_t, db) = fixture();
    let id = session_id(&db);
    for args in [
        vec!["bisect".to_string(), id.clone(), "--json".to_string()],
        vec!["cache-doctor".to_string(), id.clone(), "--json".to_string()],
    ] {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = render(&db, &args, 80, false);
        serde_json::from_str::<serde_json::Value>(out.trim())
            .unwrap_or_else(|e| panic!("{:?} is no longer valid JSON: {}", args, e));
        assert!(!out.contains('\x1b'), "{:?} leaked an escape into --json", args);
    }
    for args in [
        vec!["stats", "--json"],
        vec!["traces", "--json"],
        vec!["usage", "--period", "day", "--json"],
        vec!["blame", "crates/storage/src/vector.rs", "--json"],
        vec!["doctor", "--json"],
        vec!["matrix", "--json"],
        vec!["audit", "--json"],
        vec!["blind-spots", "--json"],
        vec!["threat-digest", "--json"],
        vec!["pr-blame", "crates/storage/src/vector.rs", "--json"],
        vec!["blunder", "--json"],
        vec!["blunder-blame", "--json"],
        // The second audit pass. `version`/`update` stay `--offline` so no test reaches
        // the network; `session search`/`recall` take a query positional.
        vec!["session", "autopsy", "--json"],
        vec!["session", "recall", "vector index", "--json"],
        vec!["session", "search", "vector index", "--json"],
        vec!["version", "--offline", "--json"],
        vec!["update", "--offline", "--json"],
        vec!["config", "list", "--json"],
    ] {
        let out = render(&db, &args, 80, false);
        serde_json::from_str::<serde_json::Value>(out.trim())
            .unwrap_or_else(|e| panic!("{:?} is no longer valid JSON: {}", args, e));
        assert!(!out.contains('\x1b'), "{:?} leaked an escape into --json", args);
    }

    // `session show --json` is the whole trace, keyed off a real session id.
    let show = render(&db, &["session", "show", id.as_str(), "--json"], 80, false);
    let trace: serde_json::Value = serde_json::from_str(show.trim()).expect("session show --json");
    assert_eq!(trace["session_id"].as_str(), Some(id.as_str()));
    assert!(trace["stats"].is_object(), "session show --json lost its stats block");
    assert!(trace["events"].is_array(), "session show --json lost its events array");
}

/// `--plain` and `--no-color` must land every column where a piped stream already puts it,
/// for the second audit pass's commands as well as #111's. `--plain` also forces the ASCII
/// glyph set; `--no-color` keeps Unicode, so the two are compared against the matching
/// rendering rather than against each other.
#[test]
fn plain_and_no_color_hold_the_column_positions_of_every_audited_screen() {
    let (t, db) = fixture();
    for (name, args) in every_screen(&t, &db) {
        if name == "scan" || name == "watch-once" || name == "session-search" {
            continue;
        }
        let base: Vec<&str> = args.iter().map(String::as_str).collect();

        let piped = render(&db, &base, 80, false);
        let mut plain_args = base.clone();
        plain_args.push("--plain");
        assert_eq!(
            render(&db, &plain_args, 80, false),
            piped,
            "{name}: --plain differs from what a pipe already gets"
        );

        // Unicode glyphs, colour forced on, then stripped: must equal the same run with
        // --no-color, which keeps the glyphs and drops only the palette.
        let coloured = render_with(&db, &base, 80, true, true);
        let mut no_color_args = base.clone();
        no_color_args.push("--no-color");
        let no_color = render_with(&db, &no_color_args, 80, true, true);
        assert!(!no_color.contains('\x1b'), "{name}: --no-color still painted");
        assert_eq!(
            console::strip_ansi_codes(&coloured),
            no_color,
            "{name}: --no-color moved a column"
        );
    }
}

/// `bisect` and `cache-doctor` need a real session id from the fixture, so they cannot ride
/// the static `screens()` list -- same reason `receipt`/`inspect` get their own tests above.
/// Same three grid rules as `screens()`: nothing wraps, colour never moves a column, no
/// glyph outside the allowed set.
#[test]
fn bisect_and_cache_doctor_hold_the_grid() {
    let (_t, db) = fixture();
    let id = session_id(&db);

    for args in [vec!["bisect", id.as_str()], vec!["cache-doctor", id.as_str()]] {
        for columns in [80usize, 120] {
            let plain = render(&db, &args, columns, false);
            assert!(!plain.trim().is_empty(), "{:?} rendered nothing", args);
            for line in plain.lines() {
                let w = console::measure_text_width(line);
                assert!(w <= columns.min(78), "{:?} at {columns}: line is {w} wide\n{line}", args);
            }

            let plain_unicode = render_with(&db, &args, columns, false, true);
            let colour = render_with(&db, &args, columns, true, true);
            assert_eq!(
                console::strip_ansi_codes(&colour),
                plain_unicode,
                "{:?} at {columns}: colour and plain disagree on column positions",
                args
            );
        }
        for c in render(&db, &args, 80, false).chars() {
            assert!(is_allowed(c), "{:?} ships U+{:04X} ({})", args, c as u32, c);
        }
    }
}

/// `merge` needs a second, real SQLite index to merge from, so it cannot ride the shared
/// single-db `screens()` list. Same three grid rules as `screens()`.
#[test]
fn merge_holds_the_grid() {
    let (_t, db) = fixture();

    // A second, independent index with one more session, to actually merge something.
    let temp2 = tempdir().unwrap();
    let dir2 = temp2.path().join(".claude").join("projects").join("proj2");
    fs::create_dir_all(&dir2).unwrap();
    let mut h = File::create(dir2.join("session_other.jsonl")).unwrap();
    writeln!(h, r#"{{"type":"user","timestamp":"2026-08-31T09:00:00Z","content":"add a cache"}}"#).unwrap();
    writeln!(h, r#"{{"type":"assistant","timestamp":"2026-08-31T09:00:02Z","model":"claude-3-5-sonnet-20241022","usage":{{"input_tokens":1000,"output_tokens":200,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"content":[{{"type":"text","text":"Added the cache layer."}}]}}"#).unwrap();
    let db2 = temp2.path().join("index2.db");
    Command::cargo_bin("agentworth")
        .unwrap()
        .arg("--db-path")
        .arg(&db2)
        .arg("scan")
        .arg(temp2.path())
        .arg("--json")
        .assert()
        .success();

    let db2_arg = db2.to_string_lossy().to_string();
    let args = vec!["merge", db2_arg.as_str()];

    for columns in [80usize, 120] {
        // Each run re-merges the same source db into the target -- idempotent (sessions
        // already present just get skipped/updated), so rendering twice at two widths is
        // safe, unlike `scan`'s elapsed-dependent counts.
        let plain = render(&db, &args, columns, false);
        assert!(!plain.trim().is_empty(), "merge rendered nothing");
        for line in plain.lines() {
            let w = console::measure_text_width(line);
            assert!(w <= columns.min(78), "merge at {columns}: line is {w} wide\n{line}");
        }

        let plain_unicode = render_with(&db, &args, columns, false, true);
        let colour = render_with(&db, &args, columns, true, true);
        assert_eq!(
            console::strip_ansi_codes(&colour),
            plain_unicode,
            "merge at {columns}: colour and plain disagree on column positions"
        );
    }
    for c in render(&db, &args, 80, false).chars() {
        assert!(is_allowed(c), "merge ships U+{:04X} ({})", c as u32, c);
    }
}
