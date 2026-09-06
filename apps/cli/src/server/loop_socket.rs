//! The socket end of the loop: one Unix socket, one JSON line per hook event.
//!
//! A Unix socket rather than a port, for the reason docs/specs/loop.md gives: it is loopback
//! by construction, so there is nothing to bind, nothing to firewall and nothing to
//! authenticate. Framing is one JSON object per line, the same shape herdr's own socket uses,
//! and the connection is one-shot -- `archie hook` writes its line and closes.
//!
//! The listener is spawned beside axum in `start_server` and dies with it. The database work
//! happens on a blocking thread through a `LoopRuntime` owned by one task, so the state
//! machine never needs a lock of its own.
//!
//! Since v0.1.22 one line shape is not fire-and-forget. A line with `"gate": true` is a
//! question -- `archie hook --gate` is blocking the agent while it waits -- so it is answered
//! in the connection's own task and gets exactly one reply line back, a serialised
//! `GateOutput`. Everything else still goes down the queue and is never replied to.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use agentworth_loop::{GateOutput, GateRequest, HookEvent};
use agentworth_storage::Storage;
use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, Mutex};

use crate::loop_runtime::LoopRuntime;

/// How many events may queue between the socket and the database before a sender waits. Well
/// past a burst from every pane on the machine; the runtime drains it far faster than hooks
/// arrive.
const QUEUE: usize = 1024;

/// How long one connection may stay open without sending a line. `archie hook` writes one line
/// and closes, so anything holding the socket past this is not a hook, and every connection
/// held is a file descriptor this process cannot get back.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Binds the loop socket and starts draining it. Returns the bound path.
///
/// A socket file left behind by a killed server would make `bind` fail forever, so a stale one
/// is removed -- but only after asking it whether anyone is home. The two cases look identical
/// on disk and are opposite in consequence: unlinking a live server's socket takes the loop
/// down silently for every agent on the machine, and the second server then answers for hooks
/// the first one is still being told about.
pub async fn start_loop_socket(storage: Arc<Storage>, path: PathBuf) -> Result<PathBuf> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    if path.exists() {
        match std::os::unix::net::UnixStream::connect(&path) {
            Ok(_) => {
                return Err(anyhow!(
                    "another archie serve already owns the loop socket at {} -- stop it, or \
                     start this one with --no-socket",
                    path.display()
                ))
            }
            // Nobody is listening (ECONNREFUSED), or it went away between the two calls
            // (ENOENT): the file is a leftover and removing it is safe.
            Err(_) => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    let listener = UnixListener::bind(&path)
        .with_context(|| format!("binding the loop socket at {}", path.display()))?;
    restrict(&path)?;

    let (tx, mut rx) = mpsc::channel::<HookEvent>(QUEUE);

    // One runtime, two callers. The drain below owns it in the normal case; a gate request
    // takes the same lock so a decision is made against the same state machine that every
    // fire-and-forget event went into, and never against a second copy of it.
    let runtime = Arc::new(Mutex::new(LoopRuntime::new(storage.clone())));

    // `spawn_blocking` per batch rather than per event so a busy machine does not pay a thread
    // hop for every keystroke-sized hook.
    let drain = runtime.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            let mut batch = vec![event];
            while let Ok(next) = rx.try_recv() {
                batch.push(next);
                if batch.len() >= 64 {
                    break;
                }
            }
            let runtime = drain.clone();
            // The lock is taken per event, not per batch. A gate request waiting behind the
            // drain waits for one event, and a busy machine's batch of sixty-four cannot spend
            // the agent's whole 50 ms budget for it.
            let applied = tokio::task::spawn_blocking(move || {
                for event in batch {
                    let mut runtime = runtime.blocking_lock();
                    if let Err(e) = runtime.apply(event) {
                        tracing::warn!("loop: could not apply a hook event: {e:#}");
                    }
                }
            })
            .await;
            if let Err(e) = applied {
                tracing::error!("loop: the runtime task failed: {e}");
                return;
            }
        }
    });

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let tx = tx.clone();
                    let runtime = runtime.clone();
                    tokio::spawn(async move { read_connection(stream, tx, runtime).await });
                }
                Err(e) => {
                    tracing::warn!("loop: socket accept failed: {e}");
                    return;
                }
            }
        }
    });

    Ok(path)
}

async fn read_connection(
    stream: UnixStream,
    tx: mpsc::Sender<HookEvent>,
    runtime: Arc<Mutex<LoopRuntime>>,
) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        let read = match tokio::time::timeout(READ_TIMEOUT, reader.read_line(&mut line)).await {
            Ok(read) => read,
            Err(_) => {
                tracing::debug!("loop: a socket connection went quiet; dropping it");
                return;
            }
        };
        match read {
            Ok(0) => return,
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("loop: reading a socket connection failed: {e}");
                return;
            }
        }
        let text = line.trim().to_string();
        if text.is_empty() {
            continue;
        }
        if is_gate_line(&text) {
            let output = gate(&text, &runtime).await;
            let mut reply = match serde_json::to_string(&output) {
                Ok(reply) => reply,
                Err(e) => {
                    tracing::warn!("loop: a gate reply would not serialise: {e}");
                    return;
                }
            };
            reply.push('\n');
            if reader.get_mut().write_all(reply.as_bytes()).await.is_err() {
                return;
            }
            let _ = reader.get_mut().flush().await;
            continue;
        }
        match serde_json::from_str::<HookEvent>(&text) {
            // Dropped rather than queued forever: the writer already exited, and the
            // spool is the path that guarantees delivery.
            Ok(event) => {
                if tx.send(event).await.is_err() {
                    return;
                }
            }
            Err(e) => tracing::warn!("loop: a socket line would not parse: {e}"),
        }
    }
}

fn is_gate_line(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| value.get("gate").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

/// One gate request, answered in-line. Anything unparseable is `allow`: a governor that
/// answers a malformed line with a halt would stop an agent for its own bug.
async fn gate(text: &str, runtime: &Arc<Mutex<LoopRuntime>>) -> GateOutput {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return GateOutput::allow();
    };
    let Some(event) = value.get("event").cloned() else {
        return GateOutput::allow();
    };
    let Ok(event) = serde_json::from_value::<HookEvent>(event) else {
        return GateOutput::allow();
    };
    let mut request = GateRequest::new(event);
    if let Some(tools) = value.get("batch_tools").and_then(|t| t.as_array()) {
        request.batch_tools = tools.clone();
    }
    let runtime = runtime.clone();
    tokio::task::spawn_blocking(move || runtime.blocking_lock().decide(request))
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("loop: the gate task failed: {e}");
            GateOutput::allow()
        })
}

/// Owner-only. The socket accepts anything written to it, so on a shared machine the file
/// mode is the whole access control.
fn restrict(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restricting {}", path.display()))?;
    Ok(())
}

/// Drains whatever the hook spooled while no server was listening. Called once at startup, so
/// an event that arrived with `archie serve` down is late, never lost.
pub fn ingest_spool_at_startup(storage: Arc<Storage>) {
    let dir = match agentworth_storage::default_spool_dir() {
        Ok(dir) => dir,
        Err(e) => {
            tracing::warn!("loop: no spool directory: {e:#}");
            return;
        }
    };
    match LoopRuntime::new(storage).ingest_spool(&dir) {
        Ok(ingest) if ingest.events > 0 => {
            tracing::info!(
                "loop: ingested {} spooled event(s) from {} file(s)",
                ingest.events,
                ingest.files
            );
        }
        Ok(_) => {}
        Err(e) => tracing::warn!("loop: could not ingest the spool: {e:#}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage() -> Arc<Storage> {
        Arc::new(Storage::open_in_memory().expect("open storage"))
    }

    /// The two socket files that look identical on disk. A live one must never be unlinked:
    /// doing so takes the loop down for every agent on the machine and leaves the first server
    /// listening on a path nothing can reach.
    #[tokio::test]
    async fn a_live_socket_is_refused_and_a_dead_one_is_replaced() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("archie.sock");

        let bound = start_loop_socket(storage(), path.clone())
            .await
            .expect("the first server binds");
        assert_eq!(bound, path);

        let second = start_loop_socket(storage(), path.clone()).await;
        let error = second.expect_err("a second server must not take the socket").to_string();
        assert!(
            error.contains("another archie serve"),
            "the error names the cause: {error}"
        );
        assert!(path.exists(), "the live socket is still there");

        // A leftover file with nothing behind it is the opposite case, and is replaced.
        let stale = dir.path().join("stale.sock");
        std::fs::write(&stale, b"not a socket").expect("write");
        start_loop_socket(storage(), stale.clone())
            .await
            .expect("a stale socket file is replaced");
    }
}
