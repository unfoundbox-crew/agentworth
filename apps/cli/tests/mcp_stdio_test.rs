//! Protocol-level integration test for `agentworth mcp` (docs/specs/mcp-server.md).
//!
//! This drives a real `rmcp` client against a real `rmcp` server speaking the real MCP wire
//! protocol (initialize, tools/list, tools/call) end to end -- the same code path `agentworth
//! mcp`'s stdio transport uses. The only substitution is the transport itself: a
//! `tokio::io::duplex` in-memory pipe pair in place of a real child process's stdin/stdout.
//! `rmcp::transport::stdio()` is just `(tokio::io::stdin(), tokio::io::stdout())` -- two
//! `AsyncRead`/`AsyncWrite` halves -- and a `tokio::io::duplex` half satisfies the exact same
//! trait bounds, so this exercises the identical framing and dispatch machinery a real stdio
//! subprocess would, without the flakiness of spawning and reaping a child process in CI.

use std::sync::Arc;

use agentworth_cli::AgentWorthMcpServer;
use agentworth_schema::{AgentWorthTrace, Provenance, TokenUsage};
use agentworth_storage::Storage;
use chrono::Utc;
use rmcp::model::CallToolRequestParams;
use rmcp::ServiceExt;

#[tokio::test]
async fn test_stdio_tools_list_and_sessions_find() {
    let storage = Storage::open_in_memory().expect("open in-memory storage");

    let prov = Provenance::new(
        "/Users/saurabh/code/unfoundbox/agentworth/sess-1.jsonl",
        "claude_code",
        100,
        12345,
        "fp_sess_1",
    );
    let mut trace = AgentWorthTrace::new("sess-1", "claude_code", prov, Utc::now());
    trace.stats.total_events = 5;
    trace.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
    storage.upsert_trace(&trace).expect("seed session");

    let server = AgentWorthMcpServer::new(Arc::new(storage));

    let (server_transport, client_transport) = tokio::io::duplex(8192);

    let server_handle = tokio::spawn(async move {
        server
            .serve(server_transport)
            .await
            .expect("server should start serving")
            .waiting()
            .await
            .expect("server should shut down cleanly");
    });

    let client = ()
        .serve(client_transport)
        .await
        .expect("client should complete the MCP handshake");

    let tools = client
        .list_tools(Default::default())
        .await
        .expect("tools/list should succeed");
    let tool_names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
    for expected in [
        "sessions_find",
        "session_get",
        "blame_find",
        "usage_summary",
        "pacing_window",
        "coverage_stats",
        "outcome_rate",
        "session_handoff",
    ] {
        assert!(
            tool_names.contains(&expected),
            "tools/list missing '{expected}': got {tool_names:?}"
        );
    }

    let call_result = client
        .call_tool(
            CallToolRequestParams::new("sessions_find")
                .with_arguments(serde_json::json!({ "limit": 10 }).as_object().unwrap().clone()),
        )
        .await
        .expect("tools/call sessions_find should succeed");

    assert_ne!(call_result.is_error, Some(true), "sessions_find returned an error result");

    let text = call_result
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.clone())
        .expect("sessions_find result should carry a text content block");
    let value: serde_json::Value = serde_json::from_str(&text).expect("result text should be JSON");
    let sessions = value["sessions"].as_array().expect("sessions array");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["session_id"], "sess-1");
    // source_path redaction is exercised in unit tests (apps/cli/src/mcp/tests.rs); here it's
    // enough to confirm the field survived the real wire round trip.
    assert!(sessions[0]["source_path"].as_str().unwrap().contains("[REDACTED_REPOSITORY]"));

    client.cancel().await.expect("client should shut down cleanly");
    server_handle.await.expect("server task should not panic");
}
