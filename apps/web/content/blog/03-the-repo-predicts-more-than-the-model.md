---
title: The repo predicts success better than the model
slug: repo-predicts-better-than-model
date: 2026-09-02
description: Across one index, the spread between the best and worst repository was 75 points. Between the best and worst model, 32. That is the opposite of what a model leaderboard would suggest — and it is one machine's data.
tags: [outcomes, measurement, models]
author: AgentWorth
---

We wanted one number about our own work that moves when we change something.
Not a leaderboard against other people — a rate we could group by things we
control, and watch.

The rate is simple. Of the sessions that claimed done, what share left
evidence? Evidence means rung 3 or higher on the outcome ladder: a test or
build passed, a commit landed, CI went green. Sessions that never claimed
anything are not in the denominator.

Across the index used here, that rate was 68.4% — 1,002 verified of 1,464
sessions that claimed done. Then we grouped it, with a minimum of 20
sessions per row so a lucky handful cannot become a finding.

## By model

37 of 43 models fell under the floor and are not shown.

| model | n | verified | rate |
| :--- | ---: | ---: | ---: |
| claude-fable-5 | 210 | 162 | 77.1% |
| claude-opus-5 | 537 | 409 | 76.2% |
| claude-opus-4-8 | 195 | 146 | 74.9% |
| claude-sonnet-5 | 531 | 319 | 60.1% |
| claude-haiku-4-5 | 33 | 18 | 54.5% |
| deepseek-v4-flash-free | 20 | 9 | 45.0% |

Top to bottom: 32 points.

## By repository

22 of 37 repos fell under the floor.

| repo | n | verified | rate |
| :--- | ---: | ---: | ---: |
| video/frontend | 35 | 35 | 100.0% |
| upscaler/frontend | 129 | 113 | 87.6% |
| mvec/engine | 84 | 71 | 84.5% |
| apps/vibelaunch | 239 | 178 | 74.5% |
| code/motionvector | 251 | 164 | 65.3% |
| upscaler/backend | 211 | 98 | 46.4% |
| katana/video | 30 | 12 | 40.0% |
| tinkers/blog | 20 | 5 | 25.0% |

Top to bottom: 75 points, at the same floor.

**The codebase spreads more than twice as wide as the model.** Whatever
drives the outcome rate here, it looks more like a property of the repo than
of the thing typing into it. That is the opposite of what a model
leaderboard would suggest, and it is why the tool defaults to grouping by
repo rather than by model.

It is not hard to guess at mechanisms — a repo with a fast test command
gives an agent something to clear rung 3 with; one where the suite takes
twenty minutes or does not run locally does not. But we did not measure the
mechanism, so that is a hypothesis, not a result.

## Now the caveats, which are load-bearing

**This is one machine.** One developer, their habits, their repos, their
choices about which model to reach for on which problem. A model that gets
handed the hard problems will look worse at them. Nothing here separates
"this model executes better" from "this model gets the easier tasks," and
one developer's index cannot separate them. The per-model table is a
description of this laptop, and the tool says so in its own output.

**Every rate above predates a bug fix and over-counts.** Rung 3 used to be
granted from the command string alone, without checking that the run exited
0. Re-running the corrected rule over the affected sessions moved four of
779, so the direction of these findings holds — but "moved by about half a
percent" is a measurement we can now state, not a hope, and the rates
themselves get re-scanned before they are quoted as current. We wrote that
one up separately in
[Tests passed now means exit 0](/blog/tests-passed-means-exit-0/).

**Two adapters carry nearly everything.** Only `claude_code` (n=1,426) and
`opencode` (n=38) clear the floor. Seven other adapters hold 3,869 sessions
between them and produce 49 outcomes total; `codex`, `cursor`, `hermes`,
`pi` and `gemini` produce zero. Their rate is not low — it is undefined,
because nothing detects an outcome there yet. A tool that rendered that as
0% would be making a claim about those agents that the data does not
support. It returns null and a reason instead.

**100% at n=35 is a small number in a table of bigger ones.** We left it in
rather than trimming the row, because hiding it would be the same kind of
editing we are complaining about.

## What we are not going to do with this

No pass/fail verdict — whether 74% is good is not something the data knows.
No cross-user comparison, ever; the index is one machine's and a cross-user
number would be a different product with different consent. And no
auto-picked model, because the table above cannot support the sentence
people would want to read out of it.

The useful version of this number is not a ranking. It is noticing that one
of your own repos sits 40 points below the others, and going to look at why.

```
npx -y agentworth scan
```

Then `agentworth outcome_rate --group-by repo`
([#75](https://github.com/unfoundbox-crew/agentworth/pull/75)).
