---
title: "The outcome ladder"
description: "Five rungs of evidence, from an agent saying done to CI proving it. Rung 3 and up need a captured exit code."
---

Every harness tells you a session finished. None of them tell you whether the
finish was real.

So AgentWorth does not store "succeeded". It stores the strongest evidence a
session actually left behind, and prefers stronger evidence over weaker.

```text
agent says "done"
    <
artifact changed
    <
test/build passed
    <
commit observed
    <
CI / PR / deployment verified
```

## The five rungs

| Rung | Name | What it takes |
| :--- | :--- | :--- |
| 1 | `done_claimed` | The session said it was finished. Nothing else. |
| 2 | `artifact_changed` | A file on disk changed during the session. |
| 3 | `test_or_build_passed` | A test or build command ran and exited 0. |
| 4 | `commit_observed` | A commit landed. |
| 5 | `ci_or_deployment_verified` | CI, a PR, or a deployment confirmed it. |

Below rung 1 sits **unflown** — inferred, uncorroborated, or no execution
evidence at all. It is the honest state, not a failure state. None of the six
values means the work went wrong; they are degrees of evidence and its absence.
That is why a low rung is never coloured like an error. It says *not confirmed
yet*, and the design system holds that line.

## Rung 3 needs an exit code

Rung 3 used to be granted from the command string alone. `cargo test` appeared
in the transcript, so the session was recorded as having passed its tests. A run
that died on a compile error and one that went green produced the same rung.

Rung 3 now requires `exit_code == Some(0)`. A test command with no captured
result comes back as `done_claimed`, summarised "result unknown".

Re-running the new rule over all 779 `claude_code` sessions sitting at rung 3:

| | sessions | share |
| :--- | ---: | ---: |
| Kept rung 3 — a real run exited 0 | 775 | 99.5% |
| Lost it — every test or build run failed | 3 | 0.4% |
| Lost it — the run's result was never captured | 1 | 0.1% |

The headline number barely moved. The reasoning did. The old rule reached the
right answer 99.5% of the time for the wrong reason, and had no way to tell the
other 0.5% apart.

## The rate, and its sample size

"Verified" means rung 3 or higher. The `outcome_rate` MCP tool groups that share
by model, adapter or repo, and prints `n` beside every row. Below a floor
(20 by default) a group is suppressed rather than shown with false confidence.

A group with rows but no detected outcomes returns `rate: null` and the reason
`no_outcome_detection`. That is not the same claim as a rate of zero: null means
nothing parses outcomes for that adapter yet, zero would mean it always fails.

Two things the ladder deliberately does not do. It never returns a pass/fail
verdict — whether 74% is good is not something the data knows. And it never
compares you to anyone else; the index is one machine's.

## What to run

```bash
agentworth scan --force
agentworth stats
agentworth traces --limit 20
agentworth matrix
```

`scan --force` matters after an upgrade: the rung is computed at index time, so
old rows keep the rule they were written under. `matrix` shows which adapters
produce outcome evidence at all.
