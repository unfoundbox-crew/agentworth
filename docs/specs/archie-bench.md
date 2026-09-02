# Archie bench

Status: the local table is **built** in #TBD; measured 2026-09-02.

**What #TBD shipped, and what it did not.** The local leaderboard is
`archie stats ladder` and the `stats_ladder` MCP tool: the ladder with the
API-equivalent spend below the evidence line, cost per verified outcome by
model, repo, adapter or effort, and the newest verified sessions. Three
departures from the plan below, each on purpose.

- **The name is `stats ladder`, not `stats bench`.** The screen leads with the
  ladder and the spend under it; the group table is the second block, not the
  page. A bench ranks models, and this deliberately does not — see "What this
  deliberately does not do".
- **One axis at a time, not the cross.** `--by model|repo|adapter|effort`
  returns one axis; the `model x repo` cross this spec measured is not built.
  The measurement below still stands and the cross is still the informative
  shape; it needs a two-key row and a wider screen than 80 columns.
- **The floor is 20, the code's number, not this spec's 10.** The open question
  below is unresolved. `agentworth_storage::OUTCOME_RATE_DEFAULT_MIN_N` is now
  the single place it lives, so settling it is a one-line change. Under it a
  rate and a cost render blank rather than being hidden: the row still ships
  its `n`.

Two preconditions cleared since this was written. Pricing was refreshed in
#115, so the dollar column is no longer one rate card applied to every model.
`sessions.effort` landed in #116, so `--by effort` reads what Codex records and
names the sessions that carry none rather than faking a value.

**Not built here:** the opt-in aggregate export across users. Nothing in #TBD
sends anything anywhere.

## The one-line version

A leaderboard built only from this machine's own sessions: model by effort by
repo, ranked on verified rate, with tokens, tool calls and an API-equivalent
cost beside every row and the `n` printed on all of them.

## The problem, stated by the person who has it

Someone posts a model leaderboard. It is measured on a benchmark I do not run,
in repos I do not have, at an effort setting nobody names. I read it, feel
something, and change nothing — because none of it is about my code.

I already have four thousand of my own sessions on this disk. The question I
actually want answered is which model, at which effort, in which of my repos,
leaves evidence behind. Not which model is best. Which one works here.

## Why now

`verified-outcome-rate.md` proved the number exists and can be grouped one axis
at a time — by model, by adapter, by repo. It also found the thing that makes a
public leaderboard misleading: **repo spreads wider than model**. This spec is
that finding taken seriously. If the codebase matters more than the model, a
row keyed on the model alone is the wrong row. The right row is the cross.

It resolves the old "Personal Leaderboard" question the same way: local, yes.
Cross-user, only ever as opt-in aggregates, never rows.

## The measurement

Everything below is one read-only pass over this machine's live index,
`~/.agentworth/agentworth.db`, 2026-09-02: **4,841 sessions**, of which 3,136
are non-stub (`total_events > 1 AND total_tokens > 0`). The repo names in the
tables are this user's own repositories on his own disk — this is his data,
counted on his machine, and it never leaves it.

The index has not been rescanned since 0.1.15, so rows carry three parser
versions. Over the non-stub rows: **3,047 at v2, 71 at v1, 18 at v0**. Over
every row: 3,102 at v2, 1,155 at v1, 584 at v0. Nothing below re-parses a
transcript; it reads the columns those parses wrote.

Two definitions, both inherited from `verified-outcome-rate.md` so the numbers
stay comparable. The denominator is a session with a non-null
`primary_outcome`. "Verified" is rung 3 or higher — `test_or_build_passed`,
`commit_observed`, `ci_or_deployment_verified`.

**Own average: 87.3%** — 1,801 verified of 2,063 non-stub sessions that claimed
done. That is 19 points above the 68.4% in `verified-outcome-rate.md`. **Do not
read that as improvement.** The two numbers come from different indexes: that
one had 10,329 rows and 12.8% of them were not sessions, this one has 4,841 and
250 (5.2%) are still not sessions — `node_modules` and `plugins/cache` paths.
`primary_outcome` also reads snake_case here, where the encoding bug that
README.md puts at the top of the build order made every row unresolved. Which
of those two moved the rate 19 points was not isolated, and this spec does not
claim it.

### 1. How many rows a bench would actually have

```sql
SELECT s.session_id, s.source_path, j.value AS model, s.primary_outcome
FROM sessions s, json_each(s.models_used) j
WHERE s.primary_outcome IS NOT NULL
  AND s.total_events > 1 AND s.total_tokens > 0;
```

Repo comes from `extract_repository_or_workspace(source_path)` as a post-group,
the same fetch-then-group shape `outcome_rate` uses. 2,246 (session, model)
pairs come back from 2,063 sessions — a session with two models is counted once
per model.

| Grouping | groups | clear n>=10 | rows covered |
| :--- | ---: | ---: | ---: |
| model | 44 | 8 | — |
| repo | 44 | 20 | — |
| model x repo | 163 | 41 | 1,964 of 2,246 (87.4%) |
| model x repo, at n>=20 | 163 | 27 | 1,768 of 2,246 (78.7%) |
| model x effort x repo | **0** | **0** | **0** |

The floor costs less than it looks. At n>=10 the bench shows 41 of 163 groups
and still covers 87% of the sessions behind them. At n>=20 it shows 27 and
covers 79%. Ten is the floor `DESIGN.md` already sets for a rate, so ten is
what this uses.

**Effort was not stored anywhere** when this was measured: no column, no
`metadata` key, no code — `metadata` is the literal string `null` on all 4,841
rows, and the string "effort" appeared nowhere in `crates/` outside a doc
comment. So the third axis was empty for every adapter, not only for Claude
Code. Build item 5 below has since landed (#116): `sessions.effort`,
`TraceStats::effort`, and a per-invocation `effort` on
`EventPayload::ModelInvocation` that the Codex adapter fills. The rows are still
empty until a rescan reparses them, and no other adapter fills it.

The source format is not the problem. Measured directly over
`~/.codex/sessions`, 448 rollout files: **447 carry an `effort` value on a
`turn_context` record**, and 435 carry `token_count` events. 213 of the 448
name a real session model — the other 235 only ever name `codex-auto-review`,
Codex's own reviewer sub-thread, which a naive read would attribute as the
session's model.

Projecting from those 213, if the adapter read what the file already holds:

| Grouping | groups | clear n>=10 | largest group |
| :--- | ---: | ---: | ---: |
| model x effort | 23 | 4 | 85 |
| model x effort x repo | 58 | 5 | 41 |

Effort values seen: `low`, `medium`, `high`, `xhigh`. Repo has to come from
`session_meta.cwd` for these; the rollout path is under `~/.codex`, so path
extraction buckets every Codex session into a home directory instead of the 23
repos the `cwd` fields actually name.

### 2. Verified rate per group, and the spread

Rate is `SUM(rung >= 3) / COUNT(*)` inside each group. The 41 groups that clear
n>=10 span the full range. Ten groups tie at 100.0%; five of them and the
bottom five:

| model | repo | n | verified | rate |
| :--- | :--- | ---: | ---: | ---: |
| claude-opus-5 | motionvector/spacepilot | 48 | 48 | 100.0% |
| claude-sonnet-5 | unfoundbox/agentworth | 55 | 55 | 100.0% |
| claude-opus-5 | motionvector/pluto | 26 | 26 | 100.0% |
| claude-sonnet-5 | tinkers/blog | 19 | 19 | 100.0% |
| claude-fable-5 | mvec/engine | 14 | 14 | 100.0% |
| … | | | | |
| claude-sonnet-5 | motionvector/studio | 15 | 10 | 66.7% |
| claude-haiku-4-5-20251001 | upscaler/backend | 20 | 12 | 60.0% |
| claude-sonnet-5 | Users/saurabh | 15 | 9 | 60.0% |
| claude-sonnet-4-6 | katana/video | 17 | 5 | 29.4% |
| deepseek-v4-flash-free | motionvector/media-scratch | 10 | 2 | 20.0% |

| Axis | at n>=10 | worst | best | spread |
| :--- | ---: | ---: | ---: | ---: |
| model x repo | 41 groups | 20.0% | 100.0% | 80.0 points |
| repo alone | 20 groups | 20.0% | 100.0% | 80.0 points |
| model alone | 8 groups | 33.3% | 90.9% | 57.5 points |

`verified-outcome-rate.md`'s finding holds on the fresh index and holds harder.
The five Claude models that clear the floor sit inside a 7.6-point band —
90.9, 90.8, 89.4, 86.9, 83.3. The repos they ran in span eighty. The 57.5-point
model spread comes entirely from three models with 12 to 20 sessions each at
the bottom of the list. **The cross is worth
building because one of its two axes carries almost all of the signal, and it is
not the one a public leaderboard ranks.**

Two of the 44 repo keys are not repos. `Users/saurabh` (36 sessions) and
`saurabh/code` (95) are directory buckets above a repository — sessions started
in a home or parent directory. They are honest keys for where the work
happened, and they should not be printed as if they were projects.

### 3. Tokens, steps and cost per verified outcome

Per-model tokens come from `session_model_usage`, not from `sessions.total_tokens`,
because a session's total gets attributed to every model it used:

```sql
SELECT u.model, u.input_tokens, u.output_tokens,
       u.cache_read_tokens, u.cache_creation_tokens,
       s.primary_outcome, s.tool_calls_count
FROM session_model_usage u
JOIN sessions s ON s.session_id = u.session_id
WHERE s.total_events > 1 AND s.total_tokens > 0
  AND s.primary_outcome IS NOT NULL;
```

Steps is `tool_calls_count`. Cost uses `crates/storage/src/pricing.rs`, the same
table `agentworth usage` prints under "cost: API-equivalent at list prices".

| model | n | verified | tokens / verified | steps / verified | $ / verified |
| :--- | ---: | ---: | ---: | ---: | ---: |
| claude-sonnet-5 | 864 | 751 | 34.0M | 98.2 | $13.93 |
| claude-opus-5 | 697 | 633 | 68.9M | 147.4 | $28.04 |
| claude-opus-4-8 | 241 | 219 | 31.4M | 118.8 | $15.81 |
| claude-fable-5 | 236 | 211 | 92.6M | 201.5 | $38.78 |
| claude-haiku-4-5-20251001 | 66 | 55 | 2.4M | 23.8 | $1.43 |
| deepseek-v4-flash-free | 20 | 9 | 40.4M | 590.2 | $21.29 |
| claude-sonnet-4-6 | 18 | 6 | 5.1M | 231.2 | $2.67 |
| gemini-3.7-flash | 12 | 4 | 1.1M | 1028.2 | $1.10 |

Two things about that table have to be said before anyone quotes it.

**The tokens are 97.4% cache reads.** Of 97.9 billion billed tokens behind
those rows: cache read 97.4%, cache write 2.2%, output 0.3%, input 0.1%. A
"tokens per verified outcome" column is mostly a measure of how long the
context sat in the window, which is `efficiency-receipts.md`'s subject, not a
measure of work done. The column ships because it is real and because
`cache-economics.md` already teaches the reader what a cache read costs — not
because a big number means a bad model.

**The dollar column cannot tell two models apart today.** Every model in this
index falls through to the pricing table's default rate — 16 of 3,316 model
rows (0.5%) match a named entry, 7 of 51 distinct models. The newest Claude
pattern in the table is `claude-3-7-sonnet`; nothing matches `claude-sonnet-5`,
`claude-opus-5`, `claude-fable-5`, `claude-haiku-4-5`, `deepseek-v4` or
`gemini-3.7`. So `$ / verified` above is one fixed rate card applied to
everyone, which makes it a re-scaled token count. **Refreshing the pricing
table is a precondition of the cost column, not a follow-up.**

Real compute cost, as opposed to API-equivalent, waits on SpacePilot. Its
`Measurement` record has no `run_id` and no token counts, so there is nothing to
join an outcome to a run on — `spacepilot-loop.md` carries that build item, on
SpacePilot's side.

### 4. Coverage: what fraction of sessions can be on the bench at all

```sql
SELECT adapter, COUNT(*) rows,
       SUM(total_events > 1 AND total_tokens > 0) non_stub,
       SUM(primary_outcome IS NOT NULL) with_outcome,
       SUM(tool_calls_count > 0) with_steps
FROM sessions GROUP BY adapter ORDER BY rows DESC;
```

| Field | all 4,841 rows | the 3,136 non-stub |
| :--- | ---: | ---: |
| a model | 65.0% | 100.0% |
| a repo (not cache or unknown) | 95.8% | 100.0% |
| an outcome | 43.7% | 65.8% |
| tokens | 64.8% | 100.0% |
| steps (tool calls) | 68.5% | 93.7% |
| an effort | **0.0%** | **0.0%** |

Model and token coverage read as 100% on non-stub rows because non-stub is
defined by having tokens; the honest number is the 65% on the left.

Three adapters put a row on the bench. Nothing else puts one:

| adapter | non-stub | bench-eligible (model and outcome) |
| :--- | ---: | ---: |
| claude_code | 3,064 | 2,025 |
| opencode | 71 | 38 |
| codex | 1 | 0 |
| antigravity, cursor, gemini, grok, hermes, pi | 0 | 0 |

This is `capability-matrix.md`'s finding again, on a fresh index: two adapters
with full extraction. A bench over twenty adapters is a bench over two until
the others parse tokens and outcomes.

## What ships

One table. Rows are `model x effort x repo`; effort is omitted from the key,
not faked, wherever the harness does not record it. Columns:

| Column | Source | Rule |
| :--- | :--- | :--- |
| `n` | count of sessions that claimed done | always printed, on every row |
| verified rate | `SUM(rung >= 3) / n` | suppressed below n<10, never shown with false confidence |
| tokens / verified | `session_model_usage`, four components summed | labelled as billed tokens, cache reads included |
| steps / verified | `tool_calls_count` | — |
| cost / verified | `pricing.rs` | labelled `COST (API-eq)`, exactly as `agentworth usage` does |

Suppressed groups are counted and reported, not hidden. A group with rows but
no outcome parsing comes back with `rate: null` and
`reason: "no_outcome_detection"` — the same distinction `outcome_rate` already
draws, because a null rate and a 0.0 rate are different claims.

## The CLI surface

    agentworth stats bench [--group-by model,effort,repo] [--min-n 10]
                           [--since ...] [--until ...] [--json]

MCP tool: `stats_bench`, same parameters, same rows.

**Why `stats`.** `cli-grammar.md` decided the noun tree, and the bench is an
aggregate over the whole index rather than something done to one session, one
repo, or one agent. That is what `stats` means there: `stats usage` is the
period rollup, `stats outcomes` is `outcome_rate`'s CLI surface. `stats bench`
is the cross of the two, and it sits beside them in help, in completions, and
on the cockpit's overview screen. It is not a `session` verb — no session id
resolves it. It is not a `repo` verb — repo is one axis of the key, not the
subject. It is not top-level, because top-level is reserved for commands that
act on the machine or the index itself.

`bench` also earns its own name rather than a flag on `stats outcomes`. The two
return different shapes: `outcome_rate` returns one axis with a baseline,
`stats_bench` returns a cross with per-row cost and effort. Folding one into the
other would give a tool two return types and one name.

## Contribution across users

Off by default, opt-in per export, **aggregates only**.

The unit that may cross the machine boundary is a group row — key, `n`,
verified count, token and step totals — never a session, never a path, never a
prompt. It takes the same route every other export takes, from AGENTS.md:
`select -> redact -> preview -> explicit approval -> export`. No background
sync, no account, no telemetry. Every row that ever leaves came from a command
a person ran, having seen the preview.

That is the same shape `spacepilot-loop.md` recommends for the corpus feed: an
`outcome_rate` table and a cost table keyed by model, runtime, system and task
class, parallel to `measurements/`. This spec adopts it rather than inventing a
second export format.

At fleet scale the floor changes meaning rather than disappearing: more sessions
in a group means more confidence in the number, never fewer numbers shown.

## New work, in order

1. **Refresh `pricing.rs`.** Add the current model families and verify against
   published list prices. Without it the cost column is one rate card for
   everyone and ranks by tokens. Smallest item here and it gates the column.
2. **`Storage::get_bench_rows(group_by, window, min_n)`.** One query over
   `sessions` joined to `session_model_usage`, the repo post-group, the shared
   rung ordering `verified-outcome-rate.md` asks to export once. Reuse that
   ordering; do not re-type the CASE ladder a fourth time.
3. **`stats_bench`, the MCP tool.** Ships before any screen, same as every tool
   in `README.md`'s second table.
4. **`stats bench`, the CLI verb**, under the grammar, after the rename lands.
5. **An `effort` column on `sessions`, and the Codex adapter reading it.**
   **Landed in #116.** `turn_context.effort` is in 447 of 448 rollout files.
   The same pass had to take `model` from `turn_context`, the repo from
   `session_meta.cwd`, and the tokens from the `token_count` events (425 of 448
   carry them, re-measured; the index held one). All four are read now.
   Session-level effort is the modal per-invocation value, which is the guess
   the open question below names. Two things this did not settle: the adapter
   records every model a session ran, `codex-auto-review` included, so choosing
   which one is "the" session model stays a query-side decision; and the rows
   already in the index keep their old values until a scan reparses them, which
   the adapter's `parser_version` bump makes happen once.
6. **The opt-in aggregate export**, through the existing export pipeline.

## What this deliberately does not do

- **No cross-user rows.** Not deferred, excluded. Aggregates or nothing.
- **No pass/fail call.** The table ranks; it does not say a model is better. Task
  difficulty is not controlled for and cannot be from this data — a repo where
  every session commits may be a repo of easy tasks.
- **No auto-picked model.** `questions.md` is right that one developer's habits
  cannot support "model X executes better", and a cross does not fix that.
- **No wall-clock ranking.** Duration is in the index and it measures when the
  human was at the desk, not how fast the model was.
- **No UI first.** Tool, then CLI, then — only if it gets used — a row on a
  screen.
- **No public page.** Nothing about this ships to a website, ever. The moment it
  does, it is the leaderboard this spec exists to not be.

## Open questions

- **Floor of 10 or 20?** `DESIGN.md` says suppress below 10; `outcome_rate`
  defaults to 20. Measured above, the difference is 41 groups against 27 and 87%
  coverage against 79%. Two floors in one product is the worse answer; which one
  wins is not settled here.
- **Multi-model sessions.** A session using two models counts once in each
  model's group, so 2,063 sessions produce 2,246 rows. Splitting by token share
  instead would change the cost column and no other. Unmeasured.
- **Does effort survive as a session-level key?** Codex writes `turn_context`
  per turn and the value changes mid-session in real files. A session-level
  effort is the first value, the modal value, or a mixed bucket — the modal
  value is the guess here, and it is a guess.
- **Should the cost column ship before the pricing refresh?** Shipping it with
  the default rate card would put a dollar figure next to a model name that is
  not that model's price. The safer answer is to suppress the column the way a
  rate is suppressed below its floor.
