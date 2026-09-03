//! AgentWorth local API and web server module.

pub mod archaeology;
pub mod live_tail;
pub mod routes;
pub mod static_files;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use agentworth_core::Scanner;
use agentworth_storage::Storage;
use anyhow::{Context, Result};

use crate::ui::views::{self, ServeView};
use crate::ui::Ui;

pub use archaeology::*;
pub use live_tail::*;
pub use routes::*;
pub use static_files::*;

/// Default port used by AgentWorth local server.
pub const DEFAULT_PORT: u16 = 3000;

/// The conventional build output location `archie serve` falls back to when no `--dist`
/// flag is given, relative to the process's current directory.
pub const DEFAULT_DIST_DIR: &str = "apps/dashboard/dist";

/// Resolves the dist directory `archie serve` should serve the web UI from.
///
/// An explicit `--dist` path is a user assertion, not a hint: if it doesn't exist, isn't a
/// directory, or has no `index.html`, this fails loudly rather than silently falling back to
/// the dashboard embedded in the binary (which previously served a 200 whose asset hashes
/// didn't match anything in the directory the user pointed at, which is indistinguishable from
/// `--dist` being entirely ignored). No `--dist` flag still probes the conventional
/// `apps/dashboard/dist` location as an opportunistic default -- that path was never asserted
/// by the user, so its absence is not an error.
pub fn resolve_dist_dir(explicit: Option<PathBuf>) -> Result<Option<PathBuf>> {
    match explicit {
        Some(dist) => {
            if !dist.exists() {
                anyhow::bail!(
                    "--dist path does not exist: {}",
                    dist.display()
                );
            }
            if !dist.is_dir() {
                anyhow::bail!(
                    "--dist path is not a directory: {}",
                    dist.display()
                );
            }
            if !dist.join("index.html").exists() {
                anyhow::bail!(
                    "--dist path has no index.html, so it doesn't look like a built web frontend: {}",
                    dist.display()
                );
            }
            Ok(Some(dist))
        }
        None => {
            let default_dist = PathBuf::from(DEFAULT_DIST_DIR);
            if default_dist.exists() {
                Ok(Some(default_dist))
            } else {
                Ok(None)
            }
        }
    }
}

/// Starts the AgentWorth local API and dashboard server.
pub async fn start_server(
    storage: Arc<Storage>,
    port: u16,
    open_browser: bool,
    dist_dir: Option<PathBuf>,
    ui: &Ui,
) -> Result<()> {
    let scanner = Arc::new(Scanner::new(storage.clone()));

    let (live_tail_tx, _) = tokio::sync::broadcast::channel(LIVE_TAIL_CHANNEL_CAPACITY);
    let watch_roots = discover_session_roots(&scanner);
    // Kept alive for the whole server lifetime by staying bound in this function's scope —
    // dropping it would silently stop the filesystem watch with no error anywhere.
    let _live_tail_watcher = match spawn_live_tail_watcher(&watch_roots, live_tail_tx.clone()) {
        Ok(watcher) => Some(watcher),
        Err(e) => {
            tracing::warn!("Failed to start live-tail filesystem watcher: {}", e);
            None
        }
    };

    let state = AppState {
        storage: storage.clone(),
        scanner,
        dist_dir: dist_dir.clone(),
        live_tail: live_tail_tx,
    };

    let app = create_router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind server on address {}", addr))?;

    let url = format!("http://localhost:{}", port);
    let index = storage.db_path().map(|p| p.to_string_lossy().to_string());

    print!(
        "{}",
        views::serve(
            ui,
            &ServeView {
                version: env!("CARGO_PKG_VERSION"),
                url: &url,
                index_path: index.as_deref(),
            }
        )
    );

    if open_browser {
        let open_url = url.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            if let Err(e) = open::that(&open_url) {
                tracing::warn!("Failed opening browser: {}", e);
            }
        });
    }

    axum::serve(listener, app)
        .await
        .context("Server exited with error")?;

    Ok(())
}
