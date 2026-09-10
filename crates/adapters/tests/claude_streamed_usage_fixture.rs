//! The Claude Code adapter against a transcript where one assistant message is written as
//! several streamed records.
//!
//! `tests/fixtures/claude-streamed-usage.jsonl` is a redacted excerpt of the real shape found
//! in `~/.claude/projects/.../subagents/agent-aa143e0ee8e834d0e.jsonl` (1.08 MB, measured
//! 2026-09-10): Claude Code splits one assistant message across one record per content block,
//! each carrying `apiBlockIndex` 0, 1, ... and each repeating the SAME `message.usage` block
//! verbatim. On that real file, 455 records held 119 unique `message.id` values; summing usage
//! once per record gave 33,777,792 tokens where the true figure is 19,294,496.
//!
//! The fixture keeps that trap in miniature: `msg_fixture_0001` appears twice (blocks 0 and 1)
//! with identical usage, `msg_fixture_0002` once. Counting per record gives 239,362; counting
//! each `message.id` once gives 159,612.

use agentworth_adapter_sdk::{AgentAdapter, SessionSource};
use agentworth_adapters::ClaudeCodeAdapter;
use agentworth_schema::EventPayload;
use std::path::{Path, PathBuf};

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("claude-streamed-usage.jsonl")
}

/// The sum of the two unique messages' usage blocks:
///   msg_fixture_0001 = 2 + 2 + 37_473 + 42_273 = 79_750
///   msg_fixture_0002 = 5 + 11 + 79_746 +    100 = 79_862
const DEDUPED_TOTAL: u64 = 159_612;

/// What the old parser produced: `msg_fixture_0001` counted twice.
const PER_RECORD_TOTAL: u64 = 239_362;

#[test]
fn test_repeated_message_id_is_counted_once() {
    let adapter = ClaudeCodeAdapter::new();
    let source = SessionSource::from_path(fixture_path(), adapter.name()).unwrap();
    let trace = adapter.parse(&source).expect("parse failed").trace;

    assert_ne!(
        trace.stats.token_usage.total(),
        PER_RECORD_TOTAL,
        "a repeated message.id must not be summed once per streamed record"
    );
    assert_eq!(trace.stats.token_usage.total(), DEDUPED_TOTAL);
    assert_eq!(trace.stats.token_usage.input_tokens, 7);
    assert_eq!(trace.stats.token_usage.output_tokens, 13);
    assert_eq!(trace.stats.token_usage.cache_read_tokens, 117_219);
    assert_eq!(trace.stats.token_usage.cache_creation_tokens, 42_373);
}

#[test]
fn test_one_model_invocation_per_unique_message_id() {
    let adapter = ClaudeCodeAdapter::new();
    let source = SessionSource::from_path(fixture_path(), adapter.name()).unwrap();
    let trace = adapter.parse(&source).expect("parse failed").trace;

    let invocations = trace
        .events
        .iter()
        .filter(|e| matches!(e.payload, EventPayload::ModelInvocation { .. }))
        .count();
    assert_eq!(
        invocations, 2,
        "two unique message ids, three usage-bearing records"
    );
}

/// The tool call carried on the SECOND record of a deduplicated message must still be parsed.
/// Dropping the whole record instead of just its usage would lose it.
#[test]
fn test_dedup_does_not_drop_the_repeat_records_content() {
    let adapter = ClaudeCodeAdapter::new();
    let source = SessionSource::from_path(fixture_path(), adapter.name()).unwrap();
    let trace = adapter.parse(&source).expect("parse failed").trace;

    assert_eq!(trace.stats.tool_calls_count, 1);
    assert_eq!(trace.stats.tools_used.get("Bash"), Some(&1));
}

/// The cost-weighted figure applies Anthropic's published cache multipliers, so a session
/// dominated by cache reads no longer reads as if it spent 19M tokens' worth of money.
#[test]
fn test_cost_weighted_total_discounts_cache_reads() {
    let adapter = ClaudeCodeAdapter::new();
    let source = SessionSource::from_path(fixture_path(), adapter.name()).unwrap();
    let trace = adapter.parse(&source).expect("parse failed").trace;

    // 7 + 13 + 1.25 * 42_373 + 0.1 * 117_219 = 64_708.15 -> 64_708
    assert_eq!(trace.stats.token_usage.cost_weighted_total(), 64_708);
    assert!(
        trace.stats.token_usage.cost_weighted_total() < trace.stats.token_usage.total(),
        "cache-read-heavy sessions must weigh less than their raw token count"
    );
}
