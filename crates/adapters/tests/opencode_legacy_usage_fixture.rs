//! The OpenCode adapter's LEGACY file-based path, against a streamed-usage trap.
//!
//! OpenCode has migrated to `opencode.db`, where the `message` table's primary key gives one
//! row per logical message for free (8,538 rows, 8,538 distinct ids on this machine's 276 MB
//! database, measured 2026-09-10). The older JSON/JSONL path has no such guarantee: it is an
//! append-only log, so a message whose usage block is written more than once was summed twice.
//!
//! No install on this machine still uses that path, so unlike the Claude Code and Gemini
//! fixtures this one is synthetic rather than a redacted excerpt of real data -- it encodes the
//! shape, not a measurement. It exists so the legacy path cannot quietly regain the bug the
//! other two adapters just lost.

use agentworth_adapter_sdk::{AgentAdapter, SessionSource};
use agentworth_adapters::OpenCodeAdapter;
use std::path::{Path, PathBuf};

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("opencode-legacy-streamed-usage.jsonl")
}

/// msg_legacy_2 (1200 + 40 + 8000 + 300 = 9,540) counted once, plus msg_legacy_3
/// (50 + 12 + 9500 + 0 = 9,562).
const DEDUPED_TOTAL: u64 = 19_102;

/// msg_legacy_2 counted twice.
const PER_RECORD_TOTAL: u64 = 28_642;

#[test]
fn test_legacy_path_credits_a_repeated_message_id_once() {
    let adapter = OpenCodeAdapter::new();
    let source = SessionSource::from_path(fixture_path(), adapter.name()).unwrap();
    let trace = adapter.parse(&source).expect("parse failed").trace;

    assert_ne!(trace.stats.token_usage.total(), PER_RECORD_TOTAL);
    assert_eq!(trace.stats.token_usage.total(), DEDUPED_TOTAL);
    assert_eq!(trace.stats.token_usage.input_tokens, 1_250);
    assert_eq!(trace.stats.token_usage.output_tokens, 52);
    assert_eq!(trace.stats.token_usage.cache_read_tokens, 17_500);
    assert_eq!(trace.stats.token_usage.cache_creation_tokens, 300);
}
