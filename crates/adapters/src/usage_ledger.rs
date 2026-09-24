//! Credit a streamed agent transcript's token usage once per logical message.

use agentworth_schema::TokenUsage;
use std::collections::HashMap;

/// Usage already credited to each message id seen so far in this file.
///
/// Several harnesses write ONE logical assistant message as several records, each repeating
/// the same usage block verbatim, so summing usage per record counts a message once per record
/// it happens to span. Measured on this machine 2026-09-10:
///
/// | harness | record shape | per-record | per message id | ratio |
/// | --- | --- | --- | --- | --- |
/// | Claude Code | one record per content block, `apiBlockIndex` 0..n, `message.id` repeated | 33,777,792 | 19,294,496 | 1.75x |
/// | Gemini CLI | one record per streamed revision, `id` repeated | 47,792,447 | 25,141,172 | 1.90x |
///
/// (Claude figures: a 1.08 MB subagent transcript, 455 records, 119 unique `message.id`.
/// Gemini figures: every `~/.gemini/tmp/*/chats/session-*.jsonl` carrying a `tokens` block,
/// 309 unique ids. Codex is NOT in this table -- its `token_count` events are cumulative
/// running totals and `codex.rs` already handles them with its own delta logic.)
///
/// So: credit each message id at most once, and let the LAST usage block seen for that id
/// win. `credit_delta` returns only what a record adds on top of what its id has already been
/// charged, which is zero for a verbatim repeat and the increment if a later record revises
/// the numbers upward. Only the usage is suppressed -- the record's own content (a tool call
/// on block 1, say) is still parsed, so nothing is dropped.
///
/// Records with no id (Claude Code's old flat `{"type":"assistant","usage":{...}}` shape, and
/// anything hand-written) are counted as they always were: no id means no way to tell a repeat
/// from a distinct message, and inventing one would be worse than the status quo.
#[derive(Default)]
pub(crate) struct UsageLedger {
    credited: HashMap<String, TokenUsage>,
}

impl UsageLedger {
    pub(crate) fn credit_delta(&mut self, message_id: Option<&str>, usage: TokenUsage) -> TokenUsage {
        let Some(id) = message_id else {
            return usage;
        };
        let already = self.credited.entry(id.to_string()).or_default();
        // Component-wise saturating subtraction: a later block that revises one counter
        // downward contributes nothing for that counter rather than wrapping around.
        let delta = TokenUsage::new(
            usage.input_tokens.saturating_sub(already.input_tokens),
            usage.output_tokens.saturating_sub(already.output_tokens),
            usage.cache_read_tokens.saturating_sub(already.cache_read_tokens),
            usage
                .cache_creation_tokens
                .saturating_sub(already.cache_creation_tokens),
        );
        *already = TokenUsage::new(
            already.input_tokens.max(usage.input_tokens),
            already.output_tokens.max(usage.output_tokens),
            already.cache_read_tokens.max(usage.cache_read_tokens),
            already.cache_creation_tokens.max(usage.cache_creation_tokens),
        );
        delta
    }
}
