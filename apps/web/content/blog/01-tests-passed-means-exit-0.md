---
title: "“Tests passed” now means exit 0"
slug: tests-passed-means-exit-0
date: 2026-09-02
description: Our own tool graded a test run by reading the command, not the result. A cargo test that died on a compile error scored the same as one that went green. Here is what the fix moved, and what it did not.
tags: [outcomes, measurement, postmortem]
author: AgentWorth
---

AgentWorth exists to check whether an agent's claim survives contact with
evidence. So it is worth saying plainly that for a while, one of our own
rungs did not.

## What the bug was

Sessions get graded on a five-rung ladder: the agent said it was done, files
on disk changed, a test or build passed, a commit landed, CI went green.
Rung 3 is the first one that means anything, because it is the first that
requires something to have run.

Rung 3 was granted from the command string. `is_test_or_build_command("cargo
test")` returned true, so the session was recorded as having passed its
tests. Nothing checked the exit code.

Nothing checked it because nothing had it. The Claude Code adapter never
parsed tool results on real transcripts — they nest under `message.content`,
and the parser was looking somewhere else. No exit code ever reached the
classifier that was supposed to use one.

So a `cargo test` that died on a compile error and a `cargo test` that went
green produced the same rung. The ladder was reading intent and reporting it
as evidence, which is the exact failure the product is named after.

Fixed in [#81](https://github.com/unfoundbox-crew/agentworth/pull/81), with
the re-scan path in
[#85](https://github.com/unfoundbox-crew/agentworth/pull/85). Rung 3 now
requires `exit_code == Some(0)`. Exit codes are parsed from `is_error` and
from "Exit code N" in the result text. A test, build, CI or deploy command
whose result was never captured cannot reach a verified rung at all — it
comes back as `done_claimed`, summarised "result unknown".

## What the fix moved

We re-ran the new rule over the raw transcripts of all 779 `claude_code`
sessions the index held at rung 3.

| | sessions | share |
| :--- | ---: | ---: |
| keep rung 3 — a real run exited 0 | 775 | 99.5% |
| lose it — every test or build run failed | 3 | 0.4% |
| lose it — the run's result was never captured | 1 | 0.1% |

Four sessions moved. Nothing moved up: of the 436 `claude_code` sessions
sitting at `artifact_changed`, not one had run a test or build command at
all, so there was nothing for a stricter rule to promote.

## Why that is not a small finding

It would be easy to read 99.5% as "the bug barely mattered." We think that
reading is wrong, and the reason is the interesting part.

A session that runs tests at all usually gets at least one green run
somewhere before it ends. That is why the old rule was right most of the
time — not because it was measuring the right thing, but because the thing
it measured happened to correlate with the right thing on this machine, in
this window, for this population of sessions.

**The defect was in the reasoning, not mostly in the arithmetic.** The old
rule reached the right answer 99.5% of the time for the wrong reason, and
had no way to tell the other 0.5% apart. A number that is right by
coincidence is not a measurement. It is a number that will be wrong the
first time the coincidence stops holding, and it will be wrong silently,
because nothing in it was ever load-bearing.

That is also the honest reading of every rate we published before the fix.
The headline outcome numbers, the per-model table, the per-repo table: all
computed under the old rule, all over-counting by an amount we can now bound
but could not before. They are marked as such in the spec, and they get
re-scanned before they get quoted again.

## The rule we now hold ourselves to

A rung is evidence or it is a claim. If the result of a command was not
captured, the honest answer is "result unknown", not a guess dressed as a
grade — and "I don't know" has to be a real value the tool can return, not
an error and not a zero. A zero says the thing failed. Null says we did not
see it. Those are different, and a tool that conflates them is doing the
same trick the agents do.

Point it at your own machine and see what your rungs look like:

```
npx -y agentworth scan
```
