//! A source gate: nothing hands a *directory* to `extract_repository_or_workspace`.
//!
//! # Why this exists
//!
//! There are two repo-key functions and picking the wrong one produces a key that looks
//! entirely plausible and silently matches nothing:
//!
//! | input | function |
//! | :--- | :--- |
//! | a session's `source_path` — a transcript file, or a synthetic path shaped like one | `extract_repository_or_workspace` |
//! | a directory — a live `cwd`, a `--workspace`, a recorded agent cwd, a checkout root | `repo_key_for_dir` |
//!
//! `extract_repository_or_workspace` is written for the first. Handed a directory it falls
//! through to a rule that keeps two path components only when something sits *under* the repo,
//! so a repository checked out directly at `/Users/x/code/motionvector` keys as
//! `"motionvector"` while every indexed session for it keys as `"code/motionvector"`. The
//! lookup then matches zero rows against a thousand indexed sessions, and reports "no session
//! for this repo in the index" rather than an error.
//!
//! That is not hypothetical. It shipped twice. The second time was `apps/cli/src/
//! antigravity_hook.rs`, a brand-new file that passed the agy hook payload's resolved
//! workspace directory to `extract_repository_or_workspace` and merged green, because nothing
//! was watching for it. This test is what watches for it.
//!
//! # What it does
//!
//! Scans every `.rs` file under `apps/` and `crates/` for calls to
//! `extract_repository_or_workspace`, and fails when the argument looks like a directory.
//! Deliberate exceptions — the documented fallback for a directory that is no longer on disk —
//! carry an allow marker on the call or in the few lines above it.

use std::path::{Path, PathBuf};

/// The function this gate guards.
const GUARDED: &str = "extract_repository_or_workspace";

/// Put this on the call (or within the four lines above it) to allow a directory-shaped
/// argument, and say why on the same line.
const ALLOW_MARKER: &str = "repo-key-gate: allow";

/// How far above a call an allow marker may sit, so it can go on the comment that explains the
/// fallback rather than being crammed onto the call itself.
const ALLOW_LOOKBACK: usize = 4;

/// Fragments that mean "this argument is a directory, not a transcript path".
///
/// `to_string_lossy` is the strongest of them: a `Path`/`PathBuf` being turned into a string to
/// be keyed is the exact shape both shipped bugs had.
const DIRECTORY_SHAPED: &[&str] = &[
    "to_string_lossy",
    "current_dir",
    "cwd",
    "workspace",
    "checkout",
    "repo_root",
    "git_root",
    "dir",
];

#[test]
fn no_call_site_hands_a_directory_to_the_transcript_path_function() {
    let root = workspace_root();
    let mut offenders = Vec::new();

    for file in rust_files(&root) {
        // The gate's own source names both functions and every marker; scanning it would
        // flag itself.
        if file.ends_with("repo_key_call_sites.rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();

        for call in calls_in(&lines) {
            if !DIRECTORY_SHAPED.iter().any(|m| call.argument.contains(m)) {
                continue;
            }
            if allowed(&lines, call.line) {
                continue;
            }
            offenders.push(format!(
                "  {}:{}\n      {GUARDED}({})",
                file.strip_prefix(&root).unwrap_or(&file).display(),
                call.line + 1,
                call.argument.trim(),
            ));
        }
    }

    assert!(
        offenders.is_empty(),
        "\n\
         A directory was handed to `{GUARDED}`, which is written for a session's `source_path`\n\
         (a transcript file). Handed a directory it keeps two path components only when\n\
         something sits *under* the repo, so a repo checked out directly at\n\
         `/Users/x/code/motionvector` keys as \"motionvector\" while every indexed session for\n\
         it keys as \"code/motionvector\". Nothing errors; the lookup just matches nothing.\n\
         \n\
         Which function:\n\
         \n\
           a session's `source_path`, or a synthetic string shaped like one\n\
               -> agentworth_schema::{GUARDED}\n\
           a directory -- a live `cwd`, a `--workspace`, a recorded agent cwd, a checkout root\n\
               -> agentworth_schema::repo_key_for_dir  (or `repo_key_for_dir_str`)\n\
         \n\
         `repo_key_for_dir` resolves the git checkout root on disk, canonicalizes it, and keys\n\
         off that, so it agrees with the indexed key by construction. It falls back to\n\
         `{GUARDED}` by itself when the directory is gone, which is the only reason to call\n\
         the string version on a directory.\n\
         \n\
         If this call really is that documented fallback, put `{ALLOW_MARKER} -- <why>` on it\n\
         or within {ALLOW_LOOKBACK} lines above.\n\
         \n\
         Found {} call site(s):\n\n{}\n",
        offenders.len(),
        offenders.join("\n\n"),
    );
}

/// One call to the guarded function: the line it opens on, and its argument text.
struct Call {
    line: usize,
    argument: String,
}

/// How many lines above a function-value use are read for the receiver it is applied to.
/// `row.cwd.as_deref().map(extract_repository_or_workspace)` wraps across four lines in this
/// codebase, and `cwd` is the whole signal.
const RECEIVER_LOOKBACK: usize = 3;

/// Every use of [`GUARDED`] in `lines`.
///
/// Two shapes, because both have shipped this bug:
///
/// - **Called** — `extract_repository_or_workspace(<arg>)`. The argument is read to its
///   balanced closing paren, so a call split across lines is still seen whole.
/// - **Passed as a value** — `.map(extract_repository_or_workspace)`, where there is no
///   argument at the use site at all. The receiver is what matters, so the surrounding lines
///   stand in for the argument.
///
/// Line comments and `use` declarations are skipped: the function is named in doc comments all
/// over this codebase, and an import line legitimately sits next to `repo_key_for_dir`.
fn calls_in(lines: &[&str]) -> Vec<Call> {
    let mut calls = Vec::new();
    let mut in_use = false;

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();

        // `use` declarations, including the braced multi-line kind, whose import lists put
        // this function's name on a line of its own next to `repo_key_for_dir`.
        if in_use {
            in_use = !trimmed.ends_with(';');
            continue;
        }
        if trimmed.starts_with("use ") || trimmed.starts_with("pub use ") {
            in_use = !trimmed.ends_with(';');
            continue;
        }
        if trimmed.starts_with("//") {
            continue;
        }

        // `match_indices` and `get` rather than slicing: the workspace denies
        // `clippy::string_slice`, and nothing here needs to assume a char boundary.
        for (at, _) in line.match_indices(GUARDED) {
            let after = at + GUARDED.len();
            let (Some(prefix), Some(rest)) = (line.get(..at), line.get(after..)) else {
                continue;
            };

            // A definition (`pub fn extract_repository_or_workspace(`) is not a use.
            if prefix.trim_end().ends_with("fn") {
                continue;
            }
            let before = prefix
                .trim_end()
                .trim_end_matches(|c: char| c.is_alphanumeric() || c == '_' || c == ':');

            let argument = if rest.starts_with('(') {
                argument_from(lines, index, after + 1)
            } else if before.ends_with('(') {
                // Passed as a value into something like `.map(...)`. Anything else -- an
                // import list, a name in a brace -- is not a use site.
                receiver_context(lines, index)
            } else {
                continue;
            };
            calls.push(Call { line: index, argument });
        }
    }
    calls
}

/// The code around a function-value use, comments stripped, standing in for the argument it is
/// about to be applied to.
fn receiver_context(lines: &[&str], index: usize) -> String {
    let first = index.saturating_sub(RECEIVER_LOOKBACK);
    lines[first..=index]
        .iter()
        .filter(|l| !l.trim_start().starts_with("//"))
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Read from `start` on line `index` until the paren opened just before it closes, following
/// the text onto later lines when the call is wrapped.
fn argument_from(lines: &[&str], index: usize, start: usize) -> String {
    let mut depth = 1usize;
    let mut out = String::new();
    let mut offset = start;

    for line in lines.iter().skip(index).take(12) {
        for ch in line.get(offset..).unwrap_or("").chars() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return out;
                    }
                }
                _ => {}
            }
            out.push(ch);
        }
        out.push(' ');
        offset = 0;
    }
    out
}

/// True when the call carries an allow marker, on its own line or just above it.
fn allowed(lines: &[&str], line: usize) -> bool {
    let first = line.saturating_sub(ALLOW_LOOKBACK);
    lines[first..=line].iter().any(|l| l.contains(ALLOW_MARKER))
}

/// Walk up from this crate until the `Cargo.toml` that declares the workspace.
fn workspace_root() -> PathBuf {
    let start = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    start
        .ancestors()
        .find(|dir| {
            std::fs::read_to_string(dir.join("Cargo.toml"))
                .is_ok_and(|toml| toml.contains("[workspace]"))
        })
        .map(Path::to_path_buf)
        .unwrap_or(start)
}

/// Every `.rs` file under the workspace's two source trees. `target/` is never inside them, so
/// there is nothing to exclude.
fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for tree in ["apps", "crates"] {
        collect(&root.join(tree), &mut found);
    }
    found.sort();
    found
}

fn collect(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target" || n == "node_modules") {
                continue;
            }
            collect(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

/// The gate must actually catch the shape that shipped twice, or it is decoration. This is the
/// `antigravity_hook.rs` line as it was merged, and the `wake.rs` line as it was before.
#[test]
fn the_gate_recognises_the_shapes_that_shipped() {
    let shipped = [
        // apps/cli/src/antigravity_hook.rs, merged 2026-09-22
        "let repo = extract_repository_or_workspace(&workspace.to_string_lossy());",
        // apps/cli/src/commands/wake.rs, the original bug
        "extract_repository_or_workspace(&workspace.to_string_lossy())",
        "std::env::current_dir().ok().map(|d| extract_repository_or_workspace(&d.to_string_lossy()))",
        // apps/cli/src/commands/governor_cmds.rs -- passed as a value, no argument in sight
        "let repo_name = cwd.as_deref().map(extract_repository_or_workspace);",
    ];
    for line in shipped {
        let lines = vec![line];
        let calls = calls_in(&lines);
        assert_eq!(calls.len(), 1, "one use expected in: {line}");
        assert!(
            DIRECTORY_SHAPED.iter().any(|m| calls[0].argument.contains(m)),
            "the gate must flag this, it is the bug that shipped: {line}"
        );
    }
}

/// `apps/cli/src/commands/loop_cmds.rs` as it was: passed as a value, with the `cwd` that gives
/// it away three lines up. The receiver, not the use site, is the evidence.
#[test]
fn the_gate_follows_a_function_value_use_back_to_its_receiver() {
    let lines = vec![
        "            let repo = row",
        "                .cwd",
        "                .as_deref()",
        "                .map(agentworth_schema::extract_repository_or_workspace);",
    ];
    let calls = calls_in(&lines);
    assert_eq!(calls.len(), 1);
    assert!(
        DIRECTORY_SHAPED.iter().any(|m| calls[0].argument.contains(m)),
        "the `cwd` three lines up is the whole signal: {}",
        calls[0].argument
    );
}

/// And it must leave the legitimate `source_path` callers alone, or it is noise nobody keeps.
#[test]
fn the_gate_leaves_transcript_path_callers_alone() {
    let fine = [
        "repo: extract_repository_or_workspace(&trace.provenance.source_path),",
        "let repo = extract_repository_or_workspace(&s.source_path);",
        ".filter(|s| extract_repository_or_workspace(&s.source_path) == *r)",
        "let repo = extract_repository_or_workspace(&candidate);",
        "extract_repository_or_workspace(session_source_path)",
    ];
    for line in fine {
        let lines = vec![line];
        let calls = calls_in(&lines);
        assert_eq!(calls.len(), 1, "one call expected in: {line}");
        assert!(
            !DIRECTORY_SHAPED.iter().any(|m| calls[0].argument.contains(m)),
            "this is a transcript path and must not be flagged: {line}"
        );
    }
}

/// A call split across lines is still one call with one argument.
#[test]
fn the_gate_reads_a_wrapped_call_whole() {
    let lines = vec![
        "        let repo = extract_repository_or_workspace(",
        "            &workspace.to_string_lossy(),",
        "        );",
    ];
    let calls = calls_in(&lines);
    assert_eq!(calls.len(), 1);
    assert!(calls[0].argument.contains("to_string_lossy"));
}

/// The function is named in doc comments throughout the codebase; those are not call sites.
#[test]
fn the_gate_ignores_comments_and_the_definition() {
    let lines = vec![
        "/// `extract_repository_or_workspace(&cwd.to_string_lossy())` is what this replaces.",
        "// extract_repository_or_workspace(&dir.to_string_lossy())",
        "pub fn extract_repository_or_workspace(source_path: &str) -> String {",
        // An import legitimately sits beside `repo_key_for_dir`, whose name contains "dir".
        "use agentworth_schema::{extract_repository_or_workspace, repo_key_for_dir};",
        "pub use provenance::{extract_repository_or_workspace, repo_key_for_dir_str};",
    ];
    assert!(calls_in(&lines).is_empty());
}

/// A braced multi-line import puts the name on a line of its own, next to `repo_key_for_dir`
/// and `canonical_repo_root` -- both of which contain "dir" and "root". Not a use site.
#[test]
fn the_gate_ignores_a_multi_line_import_block() {
    let lines = vec![
        "use agentworth_schema::{",
        "    canonical_repo_root, extract_repository_or_workspace, repo_key_for_dir,",
        "    repo_key_for_dir_str, AgentWorthTrace,",
        "};",
        "let repo = extract_repository_or_workspace(&trace.provenance.source_path);",
    ];
    let calls = calls_in(&lines);
    assert_eq!(calls.len(), 1, "only the real call, not the import");
    assert!(calls[0].argument.contains("source_path"));
}

/// An allow marker above the call silences it; without one the same call is flagged.
#[test]
fn an_allow_marker_above_the_call_silences_it() {
    let call = "        None => extract_repository_or_workspace(&dir.to_string_lossy()),";
    let marked = vec!["    // repo-key-gate: allow -- the documented gone-from-disk fallback", call];
    let bare = vec!["    // nothing here", call];

    assert!(allowed(&marked, 1));
    assert!(!allowed(&bare, 1));
}
