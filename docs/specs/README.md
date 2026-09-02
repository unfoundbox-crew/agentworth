# Specs, and the order to build them

## Where this stands, 2026-09-02

| Spec | Status | PR |
| :--- | :--- | :--- |
| `fleet-view.md` | built | #41, plus the SSE live-tail endpoint (v0.1.10) |
| `trajectory-scrubber.md` | built | #40 |
| `cache-economics.md` | built | #42 |
| `context-composition.md` | built | #46 |
| `compaction.md` | built | #57 (dashboard pane), #62 (per-session tracking) — session list still doesn't show "compacted N times" |
| `dropped-commitments.md` | built | #44 (dashboard), #TBD — detector in Rust, `agentworth loose-ends`, and carry-forward over MCP |
| `mcp-server.md` | built | #56 |
| `local-search.md` | draft, not built | precondition met — MCP server shipped 2026-09-02 (#56); zvec measured 2026-09-03 and declined (dynamic C++ library, no x86_64 macOS asset); hybrid = FTS5 + cosine + RRF in-process when built |
| `archie.md` | proposed (umbrella spec) | — |
| `desktop-app.md` | proposal, not started | — |
| `questions.md` | proposed | — |
| `verified-outcome-rate.md` | proposed, measured | — |
| `suspect-commits.md` | proposed, measured | — |
| `handoff.md` | built | #77 — `session_handoff`, `carry_forward`, `agentworth handoff`; `prompt_preview` turned out to be filled since #47, only the pre-#47 rows are null |
| `loose-ends.md` | built | #77 — `agentworth loose-ends`, the dashboard's detector ported to Rust |
| `compaction-diff.md` | built | #83 — `forgotten_context`, `agentworth forgotten`, stored round boundaries, and a handoff section |
| `asks.md` | built | #97 — `session_asks`, `agentworth asks`, a questions-to-answers index, tier 1 only |
| `market-autofix.md` | research doc, not a build item | — |
| `beliefs.md` | proposed, measured | — |
| `efficiency-receipts.md` | proposed, measured | — |
| `cli-grammar.md` | §1, §2 and §4(1) built | #118 — the noun tree, one session resolver, the `archie` name, completions, and a regenerated reference. The cockpit (§3) and `window receipt` are not built. |
| `archie-bench.md` | the local table built, #124 | #124 — `archie stats ladder` and the `stats_ladder` tool. The opt-in aggregate export is not built |
| `convergence.md` | proposed, measured | — |
| `done-gate.md` | proposed, measured | — |
| `extensions.md` | proposed | — |

Twenty-six specs sit beside this file. They are independent of each other but
not of the backend, and two of them are worth less than they look until a bug
lands first. This is the sequencing.

## Build this first, or the rest measures nothing

**The `OutcomeKind` encoding fix.** The database stores PascalCase while the
server, the API contract and the whole frontend expect snake_case. Until it
lands, every session in a real index reads as unresolved, `verified_outcomes_count`
is zero for everyone, and the outcome layer the dashboard is built around is
displaying nothing. Owned by the backend session, top of their queue.

Nothing below is worth judging until this is true.

## Then, in this order

| # | What | Spec | Depends on | Shape |
| :-- | :--- | :--- | :--- | :--- |
| 1 | `busy_timeout` on SQLite connections | — | nothing | one pragma, one test |
| 2 | ~~Populate `prompt_preview`~~ **done in #47** | — | a `scan --force` to backfill old rows | backend, small |
| 3 | Resizable session column | — | nothing | frontend only |
| 4 | Group by repo / worktree / subagent | — | nothing | frontend only |
| 4b | MCP server | `mcp-server.md` | the outcome-encoding fix, for one filter param | backend, new module, no frontend |
| 5 | Fleet strip, mtime inference | `fleet-view.md` | surfacing `sources.mtime` | frontend + one field |
| 6 | Trajectory scrubber zoom and pan | `trajectory-scrubber.md` | nothing | frontend, the largest of these |
| 6b | Cache economics | `cache-economics.md` | nothing | arithmetic on data already parsed |
| 6d | Context composition | `context-composition.md` | nothing | frontend only; the raw record is already in the browser |
| 6c | Compaction awareness | `compaction.md` | nothing | one flag already in the logs |
| 7 | SSE endpoint | `fleet-view.md` | file watcher | backend, shared with `agentworth watch` |
| 8 | Desktop app | `desktop-app.md` | 7 is nicer with it | config, then signing |
| 9 | Local search / embeddings | `local-search.md` | 4b, and real usage showing structured queries aren't enough | backend, and only if 4b shows it's warranted |

### Why that order

**1 and 2 are cheap and unblock judgement.** No busy timeout means a write
collision returns `SQLITE_BUSY` immediately, and `agentworth serve` alongside
`agentworth scan` already reaches that today. An empty `prompt_preview` is why
sessions are unrecognisable — `agent-af702e89` tells a human nothing.

**Item 2 was already done and nobody noticed** (found 2026-09-02, building
`handoff.md`). `Storage::upsert_session` has filled `prompt_preview` from the
first user message since #47, with a regression test. Every null row predates
that commit and nothing rescanned it, so what is left is a `scan --force`, not
a feature. The "never been filled" reading came from measuring the index rather
than the code, which is worth remembering the next time a column looks dead.

**3 and 4 are free wins already asked for.** Repo, worktree and subagent are all
derivable from `source_path` with no NLP and no embeddings — a 500-session
sample yields 23 distinct repos, and 440 of those 500 were subagent runs. Do
these before anyone reaches for a vector database.

**4b sits right after the free frontend wins, ahead of the fleet strip,**
because it is pure backend, needs no UI at all, and answers the actual
question `fleet-view.md`'s addendum names as the real one: a session asking
what it did yesterday, not a human reading a screen. It only waits on the
top-of-file outcome-encoding fix for one filter parameter, nothing else in
this table.

**5 before 6** because the fleet strip is smaller, ships without streaming, and
answers a question first: is the live direction interesting at all? If nobody
looks at it, 7 gets cheaper to decline.

**6b rides alongside 6** because both read the same per-event timestamps, and
because it needs no backend at all — `cache_read_tokens`, `cache_creation_tokens`
and a timestamp are already on every model invocation. It is the cheapest real
insight left in the repo.

**6 is the biggest frontend piece here.** At 6,978 events the current strip is a
solid bar. Zoom and pan turn it from a decoration into an instrument, and the
spec argues for moving the axis from sequence to time so that idle gaps become
visible instead of invisible.

**7 makes 5 honest.** mtime inference says "probably running". A file watcher
says "running". The spec is deliberate about not letting the UI claim certainty
it does not have until then.

**8 is last because it changes nothing about what the product does.** A `.dmg`
is a distribution format. It is worth doing when the thing being distributed is
worth installing.

**9 is last, and conditional, on purpose.** `local-search.md` argues that most
questions people actually ask are exact-match SQL that 4b already answers, and
that embeddings only earn their place for "sessions like this one" or "what
was this session about" — real, but not the daily case. Build 4b, see what it
can't answer, then decide whether 9 is worth it.

## Next: usefulness over panes

The table above is a build order for a dashboard. The consumer of an answer is
an agent over MCP, or a person in a chat app — neither of them opens a pane.
Seven documents cover what to build next on that basis. Nothing above gets
deleted; the panes are built and they stay.

**The MCP tool ships before any UI, for every one of these.** A tool is
testable, callable by anything, and needs no design pass. A pane needs a human
to open it, which is the bottleneck `mcp-server.md` already argued for
removing. Build the tool, use it for a week, then decide whether the screen is
worth drawing.

| # | Spec | Tool | Blocked on |
| :-- | :--- | :--- | :--- |
| A | `capability-matrix.md` (in `docs/`, not here) | — | nothing, it is written |
| B | `verified-outcome-rate.md` | `outcome_rate` | one aggregate query |
| C | `handoff.md` — **built, #TBD** | `session_handoff`, `carry_forward` | nothing left: loose ends are ported, and `prompt_preview` has been populated since #47 |
| D | `suspect-commits.md` | `suspect_commits` | absolute-path filtering, then a `session_risk` table |
| E | `compaction-diff.md` — **built, #83** | `forgotten_context` | nothing left: the round boundaries are stored and backfilled |
| F | `beliefs.md` | `claim_check` | D's absolute-path anchoring, which the same-file claims reuse |
| G | `efficiency-receipts.md` | `repeat_check`, `fanout_reads` | nothing — the P0 experiment is done and it picked the detector |
| H | `cli-grammar.md` — **built, #118 and #121** | every tool renamed to match the CLI noun | only `window receipt`, which waits on G's `fanout_reads`/`repeat_check` |
| I | `spacepilot-loop.md` | `--route`, `--resolve-entities`, `--summarize` (each a call to SpacePilot) | the contract, then `asks.md`'s tier 2 |
| J | `archie-bench.md` — **the local table built, #124** | `stats_ladder` (the name that shipped, not `stats_bench`) | nothing left for the local table: pricing was refreshed in #115 and the rung ordering is shared. The opt-in aggregate export is still open |
| K | `convergence.md` | `session_explain` | nothing — the measurement is done |
| L | `done-gate.md` | `session_gate` | nothing — the measurement is done. The plugins need no core change |
| M | `extensions.md` | — | L, which is the first row of its own table |

### Why that order

**A is first because it is already true and it changes the other four.** The
matrix says twenty adapters. Two of them extract tokens and outcomes. Every
spec below inherits that, and three of them return null rather than a number
because of it. Read it before estimating anything here.

**B is next because it is the smallest.** One aggregate query over columns that
already exist, no new table, no new parsing. It also produces the first number
anyone can act on: the verified rate spreads 75 points across repos and 32
across models, which says the codebase matters more than the model — the
opposite of what a model leaderboard would suggest.

**C is third because it removes the daily chore.** 338 hand-written handoff
files sit under `~/code`, 78 in the last eight days. It was sequenced third
because it looked gated on `prompt_preview` — item 2 of the table above, never
filled, and the one field a handoff cannot open without.

**Built, and the gate turned out not to exist.** `Storage::upsert_session` has
filled `prompt_preview` from the first user message since #47; the 2,960 null
rows were all written before it and nothing rescanned them, so the fix is
`agentworth scan --force`, not code. Verified by scanning a fresh fixture on
lenovo — the handoff opens with its real task line. A pre-#47 row still renders
"first prompt not indexed yet" and lands in `gaps`, so the document is honest
about what it is missing rather than blocked on it.

**D is fourth because its naive version is wrong.** Measured on this repo's
main, the obvious join flags 33% of commits and nine of ten sampled flags are
false, all from relative blame paths that suffix-match every repo on the disk.
Anchored properly it flags 2.6%. Ship the anchoring, or ship a feature that
loses trust on its first run.

**F is after D because it is D's shape applied to facts instead of commits.**
E proves a session forgets what it decided; F measures that it also keeps what
stopped being true — one entity's belief reversed, was corrected by hand, and
four later sessions still asserted the dead version. Ship the lookup
(`claim_check`), not the detector: measured, the detector runs at 30% precision
and the lookup inherits none of it.

**G sits after F because its first build item is not the one it was written
for.** Two weeks measured: exact re-reading is 0.5% of read payload, no-state
retries are one case in 1,496 failures, and duplicate work across sibling
subagents is 8.6 times larger than both — 416k tokens across 43 fan-outs, which
nothing looking at one session at a time can see. The detector is plumbing;
the agent gets `fanout_reads` (full detail, called before it briefs the next
child) and `repeat_check`'s `SIBLING_HAS_IT` verdict (called before one
re-read); the person gets one line in the receipt, nothing more. Nothing
blocks it, but F is smaller and G's own experiment says the
urgency is lower than the question sounded.

**H builds nothing new — it makes the thirty-two commands guessable.** Nouns
first, then completions over them, then a TUI that is the same grammar with a
cursor. It waits on the open CLI branches because the rename touches every
dispatch arm, and rebasing four branches onto it costs more than waiting.

**E is last because it needs a new table and serves 4% of sessions.** It is
also the only one nothing else can do: 402 decision-shaped sentences went into
one session's eight compaction rounds and 28 came out, with reasons surviving
at 1.7%. Worth building, after the four cheaper things.

**J comes after I because it is B's query with a second axis, and because the
column it adds is the one nothing here can price yet.** `archie-bench.md`
crosses model, effort and repo instead of taking one axis at a time, and on
the current index that cross gives 41 groups clearing the n floor and covering
87% of the sessions behind them. It sits after I for two reasons. Cost per
verified outcome is API-equivalent until SpacePilot's `Measurement` carries a
`run_id` and token counts, which is I's build item on the other side; and the
pricing table matches 0.5% of this machine's model rows today, so the dollar
column is one rate card applied to every model until that is refreshed. The
effort axis is empty for the same kind of reason — Codex writes it in 447 of
448 rollout files and the adapter reads none of them. Everything the bench
needs is measurable; three of its four columns are waiting on somebody else's
smaller fix.

**K goes after B because it is B's number with a position attached, and
because measuring it changed what it should build.** B says a session left
evidence; K says on which turn, and how much was spent after. Measured over
3,046 real sessions: two thirds ran no test, build or lint command at all, so
there is no convergence point in them to find. Of the third that did, the
median session spends 12% of its tokens after its last verified progress and
the top tenth spends over 56%. The stop rule it was written to produce did not
survive its own data — a threshold on turns-since-progress kills working
sessions at almost exactly the rate it saves tail spend, so what ships is a
budget, a coverage warning and a report, not a switch. Nothing blocks it.

**L goes after K because it is K's report moved to the one moment it can still
change something.** K reads a session after it ended. L stands at the exit and
asks for the receipt before the turn closes — a plugin inside the harness, not
a harness, which is the decision this spec records: AgentWorth reaches people
as an extension inside the loops they already run. It sits after K rather than
beside it because K settled the shape. A threshold cannot tell a working
session from a converged one, so nothing here ever ends a session; the gate
only prevents an ending, at most three times, and prints the receipt either
way. Measured on this index it would fire on 12.6% of the sessions that claimed
done, and those sessions hold 2.5% of the corpus's tokens — so it is an
evidence device, not a cost-saving one, and anyone reading the first number as
an eighth of the bill has the wrong denominator. Two findings moved the design
before it was written. Rung 1 is nearly empty — 4 sessions all-time — while
rung 2 holds 245, so the gate catches the agent that wrote code and never
checked it, not the agent that said "done" with nothing behind it. And in all
245 of those sessions no gate command ran at all and none ran and failed, so
there is one line to say rather than a decision tree for branches that have
never fired.

**M is not a build item, and it is last for that reason.** It ranks the
doorways — the gate, a PR check, a prompt segment, an editor, declarative
adapters, detector plugins, discovery — by reach per token, and says what each
can honestly know. Two of its rows need nothing from this repo at all: the
editor extension rides the API that already ships, and listing the MCP server
in the registries that already exist is an afternoon. It goes after L because
L is the first row of its own table and the only measured one.

## What none of these change

Local-only, forever. No accounts, no telemetry, no sync, no hosted dashboard —
that decision is recorded internally and is not up for revisiting here. A
desktop app does not become the excuse. Neither does
a fleet view that happens to look like a monitoring product.
