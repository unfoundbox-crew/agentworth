//! AgentWorth local API and web server module.

pub mod archaeology;
#[cfg(unix)]
pub mod home;
pub mod live_tail;
#[cfg(unix)]
pub mod loop_socket;
pub mod routes;
pub mod static_files;

use std::net::SocketAddr;
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

/// Starts the AgentWorth local API server. `/` serves the `apps/home` deck when it was
/// built into the binary, otherwise the JSON API root; the deck's static assets need no
/// on-disk dist directory (the legacy `--dist` flag left with the v0.1.27 dashboard).
#[allow(clippy::too_many_arguments)]
pub async fn start_server(
    storage: Arc<Storage>,
    port: u16,
    open_browser: bool,
    loop_socket: bool,
    home: bool,
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

    // The gateway behind `apps/home` (docs: apps/home/DESIGN.md). Unix-only, same as the loop
    // socket, since both dial a Unix socket (herdr's, in this case) directly.
    #[cfg(not(unix))]
    if home {
        tracing::warn!("--home is unix-only; the /ws route will not be started");
    }
    #[cfg(not(unix))]
    let _ = home;

    #[cfg(unix)]
    let home_handle = if home {
        Some(home::spawn(storage.clone(), scanner.clone(), live_tail_tx.clone()))
    } else {
        None
    };

    let state = AppState {
        storage: storage.clone(),
        scanner,
        live_tail: live_tail_tx,
        #[cfg(unix)]
        home: home_handle,
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

    // The loop's own listener, beside axum rather than inside it: hook events are one JSON
    // line on a Unix socket, not HTTP, and the spool they fall back to has to be drained by
    // whoever comes up first (docs/specs/loop.md section 1).
    #[cfg(unix)]
    if loop_socket {
        let spool_storage = storage.clone();
        tokio::task::spawn_blocking(move || loop_socket::ingest_spool_at_startup(spool_storage));
        match agentworth_storage::default_socket_path() {
            Ok(socket_path) => {
                match loop_socket::start_loop_socket(storage.clone(), socket_path).await {
                    Ok(bound) => println!("Loop socket {}", bound.display()),
                    Err(e) => tracing::warn!("loop: socket not started: {e:#}"),
                }
            }
            Err(e) => tracing::warn!("loop: no socket path: {e:#}"),
        }
    }
    #[cfg(not(unix))]
    let _ = loop_socket;

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
