//! web_merge gate (decisions-v1 row W): one routes table serves every spelling of the
//! server command.
//!
//! - `archie serve`, `serve --open`, and the `web` alias all build the server from the
//!   same `create_router` table (snapshot below); `--open` only opens the browser.
//! - The dead `home` spelling 404s with a pointer to `serve --open`, on the CLI and on
//!   the `/home*` HTTP paths served without the deck.
//! - LAN is out: `serve` exposes no host/bind flag and the server binds loopback only.
//! - `include_raw` over non-loopback has no HTTP code path (`include_raw` exists only on
//!   MCP stdio tools; the HTTP server binds 127.0.0.1 with no flag to change that), so
//!   the forced-OFF gate is noted as blocked, not asserted here.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agentworth_cli::app::cli_command;
use agentworth_cli::server::{
    create_router, route_entries, AppState, LIVE_TAIL_CHANNEL_CAPACITY,
};
use agentworth_core::Scanner;
use agentworth_storage::Storage;
use assert_cmd::cargo::CommandCargoExt;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use predicates::str::contains;
use tower::ServiceExt;

/// The full `/api/*` table, method + path, in `route_entries()` order. Checked in: adding,
/// removing, or renaming a route must update this const, which is the snapshot.
const EXPECTED_API_ROUTES: &[(&str, &str)] = &[
    ("GET", "/stats"),
    ("GET", "/traces"),
    ("GET", "/traces/:id"),
    ("GET", "/traces/:id/events"),
    ("GET", "/usage"),
    ("GET", "/pacing"),
    ("GET", "/blame"),
    ("GET", "/matrix"),
    ("GET", "/archaeology"),
    ("GET", "/live-tail"),
    ("POST", "/scan"),
    ("GET", "/config"),
    ("POST", "/config"),
    ("POST", "/export/:id"),
];

/// Non-`/api` routes `create_router` registers: the home-deck trio plus the `/ws`
/// gateway. Everything else falls through to the dashboard SPA fallback.
const EXPECTED_NON_API_PATHS: &[&str] = &["/home", "/home/", "/home/any/inner/route", "/ws"];

fn test_router(home_deck_enabled: bool) -> axum::Router {
    let storage = Arc::new(Storage::open_in_memory().expect("open in-memory storage"));
    let scanner = Arc::new(Scanner::new(storage.clone()));
    let (live_tail_tx, _rx) = tokio::sync::broadcast::channel(LIVE_TAIL_CHANNEL_CAPACITY);
    let state = AppState {
        storage: storage.clone(),
        scanner: scanner.clone(),
        dist_dir: None,
        live_tail: live_tail_tx,
        #[cfg(unix)]
        home: None,
        home_deck_enabled,
    };
    create_router(state)
}

async fn raw(app: axum::Router, method: &str, uri: &str) -> (StatusCode, String) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(req).await.expect("execute request");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[test]
fn api_route_table_snapshot() {
    let actual: Vec<(&str, &str)> = route_entries()
        .iter()
        .map(|e| (e.method, e.path))
        .collect();
    assert_eq!(
        actual, EXPECTED_API_ROUTES,
        "route_entries() drifted from the checked-in web_merge snapshot"
    );
}

/// Every spelling builds its server from the one `create_router` table: the same probe set
/// answers identically with the deck off and on, except the `/home*` paths the deck gate
/// owns. `/api/live-tail` is excluded: its SSE stream never ends, so `oneshot` would hang.
#[tokio::test]
async fn one_table_across_spellings() {
    let probes = [
        ("GET", "/api/stats", StatusCode::OK),
        ("GET", "/api/traces", StatusCode::OK),
        ("GET", "/api/traces/does-not-exist", StatusCode::NOT_FOUND),
        (
            "GET",
            "/api/traces/does-not-exist/events",
            StatusCode::NOT_FOUND,
        ),
        ("GET", "/api/usage", StatusCode::OK),
        ("GET", "/api/pacing", StatusCode::OK),
        ("GET", "/api/blame?file=src/lib.rs", StatusCode::OK),
        ("GET", "/api/matrix", StatusCode::OK),
        ("GET", "/api/archaeology", StatusCode::OK),
        ("GET", "/api/config", StatusCode::OK),
    ];
    for (method, uri, expected) in probes {
        let (off_status, off_body) = raw(test_router(false), method, uri).await;
        let (on_status, on_body) = raw(test_router(true), method, uri).await;
        assert_eq!(
            (off_status, on_status),
            (expected, expected),
            "{method} {uri} must be {expected} under both deck states, served from one table"
        );
        assert_eq!(
            off_body, on_body,
            "{method} {uri} must answer identically with the deck off and on"
        );
    }

    // The fallback signature, captured dynamically: an unregistered path proves what
    // "no such route" looks like on this build (dist None, deck unbuilt or not).
    let (fallback_status, fallback_body) =
        raw(test_router(false), "GET", "/api/no-such-route-xyz").await;
    for near_miss in ["/hom", "/homee", "/api/trace", "/api/scans"] {
        let (status, body) = raw(test_router(false), "GET", near_miss).await;
        assert_eq!(
            (status, body),
            (fallback_status, fallback_body.clone()),
            "{near_miss} must fall through to the SPA fallback, not a registered route"
        );
    }

    // Every non-/api path in the snapshot is registered: none of them may answer with
    // the fallback signature.
    for path in EXPECTED_NON_API_PATHS {
        let (status, body) = raw(test_router(false), "GET", path).await;
        assert!(
            (status, body.clone()) != (fallback_status, fallback_body.clone()),
            "{path} must be a registered route, not the fallback"
        );
    }
}

/// `home` is dead: without the deck, `/home*` 404s with a pointer to `serve --open`.
#[tokio::test]
async fn home_paths_404_with_pointer_to_serve_open() {
    for path in ["/home", "/home/", "/home/any/inner/route"] {
        let (status, body) = raw(test_router(false), "GET", path).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "GET {path} without the deck must be 404"
        );
        assert!(
            body.contains("serve --open"),
            "GET {path} 404 must point at `serve --open`; got: {body}"
        );
    }
}

/// CLI grammar: `web` is a visible alias of `serve`, `serve --open` parses, `home` is a
/// hidden dead spelling, and `serve` exposes no host/bind flag (LAN is out).
#[test]
fn cli_spellings_and_lan_out() {
    let mut cmd = cli_command();
    cmd.build();

    let serve = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "serve")
        .expect("`serve` subcommand exists");
    assert!(
        serve.get_visible_aliases().any(|a| a == "web"),
        "`web` must be a visible alias of `serve`; aliases: {:?}",
        serve.get_aliases().collect::<Vec<_>>()
    );
    for flag in ["open", "port", "dist", "no-socket", "home"] {
        assert!(
            serve.get_arguments().any(|a| a.get_id() == flag),
            "`serve` keeps --{flag}"
        );
    }
    for banned in ["host", "bind", "listen", "address", "lan"] {
        assert!(
            !serve.get_arguments().any(|a| {
                let id = a.get_id();
                id == banned || a.get_long().is_some_and(|l| l == banned)
            }),
            "`serve` must expose no --{banned}: the server binds loopback only (LAN out)"
        );
    }

    let home = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "home")
        .expect("dead `home` spelling still parses so it can 404 with a pointer");
    assert!(
        home.is_hide_set(),
        "`home` is dead and must be hidden; the live spellings are `serve`, `serve --open`, `web`"
    );
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    listener.local_addr().expect("local_addr").port()
}

fn wait_for_port(port: u16, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        if Instant::now() > deadline {
            panic!("server never opened port {port} within {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// `archie web --help` is `serve --help` through the alias: exits 0, documents `--open`.
#[test]
fn web_alias_boots_serve_help() {
    let mut cmd = assert_cmd::Command::cargo_bin("archie").expect("find the archie binary");
    cmd.args(["web", "--help"]).timeout(Duration::from_secs(60));
    cmd.assert()
        .success()
        .stdout(contains("--open"))
        .stdout(contains("Automatically open the Web UI"));
}

/// `archie home` 404s with a pointer to `serve --open`: nonzero exit, pointer on stderr,
/// no port ever bound. The timeout only bites pre-fix, when `home` still starts a server.
#[test]
fn home_cli_404s_with_pointer_to_serve_open() {
    let port = free_port();
    let db_dir = tempfile::tempdir().expect("tempdir for the test index");
    let db_path = db_dir.path().join("web-merge-home-test.sqlite");

    let mut cmd = assert_cmd::Command::cargo_bin("archie").expect("find the archie binary");
    cmd.args([
        "--db-path",
        db_path.to_str().expect("utf8 tempdir path"),
        "home",
        "--no-open",
        "--port",
        &port.to_string(),
    ])
    .timeout(Duration::from_secs(15));
    cmd.assert()
        .failure()
        .stderr(contains("serve --open"));
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "dead `home` must not bind any port"
    );
}

/// The surviving deck spelling still serves the deck: `serve --home` answers `/home/` 200.
/// Kept here (not only in the renamed deck test) so this lane owns its proof end to end.
#[test]
fn serve_home_still_serves_the_deck() {
    let port = free_port();
    let db_dir = tempfile::tempdir().expect("tempdir for the test index");
    let db_path = db_dir.path().join("web-merge-serve-home-test.sqlite");

    let mut child = std::process::Command::cargo_bin("archie").expect("find the archie binary");
    child
        .args([
            "--db-path",
            db_path.to_str().expect("utf8 tempdir path"),
            "serve",
            "--home",
            "--port",
            &port.to_string(),
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let child = child.spawn().expect("spawn `archie serve --home`");
    struct Guard(std::process::Child);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _guard = Guard(child);

    wait_for_port(port, Duration::from_secs(15));

    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET /home/ HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).unwrap_or(0);
    let status: u16 = resp
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    assert_eq!(status, 200, "GET /home/ on `serve --home` must be 200; got:\n{resp}");
}
