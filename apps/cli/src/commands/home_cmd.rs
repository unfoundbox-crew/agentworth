//! `archie home`: one command that serves the built `apps/home` deck and opens it.
//!
//! Everything here is `archie serve --home` plus three conveniences a brand-new user
//! needs and a developer running `npm run dev` against a live gateway does not:
//! failing loudly when the deck was never built into this binary, opening the browser
//! at `/home/` instead of the API root, and not treating "another `archie serve` already
//! owns the loop socket" as fatal.

use std::time::Duration;

use anyhow::Result;

use crate::server::DEFAULT_PORT;
use crate::ui::Ui;

#[derive(Debug, Clone, PartialEq)]
pub struct HomeCommandArgs {
    pub port: u16,
    pub no_open: bool,
}

impl Default for HomeCommandArgs {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            no_open: false,
        }
    }
}

/// The herdr install page, read out of the `herdr` binary itself (`strings ~/.local/bin/herdr
/// | grep herdr.dev`: `Home:   https://herdr.dev`) rather than typed from memory -- herdr's
/// own `--help` output carries no URL at all.
const HERDR_INSTALL_URL: &str = "https://herdr.dev";

/// `archie home [--port N] [--no-open]`.
pub fn run_home_command(args: HomeCommandArgs, db_path: Option<std::path::PathBuf>, ui: &Ui) -> Result<()> {
    if !crate::server::embedded_home_deck_is_built() {
        eprintln!(
            "the deck was not built into this binary; run npm run build in apps/home and rebuild"
        );
        std::process::exit(1);
    }

    print_herdr_status_line();

    let storage = super::open_storage(db_path)?;
    let dist_dir = crate::server::resolve_dist_dir(None)?;

    #[cfg(unix)]
    let loop_socket = !another_server_already_owns_the_loop();
    #[cfg(not(unix))]
    let loop_socket = true;

    let port = args.port;
    println!("archie home: http://localhost:{port}/home/");

    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async move {
        if !args.no_open {
            let open_url = format!("http://localhost:{port}/home/");
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                if let Err(e) = open::that(&open_url) {
                    tracing::warn!("archie home: failed opening the browser: {e}");
                }
            });
        }
        // `open_browser: false` here -- the spawn above already opens `/home/`, and
        // `start_server`'s own opener would otherwise race it to open the bare API root too.
        crate::server::start_server(storage, port, false, dist_dir, loop_socket, true, ui).await
    })
}

/// Whether a running `archie serve` already holds the loop's Unix socket -- the same check
/// `loop_socket::start_loop_socket` makes before binding, done here first so `archie home`
/// can print its own one-line explanation instead of `start_server`'s generic warning.
#[cfg(unix)]
fn another_server_already_owns_the_loop() -> bool {
    let Ok(path) = agentworth_storage::default_socket_path() else {
        return false;
    };
    if !path.exists() {
        return false;
    }
    let held = std::os::unix::net::UnixStream::connect(&path).is_ok();
    if held {
        println!("archie home: another archie serve is already running and keeps the loop; this session starts without it");
    }
    held
}

/// One line about whether riders will have herdr to seat them. herdr is checked two ways --
/// on PATH at all, and its socket actually present (`~/.config/herdr/herdr.sock`, the same
/// path `server::home::gateway` dials) -- because an installed-but-never-run herdr looks
/// identical to a missing one from here. There is no plain-PTY rider fallback in this
/// codebase yet, so both cases get the same "go install it" line rather than a claim this
/// binary cannot back up.
fn print_herdr_status_line() {
    if herdr_reachable() {
        return;
    }
    println!("herdr not found: install it to seat riders -- {HERDR_INSTALL_URL}");
}

fn herdr_reachable() -> bool {
    let on_path = std::process::Command::new("herdr")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !on_path {
        return false;
    }
    herdr_socket_path().is_some_and(|p| p.exists())
}

fn herdr_socket_path() -> Option<std::path::PathBuf> {
    let base = directories::BaseDirs::new()?;
    Some(base.home_dir().join(".config").join("herdr").join("herdr.sock"))
}
