# Capability matrix

Internal. Not the README, not the site. This is the honest version of the
adapter count, measured on one machine **2026-09-30**.

The README says twenty adapters. That is true and it is not the number anyone
should plan against. A handful produce real rows here; depth varies sharply;
one product identity (`gemini`) folds two index names.

Every count on this page is from a forced tip scan on that date, against the
live SQLite index (not a fixed snapshot copy). Tip binary **0.1.28**. Scan:

    archie scan --force --plain
    # EXIT 0 · tip 0.1.28 · 7,260 sessions · index ~/.agentworth/agentworth.db

## What was measured, and against what

**Self-reported.** Adapter `capabilities()` / `PARSER_VERSION` on tip, and
what `GET /api/matrix` exposes via `adapter_display_meta` (same flags the
adapters claim for prompts/tokens/tools/shell/outcomes). Codex is
`PARSER_VERSION` **4** (prompts, tools, shell, outcomes). Pi is
`PARSER_VERSION` **2** (tools, tokens, outcomes — not events-only). Gemini /
Antigravity (`agy`) is `PARSER_VERSION` **4** (tokens real on agy blobs).

**Observed.** Live index after that scan. Two population bars matter and they
are not the same:

| bar | predicate |
| :--- | :--- |
| **in_db** | every row in `sessions` for that `adapter` value |
| **API non-stub** | `kind = 'conversation' AND total_events > 1 AND (total_tokens > 0 OR tool_calls_count > 0)` — `NON_STUB_SQL_PREDICATE` in storage; what `/api/matrix` `sessions_count`, `/api/stats`, and default `/api/traces` use |

The 2026-09-12 matrix used `total_events > 1 AND total_tokens > 0` as its
"non-stub" bar. That is **stale vs the API**: a multi-event session with tools
but zero tokens now counts as non-stub. Do not compare 09-12 "tokens>0" counts
to today's API non-stub column as if they were the same filter.

## Observed (2026-09-30)

| adapter (index name) | tip reality | in_db → API non-stub | tokens (DB) | notes |
| :--- | :--- | ---: | ---: | :--- |
| claude_code | Full pv3 | 4,444 → 4,300 | 66.04B | OK |
| **codex** | **Deep pv4** — prompts / tools / exec / outcomes | 852 → 803 | 6.46B | 482 tools, 211 outcomes, 747 prompts, 362 exec; 49 API stubs remain in DB |
| antigravity (agy) | pv4 tokens real | 757 → 667 | 5.49B | 374 rows with `tokens > 0`; **folded into `gemini` in matrix / API identity** (see below) |
| opencode | Deep pv2 | 468 → 407 | 3.58B | Matrix used to underrate as prompts-only; tip claims and delivers depth |
| **pi** | **pv2 tools + tokens + outcomes** | 398 → 136 | 1.47B | pv2: 142 rows; pv1 residue: 256 rows at 0 tok; 26 `node_modules` junk under this name |
| grok | still thin / events | 34 → 0 | 0 | junk roots: marketplace-cache ×3, memtrace ×22 |
| gemini (CLI) | partial | 50 → 35 | 25.2M | **Separate** from the 757 agy rows; same adapter struct, different product identity |

Adapters not listed above (cursor, hermes, herdr, and the zero-row twenty)
were **not re-counted** this pass — do not copy 09-12 figures as current.

## Gemini / Antigravity dual-name (agy vs gemini)

One registered adapter (`GeminiAdapter`, `name() = "gemini"`,
`PARSER_VERSION` 4) writes two `sessions.adapter` identities via
`detect_product_identity`:

| identity | what it is | this scan |
| :--- | :--- | :--- |
| `antigravity` | Antigravity CLI / IDE (`agy`) brain & conversation stores | 757 in_db → 667 API non-stub · 5.49B tokens · 374 with `tok > 0` |
| `gemini` | Gemini CLI session files | 50 in_db → 35 API non-stub · 25.2M tokens |

`identity_names()` returns `["gemini", "antigravity"]`. `/api/matrix` therefore
shows **one** row (`adapter: "gemini"`, display name "Gemini / Antigravity")
whose `sessions_count` is the **sum** of both identities under the API
non-stub predicate, and whose `identities` array lists both names. Joining the
matrix to the index on `name()` alone still drops every `antigravity` row;
join on `identities` (or sum both names) instead.

Do not treat "gemini matrix claims full extraction" as a claim about Gemini
CLI alone — the capability flags describe the shared adapter; observed depth
for CLI (50 → 35, partial) and agy (757 → 667, tokens real) differ.

## Self-reported vs tip index (drift closed / still open)

| adapter | tip claims (pv) | 09-12 matrix-doc | 2026-09-30 observed |
| :--- | :--- | :--- | :--- |
| claude_code | Full (pv3) | Full | still Full — 4,444 → 4,300 |
| **codex** | prompts / tools / shell / outcomes (**pv4**) | **STALE:** tools=0, outcomes=0, prompts=0 | **flip:** 482 tools, 211 outcomes, 747 prompts, 362 exec on 852 → 803 |
| antigravity (agy) | tokens (**pv4**) | Sep-12 tokens 0; Sep-29 note that tokens landed | 5.49B DB tokens; 374 `tok > 0` |
| opencode | Deep (pv2) | underrated as prompts-only | 468 → 407 · 3.58B |
| **pi** | tools + tokens + outcomes (**pv2**) | **STALE:** "events only" | not events-only; 398 → 136; pv1 residue (256 @ 0 tok) still dilutes in_db |
| grok | thin | thin / events | still true — 34 → **0** non-stub; junk roots as above |
| gemini CLI | shared adapter claims full | claimed full | partial — 50 → 35 · 25.2M; separate from agy |

Codex depth flip is the largest honesty correction this pass: tip pv4 reads
`response_item` / `custom_tool_call` traffic the 09-12 doc said nothing
parsed yet. Pi is the second: tip pv2 is tools+tokens+outcomes; calling it
"events only" is wrong. Residual pv1 rows (256 @ 0 tok) and 26
`node_modules` paths under `pi` explain why in_db (398) is far above API
non-stub (136).

## Junk in the index

| | rows (2026-09-30) |
| :--- | ---: |
| `source_path` contains `node_modules` | **68** |

Down from 123 on 2026-09-12. Pi alone still carries 26 of those. Grok's
non-stub collapse (34 → 0) is dominated by junk roots (marketplace-cache ×3,
memtrace ×22), not by a thin-but-real conversation population.

## Exit codes by adapter

NOT RE-MEASURED 2026-09-30 — left as the earlier raw-transcript measurement
(Claude Code `is_error` / "Exit code N" envelope; shared backfill for most
others). See git history of this file for the 2026-09-02 table.

## Honest depth rating (tip + this scan)

| adapter | depth |
| :--- | :--- |
| claude_code | **Full (pv3).** Events, tools, shell, tokens, files, outcomes. |
| **codex** | **Deep (pv4).** Prompts, tools, exec/shell, outcomes, tokens, model, effort, repo. Measured: 747 prompts / 482 tools / 362 exec / 211 outcomes on 852 → 803. |
| opencode | **Deep (pv2).** Tokens and outcomes real; blame paths can still be relative. |
| antigravity (agy) | **pv4 — tokens real** (plus prior models-from-`gen_metadata`). Folded into **gemini** for `/api/matrix` identity. 757 → 667; 374 `tok > 0`. |
| gemini CLI | **Partial.** Same adapter as agy, separate identity. 50 → 35; 25.2M tokens. Not "full" on its own. |
| **pi** | **pv2 — tools + tokens + outcomes** (not events-only). 398 → 136; pv1 residue 256 @ 0 tok; 26 `node_modules`. |
| grok | **Events only, thin — and mostly junk.** 34 → 0 API non-stub. |
| others | **Not re-rated this pass.** Zero-row adapters remain unproven on this machine. |

## What this means for the specs

- Any surface that uses API non-stub (`/api/matrix` `sessions_count`,
  `/api/stats`, default traces) will show codex ~803 and pi ~136, not the
  in_db totals.
- Joining matrix → index on adapter name must use `identities` (or sum
  `gemini` + `antigravity`) or agy rows vanish from the gemini matrix line.
- README / site adapter-count claims should not equate "detected" with
  "deep extraction." Codex and Pi are deep on tip; grok is not; gemini CLI
  is partial; agy tokens are real but live under the `antigravity` identity.

## Refresh

Re-run after any adapter / predicate change:

    archie scan --force --plain
    archie agent list --json
    # index: ~/.agentworth/agentworth.db (or archie doctor --json → storage.path)

API non-stub (authoritative for matrix `sessions_count`):

```sql
-- NON_STUB_SQL_PREDICATE
kind = 'conversation'
  AND total_events > 1
  AND (total_tokens > 0 OR tool_calls_count > 0)
```

Per-adapter in_db → non-stub + token totals:

```sql
SELECT adapter,
  COUNT(*) AS in_db,
  SUM(kind = 'conversation'
      AND total_events > 1
      AND (total_tokens > 0 OR tool_calls_count > 0)) AS api_non_stub,
  SUM(total_tokens) AS tokens
FROM sessions
GROUP BY adapter
ORDER BY in_db DESC;
```

```sql
SELECT COUNT(*) FROM sessions WHERE source_path LIKE '%node_modules%';
```

## How measured, 2026-09-30

- Tip **0.1.28**; `archie scan --force --plain` EXIT 0; **7,260** sessions in
  `~/.agentworth/agentworth.db`.
- Counts in the Observed table are exactly the measurement set for this
  rewrite — no invented averages, verified-outcome rates, or unlisted
  adapters.
- `/api/matrix` capability flags for codex / pi / gemini already match tip
  `capabilities()` on main; this page was the stale half.
