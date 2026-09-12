# Capability matrix

Internal. Not the README, not the site. This is the honest version of the
adapter count, measured on one machine 2026-09-12.

The README says twenty adapters. That is true and it is not the number anyone
should plan against. Nine of them have ever produced a row here (`antigravity`
is a tenth name that produces rows but isn't one of the twenty — see below),
two produce tokens, two produce any outcome at all, and one produces
everything.

Every count on this page is from the index as it stood on 2026-09-12,
re-measured against `archie agent list --json` and the live SQLite index
(this machine's index carries every session, not a fixed snapshot copy, so
"as it stood" moves every scan). The Codex fix (#116) and the v0.1.23 token
fix (#163) both predate this measurement and are reflected in the numbers
below — codex tokens are no longer near-zero, see "Self-reported, and where
it disagrees".

## What was measured, and against what

Two sources, cross-checked.

**Self-reported.** `archie agent list --json` on the installed 0.1.23 binary.
It lists **20 adapters**, matching the count at the previous measurement —
`crates/adapters/src/` still holds `lib.rs` (module root) and `mcp.rs`
(tool-name normalizer, not an adapter) alongside the 20. Eleven report
`is_detected: true`, same count as before, though the adapter mix behind that
count was not individually re-diffed.

**Observed.** The live index, not a copy: 6,054 session rows, 4,800 non-stub
(`total_events > 1 AND total_tokens > 0`), 34,722 `file_modifications` rows,
spanning 2026-02-15 to 2026-09-12 (one row carries a pre-1970 epoch
timestamp and is excluded from that range as a bad value — NOT RE-MEASURED
further, i.e. its cause was not chased down).

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

#68 (rejecting non-session files at discovery) is long since merged and this
is the live index, not a pre-#68 copy — so the "Junk in the index" counts
below are post-#68 residue, not the pre-#68 baseline.

## Observed

| adapter | rows | >1 event | tokens | avg ev | tools | any outcome | verified | scored | prompt |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| claude_code | 4,061 | 4,061 | 3,866 | 384.2 | 3,544 | 2,542 | 2,241 | 3,998 | 3,971 |
| codex | 972 | 972 | 745 | 269.5 | 0 | 0 | 0 | 972 | 0 |
| antigravity | 530 | 530 | 0 | 124.0 | 449 | 88 | 9 | 476 | 455 |
| opencode | 206 | 205 | 177 | 203.7 | 159 | 72 | 24 | 206 | 205 |
| pi | 82 | 43 | 0 | 88.2 | 0 | 0 | 0 | 82 | 5 |
| cursor | 70 | 62 | 0 | 49.6 | 0 | 0 | 0 | 70 | 0 |
| gemini | 50 | 44 | 12 | 40.1 | 23 | 0 | 0 | 21 | 11 |
| hermes | 47 | 47 | 0 | 351.0 | 0 | 0 | 0 | 47 | 0 |
| grok | 34 | 34 | 0 | 450.5 | 0 | 0 | 0 | 33 | 1 |
| herdr | 2 | 1 | 0 | 2.0 | 0 | 0 | 0 | 2 | 0 |

Nine of the twenty adapters appear above. `antigravity` is a tenth name the
index uses and the matrix does not list. Eleven produced no row at all: aider,
cline, deepseek, goose, kimi, manus, minimax, openclaw, qwen, windsurf, zhipu.
Of those, `goose` and `openclaw` report `is_detected: true` and still
contributed nothing. `herdr` has moved out of this group since the last
measurement — see the depth-rating note below — but its two rows are
`fleet_snapshot` kind, not conversations, so it barely counts as "producing a
row" in the sense every other adapter here does.

Compaction is no longer absent from this table. `sessions.compaction_count`
exists now (#62 landed and this index has it): `claude_code` is the only
adapter showing any compacted sessions, 25 of its 4,061. Every other adapter
shows zero — either genuinely uncompacted or, for the lower-volume adapters,
not enough long sessions to trigger it. `compaction.md`'s own numbers still
come from parsing raw JSONL directly, not this column.

`prompt_preview` is no longer zero in every row — the #47 fix (noted in
`handoff.md`) has propagated through rescans: 3,971/4,061 claude_code rows,
455/530 antigravity, 205/206 opencode, 11/50 gemini, 5/82 pi, 1/34 grok carry
a preview. `codex`, `cursor`, `hermes` and `herdr` are still at zero.

## Self-reported, and where it disagrees

The shipped matrix claims token and outcome extraction for exactly three
adapters: `claude_code`, `codex`, `gemini`. The index disagrees with two of
those three.

| adapter | matrix claims tokens | rows with tokens | matrix claims outcomes | rows with outcomes |
| :--- | :--- | ---: | :--- | ---: |
| claude_code | yes | 3,866 | yes | 2,241 |
| codex | yes | 745 | yes | **0** |
| gemini | yes | **12** | yes | **0** |
| opencode | **no** | 177 | **no** | 24 |
| antigravity | not listed at all | 0 | not listed at all | 9 |

Two of the three original failures are now different shapes, and one is
unchanged:

1. **`codex` claims tokens and, since #116 and rescans, mostly delivers them.**
   745 of 972 sessions now carry a token total — this is the fix landing, not a
   new bug. What #116 did not touch is still missing: `tools` is 0 across every
   codex row and `any outcome` is 0. Tool calls and outcomes live in
   `response_item` and `event_msg`/`item_completed` records nothing parses yet
   (see the depth rating below) — the matrix's claim of `outcomes: true` for
   codex is still wrong, just for a narrower reason than before.
2. **`gemini` claims full extraction and mostly still produces nothing.** 50
   rows, 44 with events, 12 now with tokens (up from 0 — not chased down
   further this pass, flagged NOT RE-MEASURED for root cause), zero outcomes.
3. **`antigravity` is a real adapter name in the index and is not in the
   matrix's twenty.** The `gemini` adapter writes rows under both names; the
   capability table only knows one of them. Any consumer that joins the matrix
   to the index on adapter name silently drops 530 rows.

`opencode` is still the inverse: the matrix says it extracts nothing beyond
prompts, and it remains the second-best adapter in the whole index by tokens
and outcomes. The matrix is a hand-maintained table and it is still drifted
from the code — codex and gemini's outcome claims are the open drift now,
not codex's token claim.

## Junk in the index

Two categories of row that are not sessions.

| | rows |
| :--- | ---: |
| `source_path` contains `node_modules` | 123 |
| Resolves to `plugins/cache` via `extract_repository_or_workspace` | NOT RE-MEASURED 2026-09-12 — original figure (1,323) used the Rust helper directly; a `source_path LIKE` approximation gave 127, which is not the same method and isn't reported as a replacement |

Real examples: `.gemini/antigravity-ide/playground/…/node_modules/@tybys/
wasm-util/dist/tsdoc-metadata.json` indexed as a session, and
`~/.local/share/opencode/mcp-auth.json` — a credential file — indexed as one.
The node_modules share of the index is down to 123/6,054 = 2.0% (was 12.8% of
a much smaller, junk-heavier index). #68 rejects non-session files at
discovery now, so a full rescan prunes these; the count above still includes
whatever survived past #68 plus anything indexed before it landed.

## Exit codes by adapter

A `ShellCommand` with `exit_code: None` says a command was typed. Only `Some(0)`
says it ran and passed, and that is what rung 3 is built on. So it matters which
adapters can actually see the result.

Measured against 20 real Claude Code transcripts (17,981 Bash tool results): no
transcript carries a numeric exit code field at all. What it carries is the
harness's pass/fail envelope — `is_error` on every result, plus an "Exit code N"
line inside 519 of the 659 failures. `crates/adapters/src/exit_status.rs` turns
that envelope into a code and stitches it onto the command.

NOT RE-MEASURED 2026-09-12 — this is a raw-transcript parse (counting fields
inside JSONL Bash tool results), not a query the live index or `archie`'s
CLI/JSON surfaces answer. Left as the 2026-09-02 measurement.

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
| antigravity | **Events and tools, no tokens.** 9 outcomes from 449 tool-bearing sessions (down from 48/353 at the last measurement — not chased down further this pass). Not in the capability table under this name. |
| codex | **Tokens, model, effort and repo since #116; tools and outcomes still missing.** 745 of 972 rows now carry tokens post-fix and post-rescan — the "stays wrong until reparsed" caveat from the last measurement no longer applies. The adapter reads `session_meta.cwd` for the repository (the rollout path is under `~/.codex` and used to bucket every session into the home directory), `turn_context.model` and `turn_context.effort` per turn, and the `token_count` events' cumulative counters. Tool calls, prompts and outcomes are still unread: they live in `response_item` and `event_msg`/`item_completed` records nothing parses yet. |
| gemini | **Events and tools, barely scored.** 50 rows, 44 with events, 12 now with tokens (was 0), zero outcomes. |
| grok | **Events only, thin.** 34 sessions with events, one tool call across all of them. |
| pi | **Events only.** 43 real sessions, no tools. |
| hermes | **Detects the directory.** 47 rows, all with more than one event, average 351.0. |
| cursor | **Detects the directory.** 70 rows, 62 with more than one event, average 49.6. |
| herdr | **Not a conversational session at all.** 2 rows, both `fleet_snapshot` kind — see the note above the table. |
| aider, cline, deepseek, goose, kimi, manus, minimax, openclaw, qwen, windsurf, zhipu | **Unproven.** Zero rows on this machine, so nothing about them is verified beyond a path existing. |

Update 2026-09-07 (PR #149): `herdr` moved out of the last group, in a way the
table above cannot express. Herdr is a terminal workspace manager and writes no
transcript; its one file, `~/.config/herdr/session.json`, is a snapshot of panes
and the agent session id running in each. The adapter now reads that file (it
used to look for fields no Herdr file has). Measured through the CLI on the
redacted fixture: 1 session, 8 events (one per pane), 0 tokens, 0 tools, 0
outcomes, stored as a `fleet_snapshot`, so it never counts as a session. What it yields is the join key
`agent_session.value` from a pane to the Claude Code / Codex / Gemini session
that ran in it, nothing a depth rating measures. Its capability profile claims
0 of 7 on purpose.

Three of these — `hermes`, `cursor`, and every row in the last group — belong
under the same honest label: **detects the directory**. The adapter finds files
where they are supposed to be and gets almost nothing out of them. That is a
useful state to have shipped and it is not the same thing as support.

## What this means for the specs

- `verified-outcome-rate.md`'s `adapter` grouping has exactly two usable
  groups. Its null-rate return value exists because of this table.
- `suspect-commits.md` can only attribute commits from `claude_code` and
  `antigravity` blame rows; `opencode`'s 133 `file_modifications` rows are
  all relative paths and get dropped.
- `handoff.md` degrades to a token count on seven of nine adapters.
- Any adapter-count claim on the site or in the README should say twenty
  adapters and **two with full extraction**, or it is selling detection as
  support.

## Refresh

Re-run both halves after any adapter change:

    archie agent list --json > /tmp/aw-matrix.json
    sqlite3 -readonly ~/.agentworth/agentworth.db   # then the SQL below

Both halves matter. The matrix is what the code claims; the index is what the
code did. This page exists because those two disagreed in three places, and
still disagrees in two.

## How measured, 2026-09-12

Self-reported side:

    archie agent list --json

Observed side — live index, no copy, opened read-only (db path from
`archie doctor --json`'s `storage.path`):

    SELECT COUNT(*) FROM sessions;
    SELECT COUNT(*) FROM sessions WHERE total_events > 1 AND total_tokens > 0;
    SELECT COUNT(*) FROM file_modifications;
    SELECT MIN(started_at), MAX(started_at) FROM sessions;
    SELECT MIN(started_at) FROM sessions WHERE started_at > '2020-01-01';
    SELECT COUNT(*) FROM sessions WHERE started_at < '2020-01-01';

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

    SELECT adapter, SUM(compaction_count>0) compacted, COUNT(*) n
    FROM sessions GROUP BY adapter ORDER BY n DESC;

    SELECT COUNT(*) FROM sessions WHERE source_path LIKE '%node_modules%';

    SELECT COUNT(*) FROM file_modifications fm
    JOIN sessions s ON fm.session_id = s.session_id
    WHERE s.adapter = 'opencode';

    SELECT COUNT(*) FROM file_modifications fm
    JOIN sessions s ON fm.session_id = s.session_id
    WHERE s.adapter = 'opencode' AND fm.file_path NOT LIKE '/%';

    SELECT session_id, adapter, kind, total_events, total_tokens
    FROM sessions WHERE adapter = 'herdr';

Not re-measured this pass, and marked so inline: the `plugins/cache` junk-row
count (needs the Rust `extract_repository_or_workspace` helper, not a `LIKE`
approximation), and the exit-code-by-adapter section (a raw-JSONL transcript
parse, not a SQL or `archie` query).
