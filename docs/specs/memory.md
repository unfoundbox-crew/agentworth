# Machine memory

Status: proposed 2026-09-08, measured. Nothing built.

## The one-line version

A fact that lives outside the context window cannot be compacted away.
Archie stores memory as typed rows with receipts, an agent pulls the rows it
needs by query, and markdown is a view rendered on demand, never the store.

## The problem

Agents remember through markdown files. Three costs follow.

| Cost | Measured |
| :--- | :--- |
| Prose re-sent every turn before any work starts | four rules files, 49.6 KB, about 12k tokens, at one session start on this machine |
| Compaction keeps almost nothing | 0.3% of the window survives each round; reasons survive at 1.7% over eight rounds (`compaction-diff.md`) |
| A wrong fact in a rules file outlives its correction | a host called asleep for days after a person said it was up; four later sessions repeated it (`beliefs.md`) |

Prose has a fourth cost no number shows: nothing can query it. A session
cannot ask a paragraph "is this still true" or "who wrote this and when".

## What the measurement corrected

The hypothesis said markdown is the expensive format. On this machine it is
the cheapest one Archie emits.

| Wake report, same data | Bytes | Tokens, estimated |
| :--- | ---: | ---: |
| markdown, 30-line budget | 1,943 | 542 |
| JSON, compact | 4,373 | 1,552 |
| handoff markdown | 1,238 | 481 |
| handoff JSON, compact | 5,996 | 1,897 |

Serialisation is not the tax. Three things are: hydrating everything whether
needed or not, losing it at compaction, and storing facts as sentences nobody
can check. The design attacks those three and leaves markdown as the human
view it is good at.

## The model

Five definitions carry the whole design.

**Receipt.** `r = (session_id, seq)`, a position in a raw transcript on disk.
Every fact carries one. A fact without a receipt is not stored.

**Entity.** `e = (kind, key)`. Kinds are closed: `session`, `agent`,
`machine`, `repo`, `path`, `commit`, `host`, `tool`, `run`, `anchor`, `rule`,
`decision`, `question`, `message`. The key is the resolved identity, an
absolute path, a commit hash, a host fingerprint. Two rows with one key are
one entity.

**Fact.** `f = (s, p, o, r, role, method, k, t_obs, t_from, t_to)`.

| Field | Meaning |
| :--- | :--- |
| `s`, `p`, `o` | subject entity, predicate from a closed vocabulary, object entity or literal |
| `role` | who said it: `user`, `tool`, `assistant`, `harness` |
| `method` | the deterministic extractor and its version, never a model |
| `k` | evidence rung, 0 for "said" through 5 for `ci_or_deployment_verified` |
| `t_obs` | when Archie learned it |
| `t_from`, `t_to` | when it was true in the world; `t_to` open until superseded |

**Memory.** `M` is the append-only set of facts. Supersession is the only
update: a new fact with the same `(s, p)` and a different `o` closes the old
one by setting `t_to = t_obs(new)`. Nothing is deleted. The current view is
`M_now = { f : t_to is open }`. A user-role fact supersedes an assistant one
on the same `(s, p)` regardless of order; the person is the newest evidence.

**Rank.** `w(f) = role_weight(f) · (1 + k) · precision(method) · 2^(-age/λ)`,
with `user` 1.0, `tool` 0.8, `assistant` 0.5, `harness` 0.2, and `precision`
the hand-measured number each extractor already carries (asks 0.86,
corrections 0.86, ungrounded claims 0.30 and therefore not stored yet).

Two operations sit on top.

**Hydration.** `H(q, B)`: the facts matching selector `q`, ordered by `w`,
cut at `B` bytes. One row per fact, fixed columns, tab separated. At `B = 0`
the agent holds only the query handle, which is the zero-token case.

**Projection.** `π(F, budget)` renders facts as markdown for a person. It is
a pure function: every line traces to a receipt, and nothing is stored. The
wake and handoff renderers are already `π`; today they read structs, tomorrow
they read rows.

**Staleness.** A fact about a path carries the file's hash at `t_obs`.
Hydration re-hashes and marks a mismatch with the session that wrote it. That
is `session_drift`, applied to every fact instead of one read set.

The property that justifies the design, stated plainly: the harness keeps a
fraction `s ≈ 0.003` of the window per compaction round. A fact in `M` is not
in the window, so its survival is 1 for any number of rounds. Its cost moves
from "held in the window every turn" to "one indexed query when asked".

## What Archie already stores

Read from `crates/storage/src/lib.rs` and the live index, 2026-09-08.

| Today | In the model |
| :--- | :--- |
| `sessions`, `machines`, `identity_sightings` | entities `session`, `machine`, `agent` |
| `file_modifications` (32,974 rows) | fact `session touched path`, `k=1` |
| `support_set`, `tool_intents`, `intent_paths` | facts `read` and `predicted_write`, with the hash for staleness |
| `trace_anchors` | entity `anchor` (`run_id`, `sha256`, `pane_id`) |
| `OutcomeEvidence` and `verify.rs` | `k` and `confidence` |
| `session_compaction` (59 rows) | round boundaries; which facts were in a dropped span |
| `trajectory_chunks`, six kinds, 71,632 rows | verbatim text, loaded lazily; the fact stores the receipt, not the sentence |
| `LooseEnd`, `Statement`, `ForgottenStatement`, `Ask` | facts `promised`, `decided`, `rejected`, `because`, `asked`, `answered`, computed on demand today |
| `agent_state.last_stop`, the JSON blob | facts `moved_by` |

The loop tables are nearly empty on this machine: 3 tool intents, no support
set rows. The live path exists and is not yet feeding anything. It is the
write path this design needs.

## Schema

Two tables and one view, in the same WAL database, same guarded-`ALTER`
migration style as everything else.

```sql
CREATE TABLE entities (
  id INTEGER PRIMARY KEY, kind TEXT NOT NULL, key TEXT NOT NULL,
  first_seen TEXT, last_seen TEXT, UNIQUE(kind, key));

CREATE TABLE facts (
  id TEXT PRIMARY KEY,                -- ulid, time ordered
  subject INTEGER NOT NULL REFERENCES entities(id),
  predicate TEXT NOT NULL,            -- closed vocabulary, versioned
  object INTEGER REFERENCES entities(id),
  literal TEXT,                       -- when the object is a value, not an entity
  session_id TEXT NOT NULL, seq INTEGER NOT NULL,   -- the receipt
  role TEXT NOT NULL, method TEXT NOT NULL,
  rung INTEGER NOT NULL DEFAULT 0, confidence REAL,
  observed_at TEXT NOT NULL, valid_from TEXT, valid_to TEXT,
  anchor_sha256 TEXT, text_sha256 TEXT, superseded_by TEXT);

CREATE INDEX facts_sp ON facts(subject, predicate, valid_to);
CREATE INDEX facts_o  ON facts(object);
CREATE INDEX facts_r  ON facts(session_id, seq);
CREATE VIEW v_now AS SELECT * FROM facts WHERE valid_to IS NULL;
```

`text_sha256` lets a fact prove it is the sentence at its receipt without
storing the sentence. The transcript stays the source.

Predicates ship as one table, versioned like `PARSER_VERSION`:
`touched`, `read`, `predicted_write`, `ran`, `passed`, `committed`,
`decided`, `rejected`, `because`, `promised`, `asked`, `answered`,
`asserted`, `corrected`, `moved_by`, `sent`, `claimed`.

## The wire form

    archie memory query <selector> [--budget 4096] [--md]
    memory_query(selector, budget)         MCP, rows by default, --md for π
    memory_assert(fact)                    MCP, receipt required, no model text

A row: `predicate<TAB>subject<TAB>object<TAB>rung<TAB>role<TAB>receipt<TAB>valid_from`.
One row costs about a third of the markdown sentence it replaces, and a
session that needs six rows does not pay for the other two hundred.

`memory_assert` accepts a fact only with a receipt into the caller's own
session. Deterministic extractors and the hook loop write everything else.

## Where hydration happens

| Moment | Query | Budget |
| :--- | :--- | :--- |
| `SessionStart`, `session_wake` | this repo: open `promised`, current `decided` and `rejected`, `corrected` since the last session | 4 KB |
| `PostCompact`, through `UserPromptSubmit` stdout | this session's own `decided`, `rejected`, `because`, `promised`, stale rows flagged | 4 KB |
| before asserting | `claim_check`: `asserted` and `corrected` on the claim's anchors | 1 KB |
| never | everything else stays in the store until asked | 0 |

The second row is the re-injection `compaction-diff.md` declined. It is
allowed now because a row is verbatim by construction and carries its
receipt; nothing is paraphrased. The injection path is documented for one
harness and must be verified on a real turn before anything builds on it.

Latency target: under 1 ms per query in process on the open connection,
measured, not assumed. Over the socket, one round trip more.

## Where the field is, and how a harness plugs in

Surveyed 2026-09-08, nineteen harnesses, the MCP changelog, eight memory
servers, the benchmarks. The record with sources is
`docs/research/memory-landscape-2026-09.md`. What it settles:

| Finding | Consequence here |
| :--- | :--- |
| MCP tools reach 17 of 19 harnesses; the 2026-07-28 spec removed sessions and deprecated sampling | memory is a tool that returns rows, never a resource; the server cannot ask a model anything |
| stdout-injecting hooks exist in 3 harnesses, compaction events in 2 | session-start injection is per harness; `session_wake` is the universal pull |
| the largest neighbour, claude-mem, injects at `SessionStart` and compresses observations with a model | same hook, opposite representation; the receipt is the difference |
| the one independent ablation: verbatim beats extracted by 16 to 22 points | facts point at receipts and load the words; no model writes a fact |
| Anthropic's memory tool and Letta both converged on files with history | the rules-file projection is not optional; facts must be diffable on disk |
| no memory server publishes a token number for its payload | the 542 against 1,552 measurement stands alone and says so |

The plugin surface, in order of reach:

1. Transcript adapters on disk, 22 today. The only path that reaches every
   harness. Nothing to add.
2. One MCP stdio server with `memory_query`, deterministic tool order, `ttlMs`
   on lists, rows under 4 KB. Well inside the 25k-token output cap one client
   reports.
3. `SKILL.md`, 100 tokens at start, the body says which tool to call. A skill
   cannot carry a server, so it points at the MCP tool and the CLI.
4. One line in each indexed repo's `AGENTS.md`, written by `archie init` on
   request, never silently. The file every harness reads.
5. Hook shims for Claude Code (built), Codex (built, flagged) and Copilot CLI
   (new, `sessionStart` and `events.jsonl`). Stop there.
6. ATIF export (built) as the interchange format; NVIDIA and Arize already
   read it.

Not bet on: compaction hooks beyond the two that exist, native memory features
(all closed), ACP as a capture path (a client integration, not a plugin).

## Rules files become views

A rules file today mixes facts, which go stale, with judgment, which does
not. Facts move into `M`: a host's state, a port, an account boundary, each
a row with a receipt, superseded by the correction when the person makes
one. A generated section of the file is `π(M_now, repo)` under a marker that
says so. The person keeps the judgment. The stale-host bug becomes one
superseded row and every projection re-renders.

## Deliberately not built

- **No model writes a fact.** Extraction stays deterministic with a measured
  precision, and an extractor under about 0.8 does not write to `facts`.
- **No graph database, no new engine.** Two tables and SQL joins. The vector
  chunks stay for "sessions like this"; they are not the memory.
- **No transcript in the index.** Receipts and hashes only, text lazily.
- **No push into a running session** beyond the two hook moments above.
- **No cross-machine sync.**

## Sequencing

| # | Work | Tokens |
| :-- | :--- | ---: |
| 1 | `entities`, `facts`, predicates, `memory_query` rows, backfill from the eight tables above | 250k |
| 2 | decision, commitment, question and correction facts written at scan time from the three shipped extractors, receipts only | 150k |
| 3 | wake and handoff as `π` over rows; bytes and tokens measured against 542 and 481 | 150k |
| 4 | `PostCompact` re-hydration through the hook, verified on a real turn | 150k |
| 5 | `claim_check` on `asserted` and `corrected` | 100k |
| 6 | the generated rules-file section | 100k |

## Open questions

- Entity resolution for bare nouns. Paths and hashes resolve; "the host"
  does not, and `beliefs.md` already names this as the hard part.
- The default budget. 4 KB is the wake report's size today; measure whether
  rows at that budget carry more facts than the 30 lines do.
- Whether `because` at 174 matches per session is precise enough to store,
  or only to return on demand.
- Whether a superseded decision should hydrate at all. It is forgotten either
  way, and returning it may re-suggest a dead end.
