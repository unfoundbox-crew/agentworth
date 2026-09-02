# Convergence

Status: proposed, measured 2026-09-02.

## The one-line version

Where in a session the last verified progress happened, how much was spent
after it, and whether anything that could fail ran at all.

## The problem, stated by the person who has it

A session ends. I do not know whether it ended because the work was done or
because it ran out of road. Both look the same from outside: the last message
says something reasonable, the token counter says a large number, and nothing
tells me which turn stopped earning.

`verified-outcome-rate.md` tells me a session left evidence. It does not tell
me *when*. A session that passed its tests on turn 40 and then ran 400 more
turns scores the same as one that passed on turn 430.

## Where the idea comes from

Yoko Li, "Knowing When to Stop: The Art of Making a Loop Converge", a16z,
2026-08-06. Fetched and read 2026-09-02.

Two things from it are worth carrying over. The first: the verifier is not
only the stop condition, it also defines what the loop treats as progress —
which is the same claim AgentWorth's outcome ladder already makes, arrived at
from the other direction. The second: a loop with no visible cost tracking
keeps spending past the point of diminishing return, because nothing in it can
see the return.

The 67% figure is real and it is **not** a per-developer average. It is one
task in the post: a Lighthouse score the agent could not move, where 67% of
that single bill bought nothing. Quoting it as a population statistic would be
inventing a number. Everything below is measured here instead.

## The measurement

Saurabh's index, his sessions, one machine. `~/.agentworth/agentworth.db`,
`agentworth` 0.1.15, 4,841 session rows.

Parser versions in the index as read: `claude_code` 3,102 rows at v2 and 89 at
v0; `codex` 683 at v1, 191 at v0; `antigravity` 335 at v1, 74 at v0;
`opencode` 82 at v1. Nothing has been rescanned on 0.1.15.

**SQLite could not answer this on its own.** There is no events table. The
index stores per-session aggregates, plus `trajectory_chunks`, which is a
sparse sample (2,930 `tool_invocation` chunks across 447 sessions), not an
event log. Tool calls and exit codes live in the raw transcripts, which
AGENTS.md says to load lazily — so the measurement walks them.

Three predicates were ported from the Rust so the numbers mean what the
product means, not what a new script decided they mean:

| Ported from | What it decides |
| :--- | :--- |
| `crates/outcomes/src/outcome.rs` | test/build, commit, and CI/deploy command sets, and `has_test_failure_markers` |
| `crates/adapters/src/exit_status.rs` | `exit_code_from_result` — the harness's `is_error` envelope, plus an "Exit code N" phrase |
| `crates/schema/src/provenance.rs` | `extract_repository_or_workspace` |

Lint is not in the Rust classifier. It is counted separately below rather than
folded into the product's own definition of a gate.

**Corpus.** 3,046 of the 3,064 non-stub `claude_code` sessions parsed (18 files
gone or empty), plus all 71 non-stub `opencode` sessions, read from opencode's
own SQLite store. 2,463 of the claude_code sessions are subagent runs.

**The walk was checked against the index.** Summing usage the way the adapter
does reproduces `sessions.total_tokens` to within 2% for 3,042 of 3,046
sessions — median relative difference 0.0000, p90 0.0000. The four that differ
were not chased down; at four in three thousand they do not move anything
below.

Two numbers to hold before reading any percentage. Token totals here include
cache reads, which is what the index counts, and cache reads grow with context
— so a late turn is worth more tokens than an early one for reasons that have
nothing to do with progress. Where that changes the answer, the non-cache-read
figure is given too. And the corpus is concentrated: the top 100 of 3,046
sessions hold 58.7% of all tokens. Medians and pooled shares are both printed
for that reason.

### 1. Verifier coverage

Did anything that could fail actually run.

| | sessions | share |
| :--- | ---: | ---: |
| ran a test or build command | 1,020 | 33.5% |
| ran a lint command | 168 | 5.5% |
| ran either | 1,032 | 33.9% |
| **naked — ran neither** | **2,014** | **66.1%** |
| reached at least one verified progress | 1,072 | 35.2% |

Two thirds of sessions never ran a gate. In them there is no convergence point
to find, because there was never a point at which anything was verified. They
hold 15.6% of the corpus's tokens.

Of the 1,020 that did run a gate, 11 never got a single exit 0 out of it.

By repo, coverage spreads 66 points. Every group above an n floor of 10, none
dropped:

| repo | n | ran a gate | naked |
| :--- | ---: | ---: | ---: |
| unfoundbox/agentworth | 80 | 66.2% | 33.8% |
| video/frontend | 58 | 60.3% | 39.7% |
| upscaler/frontend | 207 | 53.6% | 46.4% |
| mvec/engine | 144 | 47.2% | 52.8% |
| a client repo, not ours | 222 | 43.7% | 56.3% |
| code/motionvector | 450 | 41.8% | 58.2% |
| apps/vibelaunch | 441 | 35.4% | 64.6% |
| motionvector/motionvector | 51 | 35.3% | 64.7% |
| upscaler/free | 23 | 34.8% | 65.2% |
| saurabh/code | 123 | 34.1% | 65.9% |
| katana/video | 43 | 30.2% | 69.8% |
| motionvector/studio | 179 | 30.2% | 69.8% |
| unfoundbox/memes | 46 | 28.3% | 71.7% |
| a second client repo | 11 | 27.3% | 72.7% |
| motionvector/spacepilot | 144 | 25.0% | 75.0% |
| motionvector/pluto | 178 | 20.2% | 79.8% |
| upscaler/backend | 421 | 18.3% | 81.7% |
| Users/saurabh | 32 | 15.6% | 84.4% |
| resolution/lab | 28 | 7.1% | 92.9% |
| tinkers/blog | 45 | 4.4% | 95.6% |
| apps/learn | 21 | 0.0% | 100.0% |
| apps/studio | 49 | 0.0% | 100.0% |

Fifteen more groups fell under the floor. `Users/saurabh` is not a repo — it is
what `extract_repository_or_workspace` returns for a session started outside
one, and it is in the table because hiding it would flatter the spread.

`apps/studio` and `apps/learn` are at zero, which is the honest reading of a
frontend repo where the loop is a browser and not a command. The gate exists;
it is not a shell command, and nothing here can see it.

Coverage is the finding with the largest population behind it. Before any stop
rule is worth writing, two thirds of sessions have nothing to stop *on*.

### 2. The convergence point

Spend after the last verified progress, over the 1,072 sessions that reached
one.

| measure | median | mean | p90 |
| :--- | ---: | ---: | ---: |
| share of tokens (as the index counts them) | 11.6% | 20.5% | 55.9% |
| share of tokens, cache reads excluded | 4.0% | 12.3% | 39.8% |
| share of turns | 8.8% | 16.6% | 47.5% |
| turns after | 9 | 31.7 | 68 |
| wall seconds after | 88 | 6,171 | 2,386 |

Pooled, 14.6% of the tokens in those 1,072 sessions fall after the last
verified progress; 17.8% with cache reads excluded. Counting the naked
sessions as entirely post-convergence — which is what they are, having never
verified anything — the figure over all 3,046 sessions is 27.9%.

So: nothing here resembles 67%, and the median session's tail is small. The
distribution is what matters, not the average. Half of sessions spend under 12%
of their tokens after their last proof; a tenth spend over 56%.

Wall time is not usable as a spend measure. The mean of 6,171 seconds against a
median of 88 is idle time between turns — a session left open overnight, not
work.

By repo, sessions with at least one verified progress. Every group above an n
floor of 10:

| repo | n | median tokens after | p90 | median turns after | pooled |
| :--- | ---: | ---: | ---: | ---: | ---: |
| motionvector/spacepilot | 38 | 22.5% | 50.0% | 17.6% | 7.8% |
| unfoundbox/memes | 13 | 17.9% | 72.0% | 14.8% | 5.8% |
| a client repo, not ours | 95 | 16.7% | 55.3% | 14.3% | 10.6% |
| code/motionvector | 185 | 16.6% | 66.8% | 11.6% | 19.5% |
| mvec/engine | 69 | 13.8% | 51.4% | 10.5% | 11.5% |
| motionvector/pluto | 36 | 13.1% | 35.5% | 10.2% | 2.7% |
| motionvector/motionvector | 18 | 13.0% | 37.8% | 8.8% | 3.0% |
| saurabh/code | 42 | 12.5% | 30.9% | 9.3% | 12.8% |
| katana/video | 12 | 12.4% | 52.4% | 9.1% | 17.7% |
| video/frontend | 35 | 12.4% | 74.0% | 10.1% | 4.0% |
| upscaler/backend | 88 | 10.3% | 69.0% | 7.4% | 39.2% |
| upscaler/frontend | 110 | 8.5% | 33.6% | 5.9% | 3.5% |
| apps/vibelaunch | 172 | 8.5% | 60.5% | 6.2% | 10.2% |
| unfoundbox/agentworth | 62 | 6.7% | 37.7% | 5.0% | 6.9% |
| motionvector/studio | 60 | 5.5% | 31.4% | 4.0% | 11.2% |

Fifteen groups fell under the floor. The median column spreads 17 points; the
pooled column spreads 36, and the two disagree about which repo is worst.
`upscaler/backend` sits in the lower half on median and top of the table on
pooled share, so its tail is a few very large sessions rather than a habit.
`motionvector/pluto` is the reverse. Both columns are printed because either
alone misleads.

By adapter, only two produce tokens at all:

| adapter | n | ran a gate | reached progress | median tokens after | p90 |
| :--- | ---: | ---: | ---: | ---: | ---: |
| claude_code | 3,046 | 33.9% | 35.2% | 11.6% | 55.9% |
| opencode | 71 | 22.5% | 22.5% | 28.8% | 79.1% |

opencode is a real second reading, not a rounding of the first: it carries a
numeric `exit` in `state.metadata`, so no envelope inference is needed there.
16 of its 71 sessions reached a verified progress, median tail 28.8%.

Longer sessions have proportionally smaller tails and absolutely larger ones:

| total turns | n | median tokens after | median turns after | p90 turns after |
| :--- | ---: | ---: | ---: | ---: |
| under 50 | 208 | 19.7% | 5 | 17 |
| 50–150 | 385 | 11.8% | 8 | 38 |
| 150–400 | 315 | 7.4% | 13 | 89 |
| 400+ | 164 | 7.1% | 31 | 247 |

### 3. Identical-failure runs

Three or more consecutive failures of the same command, with no successful run
of that command in between. Other commands in between are normal — the agent
edits a file and re-runs.

124,632 Bash commands carried a captured result. 4,188 of them failed (3.4%).

| keyed on | sessions | runs | median length | of runs, on a gate command |
| :--- | ---: | ---: | ---: | ---: |
| the exact command string | 18 (0.6%) | 36 | 3 | 0 |
| the command's first two words | 38 (1.2%) | 52 | 3 | 1 |

**It is rare and it is expensive.** Median tokens after the third failure:
38.8% of the session keyed exactly, 57.8% by command family. Median turns
after: 910 and 175. The 38 family-keyed sessions sit in front of 11.1% of every
token in the corpus — but the top three of those 38 hold 48% of that, so this
is a handful of very long sessions, not a broad pattern. Say "rare, and when it
happens it is large", never "11% of spend".

**It is not a clean stop trigger.** Of the 38, 27 had already made verified
progress, and 19 of those 27 went on to verify again after the run fired. As an
automatic kill it would be wrong roughly seven times in ten.

**AgentWorth already has a loop detector, and it measures something else.**
`crates/outcomes/src/loops.rs` flags three identical consecutive tool calls,
regardless of whether they failed. It fires on 761 of these 3,046 sessions
(25.0%) against this signal's 38, and 11 of the 38 are invisible to it. It also
points the wrong way for this purpose:

| | median tokens after last progress | ran a gate |
| :--- | ---: | ---: |
| sessions with an existing loop alert | 7.1% | 74.2% |
| sessions without one | 19.4% | 20.4% |

A loop alert predicts a *busy* session, not a wasteful one. Repetition and
repeated failure are different signals and this spec needs the second one.

### Falsification: the turn-gap does not discriminate

The hypothesis the stop-rule recommender was written for: **H1 — a threshold on
turns since the last verified progress separates a converged session from a
working one.**

It does not. The gaps that *ended* in a verified progress and the tail that
ended the session have nearly the same distribution:

| percentile | gaps that ended in progress (n=9,168) | tails after last progress (n=1,072) |
| :--- | ---: | ---: |
| p50 | 9 turns | 9 turns |
| p75 | 28 turns | 26 turns |
| p90 | 67 turns | 68 turns |
| p95 | 110 turns | 135 turns |

What a threshold would have bought and cost, over the 1,072 sessions:

| stop after N turns with no progress | of the tail saved | sessions falsely interrupted |
| ---: | ---: | ---: |
| 20 | 76.0% | 86.8% |
| 40 | 63.5% | 68.5% |
| 60 | 55.3% | 50.1% |
| 100 | 43.1% | 29.1% |
| 200 | 25.1% | 8.6% |

There is no knee. At every threshold the share of sessions it kills mid-work is
close to the share of tail it saves. A token threshold behaves the same way
(p90 gap 16.8M, p90 tail 20.5M). Confidence: **moderate** — one machine, one
harness for 98% of the corpus, and a definition of progress limited to shell
commands.

So the recommender does not ship a turns-since-progress kill switch. What
survives falsification is a budget, a coverage warning, and a report.

## What the data cannot answer today

| gap | why |
| :--- | :--- |
| Convergence for 7 of 9 adapters | `codex`, `cursor`, `gemini`/`antigravity`, `grok`, `hermes`, `pi` produce no tokens at all in this index (`capability-matrix.md`). With no spend there is no share of spend |
| Any of it from SQLite alone | no events table; `trajectory_chunks` is a sample. Every number above required a raw-transcript walk |
| Per-repo opencode | every opencode source path resolves to the same bucket through `extract_repository_or_workspace`, because the path is a synthetic `.db::…#session` composite with no repo in it |
| Non-shell gates | a browser check, a manual look, a preview that rendered. `apps/studio` reads as 100% naked and is probably not |
| Whether the tail was wasted | a session can keep working correctly after its last test run and simply never run one again. This measures spend after evidence, not spend without value |
| Backgrounded and interrupted runs | they keep `exit_code: None` by design (`capability-matrix.md`) and so can never count as progress |

## `session explain`

CLI `session explain [id]`, MCP `session_explain` — the names the grammar in
`cli-grammar.md` gives a per-session verb. Resolves a session the same way
every other `session` verb does: prefix, `--last`, the picker on a TTY.

No model call, ever. AGENTS.md's invariant holds: this reads the index and the
transcript and prints what it found.

| Param | Type | Default |
| :--- | :--- | :--- |
| `session_id` | string | required over MCP; `--last` on the CLI |
| `format` | `text` \| `json` | `text` |

Returns the loop's shape, the convergence point, and which condition was
missing. ASCII and the CLI's allowed glyph set only (`apps/cli/src/ui/mod.rs`:
ASCII, U+2500–259F, and `● ○ · — →`).

```
  session  0b7dc78b   unfoundbox/agentworth   claude_code   412 turns
  ─────────────────────────────────────────────────────────────────────
  turn     0         100        200        300        400
           ├──────────┼──────────┼──────────┼──────────┤
  edits    ··████··███···████··██··█████··████···········
  gates    ····●········○·●········●·····●···○···○····○··
  commits  ················●···········●·················
                                         ^
  ─────────────────────────────────────────────────────────────────────
  convergence point   turn 268 — a test or build command exited 0
  after it            144 turns   38.2M tokens   31% of this session
  this repo's median  5.0% of turns, 6.7% of tokens   (n=62)

  missing             a stopping rule
                      3 gate runs after turn 268, none of them passed
```

`●` is a gate that passed, `○` one that ran and did not, `·` a turn with
neither, and `^` marks the convergence point. Naked sessions get the same
diagram with an empty `gates` row and a different last line.

The missing condition is named from what the transcript shows, and only three
of the four in Li's list are observable here:

| condition | how it reads from the data | when it is named |
| :--- | :--- | :--- |
| an observable current state | any test, build, or lint command ran | no gate ran at all — 66.1% of sessions |
| localized change | file modifications happened between gate runs | edits continued with no gate after them |
| a stopping rule | the session ended at or near its last verified progress | the tail is above this repo's p90 |
| a target | not observable | never claimed |

A session missing none of the three gets told so in one line. "Converged" is
not a call this tool can make — it is the absence of a named defect.

## The stop-rule snippet

`session explain --snippet`, or `stats convergence --snippet`, writes a block
for `CLAUDE.md` or `AGENTS.md`. Every number in it comes from the caller's own
index. Nothing is a default, and the block says where it came from so a stale
one is visible.

```
<!-- agentworth convergence: unfoundbox/agentworth, 80 sessions
     (62 with verified progress), index of 2026-09-02 -->
Run a test, build, or lint command before calling a task done. In this repo
34% of past sessions ran none, and nothing in those is checkable.

Budget: 332 turns. Three of four past sessions in this repo had reached
their last verified progress by then; past 505 turns you are in the top
tenth. Say where you are rather than stopping silently.

If the same command fails three times in a row, stop and report it. Do not
run it a fourth time with the same arguments.
```

Three deliberate choices in that text:

**The budget is a checkpoint, not a kill.** 332 is this repo's own p75 for
turns-to-last-progress (p50 173, p90 505, p95 566, n=62); over the whole index
it is 243 (p50 107, p90 456, p95 718). The repo number is the one written,
because repo spreads wider than anything else here, the same way it does in
`verified-outcome-rate.md`. The falsification above says a threshold cannot
tell a converged session from a working one, so the snippet asks the agent to
say something, not to stop.

**The identical-failure line stays in even though it would misfire seven times
in ten as a kill.** It costs a sentence, not a session — the agent reports and
continues if told to. That trade only works because the instruction is "report",
and the snippet must not be rewritten into "exit".

**The coverage line goes first** because it is the finding with the largest
population behind it.

## New work

1. `ConvergenceScan` in `agentworth-outcomes`: walk a trace's events, emit
   every verified-progress position (turn index, cumulative tokens, timestamp)
   plus the gate runs that did not pass. Reuses the existing command
   predicates and `exit_status`; adds no new command sets except lint, which
   goes beside them as its own function.
2. Storage: three columns on `sessions` — `convergence_turn`,
   `convergence_tokens_after`, `verifier_runs` — written by the scanner, and a
   `PARSER_VERSION` bump on every adapter whose output this changes. Not a new
   table: `session_risk` is the shape for per-session derived signals and this
   is smaller than what already lives there.
3. The lint command set. Named separately from `is_test_or_build_command` so
   the outcome ladder's rung 3 does not silently widen.
4. `session_explain` in `apps/cli/src/mcp/`, and `session explain` composing
   the same strings from `apps/cli/src/ui/views.rs` — the one-rendering-path
   rule in `cli-grammar.md`.
5. `--snippet`, which is a formatter over the three aggregates and needs
   nothing new underneath.
6. A fixture: a small synthetic index with one converged session, one naked
   session, and one with a three-failure run, exercised through
   `doctor --self-test` the way `apps/cli/tests/doctor_self_test.rs` does.

## Deliberately not built

- **No stop switch.** Nothing here kills a session, and no snippet it writes
  tells an agent to exit. The measurement says a turn threshold cannot tell
  work from waste; shipping one anyway would be selling a number that was
  falsified on the page above.
- **No live intervention.** `agentworth watch` exists and already has the loop
  sentinel. This is a post-hoc read.
- **No "wasted tokens" figure.** Spend after evidence is not spend without
  value. The tool says how much followed the last proof, and stops there.
- **No cost in dollars.** `cache-economics.md` owns pricing, and per-model
  rates are not in this index.
- **No cross-session or cross-user rollup beyond repo and adapter.** Same
  reason as `verified-outcome-rate.md`: one machine's data is one machine's.

## Sequencing

1. The lint predicate and `ConvergenceScan`, with tests over a fixture. Cheap,
   and it is the only new logic.
2. The three columns and the scanner write.
3. `session_explain` over MCP. It is testable and needs no design pass.
4. `--snippet`, once the tool has been used enough to know the three lines are
   the right three.
5. A dashboard row, only if 3 and 4 get used.

It sits after `verified-outcome-rate.md` (B) in the build order, because that
spec's rung ordering and repo grouping are the two things this reuses, and
beside `archie-bench.md` (J), which takes B's rate out across model, effort and
repo. J asks which setup leaves evidence; this asks where in a session the
evidence stopped arriving. Same ladder, two different cuts of it.

## Open questions

- Does the convergence point move once compaction is accounted for? A session
  compacted eight times has a tail measured against a context that was
  rewritten underneath it. `compaction-diff.md` has the round boundaries;
  nothing here uses them.
- Is a gate command the right definition of progress for a frontend repo?
  `apps/studio` reads 100% naked and almost certainly is not. A browser check
  leaves no shell command, and no adapter sees one.
- Should a session with no verified progress report a convergence point of
  turn 0, or refuse to answer? Reported as "never" here. Folding those into
  the same average is what turns 14.6% into 27.9%, and both numbers are true
  of different questions.
- Is p75 the right budget line? Picked because p50 fires on half of all
  healthy sessions and p90 fires too late to matter. Not measured against what
  a person would want to be interrupted at.
- The identical-failure key: exact string or first two words? The family key
  finds twice as many and one of its 52 runs is a gate command. Neither has
  been checked by hand against what the sessions were actually doing.
