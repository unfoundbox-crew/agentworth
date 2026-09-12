//! Integration test for `archie home`: spawns the real `archie` binary as a child process on
//! a free port with `--no-open`, and asserts `/home/` answers with the deck's built
//! `index.html` and `/ws` completes a real WebSocket handshake -- the two things a brand-new
//! user's browser needs, checked against the actual server rather than a router unit test.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::cargo::CommandCargoExt;

/// Binding to port 0 and reading back the OS-assigned port, then dropping the listener before
/// `archie home` binds it itself. A small race (something else grabs the port in between) is
/// acceptable for a local test run; retrying would add more flakiness than it removes.
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
    listener.local_addr().expect("local_addr").port()
}

/// Kills the child on drop, including on a failed assertion -- otherwise a failing test run
/// leaves an `archie home` process bound to the test's port.
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_port(port: u16, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        if Instant::now() > deadline {
            panic!("archie home never opened port {port} within {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A minimal blocking HTTP/1.1 GET over a raw socket -- avoids pulling reqwest's async runtime
/// into what is otherwise a synchronous test. Returns (status code, full response text).
fn http_get(port: u16, path: &str) -> (u16, String) {
    let mut stream =
        TcpStream::connect(("127.0.0.1", port)).unwrap_or_else(|e| panic!("connect to {path}: {e}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).unwrap_or(0);
    let status = resp
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    (status, resp)
}

#[test]
fn archie_home_serves_the_deck_and_upgrades_ws() {
    let port = free_port();
    let db_dir = tempfile::tempdir().expect("tempdir for the test index");
    let db_path = db_dir.path().join("home-command-test.sqlite");

    let mut cmd = Command::cargo_bin("archie").expect("find the archie binary");
    cmd.args([
        "--db-path",
        db_path.to_str().expect("utf8 tempdir path"),
        "home",
        "--no-open",
        "--port",
        &port.to_string(),
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

    let child = cmd.spawn().expect("spawn `archie home`");
    let _guard = ChildGuard(child);

    wait_for_port(port, Duration::from_secs(10));

    let (status, body) = http_get(port, "/home/");
    assert_eq!(status, 200, "GET /home/ should be 200; full response:\n{body}");
    assert!(
        body.contains("id=\"root\"") && body.contains("/home/assets/"),
        "expected the built apps/home deck's index.html (id=\"root\", /home/assets/ script \
         tag) -- did `npm run build` run in apps/home before this test compiled the binary? \
         full response:\n{body}"
    );

    // axum's WebSocketUpgrade extractor requires Connection: Upgrade, Upgrade: websocket, a
    // Sec-WebSocket-Version of 13, and *some* base64 Sec-WebSocket-Key -- this one is the
    // example key from RFC 6455 section 1.2, not anything the server inspects for content.
    let mut ws_stream =
        TcpStream::connect(("127.0.0.1", port)).expect("connect for the websocket handshake");
    ws_stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let ws_req = format!(
        "GET /ws HTTP/1.1\r\n\
         Host: 127.0.0.1:{port}\r\n\
         Connection: Upgrade\r\n\
         Upgrade: websocket\r\n\
         Sec-WebSocket-Version: 13\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
    );
    ws_stream.write_all(ws_req.as_bytes()).unwrap();
    let mut buf = [0u8; 512];
    let n = ws_stream.read(&mut buf).unwrap_or(0);
    let ws_resp = String::from_utf8_lossy(&buf[..n]);
    assert!(
        ws_resp.starts_with("HTTP/1.1 101"),
        "expected a 101 websocket upgrade on /ws (archie home implies --home); got:\n{ws_resp}"
    );
}
