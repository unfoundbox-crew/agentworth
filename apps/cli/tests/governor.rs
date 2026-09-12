//! The brake, end to end through the real binary and a real `archie serve`
//! (docs/specs/governor.md, "The brake, v0.1.22").
//!
//! Everything here is the shipped path: a child `archie serve` binds the loop socket inside the
//! sandbox `HOME`, `archie hook` records events on it asynchronously, and `archie hook --gate`
//! asks it what to do and acts on the answer. The one thing the test controls that a real agent
//! does not is the transcript: it writes the assistant records the meter reads, because that is
//! where the token counts live.
//!
//! The gate has 50 ms by design. A debug build under a loaded CI box does not, so the budget is
//! raised through the same environment variable a slow machine would use -- the round trip is
//! what is under test here, not the clock.

use assert_cmd::Command;
use serde_json::{json, Value};
use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};
use tempfile::{tempdir, TempDir};

const SESSION: &str = "3b1f6a20-77c4-4e91-a0d8-5c2b9e13f742";

/// A `serve` that is killed however this test leaves: a passing assertion, a failing one, or a
/// panic. An orphaned child holding the socket would make every later run of this file fail.
struct Serve(Child);

impl Drop for Serve {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}

fn archie(home: &Path, db: &Path) -> Command {
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    cmd.env("HOME", home)
        .env("USERPROFILE", home)
        .env("ARCHIE_GATE_BUDGET_MS", "5000")
        .env_remove("HERDR_PANE_ID")
        .arg("--db-path")
        .arg(db);
    cmd
}

fn start_serve(home: &Path, db: &Path) -> Serve {
    start_serve_with(home, db, &[])
}

fn start_serve_with(home: &Path, db: &Path, env: &[(&str, &str)]) -> Serve {
    let bin = assert_cmd::cargo::cargo_bin("agentworth");
    let mut command = std::process::Command::new(bin);
    for (key, value) in env {
        command.env(key, value);
    }
    let child = command
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("HERDR_PANE_ID")
        .arg("--db-path")
        .arg(db)
        .arg("serve")
        .arg("--port")
        .arg(free_port().to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("archie serve starts");
    let serve = Serve(child);

    let socket = home.join(".agentworth").join("archie.sock");
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if socket.exists() && std::os::unix::net::UnixStream::connect(&socket).is_ok() {
            return serve;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("archie serve never bound {}", socket.display());
}

/// One async hook event, the v0.1.21 path. The socket queue is drained on another task, so the
/// gate that follows waits for the events it is about to be asked about.
fn hook(home: &Path, db: &Path, payload: Value) {
    archie(home, db)
        .arg("hook")
        .write_stdin(payload.to_string())
        .assert()
        .success()
        .stdout("");
    std::thread::sleep(Duration::from_millis(60));
}

/// One sync gate call: `(exit code, stdout as JSON if any, stderr)`.
fn gate(home: &Path, db: &Path, payload: Value) -> (i32, Option<Value>, String) {
    let output = archie(home, db)
        .arg("hook")
        .arg("--gate")
        .write_stdin(payload.to_string())
        .output()
        .expect("the gate runs");
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let parsed = if stdout.is_empty() {
        None
    } else {
        Some(serde_json::from_str(&stdout).expect("the gate printed JSON"))
    };
    (
        output.status.code().unwrap_or(-1),
        parsed,
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

fn sandbox() -> (TempDir, PathBuf, PathBuf, PathBuf) {
    let home = tempdir().unwrap();
    let db = home.path().join("index.db");
    let config = home.path().join(".agentworth");
    std::fs::create_dir_all(&config).unwrap();
    // Two rules, both blocking, both written by a person -- which is the only way either of
    // them turns on.
    std::fs::write(
        config.join("policy.toml"),
        "[thrash]\nedits = 2\naction = \"halt\"\n\n[spend]\ntokens = 5000\naction = \"halt\"\n",
    )
    .unwrap();
    let work = home.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let transcript = home.path().join("transcript.jsonl");
    std::fs::write(&transcript, "").unwrap();
    (home, db, work, transcript)
}

fn assistant(id: &str, input: u64, output: u64) -> String {
    json!({
        "type": "assistant",
        "timestamp": "2026-09-06T10:00:00.000Z",
        "message": {
            "model": "claude-fable-5-1",
            "id": id,
            "role": "assistant",
            "content": [{"type": "text", "text": "…"}],
            "usage": {
                "input_tokens": input,
                "output_tokens": output,
                "cache_read_input_tokens": 0,
                "cache_creation_input_tokens": 0
            }
        }
    })
    .to_string()
}

fn edit(work: &Path, transcript: &Path, file: &Path, id: &str) -> (Value, Value) {
    (
        json!({
            "session_id": SESSION, "hook_event_name": "PreToolUse", "cwd": work,
            "transcript_path": transcript, "tool_name": "Edit", "tool_use_id": id,
            "tool_input": {"file_path": file, "old_string": "a", "new_string": "b"},
        }),
        json!({
            "session_id": SESSION, "hook_event_name": "PostToolUse", "cwd": work,
            "transcript_path": transcript, "tool_name": "Edit", "tool_use_id": id,
            "tool_input": {"file_path": file}, "tool_response": "ok",
        }),
    )
}

#[test]
fn the_governor_halts_a_thrashing_session_then_a_spending_one_and_fails_open_when_serve_dies() {
    let (home, db, work, transcript) = sandbox();
    let home = home.path();
    let file = work.join("x.rs");
    std::fs::write(&file, "fn main() {}\n").unwrap();
    let serve = start_serve(home, &db);

    hook(
        home,
        &db,
        json!({
            "session_id": SESSION, "hook_event_name": "SessionStart", "cwd": work,
            "transcript_path": transcript,
        }),
    );

    let (pre, post) = edit(&work, &transcript, &file, "toolu_edit_1");
    hook(home, &db, pre);
    hook(home, &db, post);

    hook(
        home,
        &db,
        json!({
            "session_id": SESSION, "hook_event_name": "PreToolUse", "cwd": work,
            "transcript_path": transcript, "tool_name": "Bash", "tool_use_id": "toolu_test_1",
            "tool_input": {"command": "cargo test"},
        }),
    );
    hook(
        home,
        &db,
        json!({
            "session_id": SESSION, "hook_event_name": "PostToolUse", "cwd": work,
            "transcript_path": transcript, "tool_name": "Bash", "tool_use_id": "toolu_test_1",
            "tool_input": {"command": "cargo test"},
            "tool_response": {"is_error": true, "stdout": "running 3 tests\ntest result: FAILED. 1 failed"},
        }),
    );

    let (pre, post) = edit(&work, &transcript, &file, "toolu_edit_2");
    hook(home, &db, pre);
    hook(home, &db, post);

    // (b) two edits, nothing passing in between: the batch is the last thing before the next
    // model call, and it does not happen.
    let (code, stdout, _) = gate(
        home,
        &db,
        json!({
            "session_id": SESSION, "hook_event_name": "PostToolBatch", "cwd": work,
            "transcript_path": transcript,
            "tools": [{
                "tool_name": "Edit", "tool_use_id": "toolu_edit_2",
                "tool_input": {"file_path": file}
            }],
        }),
    );
    assert_eq!(code, 0, "a halt is exit 0 with a body, never a failed hook");
    let stdout = stdout.expect("the halt printed JSON");
    assert_eq!(stdout["continue"], false);
    let truth = stdout["additionalContext"].as_str().expect("ground truth");
    assert!(truth.contains("x.rs"), "the file is named: {truth}");
    assert!(
        truth.contains("edited 2 times"),
        "the edit the batch repeats is counted once, not twice: {truth}"
    );
    assert!(truth.contains("cargo test"), "the failing command is quoted: {truth}");
    assert!(
        truth.contains("test result: FAILED"),
        "its last output line, not the serialised result object: {truth}"
    );

    // The same truth arrives on the next prompt, so the model resumes knowing why it stopped.
    let (code, stdout, _) = gate(
        home,
        &db,
        json!({
            "session_id": SESSION, "hook_event_name": "UserPromptSubmit", "cwd": work,
            "transcript_path": transcript, "prompt": "carry on",
        }),
    );
    assert_eq!(code, 0);
    let resumed = stdout.expect("the prompt carried context");
    let resumed = resumed["additionalContext"].as_str().expect("ground truth");
    assert!(resumed.contains("x.rs"), "{resumed}");
    assert!(resumed.contains("cargo test"), "{resumed}");

    // (c) the meter, from the transcript the events name: two turns, 6,000 tokens, over a cap
    // of 5,000.
    {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&transcript)
            .unwrap();
        writeln!(file, "{}", assistant("msg_1", 3_000, 0)).unwrap();
        writeln!(file, "{}", assistant("msg_2", 2_900, 100)).unwrap();
    }
    hook(
        home,
        &db,
        json!({
            "session_id": SESSION, "hook_event_name": "PreToolUse", "cwd": work,
            "transcript_path": transcript, "tool_name": "Read", "tool_use_id": "toolu_read_1",
            "tool_input": {"file_path": file},
        }),
    );

    let (code, stdout, _) = gate(
        home,
        &db,
        json!({
            "session_id": SESSION, "hook_event_name": "PostToolBatch", "cwd": work,
            "transcript_path": transcript, "tools": [],
        }),
    );
    assert_eq!(code, 0);
    let stdout = stdout.expect("the spend halt printed JSON");
    assert_eq!(stdout["continue"], false);
    let message = stdout["systemMessage"].as_str().unwrap_or_default();
    let truth = stdout["additionalContext"].as_str().unwrap_or_default();
    assert!(
        message.contains("5000") || truth.contains("5000"),
        "the cap is named: {message} / {truth}"
    );

    // Suspended: every prompt is refused before any model call, and stderr is the reason.
    let (code, stdout, stderr) = gate(
        home,
        &db,
        json!({
            "session_id": SESSION, "hook_event_name": "UserPromptSubmit", "cwd": work,
            "transcript_path": transcript, "prompt": "keep going",
        }),
    );
    assert_eq!(code, 2, "exit 2 blocks the prompt");
    assert!(stdout.is_none(), "a blocked prompt says nothing on stdout");
    assert!(!stderr.trim().is_empty(), "the person is told why");
    assert!(
        stderr.contains(&format!("archie policy lift {SESSION}")),
        "and how to unblock it: {stderr}"
    );
    assert!(stderr.contains("policy.toml"), "or how to raise the cap: {stderr}");

    archie(home, &db)
        .arg("policy")
        .arg("lift")
        .arg(SESSION)
        .assert()
        .success();

    let (code, _, _) = gate(
        home,
        &db,
        json!({
            "session_id": SESSION, "hook_event_name": "UserPromptSubmit", "cwd": work,
            "transcript_path": transcript, "prompt": "keep going",
        }),
    );
    assert_eq!(code, 0, "a lifted session can prompt again");

    // A lift buys another cap's worth of work, not one prompt: the cap is measured from the
    // 6,000 tokens already spent, so the next batch is allowed.
    let (code, stdout, _) = gate(
        home,
        &db,
        json!({
            "session_id": SESSION, "hook_event_name": "PostToolBatch", "cwd": work,
            "transcript_path": transcript, "tools": [],
        }),
    );
    assert_eq!(code, 0);
    // The thrash rule still holds -- two edits, nothing passing -- but the spend rule does not,
    // so the session is not suspended again and its next prompt goes through.
    let message = stdout
        .as_ref()
        .and_then(|s| s["systemMessage"].as_str())
        .unwrap_or_default();
    assert!(
        !message.contains("cap"),
        "a lifted session is not re-suspended on its next batch: {message}"
    );
    let (code, _, _) = gate(
        home,
        &db,
        json!({
            "session_id": SESSION, "hook_event_name": "UserPromptSubmit", "cwd": work,
            "transcript_path": transcript, "prompt": "still going",
        }),
    );
    assert_eq!(code, 0, "and it can still submit a prompt");

    // Another 5,100 tokens past the lift, and it halts again -- naming where the lift was.
    {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&transcript)
            .unwrap();
        writeln!(file, "{}", assistant("msg_3", 5_100, 0)).unwrap();
    }
    hook(
        home,
        &db,
        json!({
            "session_id": SESSION, "hook_event_name": "PreToolUse", "cwd": work,
            "transcript_path": transcript, "tool_name": "Read", "tool_use_id": "toolu_read_2",
            "tool_input": {"file_path": file},
        }),
    );
    let (_, stdout, _) = gate(
        home,
        &db,
        json!({
            "session_id": SESSION, "hook_event_name": "PostToolBatch", "cwd": work,
            "transcript_path": transcript, "tools": [],
        }),
    );
    let stdout = stdout.expect("the cap trips again past the lift");
    assert_eq!(stdout["continue"], false);
    let truth = stdout["additionalContext"].as_str().unwrap_or_default();
    assert!(
        truth.contains("Lifted at 6000 tokens") && truth.contains("11000"),
        "the halt says where the lift was and where the next one is: {truth}"
    );

    let burn = archie(home, &db)
        .arg("session")
        .arg("burn")
        .arg(SESSION)
        .arg("--json")
        .output()
        .expect("burn runs");
    let burn: Value = serde_json::from_slice(&burn.stdout).expect("burn printed JSON");
    assert_eq!(burn["tokens"], 11_100, "every turn counted once, none twice");
    assert_eq!(burn["turns"], 3);

    // (d) with nothing listening the gate is invisible to the agent, and the miss is a row.
    drop(serve);
    std::thread::sleep(Duration::from_millis(300));
    let (code, stdout, stderr) = gate(
        home,
        &db,
        json!({
            "session_id": SESSION, "hook_event_name": "PostToolBatch", "cwd": work,
            "transcript_path": transcript, "tools": [],
        }),
    );
    assert_eq!(code, 0, "a dead server never fails the agent");
    assert!(stdout.is_none(), "and never speaks to the model");
    assert!(stderr.trim().is_empty());

    let spooled = std::fs::read_to_string(
        home.join(".agentworth")
            .join("spool")
            .join(format!("{SESSION}.jsonl")),
    )
    .expect("the spool file exists");
    assert!(
        spooled.lines().any(|line| {
            serde_json::from_str::<Value>(line)
                .ok()
                .is_some_and(|v| v["gate_miss"] == true)
        }),
        "the miss is on record: {spooled}"
    );
}

/// A runtime slower than the agent's budget must cost the agent the budget and nothing more.
/// The gate is one deadline across connect, write and read -- not 50 ms for each of them.
#[test]
fn a_runtime_slower_than_the_budget_fails_open_inside_it() {
    let (home, db, work, transcript) = sandbox();
    let home = home.path();
    let _serve = start_serve_with(home, &db, &[("ARCHIE_GATE_TEST_DELAY_MS", "3000")]);

    let started = Instant::now();
    let output = Command::cargo_bin("agentworth")
        .unwrap()
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("ARCHIE_GATE_BUDGET_MS", "50")
        .env_remove("HERDR_PANE_ID")
        .arg("--db-path")
        .arg(&db)
        .arg("hook")
        .arg("--gate")
        .write_stdin(
            json!({
                "session_id": SESSION, "hook_event_name": "PostToolBatch", "cwd": work,
                "transcript_path": transcript, "tools": [],
            })
            .to_string(),
        )
        .output()
        .expect("the gate runs");
    let elapsed = started.elapsed();

    assert!(output.status.success(), "a slow server never fails the agent");
    assert!(
        output.stdout.is_empty(),
        "and never speaks to the model: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        elapsed < Duration::from_millis(1_500),
        "the whole round trip is one budget, not one per step: {elapsed:?}"
    );
}

/// (e) The Codex snippet: three gated events, and `PreToolUse` among them -- Codex has no batch
/// hook, so its brake is per call.
#[test]
fn the_codex_snippet_parses_and_gates_pre_tool_use() {
    let (home, db, _work, _transcript) = sandbox();
    let output = archie(home.path(), &db)
        .arg("hook")
        .arg("print")
        .arg("codex")
        .output()
        .expect("the snippet prints");
    let text = String::from_utf8(output.stdout).unwrap();
    let (comment, snippet) = text.split_once('\n').expect("a comment line then the JSON");
    assert!(comment.contains("hooks.json"), "{comment}");
    let value: Value = serde_json::from_str(snippet).expect("the snippet is JSON");
    let hooks = value["hooks"].as_object().expect("a hooks object");
    let pre = &hooks["PreToolUse"][0]["hooks"][0];
    assert_eq!(pre["command"], "archie hook --gate");
    assert!(pre.get("async").is_none(), "a gate cannot be async");
    assert_eq!(hooks["SessionStart"][0]["hooks"][0]["async"], true);
}
