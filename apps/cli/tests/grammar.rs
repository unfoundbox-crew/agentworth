//! The noun-verb grammar, end to end through the real binary.
//!
//! The per-command *rendering* is covered by `output_snapshots.rs` and the alias-to-handler
//! mapping by the unit tests in `apps/cli/src/app.rs`. What is left, and what this file
//! covers, is the behaviour you can only see by running the binary: the new verbs actually
//! produce output against a fixture index, and session resolution exits with the codes a
//! script depends on.
//!
//! Fixture per AGENTS.md's rule: a small synthetic Claude Code index, built here, scanned
//! through the real `scan` command. Two of its sessions share an id prefix on purpose --
//! the Claude adapter derives a session id from the file stem, so the file names are what
//! make the ambiguous-prefix case reproducible.

use assert_cmd::Command;
use std::fs::{self, File};
use std::io::Write;
use tempfile::{tempdir, TempDir};

/// Three sessions: one that ran a passing test, one that only claimed done, and one more
/// sharing the second's id prefix so an ambiguous prefix has something to be ambiguous
/// between.
fn fixture() -> (TempDir, std::path::PathBuf) {
    let temp = tempdir().unwrap();
    let dir = temp.path().join(".claude").join("projects").join("proj");
    fs::create_dir_all(&dir).unwrap();

    let mut f = File::create(dir.join("alpha_tested.jsonl")).unwrap();
    writeln!(f, r#"{{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"fix the vector index"}}"#).unwrap();
    writeln!(f, r#"{{"type":"assistant","timestamp":"2026-08-29T10:00:02Z","model":"claude-3-5-sonnet-20241022","usage":{{"input_tokens":5000,"output_tokens":1200,"cache_read_input_tokens":9000,"cache_creation_input_tokens":400}},"content":[{{"type":"tool_use","id":"t1","name":"FileEdit","input":{{"file_path":"crates/storage/src/vector.rs","diff":"+ fn cosine() {{}}"}}}}]}}"#).unwrap();
    writeln!(f, r#"{{"type":"tool_result","timestamp":"2026-08-29T10:00:04Z","tool_use_id":"t1","content":"File modified successfully","is_error":false}}"#).unwrap();
    writeln!(f, r#"{{"type":"assistant","timestamp":"2026-08-29T10:00:05Z","content":[{{"type":"tool_use","id":"t2","name":"Bash","input":{{"command":"cargo test"}}}}],"usage":{{"input_tokens":200,"output_tokens":30,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}"#).unwrap();
    writeln!(f, r#"{{"type":"tool_result","timestamp":"2026-08-29T10:00:07Z","tool_use_id":"t2","content":"test result: ok. 8 passed; 0 failed","is_error":false}}"#).unwrap();

    for suffix in ["one", "two"] {
        let mut g = File::create(dir.join(format!("shared_prefix_{suffix}.jsonl"))).unwrap();
        writeln!(g, r#"{{"type":"user","timestamp":"2026-08-30T11:00:00Z","content":"tidy the readme"}}"#).unwrap();
        writeln!(g, r#"{{"type":"assistant","timestamp":"2026-08-30T11:04:00Z","model":"claude-3-5-haiku-20241022","usage":{{"input_tokens":900,"output_tokens":250,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"content":[{{"type":"text","text":"All done, the readme is tidy now."}}]}}"#).unwrap();
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

fn run(db: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    cmd.arg("--db-path").arg(db);
    for a in args {
        cmd.arg(a);
    }
    cmd.env("COLUMNS", "80").env("NO_COLOR", "1");
    cmd.output().unwrap()
}

fn stdout_of(out: &std::process::Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

/// One case per new verb, JSON so the assertion is about content rather than layout.
#[test]
fn every_new_verb_answers_against_a_fixture_index() {
    let (_temp, db) = fixture();

    let cases: &[(&[&str], &str)] = &[
        (&["repo", "list", "--json"], "total_repos"),
        (&["agent", "show", "claude_code", "--json"], "indexed_sessions"),
        (&["window", "list", "--json"], "windows"),
        (&["window", "show", "--json"], "window_hours"),
        (&["stats", "outcomes", "--json"], "baseline"),
        (&["session", "list", "--json"], "session_id"),
        (&["session", "list", "--unproven", "--json"], "total_blind_spots"),
        (&["agent", "list", "--json"], "total_adapters"),
        (&["stats", "usage", "--period", "day", "--json"], "period"),
    ];

    for (args, needle) in cases {
        let out = run(&db, args);
        assert!(
            out.status.success(),
            "`{}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            stdout_of(&out).contains(needle),
            "`{}` output has no `{needle}`",
            args.join(" ")
        );
    }
}

/// The human rendering of each new verb has to survive the glyph and width rules the rest
/// of the UI is held to. `output_snapshots.rs` owns the general form of this check; here it
/// is only about the four screens this PR adds.
#[test]
fn the_new_screens_stay_inside_the_allowed_glyph_set() {
    let (_temp, db) = fixture();
    for args in [
        vec!["repo", "list"],
        vec!["agent", "show", "claude_code"],
        vec!["window", "list"],
        vec!["stats", "outcomes"],
    ] {
        let out = run(&db, &args);
        assert!(out.status.success(), "`{}` failed", args.join(" "));
        for c in stdout_of(&out).chars() {
            let ok = c.is_ascii()
                || matches!(c as u32, 0x2500..=0x259F)
                || matches!(c, '·' | '—' | '→');
            assert!(ok, "`{}` printed a disallowed glyph {c:?}", args.join(" "));
        }
    }
}

/// An ambiguous prefix is never guessed at: exit 2, and the candidates on stdout.
#[test]
fn an_ambiguous_prefix_exits_2_with_the_candidates() {
    let (_temp, db) = fixture();
    let out = run(&db, &["session", "show", "shared_prefix"]);
    assert_eq!(out.status.code(), Some(2), "ambiguous prefix should exit 2");
    let printed = stdout_of(&out);
    assert!(
        printed.contains("shared_prefix_one") && printed.contains("shared_prefix_two"),
        "both candidates should be listed; got:\n{printed}"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("ambiguous"));
}

/// The same contract for every show-style verb, not just `session show`.
#[test]
fn every_show_style_verb_exits_2_on_an_ambiguous_prefix() {
    let (_temp, db) = fixture();
    for verb in [
        vec!["session", "show"],
        vec!["session", "export"],
        vec!["session", "receipt"],
        vec!["session", "handoff"],
        vec!["session", "forgotten"],
        vec!["session", "loose-ends"],
        vec!["session", "cache"],
        vec!["session", "bisect"],
    ] {
        let mut args = verb.clone();
        args.push("shared_prefix");
        let out = run(&db, &args);
        assert_eq!(
            out.status.code(),
            Some(2),
            "`{}` should exit 2 on an ambiguous prefix",
            args.join(" ")
        );
    }
}

/// A unique prefix still resolves silently -- the ambiguity check must not have made every
/// prefix an error.
#[test]
fn a_unique_prefix_still_resolves() {
    let (_temp, db) = fixture();
    let out = run(&db, &["session", "show", "alpha", "--json"]);
    assert!(out.status.success(), "a unique prefix should resolve");
    assert!(stdout_of(&out).contains("alpha_tested"));
}

/// The test harness is never a TTY, so this is the off-a-TTY contract: no id given means
/// exit 2 with the listing on stdout, never a hung prompt.
#[test]
fn an_omitted_id_off_a_tty_exits_2_and_prints_the_list() {
    let (_temp, db) = fixture();
    for verb in [
        vec!["session", "show"],
        vec!["session", "cache"],
        vec!["session", "bisect"],
        vec!["session", "handoff"],
    ] {
        let out = run(&db, &verb);
        assert_eq!(
            out.status.code(),
            Some(2),
            "`{}` with no id off a TTY should exit 2",
            verb.join(" ")
        );
        assert!(
            stdout_of(&out).contains("alpha_tested"),
            "`{}` should print the listing to stdout",
            verb.join(" ")
        );
    }
}

/// `--current` is the same thing as `--last`, said on purpose.
#[test]
fn current_and_last_resolve_the_same_session() {
    let (_temp, db) = fixture();
    let last = run(&db, &["session", "show", "--last", "--json"]);
    let current = run(&db, &["session", "show", "--current", "--json"]);
    assert!(last.status.success() && current.status.success());
    assert_eq!(stdout_of(&last).len(), stdout_of(&current).len());
}

/// Retired spellings run and are absent from help; the noun tree is present in help.
#[test]
fn retired_spellings_run_but_do_not_appear_in_help() {
    let (_temp, db) = fixture();

    let help = run(&db, &["--help"]);
    let help_text = stdout_of(&help);
    for noun in ["session", "agent", "repo", "window", "stats", "completions"] {
        assert!(help_text.contains(noun), "`{noun}` should be in --help");
    }
    for retired in ["blind-spots", "threat-digest", "cache-doctor", "pr-blame"] {
        assert!(
            !help_text.contains(retired),
            "`{retired}` should be hidden from --help"
        );
    }

    for args in [
        vec!["traces", "--limit", "2", "--json"],
        vec!["matrix", "--json"],
        vec!["blind-spots", "--json"],
        vec!["usage", "--period", "day", "--json"],
    ] {
        let out = run(&db, &args);
        assert!(
            out.status.success(),
            "retired spelling `{}` should still run",
            args.join(" ")
        );
    }
}

/// Static completion scripts generate for every shell the spec names.
#[test]
fn completion_scripts_generate_for_every_shell() {
    let (_temp, db) = fixture();
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let out = run(&db, &["completions", shell]);
        assert!(out.status.success(), "completions {shell} failed");
        assert!(
            stdout_of(&out).len() > 200,
            "completions {shell} produced no script"
        );
    }
}
