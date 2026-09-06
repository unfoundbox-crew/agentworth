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

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agentworth_loop::HookEvent;
use agentworth_storage::Storage;
use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

use crate::loop_runtime::LoopRuntime;

/// How many events may queue between the socket and the database before a sender waits. Well
/// past a burst from every pane on the machine; the runtime drains it far faster than hooks
/// arrive.
const QUEUE: usize = 1024;

/// Binds the loop socket and starts draining it. Returns the bound path.
///
/// A stale socket file left by a killed server would make `bind` fail with `EADDRINUSE`
/// forever, so it is removed first: nothing else owns this path, and a live server holding it
/// is the case a second `archie serve` on the same index already conflicts on.
pub async fn start_loop_socket(storage: Arc<Storage>, path: PathBuf) -> Result<PathBuf> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)
        .with_context(|| format!("binding the loop socket at {}", path.display()))?;
    restrict(&path)?;

    let (tx, mut rx) = mpsc::channel::<HookEvent>(QUEUE);

    // One runtime, one owner. `spawn_blocking` per batch rather than per event so a busy
    // machine does not pay a thread hop for every keystroke-sized hook.
    let runtime_storage = storage.clone();
    tokio::spawn(async move {
        let mut runtime = LoopRuntime::new(runtime_storage);
        while let Some(event) = rx.recv().await {
            let mut batch = vec![event];
            while let Ok(next) = rx.try_recv() {
                batch.push(next);
                if batch.len() >= 64 {
                    break;
                }
            }
            runtime = match tokio::task::spawn_blocking(move || {
                for event in batch {
                    if let Err(e) = runtime.apply(event) {
                        tracing::warn!("loop: could not apply a hook event: {e:#}");
                    }
                }
                runtime
            })
            .await
            {
                Ok(runtime) => runtime,
                Err(e) => {
                    tracing::error!("loop: the runtime task failed: {e}");
                    return;
                }
            };
        }
    });

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let tx = tx.clone();
                    tokio::spawn(async move { read_connection(stream, tx).await });
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

async fn read_connection(stream: UnixStream, tx: mpsc::Sender<HookEvent>) {
    let mut lines = BufReader::new(stream).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<HookEvent>(&line) {
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
            Ok(None) => return,
            Err(e) => {
                tracing::warn!("loop: reading a socket connection failed: {e}");
                return;
            }
        }
    }
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
