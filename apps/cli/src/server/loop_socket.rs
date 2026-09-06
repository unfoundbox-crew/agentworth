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
use std::time::Duration;

use agentworth_loop::HookEvent;
use agentworth_storage::Storage;
use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

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
        let next = match tokio::time::timeout(READ_TIMEOUT, lines.next_line()).await {
            Ok(next) => next,
            Err(_) => {
                tracing::debug!("loop: a socket connection went quiet; dropping it");
                return;
            }
        };
        match next {
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
