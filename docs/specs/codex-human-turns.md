# Codex human turns

Status: built 2026-10-01 (PR #224). Companion to the human-turn lane
(`crates/adapters/src/human_turns/`), which covers Claude Code, Antigravity,
and Codex (`~/.codex/history.jsonl`).

## The problem

Insights friction / `day_hour` / vocabulary read the `human_turns` table.
That table is filled from harness **prompt histories** the session adapters
do not own as "human input". Before this SPEC's implement PR the sources were:

| Source constant | Homes / files (from `human_turns/mod.rs`) |
| :--- | :--- |
| `claude` | `~/.claude/history.jsonl`, `~/.claude/projects/**/<session>.jsonl` |
| `antigravity` | `~/.gemini/antigravity-cli/history.jsonl`, `…/brain/<session>/transcript.jsonl` |

After #224 the same table also includes `codex` from `~/.codex/history.jsonl`
(see Acceptance / Measured below). Codex has a deep session adapter (`crates/adapters/src/codex.rs`,
`PARSER_VERSION` 4) over `~/.codex/sessions/**/rollout-*.jsonl`, and those
rollouts already emit `UserMessage` into **session** traces. They do **not**
feed the human-turn lane. So Codex keyboard history is invisible to
insights friction / `day_hour` even when Codex sessions are indexed.

## Find the Codex human-history path (measured, do not invent)

Searched on 2026-09-30 against (1) `crates/adapters/src/human_turns/`,
(2) `crates/adapters/src/codex.rs`, (3) the live `~/.codex/` tree on the
owner machine. **Do not invent alternate paths in implement PRs.**

### Primary source (in scope)

| Path | Shape (every line on the measured machine) | Role |
| :--- | :--- | :--- |
| `~/.codex/history.jsonl` | `{ "session_id": "<uuid>", "ts": <epoch_secs int>, "text": "<prompt>" }` — 218/218 lines carry exactly those three keys | Direct analogue of Claude / Antigravity rolling `history.jsonl`. **This is the file the human-turn ingestor must add.** |

Discovery home: `codex_home = $HOME/.codex` (same root `CodexAdapter` already
uses for detection). Enumerate the single file
`codex_home.join("history.jsonl")` the same way Claude / Antigravity get
their `history.jsonl`.

### Related paths under `~/.codex/` — explicitly out of this SPEC's ingest

These exist on disk; listing them prevents a later session from "discovering"
them as the human-turn source by accident:

| Path | What it is | Why not here |
| :--- | :--- | :--- |
| `~/.codex/sessions/<yyyy>/<mm>/<dd>/rollout-*.jsonl` (+ `archived_sessions/`) | Codex session transcripts; already owned by `CodexAdapter` | Session lane, not human-turn history. User turns there are `UserMessage` in traces. |
| `~/.codex/transcription-history.jsonl` | Voice / dictation transcripts (`{id, createdAtMs, text}`) | Voice is out of COS train scope. |
| `~/.codex/dictation-history/` | Hashed voice blobs | Same. |
| `~/.codex/thread_history_1.sqlite` | Projection of rollouts (`thread_turns` / `thread_items`, incl. `item_type = 'userMessage'`) | Codex-internal mirror of session content, not a prompt-history file. Do not dual-read it into `human_turns`. |

If a later product call wants voice or sqlite projection, that is a new SPEC.

## Record mapping (for the implement PR)

Add a third source constant alongside `CLAUDE` / `ANTIGRAVITY`:

- `source = "codex"`
- Text field: `text` (string)
- Timestamp field: `ts` (integer **epoch seconds**, not ms — Claude history uses ms; the shared `parse_timestamp` already treats values ≤ 1e11 as seconds)
- Session id field: `session_id` (UUID string) → `HumanTurn.session_id`

Reuse `clean_turn_text`, friction / vocab classification, and the existing
dedup signature (`sha256(<timestamp_ms>|<first 60 chars>)`). No new taxonomy
rules are required for v1 of this SPEC — classification stays the shared
`FRICTION_RULES` / `VOCAB_CLUSTERS` in `taxonomy.rs`.

## Turn ↔ session link (measured gap)

Indexed Codex `sessions.session_id` values are the **rollout file stem**
(`derive_session_id` in `codex.rs` = `path.file_stem()`), e.g.

`rollout-2026-02-15T19-01-31-019c617f-ce70-7971-92de-93d8ff78ecbc`

while `history.jsonl` stores the bare UUID:

`019c617f-ce70-7971-92de-93d8ff78ecbc`

On the owner index (2026-09-30): **0 / 49** history UUIDs exact-match a
`sessions.session_id`; **46 / 49** match when the session id **contains** the
history UUID (suffix of the rollout stem). Exact id equality
(`turn_session_match` tier-1 today) therefore under-links Codex turns.

Implement PR must pick one honest repair (SPEC preference, quality over
cleverness):

1. **Preferred:** when ingesting Codex history rows, also attempt a suffix /
   "UUID-in-rollout-stem" join in the turn-link helpers used by insights
   (`crates/storage/src/insights.rs` `turn_session_match` /
   `query_turn_link`), scoped to `source = 'codex'` (or to session ids that
   start with `rollout-`), **or**
2. Store a normalized `session_id` that matches whatever the Codex adapter
   already writes — only if that can be done without rewriting historical
   session rows.

Acceptance must include a measured link rate on a real index after ingest
(report linked_by_id / unmatched for `source='codex'`), not a claim.

Path-tier link will not help for `history.jsonl` (one rolling file ≠ one
rollout `source_path`). Repo-tier remains the weak fallback.

## Taxonomy bump plan

`INGESTION_VERSION` lives in `crates/adapters/src/human_turns/taxonomy.rs`
(currently **2**: rows carry `source_path`). Orchestrator
(`crates/core/src/turns.rs`) wipes and re-ingests every turn source when the
stored version disagrees.

| Step | Action |
| :--- | :--- |
| 1 | Add Codex enumerate + `codex_record_fields` + `CODEX` source constant. |
| 2 | Bump `INGESTION_VERSION` **2 → 3** in the same commit that changes derived population (new source rows are a population change; fingerprints alone cannot say "Codex was missing"). |
| 3 | Document the bump in the taxonomy module comment (same style as the v2 note). |
| 4 | No FRICTION_RULES / VOCAB_CLUSTERS edit in the first implement PR unless a measured Codex corpus proves a missing trigger — that would be a further bump (3 → 4) with its own rationale. |

A taxonomy-only bump without Codex files is wasted wipe cost; do not bump
ahead of the source landing.

## Insights friction / `day_hour` hooks

No new insights API shape is required for v1. Once Codex rows land in
`human_turns`:

- `day_hour`, `friction`, `vocabulary` aggregates in
  `crates/storage/src/insights.rs` already `SELECT … FROM human_turns` with
  no source filter — Codex turns join the same tiles.
- Drill views `view=day_hour|friction` reuse the same table.
- Deferred list (`friction_triggers`, hour heatmap, vocabulary) already
  clears when `turn_total > 0`; more Codex turns only strengthen that.
- Compact insights headline (`friction_rate`, peak `day_hour` cell) reads
  the same payload.

Explicit hooks the implement PR **should** still touch:

1. **Coverage / honesty:** if any coverage copy or matrix row claims human
   turns are Claude+Agy only, update it. Prefer a measured note in
   `docs/capability-matrix.md` / REFERENCE only when counts exist post-scan —
   do not invent rates in the SPEC PR.
2. **Turn-link counters** (above) so filtered insights (`adapter=codex`) do
   not silently drop Codex human turns.
3. **Tests:** golden fixture under the human_turns test layout
   (`codex-home/history.jsonl` via a `rooted`-style constructor extension),
   plus one insights test that a Codex-sourced friction / day_hour row
   appears after ingest.

## Acceptance criteria

1. `HumanTurnIngestor` discovers `~/.codex/history.jsonl` when present; absent
   file is a quiet miss (same as missing Claude history), never an error.
2. Every well-formed `{session_id, ts, text}` line that survives
   `clean_turn_text` becomes a `human_turns` row with `source = 'codex'`.
3. `INGESTION_VERSION == 3`; a scan after upgrade wipes prior turn rows and
   re-parses Claude + Antigravity + Codex sources.
4. `agentworth insights --json` (and `/api/insights`) include Codex turns in
   `day_hour` / `friction` / `vocabulary` without a new endpoint.
5. Turn↔session link for Codex history UUIDs vs `rollout-…-<uuid>` session
   ids is measured and improved vs the pre-change 0% exact-id baseline on a
   real index (target: majority of history UUIDs that appear in some
   indexed rollout stem link at tier-1 or an explicit Codex UUID tier —
   report the number in the implement PR body).
6. Fixtures + unit tests cover parse, skip-degenerates, and version bump
   wipe behavior; no network; no reading `auth.json` or other secrets under
   `~/.codex/`.
7. SPEC stays the contract: implement is a **follow-up PR**, not this one.

## Non-goals

- Implementing the feature in this SPEC PR.
- Ingesting `transcription-history.jsonl`, `dictation-history/`, or
  `thread_history_1.sqlite`.
- Re-parsing Codex rollouts into `human_turns` (session adapter already owns
  those user messages).
- Pi / OpenAI / other adapter human-turn sources.
- Voice surfaces, Show HN, dsh, merging to `main` without COS approval.
- Changing FRICTION_RULES / vocabulary clusters "while we are here".
- Rewriting Codex `PARSER_VERSION` or session `derive_session_id` unless
  required for an agreed link strategy (prefer insights-side UUID match).
- P4/P5 app-merge work.

## Implementation sketch (for the follow-up PR only)

Touch list (expected, not inventing files outside this lane):

- `crates/adapters/src/human_turns/mod.rs` — `CODEX`, `codex_home`, enumerate
  `history.jsonl`
- `crates/adapters/src/human_turns/pipeline.rs` — `codex_record_fields`
- `crates/adapters/src/human_turns/taxonomy.rs` — bump `INGESTION_VERSION`
- `crates/core/src/turns.rs` — picks up version via existing constant
- `crates/storage/src/insights.rs` — Codex UUID ↔ rollout-stem link repair
- Tests under `crates/adapters` / `crates/core` / `crates/storage` as
  appropriate; schema fixtures if the turn-home layout grows a `codex-home/`

## References

- `crates/adapters/src/human_turns/{mod,pipeline,taxonomy}.rs` — current lane
- `crates/adapters/src/codex.rs` — session roots, `derive_session_id`, rollout
  naming (`~/.codex/sessions/.../rollout-*.jsonl`)
- `crates/storage/src/insights.rs` — `day_hour` / `friction` / deferred /
  `turn_session_match`
- `docs/REFERENCE.md` — `insights_get`, drill `view=ladder|day_hour|friction`,
  human-turn reingest / taxonomy bump notes
