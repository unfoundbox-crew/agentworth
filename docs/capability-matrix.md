# Capability matrix

Internal. Not the README, not the site. This is the honest version of the
adapter count, measured on one machine 2026-09-02.

The README says twenty adapters. That is true and it is not the number anyone
should plan against. Nine of them have ever produced a row here, two produce
tokens, two produce outcomes, and one produces everything.

Every count on this page is from the index as it stood on 2026-09-02. The Codex
adapter was fixed after that snapshot (#114) and its rows are stale until a
rescan reparses them; the Codex entries below say so where it matters. Nothing
else here has been re-measured since.

## What was measured, and against what

Two sources, cross-checked.

**Self-reported.** `npx -y agentworth@latest matrix --json` on the shipped
0.1.13 binary. It lists **20 adapters**, not 21 — `crates/adapters/src/` holds
22 files, of which `lib.rs` is the module root and `mcp.rs` is a tool-name
normalizer, not an adapter. Eleven report `is_detected: true`.

**Observed.** A copy of the local index: 10,329 session rows, 2,960 non-stub
(`total_events > 1 AND total_tokens > 0`), 25,206 `file_modifications` rows,
spanning 2026-06-21 to 2026-09-01.

```sql
SELECT adapter, COUNT(*) n,
  SUM(total_events > 1) ev, SUM(total_tokens > 0) tok,
  ROUND(AVG(total_events),1) avg_ev,
  SUM(tool_calls_count > 0) tools,
  SUM(primary_outcome IS NOT NULL) outcome,
  SUM(primary_outcome IN ('test_or_build_passed','commit_observed',
      'ci_or_deployment_verified')) verified,
  SUM(composite_score IS NOT NULL) scored,
  SUM(prompt_preview IS NOT NULL) preview
FROM sessions GROUP BY adapter ORDER BY n DESC;
```

The index copy predates #68, so its counts include non-session rows that a
current full scan now prunes at discovery. Where that matters, it is called
out below.

## Observed

| adapter | rows | >1 event | tokens | avg ev | tools | any outcome | verified | scored | prompt |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| claude_code | 6,374 | 3,015 | 2,888 | 198.1 | 2,699 | 1,426 | 983 | 5,865 | 0 |
| codex | 1,156 | 874 | 1 | 218.3 | 0 | 0 | 0 | 1,142 | 0 |
| hermes | 982 | 47 | 0 | 17.7 | 0 | 0 | 0 | 982 | 0 |
| antigravity | 785 | 407 | 0 | 13.3 | 353 | 48 | 20 | 694 | 0 |
| cursor | 707 | 62 | 0 | 5.8 | 0 | 0 | 0 | 705 | 0 |
| grok | 90 | 34 | 0 | 179.8 | 1 | 1 | 0 | 88 | 0 |
| opencode | 86 | 81 | 71 | 385.5 | 65 | 38 | 19 | 85 | 0 |
| pi | 82 | 37 | 0 | 84.5 | 0 | 0 | 0 | 82 | 0 |
| gemini | 67 | 44 | 0 | 25.2 | 23 | 0 | 0 | 31 | 0 |

Eight of the twenty adapters appear above. `antigravity` is a ninth name the
index uses and the matrix does not list. Twelve produced no row at all: aider,
cline, deepseek, goose, herdr, kimi, manus, minimax, openclaw, qwen, windsurf,
zhipu. Of those, `goose`, `herdr` and `openclaw` report `is_detected: true` and
still contributed nothing.

Compaction is absent from this table. The index copy predates #62 and has no
`compaction_count` column, so per-adapter compaction coverage cannot be stated
from it. `compaction.md`'s numbers came from parsing raw JSONL directly.

`prompt_preview` is zero in every row. The column exists, nothing writes it.

## Self-reported, and where it disagrees

The shipped matrix claims token and outcome extraction for exactly three
adapters: `claude_code`, `codex`, `gemini`. The index disagrees with two of
those three.

| adapter | matrix claims tokens | rows with tokens | matrix claims outcomes | rows with outcomes |
| :--- | :--- | ---: | :--- | ---: |
| claude_code | yes | 2,888 | yes | 1,426 |
| codex | yes | **1** | yes | **0** |
| gemini | yes | **0** | yes | **0** |
| opencode | **no** | 71 | **no** | 38 |
| antigravity | not listed at all | 0 | not listed at all | 48 |

Three separate failures in one table:

1. **`codex` claims tokens and produces one row with any.** 1,156 sessions,
   874 of them with real event counts, one with a token total. Cause found and
   fixed in #114: the parser read a top-level `usage`/`model` that Codex
   rollouts do not have. Every field sits under `payload`, keyed by a top-level
   `type` — `turn_context` carries `model` and `effort`, `event_msg` /
   `token_count` carries the counters, `session_meta` carries `cwd`. Measured
   over the 448 rollout files here: 447 carry `turn_context.effort` and
   `turn_context.model`, 425 carry `token_count` events, 448 carry
   `session_meta.cwd`. **The rows counted above were written by parser version 1
   and stay wrong until they are reparsed** — the adapter's `parser_version` bump
   makes a normal `agentworth scan` do that once, or `scan --force` does it now.
2. **`gemini` claims full extraction and produces nothing.** 67 rows, 44 with
   events, zero tokens, zero outcomes.
3. **`antigravity` is a real adapter name in the index and is not in the
   matrix's twenty.** The `gemini` adapter writes rows under both names; the
   capability table only knows one of them. Any consumer that joins the matrix
   to the index on adapter name silently drops 785 rows.

`opencode` is the inverse and the more interesting one: the matrix says it
extracts nothing beyond prompts, and it is the second-best adapter in the whole
index by tokens and outcomes. The matrix is a hand-maintained table that has
drifted from the code.

## Junk in the index

Two categories of row that are not sessions.

| | rows |
| :--- | ---: |
| `source_path` contains `node_modules` | 730 |
| Resolves to `plugins/cache` via `extract_repository_or_workspace` | 1,323 |

Real examples: `.gemini/antigravity-ide/playground/…/node_modules/@tybys/
wasm-util/dist/tsdoc-metadata.json` indexed as a session, and
`~/.local/share/opencode/mcp-auth.json` — a credential file — indexed as one.
**12.8% of this index is not a session.** #68 rejects non-session files at
discovery now, so a full rescan prunes these; every row above still includes
them, which is why the raw counts are stated alongside the non-stub ones.

## Exit codes by adapter

A `ShellCommand` with `exit_code: None` says a command was typed. Only `Some(0)`
says it ran and passed, and that is what rung 3 is built on. So it matters which
adapters can actually see the result.

Measured against 20 real Claude Code transcripts (17,981 Bash tool results): no
transcript carries a numeric exit code field at all. What it carries is the
harness's pass/fail envelope — `is_error` on every result, plus an "Exit code N"
line inside 519 of the 659 failures. `crates/adapters/src/exit_status.rs` turns
that envelope into a code and stitches it onto the command.

| adapter | exit status in the source format | status |
| :--- | :--- | :--- |
| claude_code | `is_error` on every `tool_result`; "Exit code N" in the error text; `toolUseResult` sidecar marks backgrounded and interrupted runs | **read, measured.** Backgrounded and interrupted runs stay `None` — launched is not finished |
| windsurf | explicit `exit_code` field | read (already did) |
| aider (JSONL) | explicit `exit_code` field | read (already did) |
| aider (markdown chat history) | **none** | now `None`. It used to hardcode `Some(0)`, manufacturing the exact proof rung 3 asks for |
| opencode | a `status` string of `error`/`failed` | read (already did) on the primary path; the second path now backfills |
| antigravity/gemini, cline, codex, cursor, deepseek, goose, grok, herdr, hermes, kimi, manus, minimax, openclaw, pi, qwen, zhipu | an `is_error` flag on the tool result | backfilled by the shared pass. **Unverified** — no sample sessions for these on this machine, so what is proven is that the adapter reads the field it parses, not that real files carry it |

The stitching is a separate pass because every adapter emits its `ShellCommand`
when it sees the *request*, several records before the answer arrives. It only
fills gaps, and only where the `ShellCommand` sits directly behind its own
`ToolCall`, so one command's result cannot land on another's.

## Honest depth rating

One line each, from what the index shows, not from what the adapter claims.

| adapter | depth |
| :--- | :--- |
| claude_code | **Full.** Events, tools, shell, tokens, files, outcomes. Everything else in this product is really built on this one. |
| opencode | **Deep, undersold.** Tokens and outcomes both real; blame paths are relative, which breaks file attribution. |
| antigravity | **Events and tools, no tokens.** 48 outcomes from 353 tool-bearing sessions. Not in the capability table under this name. |
| codex | **Tokens, model, effort and repo since #114; tools and outcomes still missing.** The 874 rows counted above predate the fix and read as events-only until a rescan. The adapter now reads `session_meta.cwd` for the repository (the rollout path is under `~/.codex` and used to bucket every session into the home directory), `turn_context.model` and `turn_context.effort` per turn, and the `token_count` events' cumulative counters. Tool calls, prompts and outcomes are still unread: they live in `response_item` and `event_msg`/`item_completed` records nothing parses yet. |
| gemini | **Events and tools, nothing scored.** 44 real sessions, zero tokens, zero outcomes. |
| grok | **Events only, thin.** 34 sessions with events, one tool call across all of them. |
| pi | **Events only.** 37 real sessions, no tools. |
| hermes | **Detects the directory.** 982 rows, 47 with more than one event, average 17.7. |
| cursor | **Detects the directory.** 707 rows, 62 with more than one event, average 5.8 — the shallowest thing here that still counts as an adapter. |
| aider, cline, deepseek, goose, herdr, kimi, manus, minimax, openclaw, qwen, windsurf, zhipu | **Unproven.** Zero rows on this machine, so nothing about them is verified beyond a path existing. |

Three of these — `hermes`, `cursor`, and every row in the last group — belong
under the same honest label: **detects the directory**. The adapter finds files
where they are supposed to be and gets almost nothing out of them. That is a
useful state to have shipped and it is not the same thing as support.

## What this means for the specs

- `verified-outcome-rate.md`'s `adapter` grouping has exactly two usable
  groups. Its null-rate return value exists because of this table.
- `suspect-commits.md` can only attribute commits from `claude_code` and
  `antigravity` blame rows; `opencode`'s 127 rows are relative paths and get
  dropped.
- `handoff.md` degrades to a token count on seven of nine adapters.
- Any adapter-count claim on the site or in the README should say twenty
  adapters and **two with full extraction**, or it is selling detection as
  support.

## Refresh

Re-run both halves after any adapter change:

    npx -y agentworth@latest matrix --json > /tmp/aw-matrix.json
    cp ~/.agentworth/agentworth.db /tmp/aw.db   # then the SQL above

Both halves matter. The matrix is what the code claims; the index is what the
code did. This page exists because those two disagreed in three places.
