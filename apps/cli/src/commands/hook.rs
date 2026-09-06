//! `archie hook`: the client end of the loop (docs/specs/loop.md section 1).
//!
//! This runs inside the agent's own process tree, on every hook, so its one hard rule is that
//! it can never cost the agent anything: it reads stdin, spends at most 50 ms trying the
//! socket, falls back to the spool, and exits 0 whatever happened. Nothing reaches stdout --
//! Claude Code reads a hook's stdout, and a line there is a line in the agent's context.
//! Failures are visible under `--verbose` on stderr and nowhere else.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

use agentworth_loop::{HookEvent, SpoolWriter};

/// How long the socket gets before the spool wins. The budget is the whole point: an agent
/// waiting on Archie is a worse outcome than an event arriving a scan late.
const SOCKET_BUDGET: Duration = Duration::from_millis(50);

/// The most stdin this reads. A hook payload carries a tool result, and a tool result can be a
/// whole file: past this the event is not worth the memory it would cost every hook on the
/// machine, and a truncated payload will not parse, so it is dropped rather than half-stored.
const MAX_STDIN_BYTES: u64 = 8 * 1024 * 1024;

/// Where events go when the config directory is not under the user's home. `default_db_dir`
/// falls back to a relative `.agentworth`, which for a hook means spooling into whatever
/// repository the agent happened to be standing in.
const TEMP_SPOOL_DIR: &str = "agentworth-spool";

/// The hook events `archie hook print claude` registers. Every one Claude Code documents
/// (code.claude.com/docs/en/hooks, read 2026-09-06); the loop ignores the ones it has no rule
/// for rather than asking the harness to send fewer.
pub const CLAUDE_HOOK_EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "Stop",
    "SubagentStart",
    "SubagentStop",
    "PreCompact",
    "PostCompact",
    "CwdChanged",
    "SessionEnd",
];

/// The events that take a tool matcher. The rest are fired once per session or per turn and
/// a matcher on them matches nothing.
const MATCHED_EVENTS: &[&str] = &["PreToolUse", "PostToolUse", "PostToolUseFailure"];

/// Reads one hook payload and delivers it. Always `Ok(())`: a hook that fails the agent is a
/// hook that gets uninstalled.
pub fn run_hook_command(verbose: bool) -> anyhow::Result<()> {
    let mut input = String::new();
    if let Err(e) = std::io::stdin()
        .lock()
        .take(MAX_STDIN_BYTES)
        .read_to_string(&mut input)
    {
        note(verbose, &format!("could not read stdin: {e}"));
        return Ok(());
    }
    if input.len() as u64 == MAX_STDIN_BYTES {
        note(
            verbose,
            "the payload hit the 8 MiB cap; it will be dropped unless it happens to parse",
        );
    }
    let value: serde_json::Value = match serde_json::from_str(&input) {
        Ok(value) => value,
        Err(e) => {
            note(verbose, &format!("stdin was not hook JSON: {e}"));
            return Ok(());
        }
    };
    let env: HashMap<String, String> = std::env::vars().collect();
    let event = match HookEvent::from_stdin_json(value, &env) {
        Ok(event) => event,
        Err(e) => {
            note(verbose, &format!("hook JSON had no session_id: {e}"));
            return Ok(());
        }
    };

    match deliver(&event, verbose) {
        Ok(Delivery::Socket) => note(verbose, "delivered on the socket"),
        Ok(Delivery::Spool(path)) => {
            note(verbose, &format!("spooled to {}", path.display()));
        }
        Err(e) => note(verbose, &format!("event dropped: {e:#}")),
    }
    Ok(())
}

enum Delivery {
    Socket,
    Spool(PathBuf),
}

fn deliver(event: &HookEvent, verbose: bool) -> anyhow::Result<Delivery> {
    let line = serde_json::to_string(event)?;
    if let Some(error) = try_socket(&line) {
        tracing::debug!("loop: socket unavailable ({error}), spooling");
        let path = SpoolWriter::append(&spool_dir(verbose), event)?;
        return Ok(Delivery::Spool(path));
    }
    Ok(Delivery::Socket)
}

/// The spool directory, but never inside whatever repository the agent is standing in.
///
/// `agentworth_storage::default_db_dir` falls back to a *relative* `.agentworth` when it cannot
/// find a home directory. For every other command that is a visible directory in the cwd; for a
/// hook, which runs inside the agent's own checkout, it would write session events into the
/// repository under review. So the fallback is the system temp directory instead.
fn spool_dir(verbose: bool) -> PathBuf {
    let under_home = agentworth_storage::default_spool_dir()
        .ok()
        .filter(|dir| home_dir().is_some_and(|home| dir.starts_with(home)));
    match under_home {
        Some(dir) => dir,
        None => {
            note(
                verbose,
                "no home directory to spool under; using the system temp directory",
            );
            std::env::temp_dir().join(TEMP_SPOOL_DIR)
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
}

/// `None` on success, the reason otherwise. Not a `Result` because every caller treats the
/// reason as a note, never as a failure.
#[cfg(unix)]
fn try_socket(line: &str) -> Option<String> {
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    let path = agentworth_storage::default_socket_path().ok()?;
    if !path.exists() {
        return Some("no socket".to_string());
    }
    // `connect` on a Unix socket has no timeout argument -- it either finds a listener with a
    // free backlog slot or fails immediately, so the budget below covers the write.
    let mut stream = match UnixStream::connect(&path) {
        Ok(stream) => stream,
        Err(e) => return Some(e.to_string()),
    };
    if let Err(e) = stream.set_write_timeout(Some(SOCKET_BUDGET)) {
        return Some(e.to_string());
    }
    if let Err(e) = stream.write_all(line.as_bytes()).and_then(|()| {
        stream.write_all(b"\n")?;
        stream.flush()
    }) {
        return Some(e.to_string());
    }
    None
}

#[cfg(not(unix))]
fn try_socket(_line: &str) -> Option<String> {
    Some("no Unix sockets on this platform".to_string())
}

/// The `~/.claude/settings.json` snippet, printed and never written. Writing it would mean
/// merging into a file the user owns and may have hand-edited; printing lets them see what
/// they are pasting.
pub fn print_claude_snippet() -> anyhow::Result<()> {
    let hooks = claude_hooks_value();
    println!("// paste into the \"hooks\" object of ~/.claude/settings.json");
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({ "hooks": hooks }))?
    );
    Ok(())
}

/// The `hooks` object itself, so the test can parse what the command prints.
pub fn claude_hooks_value() -> serde_json::Value {
    let mut hooks = serde_json::Map::new();
    for event in CLAUDE_HOOK_EVENTS {
        let mut entry = serde_json::Map::new();
        if MATCHED_EVENTS.contains(event) {
            entry.insert("matcher".to_string(), serde_json::json!("*"));
        }
        entry.insert(
            "hooks".to_string(),
            serde_json::json!([{
                "type": "command",
                "command": "archie hook",
                "async": true,
            }]),
        );
        hooks.insert(
            (*event).to_string(),
            serde_json::Value::Array(vec![serde_json::Value::Object(entry)]),
        );
    }
    serde_json::Value::Object(hooks)
}

fn note(verbose: bool, message: &str) {
    if verbose {
        eprintln!("archie hook: {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_snippet_registers_every_event_asynchronously() {
        let hooks = claude_hooks_value();
        let map = hooks.as_object().expect("an object");
        assert_eq!(map.len(), CLAUDE_HOOK_EVENTS.len());
        for (name, entries) in map {
            let entry = &entries.as_array().expect("an array")[0];
            let command = &entry["hooks"][0];
            assert_eq!(command["command"], "archie hook", "{name}");
            assert_eq!(command["async"], true, "{name}");
            assert_eq!(
                entry.get("matcher").is_some(),
                MATCHED_EVENTS.contains(&name.as_str()),
                "{name} matcher"
            );
        }
    }
}
