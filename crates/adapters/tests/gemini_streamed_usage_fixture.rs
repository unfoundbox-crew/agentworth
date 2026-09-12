//! The Gemini adapter against the record shape Gemini CLI actually writes.
//!
//! `tests/fixtures/gemini-streamed-usage.jsonl` copies the shape of
//! `~/.gemini/tmp/<project>/chats/session-*.jsonl`, measured 2026-09-10. Two things about it
//! were wrong before this fixture existed:
//!
//! 1. The real counters live in a `tokens` block named `input` / `output` / `cached` /
//!    `thoughts` / `tool`. `extract_token_usage` only knew `promptTokenCount`-style and
//!    OpenAI-style names, so it returned all zeros and NO `ModelInvocation` was emitted --
//!    every Gemini CLI session on this machine indexed as zero tokens despite 25,141,172 real
//!    ones across its chat logs.
//! 2. Gemini CLI rewrites a message as it streams, repeating the whole `tokens` block on every
//!    revision under the same `id`. Summing per record gave 47,792,447 against a true
//!    25,141,172 -- 1.90x.
//!
//! The fixture holds both: `msg-fixture-2` appears twice with a REVISED (upward) usage block,
//! so the last one must win rather than the two being added.

use agentworth_adapter_sdk::{AgentAdapter, SessionSource};
use agentworth_adapters::GeminiAdapter;
use agentworth_schema::EventPayload;
use std::path::{Path, PathBuf};

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("gemini-streamed-usage.jsonl")
}

/// msg-fixture-2's LAST block (14,549 + 70 + 0 + 240 + 0 = 14,859) plus msg-fixture-3
/// (200 + 15 + 14,000 + 5 + 0 = 14,220).
const DEDUPED_TOTAL: u64 = 29_079;

/// Both of msg-fixture-2's revisions added together, plus msg-fixture-3.
const PER_RECORD_TOTAL: u64 = 43_758;

#[test]
fn test_gemini_cli_token_block_is_read_at_all() {
    let adapter = GeminiAdapter::new();
    let source = SessionSource::from_path(fixture_path(), adapter.name()).unwrap();
    let trace = adapter.parse(&source).expect("parse failed").trace;

    assert_ne!(
        trace.stats.token_usage.total(),
        0,
        "the `tokens` block Gemini CLI writes must be recognised, not silently read as zero"
    );
}

#[test]
fn test_repeated_id_takes_the_last_usage_block() {
    let adapter = GeminiAdapter::new();
    let source = SessionSource::from_path(fixture_path(), adapter.name()).unwrap();
    let trace = adapter.parse(&source).expect("parse failed").trace;

    assert_ne!(trace.stats.token_usage.total(), PER_RECORD_TOTAL);
    assert_eq!(trace.stats.token_usage.total(), DEDUPED_TOTAL);
    assert_eq!(trace.stats.token_usage.input_tokens, 14_749);
    // output + thoughts + tool, which is how Gemini's own `total` counts them.
    assert_eq!(trace.stats.token_usage.output_tokens, 330);
    assert_eq!(trace.stats.token_usage.cache_read_tokens, 14_000);

    // Three invocation events, not two: msg-fixture-2's second record REVISES its usage
    // upward, so it still contributes -- but only the 180-token increment, not another whole
    // 14,859. A verbatim repeat (Claude Code's shape) contributes a zero delta and emits no
    // event at all; see `claude_streamed_usage_fixture.rs`.
    let invocations = trace
        .events
        .iter()
        .filter(|e| matches!(e.payload, EventPayload::ModelInvocation { .. }))
        .count();
    assert_eq!(invocations, 3);
}
