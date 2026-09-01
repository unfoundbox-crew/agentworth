# Cache economics — what returning cold actually costs

Status: proposed. Nothing here is built.

A session's own logs already record what every turn paid to re-read its own
history. Nobody surfaces it, so nobody learns from it. This spec turns that into
a number a developer can act on.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                                                                             │
│   ~/.claude/projects/…/session.jsonl        already on disk, already parsed │
│                    │                                                        │
│                    ▼                                                        │
│   ┌──────────────────────────────┐                                          │
│   │  model_invocation events     │   1,802 in one real session              │
│   │                              │                                          │
│   │  timestamp                   │  ── when the turn happened               │
│   │  token_usage.cache_read      │  ── what was reused        (cheap)       │
│   │  token_usage.cache_creation  │  ── what was re-sent       (expensive)   │
│   │  token_usage.input / output  │  ── the new work                          │
│   └──────────────┬───────────────┘                                          │
│                  │                                                          │
│                  ▼   derive, no new data needed                             │
│   ┌──────────────────────────────────────────────────────────┐              │
│   │  gap(t) = timestamp[n] − timestamp[n−1]                  │              │
│   │  warmth  = cache_read ÷ (cache_read + cache_creation)    │              │
│   │  cliff   = a cache_creation spike after a long gap       │              │
│   └──────────────┬───────────────────────────────────────────┘              │
│                  │                                                          │
│                  ▼                                                          │
│   ┌──────────────────────────────────────────────────────────┐              │
│   │  Session inspector          "this break cost 340k tokens" │              │
│   │  Overview                   "68% of today ran warm"       │              │
│   │  Existing CacheCliffWidget  attribute cliffs to TIME too  │              │
│   └──────────────────────────────────────────────────────────┘              │
│                                                                             │
│   Nothing leaves the machine. This is arithmetic on logs you already have.   │
└─────────────────────────────────────────────────────────────────────────────┘
```

## The idea in one paragraph

It is tempting to describe a long-running agent session as a colleague who
holds context, where returning after a break costs them some re-orientation.
That is the wrong model, and the wrong model leads to the wrong feature. The
model holds nothing. It has no memory between turns at all. What looks like
memory is the entire transcript being re-sent on every single request, and the
cache is a discount on that re-sending which expires. The honest analogy is a
colleague with perfect amnesia who must re-read the whole conversation before
each sentence they speak — and the cache is a photocopy of that transcript that
gets shredded after an hour.

That distinction is the point of the feature. If the model remembered, the goal
would be helping it remember better. It does not, so the only levers are: keep
the transcript small, keep it queryable, and know what a gap costs before you
take one.

## Why this belongs in AgentWorth specifically

No harness can tell you this about itself. Claude Code sees one session. Cursor
sees its own. None of them see the gap *between* sessions, or across tools, or
what yesterday's habits cost today. AgentWorth already reads all of them, and
the numbers are sitting in the logs unused.

This is also the most honest thing the product can say. Not "your agent claimed
done" — which is a judgement — but "here is what the substrate charged you,
measured from your own files."

## What already exists

Verified against a live index, not assumed.

| Thing | Where | Status |
| :--- | :--- | :--- |
| `cache_read_tokens`, `cache_creation_tokens` | `TokenUsage`, per `model_invocation` event | parsed today |
| per-event `timestamp` | `NormalizedEvent` | parsed today |
| cliff detection | `CacheCliffWidget` | built, but attributes cliffs to model switches only |
| daily rollups | `/api/usage`, `/api/pacing` | built |

Nothing new needs to be captured. Every metric below is arithmetic over data
the scanner already stores.

## The metrics

### 1. Warmth

```
warmth = cache_read ÷ (cache_read + cache_creation)
```

One number per session, and one per day. "68% of today ran warm" is
immediately legible and immediately actionable — a low number means you are
paying to re-establish context you already had.

### 2. The cost of a gap

For each pair of consecutive turns, pair the elapsed time against the
`cache_creation_tokens` on the later one. A long gap followed by a large
creation spike is a cold restart, priced.

The output a developer wants is one sentence: **"your 3-hour break cost 340k
tokens of re-reading."** Not a chart they have to interpret.

### 3. Where the cliff edge actually is

Cache time-to-live is a published number that changes, and it may differ by
plan. Do not hardcode it and do not assert it in the UI.

Instead, plot gap length against creation spike across every session in the
index. The boundary emerges from the user's own data. That is a stronger claim
than any documentation — it is measured, it is theirs, and it stays correct when
the policy changes underneath.

This is the most interesting metric here and the one nothing else can produce.

### 4. Attribute cliffs correctly

`CacheCliffWidget` finds cliffs and blames model switches. A switch is one
cause. A time gap is another, and probably the more common one. Same detection,
one more attribution.

## Where it appears

Three places, smallest first. This is a measurement, not a new product surface.

**Session inspector.** One line beside token economics: warmth for this session,
and the largest single gap cost if there was one. It sits with the other
economics rather than getting its own section.

**Overview.** One line: warmth today, warmth this week. If it does not move,
nobody needs to look at it.

**CacheCliffWidget.** Cliffs gain a cause. No new component.

Deliberately not: a dedicated tab, a dashboard, or a chart nobody asked for. If
warmth turns out to be the number people watch, it earns more room later.

## Design

Follows the existing system, no exceptions requested.

| Element | Token |
| :--- | :--- |
| warmth bar, warm portion | `--mv-cat-5` |
| warmth bar, re-created portion | `--mv-warn` — this is genuinely a cost signal |
| gap markers on the timeline | `--mv-faint`, dashed |
| numbers | `font-variant-numeric: tabular-nums` |

`--mv-warn` is used deliberately and sparingly: cache re-creation is money
actually spent, which is the same standard `CacheCliffWidget` already meets.
Warmth itself is not a verdict — a cold session is not a failure, it is a
Tuesday — so it never uses `--mv-danger`.

Large token counts render readably (`340k`, not `340219`) with the exact figure
on hover.

## Out of scope

- Predicting cost before a request. This measures what happened.
- Telling anyone when to take a break.
- Any outbound call. Cache TTL is inferred from the user's own data, never
  fetched from a provider.
- Cross-machine aggregation. That is `feat/cross-machine-merge`, separately.

## Open questions

- Does warmth belong on the fleet strip? It is arguably the most useful single
  number about how a day is going, but the strip is meant to stay small.
- How should a session with one turn render? Warmth is undefined with no prior
  context, and "0%" would be a lie.
- Should the inferred TTL boundary be shown as a number, or only as the shape of
  the scatter? A number invites treating an inference as documentation.

## Decided here

- Time gaps get equal billing with model switches as a cliff cause.
- TTL is inferred from the user's data and never asserted from documentation.
- Warmth uses a category colour, not a state colour. A cold session is normal.

## Measured, 2026-09-01 — two things in this spec were wrong

Built and checked against 34 real sessions on the owner's machine: **32,901
model-invocation pairs** and **619 cold starts**. Three findings, two of which
correct the text above.

### The cliff is at 60 minutes, and it is sharp

The spec says not to hardcode a TTL and to let the boundary emerge from the
user's own data. It does, cleanly:

| Gap before the call | n | Median warmth | Cold (<50% warm) |
| :--- | ---: | ---: | ---: |
| under 20m | 32,735 | 99.7% | 1–8% |
| 20–30m | 52 | 99.4% | 23% |
| 30–50m | 32 | 98.5% | ~32% |
| 50–60m | 8 | 99.5% | **0%** |
| 60–70m | 8 | **6.4%** | **100%** |
| 70m+ | 66 | ~7% | ~99% |

Nothing is cold at 50–60 minutes and everything is cold at 60–70. Crossing it
costs a median of roughly **400k tokens** re-created.

### "A time gap is the more common cause" — it is not

The spec says `CacheCliffWidget` blames model switches and that a time gap is
the more common cause. Across 619 real cold starts:

| Cause | Share |
| :--- | ---: |
| No idle gap and no model change | **83%** |
| Idle gap ≥ 30m | 13% |
| Model switch | 3% |
| First call of the session | 2% |

Both named causes together explain under a fifth. The majority have no visible
cause in the trace at all — worth investigating (parallel subagents writing one
file, and compaction resets, are both plausible), but not worth guessing at in
the UI. So an unexplained cold start now says exactly that, rather than blaming
whichever small gap preceded it.

### `CacheCliffWidget` does not detect anything

It is a **simulation**: a slider (`switchTurn`, default 28) driving
`calculateTurnCost` over a hypothetical 40-turn session. It reads no session
data and finds no cliffs. "Same detection, one more attribution" was not
available, because there was no detection — real detection had to be built.

### What shipped

The inspector line, from real per-session data. Not shipped: warmth on the
overview, which needs an index-wide aggregate no endpoint exposes — and a zero
there would be a false measurement rather than an absence.
