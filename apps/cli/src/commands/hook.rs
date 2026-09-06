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

use agentworth_loop::{GateOutput, HookEvent, SpoolWriter};

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

/// The Claude Code events `archie hook print claude --govern` gates synchronously. Only two:
/// `PostToolBatch` is the one lever that stops the next model call, and `UserPromptSubmit` is
/// the one that can refuse to start a turn at all (docs/specs/governor.md, the rule table).
pub const CLAUDE_GATE_EVENTS: &[&str] = &["PostToolBatch", "UserPromptSubmit"];

/// How long a sync hook may take before Claude Code gives up on it. Well past the gate's own
/// 50 ms budget, so the budget is what actually bounds the agent's wait.
const GATE_TIMEOUT_SECONDS: u64 = 5;

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

/// The sync gate. Reads one hook payload, asks `archie serve` what to do, and does it -- or,
/// on any failure at all, exits 0 with nothing on stdout and one `gate_miss` line in the spool.
///
/// There is no fail-closed mode. A governor that can brick every agent on the machine when its
/// own server dies is a worse failure than one missed halt (docs/specs/governor.md).
pub fn run_gate_command(verbose: bool) -> anyhow::Result<()> {
    let mut input = String::new();
    if std::io::stdin()
        .lock()
        .take(MAX_STDIN_BYTES)
        .read_to_string(&mut input)
        .is_err()
    {
        return Ok(());
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&input) else {
        note(verbose, "stdin was not hook JSON");
        return Ok(());
    };
    let env: HashMap<String, String> = std::env::vars().collect();
    let Ok(event) = HookEvent::from_stdin_json(value.clone(), &env) else {
        note(verbose, "hook JSON had no session_id");
        return Ok(());
    };
    let batch_tools = value
        .get("tools")
        .and_then(|t| t.as_array())
        .cloned()
        .unwrap_or_default();
    let request = serde_json::json!({
        "gate": true,
        "event": event,
        "batch_tools": batch_tools,
    });

    let output = match ask_gate(&request.to_string()) {
        Some(output) => output,
        None => {
            // The miss is on the record, and the agent never knows it happened.
            let mut missed = value;
            if let Some(object) = missed.as_object_mut() {
                object.insert("gate_miss".to_string(), serde_json::Value::Bool(true));
                object.remove("tool_response");
            }
            if let Err(e) = append_raw(&spool_dir(verbose), &event.session_id, &missed) {
                note(verbose, &format!("could not record the gate miss: {e:#}"));
            }
            return Ok(());
        }
    };
    if let Some(stdout) = &output.stdout {
        println!("{stdout}");
    }
    if let Some(stderr) = &output.stderr {
        eprintln!("{stderr}");
    }
    if output.exit_code != 0 {
        std::process::exit(output.exit_code);
    }
    Ok(())
}

/// One round trip inside `SOCKET_BUDGET`: connect, write the line, read one reply line.
/// `None` for every failure, which is the whole point.
#[cfg(unix)]
fn ask_gate(line: &str) -> Option<GateOutput> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;

    let path = agentworth_storage::default_socket_path().ok()?;
    let budget = gate_budget();
    let mut stream = UnixStream::connect(&path).ok()?;
    stream.set_write_timeout(Some(budget)).ok()?;
    stream.set_read_timeout(Some(budget)).ok()?;
    stream.write_all(line.as_bytes()).ok()?;
    stream.write_all(b"\n").ok()?;
    stream.flush().ok()?;
    let mut reply = String::new();
    BufReader::new(&stream).read_line(&mut reply).ok()?;
    serde_json::from_str(reply.trim()).ok()
}

#[cfg(not(unix))]
fn ask_gate(_line: &str) -> Option<GateOutput> {
    None
}

/// The round-trip budget. `SOCKET_BUDGET` unless `ARCHIE_GATE_BUDGET_MS` says otherwise --
/// the escape hatch for a machine slow enough that 50 ms is a coin flip, and the knob the
/// integration test turns rather than racing the clock.
fn gate_budget() -> Duration {
    std::env::var("ARCHIE_GATE_BUDGET_MS")
        .ok()
        .and_then(|ms| ms.parse::<u64>().ok())
        .map_or(SOCKET_BUDGET, Duration::from_millis)
}

/// Appends one raw JSON line to the session's spool file. `SpoolWriter` takes a `HookEvent`;
/// a gate miss is not one -- it is the payload plus a flag -- so it goes down the same path
/// by hand rather than losing the flag to a round trip through the type.
fn append_raw(dir: &std::path::Path, session_id: &str, value: &serde_json::Value) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{session_id}.jsonl")))?;
    writeln!(file, "{value}")
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
pub fn print_claude_snippet(govern: bool) -> anyhow::Result<()> {
    let hooks = claude_hooks_value(govern);
    println!("// paste into the \"hooks\" object of ~/.claude/settings.json");
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({ "hooks": hooks }))?
    );
    Ok(())
}

/// The `hooks` object itself, so the test can parse what the command prints. With `govern`,
/// the async recorders are joined by the two sync gates -- a gate must not be `async`, because
/// an `async` hook cannot block and blocking is the entire point of it.
pub fn claude_hooks_value(govern: bool) -> serde_json::Value {
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
    if govern {
        for event in CLAUDE_GATE_EVENTS {
            let gate = serde_json::json!({
                "type": "command",
                "command": "archie hook --gate",
                "timeout": GATE_TIMEOUT_SECONDS,
            });
            match hooks.get_mut(*event).and_then(|e| e.as_array_mut()) {
                Some(entries) => entries.push(serde_json::json!({"hooks": [gate]})),
                None => {
                    hooks.insert(
                        (*event).to_string(),
                        serde_json::json!([{"hooks": [gate]}]),
                    );
                }
            }
        }
    }
    serde_json::Value::Object(hooks)
}

/// Codex's `hooks.json`, per learn.chatgpt.com/docs/hooks (read 2026-09-06). Three events run
/// the sync gate, because Codex has no batch hook: its brake is `PreToolUse` deny,
/// `PostToolUse` block, and the `UserPromptSubmit` refusal, and it is weaker than Claude
/// Code's for exactly that reason (docs/specs/governor.md, "On Codex").
pub const CODEX_HOOK_EVENTS: &[&str] = &[
    "PreToolUse",
    "PostToolUse",
    "UserPromptSubmit",
    "SessionStart",
    "Stop",
    "SessionEnd",
];

/// The Codex events that run `archie hook --gate` instead of the async recorder.
pub const CODEX_GATE_EVENTS: &[&str] = &["PreToolUse", "PostToolUse", "UserPromptSubmit"];

pub fn print_codex_snippet() -> anyhow::Result<()> {
    println!("// write to ~/.codex/hooks.json, or paste under [hooks] in ~/.codex/config.toml");
    println!("{}", serde_json::to_string_pretty(&codex_hooks_value())?);
    Ok(())
}

/// The whole `hooks.json` document, so the test can parse what the command prints.
pub fn codex_hooks_value() -> serde_json::Value {
    let mut hooks = serde_json::Map::new();
    for event in CODEX_HOOK_EVENTS {
        let gated = CODEX_GATE_EVENTS.contains(event);
        let mut command = serde_json::Map::new();
        command.insert("type".to_string(), serde_json::json!("command"));
        command.insert(
            "command".to_string(),
            serde_json::json!(if gated { "archie hook --gate" } else { "archie hook" }),
        );
        if gated {
            command.insert("timeout".to_string(), serde_json::json!(GATE_TIMEOUT_SECONDS));
        } else {
            command.insert("async".to_string(), serde_json::json!(true));
        }
        let mut entry = serde_json::Map::new();
        if *event == "PreToolUse" || *event == "PostToolUse" {
            entry.insert("matcher".to_string(), serde_json::json!("*"));
        }
        entry.insert(
            "hooks".to_string(),
            serde_json::Value::Array(vec![serde_json::Value::Object(command)]),
        );
        hooks.insert(
            (*event).to_string(),
            serde_json::Value::Array(vec![serde_json::Value::Object(entry)]),
        );
    }
    serde_json::json!({ "hooks": serde_json::Value::Object(hooks) })
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
        let hooks = claude_hooks_value(false);
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

    /// A gate that is `async` cannot block, and a governor that cannot block is a report.
    #[test]
    fn the_governed_snippet_adds_sync_gates_and_leaves_the_recorders_alone() {
        let hooks = claude_hooks_value(true);
        let map = hooks.as_object().expect("an object");
        assert!(map.contains_key("PostToolBatch"), "the batch gate is registered");
        for event in CLAUDE_GATE_EVENTS {
            let entries = map[*event].as_array().expect("an array");
            let gate = entries
                .iter()
                .find_map(|entry| {
                    entry["hooks"]
                        .as_array()?
                        .iter()
                        .find(|h| h["command"] == "archie hook --gate")
                })
                .unwrap_or_else(|| panic!("{event} has no gate"));
            assert!(gate.get("async").is_none(), "{event} gate must not be async");
            assert_eq!(gate["timeout"], GATE_TIMEOUT_SECONDS, "{event}");
        }
        let recorders = map["UserPromptSubmit"].as_array().expect("an array");
        assert!(
            recorders
                .iter()
                .any(|entry| entry["hooks"][0]["command"] == "archie hook"),
            "the async recorder survives beside the gate"
        );
    }

    #[test]
    fn the_codex_snippet_gates_the_three_events_codex_can_block_on() {
        let value = codex_hooks_value();
        let hooks = value["hooks"].as_object().expect("a hooks object");
        assert_eq!(hooks.len(), CODEX_HOOK_EVENTS.len());
        for (name, entries) in hooks {
            let command = &entries[0]["hooks"][0];
            if CODEX_GATE_EVENTS.contains(&name.as_str()) {
                assert_eq!(command["command"], "archie hook --gate", "{name}");
                assert!(command.get("async").is_none(), "{name}");
            } else {
                assert_eq!(command["command"], "archie hook", "{name}");
                assert_eq!(command["async"], true, "{name}");
            }
        }
    }

    /// Codex sends fields Claude Code does not. The parser has to keep them rather than fail.
    #[test]
    fn a_codex_payload_parses_with_its_own_extra_fields() {
        let event = HookEvent::from_stdin_json(
            serde_json::json!({
                "session_id": "0198f0aa-1c2d-7e3f-8a9b-0c1d2e3f4a5b",
                "hook_event_name": "PreToolUse",
                "turn_id": "turn_7",
                "model": "gpt-5.3-codex",
                "cwd": "/repo",
                "tool_name": "shell",
                "tool_input": {"command": ["cargo", "test"]}
            }),
            &HashMap::new(),
        )
        .expect("a Codex payload parses");
        assert_eq!(event.tool_name.as_deref(), Some("shell"));
        assert_eq!(event.raw["turn_id"], "turn_7");
        assert_eq!(event.raw["model"], "gpt-5.3-codex");
    }
}
