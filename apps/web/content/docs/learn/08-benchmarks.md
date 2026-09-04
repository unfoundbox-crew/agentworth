---
title: "Benchmarks"
description: "What one real index looks like when you grade every session: 2,960 sessions on one laptop, 34% cleared the evidence line. The numbers the landing page used to carry, kept here."
---

These are measurements from one machine, not a leaderboard. They answer one
question: when you grade every session an agent ran, how far did each one
actually get?

## One laptop, 2,960 sessions, graded

Source: `archie stats` on the author's laptop, 2026-09-02, over an index of
2,960 sessions. Every figure is copied from that output; the six rows are the
whole index and the percentages sum to 100.

| Rung | What the session left behind | Sessions | Share |
| :-- | :--- | --: | --: |
| 0 | Nothing verified, or still running | 1,490 | 50.3% |
| 1 | The agent said it was done | 7 | 0.2% |
| 2 | Some files on disk changed | 449 | 15.2% |
| 3 | A test or a build passed | 808 | 27.3% |
| 4 | A commit landed in git | 120 | 4.1% |
| 5 | CI or a deploy went green | 86 | 2.9% |

Evidence starts at rung 3. Half of the sessions never got far enough to tell,
and rungs 1 and 2 are things an agent can produce without doing much. Only the
last three rows left a trace someone else can check: **1,014 sessions, 34.3%,
cleared that line.**

What the rungs mean, and why rung 3 needs a captured exit code, is in
[The outcome ladder](/docs/learn/the-outcome-ladder/).

## Get the same table for your machine

```bash
archie scan
archie stats ladder
```

`archie stats ladder` groups the index by model and repository and shows, for
each group with enough sessions, how often it reached each rung. `--min-n`
sets the floor (default 20), `--json` gives the rows. The same table is one
MCP call away as `stats_ladder`, so your agent can read it too.

## What this is not

It is not a model benchmark. The groups are one person's models on one
person's repositories, and a rung says what evidence a session left, not
whether the work was good. The opt-in aggregate export that would make
cross-machine comparison possible is designed in
[`archie-bench.md`](/docs/specs/archie-bench/) and not built.
