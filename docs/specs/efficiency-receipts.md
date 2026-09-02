# Efficiency receipts

Status: proposed, P0 measured 2026-09-02. Nothing built.

## The one-line version

Two weeks of local transcripts, four deterministic detectors, and a receipt per
five-hour window saying which repeated work was provable, how long it sat in the
window afterwards, and how high the evidence ladder got in return.

## The problem, stated by the person who has it

My quota is gone in one to two hours. Not at hour four, not on day six — one to
two hours, and a lot of it goes on senseless mistakes and on reading the same
thing again.

I do not want another token dashboard. I want to know which of that was
avoidable, with the receipt attached, so I can stop doing it.

## Two branches, not one number

The tempting model is `tier → bucket size → wasted tokens`. It cannot be built,
because the middle term is not observable (see the provider boundary below).
The model that can be built has two branches feeding one outcome:

```text
              PROVIDER LIMIT
             partly unknowable
                    │
     ┌──────────────┴──────────────┐
     │                             │
DIRECT INEFFICIENCY        CONTEXT AMPLIFICATION
provable locally           measurable proxy
re-reads, retries,         payload that survives
churn, rehydration         turn after turn
     │                             │
     └──────────────┬──────────────┘
                    ▼
               TASK VALUE
          test → commit → CI
```

**Token-turn exposure** is the proxy, defined exactly:

    exposure = payload_tokens × model turns after the event,
               counted until compaction, a clear, or session end

One unit is one token sitting in the window across one model turn. A 15k read
followed by twenty turns is 300k token-turns; the same read one turn before a
compaction is 15k. It is a derived number about local context, and it is never
called quota, spend, or provider consumption.

## The provider boundary

Locally observable: input, output, cache-read and cache-creation tokens; tool
calls and results; reads, patches, commands, errors, subagents; compaction
pre/post tokens; tier metadata; any rate-limit message that actually appears.

Not observable, and not to be guessed: the absolute size of a five-hour or
weekly allowance, the accounting formula, how cache reads are weighted against
writes and output, server-side eviction, consumption from the same account on
other surfaces.

**No primary Anthropic source publishes an absolute token budget for a Max 5x or
Max 20x five-hour or weekly limit.** What is documented is relative multipliers,
a five-hour reset, the existence of weekly limits, and that usage depends on
model, context length and effort. The API's documented rate-limit and
cache-pricing mechanics describe the API, not the subscription; inferring one
from the other is not evidence.

So tier is a **partition**, not a bucket. It groups observations; it does not
convert to tokens. Where a budget would go, the product learns a **depletion
frontier** instead — "four local exhaustion events clustered near this
combination of usage, cache mix, model and effort" — and prints the caveat that
other surfaces may have drawn on the same allowance. A frontier needs observed
exhaustion events to exist. This machine has none (below), so P4 has no data yet
and the receipt says so rather than showing an empty gauge.

## The nine signatures

Deterministic. No model classification anywhere in the detector.

| Signature | Detection | Main false positive |
| :--- | :--- | :--- |
| Exact re-read | same canonical path, same result hash, no intervening write | verifying a write |
| Duplicate inspection | same normalised command, cwd and result hash | intentional recheck |
| No-state retry | same failed command, same error hash, no state change between | flaky or network command |
| Invocation mistake | bad path, flag or executable before any useful state change | environment discovery |
| Edit oscillation | `A→B→A`, or a patch and its exact inverse | exploration |
| Post-compaction rehydration | unchanged information reacquired across a compaction boundary | necessary recovery |
| Duplicate subagent work | siblings acquire the same `(path, hash)` or `(command, hash)` | independent verification |
| Cold-start rebootstrap | the same repo reacquires the same opening context | a deliberately fresh session |
| Exact abandoned work | edits reverted exactly, with no rung gained | exploration |

Every finding carries one of four labels, and nothing is promoted without a rule
that earns it:

    PROVEN_DUPLICATE     identical bytes, no state change between
    PROBABLE_WASTE       identical bytes, state change not provable
    AMPLIFICATION_RISK   payload with high exposure, duplication unproven
    UNKNOWN              detected, not classifiable

Intent is not observable. Nothing here claims a session *should not* have done
something; it claims the same bytes arrived twice.

## What the data already answers

Fourteen days of one machine's Claude Code transcripts, 2026-08-19 to
2026-09-01: **872 non-stub sessions, 126,417 model turns**, 6,047 file reads
(10.4M payload tokens), 42,915 shell results (14.7M), 6,152 writes, and 25
compaction boundaries across 11 sessions. Payload is estimated at **4 characters
per token** over tool-result text — an estimate, stated as one, not a measured
count.

785 of the 872 sessions are subagent runs, each in its own transcript. 529 of
them link to a parent by an exact match between the child's first user message
and the parent's spawn prompt; 73 parent sessions and 43 fan-outs of more than
one child.

### A — exact re-reads

Same canonical path, same result hash, no write to that path between.

| Rule | Events | Payload | Exposure |
| :--- | ---: | ---: | ---: |
| as first written | 71 | 51,086 | 13.8M |
| after the hand-check fixes | 7 | 48,552 | 12.8M |
| — same compaction epoch (re-read) | 2 | 3,849 | 0.5M |
| — across a boundary (rehydration) | 5 | 44,703 | 12.3M |

**Ten flagged cases read by hand: one was real.** Eight were images, whose tool
result carries no text — the hash of an empty string collides with itself, so
every screenshot re-opened at the same path matched. The ninth was a job-output
file being polled while a remote task ran. Two deterministic fixes follow, and
neither needs a model: drop reads whose result has no text or falls under a
100-token floor, and treat any shell command naming the path as a possible
write, not just an `Edit`/`Write` call. The seven survivors all read as real,
five of them post-compaction rehydration in a single long session.

48,552 tokens is **0.5% of the 10.4M tokens this corpus spent on reads.**

Repeated read-only shell inspection — the same viewer command, cwd and result
hash — adds 81 events and 708 tokens. Also negligible.

### B — no-state retries

Same normalised command, same cwd, same error hash, network commands excluded
through a word list held in a data file.

**Under the strict rule — no file write and no successful command between —
there are zero.** 1,496 shell results failed in these two weeks and not one
identical failure repeated with nothing at all happening in between.

Relaxing to "no file write between" gives one case: a `git add` on an ignored
path, failing twice, 234 tokens.

Before the network exclusion was fixed the same rule returned 49 cases, and all
49 were CI polling. The exclusion tested the command's first token, and every
one of these commands began `cd <dir> && …`, so the network tool sat in
position three or inside a command substitution. The fix is to test every
segment of a chain and every word in it, which is why the number moved from 49
to 1.

### C — edit churn

32 cases in 21 sessions: 27 exact inverse edits (`old`/`new` swapped on the same
file) and 5 whole-file writes returning to a previous content hash with a
different one in between. Small, and cheap to detect exactly.

### D — duplicate subagent work

Siblings are children of the same parent session, matched through the spawn
prompt. Same 100-token floor as A.

| Sibling definition | Duplicate reads | Payload | Exposure |
| :--- | ---: | ---: | ---: |
| same fan-out turn (13 families) | 7 | 41,864 | 2.2M |
| same parent session (43 families) | 130 | 416,452 | 74.3M |

Duplicate commands add 7 more cases and 5,388 tokens.

**This is the largest deterministic direct inefficiency in the corpus by a
factor of 8.6 over exact re-reads**, and it is invisible to anything that looks
at one session at a time. What repeats is shared context: plan documents,
specs, and the same few source files, acquired independently by three or four
agents working the same fan-out.

Confidence in the sibling linkage: high for the pairing itself (an exact hash
match on the spawn prompt, 529 of 785 subagent sessions linked), lower for the
claim that the duplication was avoidable. Each agent genuinely needed the
bytes. The honest label is `PROBABLE_WASTE` — the same bytes entered four
context windows, and a shared brief could have carried them once.

### Per five-hour window

Windows are fixed 5-hour buckets in UTC. The provider's window starts at a first
message and is not observable locally, so this is an approximation, and it is
labelled as one. 40 windows carry sessions; 19 carry a flag. Rung comes from
`primary_outcome` over `NON_STUB_SQL_PREDICATE`, taken as the highest reached by
any session starting in the window.

| Window (UTC) | Sessions | Turns | Flags | Re-read + retry | Dup subagent | Exposure (Mtt) | Top rung |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | :--- |
| 08-22 18:00 | 11 | 816 | 10 | 0 | 16,611 | 1.2 | test |
| 08-25 01:00 | 7 | 818 | 12 | 0 | 27,958 | 1.5 | test |
| 08-26 02:00 | 37 | 4,475 | 7 | 0 | 23,188 | 5.9 | test |
| 08-26 17:00 | 54 | 6,642 | 12 | 0 | 20,989 | 2.4 | ci |
| 08-30 16:00 | 12 | 1,962 | 3 | 0 | 12,290 | 1.3 | test |
| 08-31 07:00 | 25 | 3,397 | 16 | 0 | 68,486 | 11.1 | commit |
| 08-31 12:00 | 13 | 4,178 | 4 | 31,941 | 2,516 | 11.2 | commit |
| 09-01 03:00 | 55 | 11,106 | 24 | 12,762 | 88,561 | 19.7 | ci |
| 09-01 08:00 | 64 | 12,909 | 35 | 2,499 | 93,153 | 21.9 | commit |
| 09-01 13:00 | 23 | 4,103 | 4 | 0 | 19,848 | 3.2 | none |

Across all 40 windows: **470,626 tokens of direct inefficiency and 87.3M
token-turns of exposure** — 1.9% of the 25.1M tokens of tool payload the corpus
acquired. The busiest windows are also the wasteful ones, and they still reach
`commit` and `ci`. Waste and outcome are not opposites; a window can carry both,
which is exactly why the receipt prints them side by side.

**The rungs in this table over-count.** The index copy was written before #81
(rung 3 requires `exit_code == 0`) and #85 (parser versions trigger reparse) and
has no `parser_version` column, so rung 3 here was granted from the command
string alone: a `cargo test` that died on a compile error scores the same as one
that went green. Measured against re-scanned transcripts elsewhere, that rule
was right 99.5% of the time for the wrong reason. Re-scan before quoting any row.

### Rate-limit moments

**Zero.** Not one first-party limit-stop event in 872 sessions.

Every text match for a limit turned out to be prose *about* limits — provider
documentation being read, a spec being drafted, or a third-party model gateway's
own weekly cap — never the harness stopping. The 179 `api_error` records in the
window are all connection errors; none is a 429.

### Falsification

The hypothesis under test was **H1: exact re-reading is the largest
deterministic direct inefficiency, and it predicts early quota depletion.**

Against the outcomes the experiment was designed around, this is **C, and not
by a small margin**. Re-reading is 48,552 tokens, 0.5% of read payload and
10% of direct inefficiency. Duplicate subagent work is 8.6 times larger.

The second half of H1 is not falsified — it is **untestable on this corpus**.
There are no exhaustion events to correlate against. Any claim that some
detector "predicts early depletion" would be an assertion with nothing behind
it, and the depletion frontier stays unbuilt until the events exist.

There is also a real pull toward outcome B. Direct inefficiency is 470k tokens
and exposure is 87.3M token-turns — a ratio of 185 to 1. What the bytes cost on
arrival is not what they cost by the end of the window.

Confidence: **low to moderate.** One machine, fourteen days, seven surviving
re-read events, zero limit events, and a payload estimate rather than a count.
Enough to stop building re-read prevention first. Not enough to claim a cause of
depletion, and the product must not say one.

## The Efficiency Receipt

One per five-hour window. Four blocks, and the ladder sits beside the waste
rather than under it. This is a real window from the measurement above.

```text
┌───────────────────────────────────────────────────┐
│ DIRECT INEFFICIENCY             101.3k tokens     │
│   duplicate subagent reads       88.3k  (21 pairs)│
│   post-compaction rehydration    12.8k  (2)       │
│   duplicate subagent commands     0.3k  (1)       │
│   exact re-reads, retries, churn  none            │
├───────────────────────────────────────────────────┤
│ CONTEXT EXPOSURE                19.7M token-turns │
│ derived from local context — NOT provider quota   │
├───────────────────────────────────────────────────┤
│ VERIFIED RUNG REACHED           ci (5)            │
│ 55 sessions · 11,106 turns · 18 reached rung ≥3   │
├───────────────────────────────────────────────────┤
│ DEPLETION FRONTIER              no data           │
│ 0 observed limit events in this index             │
└───────────────────────────────────────────────────┘
```

Duplicate subagent work is a first-class row, not a footnote — it is the
largest line in the measurement and the only one no single-session tool can
see. The verified rung is a first-class block for the same reason the outcome
rate exists: a window that wasted 90k tokens and shipped a green CI run is a
different day from one that wasted 90k and shipped nothing, and a receipt that
prints only the first number invites the wrong fix.

Every number resolves to `session_id`, `event_seq`, the detector that fired, the
label it assigned, and the source hash. A number with no receipt does not print.

## The MCP tool

    repeat_check(kind, path?, range?, command?, cwd?, session?)

Pull only. No hooks, no per-turn feed. A session calls it when it is about to
re-read, re-run, or re-investigate — the way it already calls
`forgotten_context` before planning. Three verdicts, one line each, plus a
receipt:

    UNCHANGED — this path was read at e143 and nothing has written to it
    since → skip the re-read [8f2:e143→e211]

    NO_STATE_CHANGE — this command failed identically at e522 and e541 with
    no mutation between → change something before retrying [8f2:e522,e541]

    DROPPED — this existed before the compaction at e388 and the source is
    unchanged → call forgotten_context [8f2:e143,e388]

    SIBLING_HAS_IT — a sibling agent from the same fan-out read this exact
    content at e211 → ask for it rather than reading it [a1:e211]

`SIBLING_HAS_IT` is new, and the measurement is why: it covers the largest
category found, and it is the only verdict that can save a whole context window
rather than a single read.

The answer is a verdict, one fact, one suggested action, and a receipt. Nothing
longer. A tool that tells an agent to stop expanding its context, and expands it
while doing so, has lost the argument.

## Deliberately not built

- **No quota estimate.** Not deferred — excluded, until a primary source
  publishes an absolute budget. The receipt reports token-turns and says what
  they are not.
- **No LLM classification of waste.** The nine signatures are deterministic or
  they do not ship. Intent stays unjudged.
- **No hooks and no per-turn instrumentation.** Pull only, like every other tool
  here.
- **No "you wasted X dollars".** The conversion does not exist for a
  subscription, and inventing one would be the most quotable false number in
  the product.
- **No UI first.** The CLI report and the MCP tool ship and get used before any
  pane is drawn.
- **No cross-user anything.** One machine's transcripts are personal analytics,
  never a benchmark.

## Sequencing

| Step | What | Why here |
| :--- | :--- | :--- |
| P0 | the four detectors offline, measured | done — and it moved the build order |
| P1 | duplicate-subagent detector, receipts, `agentworth efficiency` | the largest measured category goes first |
| P2 | `repeat_check`, with `SIBLING_HAS_IT` | pull-only prevention for the same category |
| P3 | exact re-read and rehydration detectors in Rust | small, but the cross-epoch cases are the expensive ones |
| P4 | depletion frontier | blocked on observing any limit event at all |

P1 and P3 swapped places because of the measurement. Building re-read
prevention first would have been the wrong first move, and the two weeks that
proved it cost less than the feature would have.

## Open questions

- **Is duplicate subagent work waste, or the price of parallelism?** Four agents
  reading the same spec is four windows carrying it once each. The fix is a
  shared brief, not fewer agents — but nothing here measures whether the shared
  brief would have been enough.
- **Does the sibling linkage generalise?** It works because a spawn prompt is
  copied verbatim into the child transcript. Adapters that do not do that get no
  linkage and no `SIBLING_HAS_IT`.
- **What is the right payload floor?** 100 tokens removed the polling and error
  strings. It was picked from a hand-check of ten, not measured.
- **Are fixed five-hour buckets good enough?** The provider's window starts at a
  first message. A misaligned bucket splits one real window across two rows.
- **How many limit events are needed before a frontier means anything?** Four
  was the number the research sketch used. It has no basis and this index has
  zero.
