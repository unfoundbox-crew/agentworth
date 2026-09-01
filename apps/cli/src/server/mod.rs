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
use console::style;

pub use archaeology::*;
pub use live_tail::*;
pub use routes::*;
pub use static_files::*;

/// Default port used by AgentWorth local server.
pub const DEFAULT_PORT: u16 = 3000;

/// Starts the AgentWorth local API and Web UI server.
pub async fn start_server(
    storage: Arc<Storage>,
    port: u16,
    open_browser: bool,
    dist_dir: Option<PathBuf>,
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

    println!();
    println!(
        "{}",
        style("┌──────────────────────────────────────────────────────────┐").bold()
    );
    println!(
        "{}",
        style("│ 🚀 AgentWorth Local API & Explorer Server                │")
            .bold()
            .cyan()
    );
    println!(
        "{}",
        style("├──────────────────────────────────────────────────────────┤").bold()
    );
    println!(
        "│ Local URL:       {:<40}│",
        style(&url).bold().green().underlined()
    );
    println!(
        "│ API Stats:       {:<40}│",
        style(format!("{}/api/stats", url)).dim()
    );
    println!(
        "│ API Traces:      {:<40}│",
        style(format!("{}/api/traces", url)).dim()
    );
    println!(
        "│ API Archaeology: {:<40}│",
        style(format!("{}/api/archaeology", url)).dim()
    );
    println!(
        "│ Live Tail (SSE): {:<40}│",
        style(format!("{}/api/live-tail", url)).dim()
    );
    if let Some(path) = storage.db_path() {
        let path_str = path.to_string_lossy();
        let display_path = if path_str.len() > 38 {
            format!("...{}", &path_str[path_str.len() - 35..])
        } else {
            path_str.to_string()
        };
        println!("│ Database Index:  {:<40}│", style(display_path).dim());
    }
    println!(
        "{}",
        style("└──────────────────────────────────────────────────────────┘").bold()
    );
    println!("Press {} to stop server.", style("Ctrl+C").bold().yellow());
    println!();

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
