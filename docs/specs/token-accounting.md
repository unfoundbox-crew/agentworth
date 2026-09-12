# Token accounting

Status: built. Measured 2026-09-10, on this machine's own index.

Two separate things were wrong with the token numbers AgentWorth reports. One
was a bug. The other is a definition that was never written down.

## 1. One message, counted many times

Several harnesses write ONE logical assistant message as SEVERAL records, and
each record repeats the same usage block verbatim. Summing usage per record
counts a message once per record it happens to span.

| harness | record shape | per-record | per message id | ratio |
| :--- | :--- | ---: | ---: | ---: |
| Claude Code | one record per content block, `apiBlockIndex` 0..n, `message.id` repeated | 33,777,792 | 19,294,496 | 1.75x |
| Gemini CLI | one record per streamed revision, `id` repeated | 47,792,447 | 25,141,172 | 1.90x |
| Codex | cumulative running totals in `token_count` | — | — | already correct |
| OpenCode (SQLite) | one row per message, `id` is the primary key | — | — | structurally correct |

Claude figures are one real 1.08 MB subagent transcript: 455 records, 119 unique
`message.id`. Gemini figures are every `~/.gemini/tmp/*/chats/session-*.jsonl`
on this machine carrying a `tokens` block, 309 unique ids.

The fix is one shared ledger, `crates/adapters/src/usage_ledger.rs`: credit each
message id at most once and let the LAST usage block seen for that id win. A
record whose usage is a verbatim repeat contributes a zero delta and emits no
`ModelInvocation`; a record that revises its usage upward contributes only the
increment. The record's own content is still parsed either way — a tool call
sitting on block 1 is not dropped.

Codex is deliberately outside this. Its counters are cumulative running totals,
not per-message figures, and `codex.rs` already takes deltas of them. Plain
id-dedup would be the wrong fix there.

OpenCode's live path reads `opencode.db`, where the `message` table's primary
key gives one row per message for free — 8,538 rows, 8,538 distinct ids on this
machine's 276 MB database. Its older file-based path has no such guarantee and
now carries the same ledger, written from the shape rather than a measurement:
no install here still uses it.

A second Gemini bug turned up alongside this one. `extract_token_usage` knew
`promptTokenCount`-style and OpenAI-style field names but not the ones Gemini
CLI actually writes — `input` / `output` / `cached` / `thoughts` / `tool`,
inside a `tokens` block. Every Gemini CLI session on this machine indexed as
zero tokens despite 25M real ones. Fixing the dedup alone would have done
nothing; fixing the names alone would have introduced a fresh 1.90x inflation.
Both land together.

Adapters that changed bump `PARSER_VERSION`, so an incremental scan reparses
files whose bytes never changed: Claude Code 2 → 3, Gemini 1 → 2, OpenCode
1 → 2.

## 2. `total_tokens` is context volume, not spend

`total_tokens` is the raw sum of all four counters, and `cache_read_tokens`
enters at face value. On a long session that makes it a measure of how big the
context grew, not of what the session cost. The transcript above: 19,042,532 of
its 19,294,496 tokens — 98.7% — were cache reads of a prompt already paid for.

`total_tokens` keeps that meaning; changing it would break every stored row and
every client. What is new is that the four counters are now exposed separately
everywhere, alongside a weighted figure:

```
cost_weighted_tokens = input + output + 1.25 x cache_creation + 0.1 x cache_read
```

Those are Anthropic's published prompt-caching multipliers relative to the base
input price, verified 2026-09-10 against
<https://platform.claude.com/docs/en/docs/build-with-claude/prompt-caching>.

What it is not:

- **Not dollars.** A base token's price varies by model, and the weight does not
  know the model.
- **Not exact for every cache entry.** A 1-hour cache write is 2x, not 1.25x,
  and `cache_creation_tokens` does not record which TTL was bought — the common
  5-minute case is assumed. Fable 5.1 and Mythos 5.1 read cache at 0.025x rather
  than 0.1x.

It is a comparable weight for ranking sessions against each other. Read
`total_tokens` as context volume; rank spend by `cost_weighted_tokens`.

### Where it shows up

| surface | what changed |
| :--- | :--- |
| `session_list` / `/api/traces` | `SessionSummary` gains `input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_creation_tokens`, `cost_weighted_tokens` |
| `session_show` | same, through the same summary |
| `stats_usage` | rows gain `cost_weighted_tokens`; the CLI table gains CACHE WR and WEIGHTED columns |
| `stats_ladder` | `LadderSessionRow` gains `cost_weighted_tokens`; RECENT VERIFIED gains a WEIGHTED column beside TOKENS |

The gap between the TOKENS and WEIGHTED columns is the point. A session can top
the raw list purely by re-reading a large cached prompt.

Every new field is `#[serde(default)]`, so an older client reading a newer
server sees them appear rather than fail, and a newer client reading an older
server gets zeros rather than an error.
