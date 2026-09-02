---
title: What survives compaction
slug: what-survives-compaction
date: 2026-09-02
description: One session compacted eight times kept about a third of one percent per round. We diffed what went in against what came out — 402 decision-shaped sentences in, 28 out, and reasons survived at 1.7%.
tags: [compaction, context, measurement]
author: AgentWorth
---

When a conversation outgrows the context window, the harness summarises
everything so far and starts again from the summary. The full transcript
stays on disk. The model's view of it does not.

That asymmetry is the whole reason we can measure this. Compaction is
destructive from inside the session and lossless from outside, so the span
that was dropped and the summary that replaced it both still exist, side by
side, in a file nothing reads.

## How much is dropped

One real session: 68.6 MB, 19,955 events, compacted eight times.

| Round | Context before | Summary after | Survived |
| ---: | ---: | ---: | ---: |
| 1 | 6,516,597 | 27,542 | 0.42% |
| 2 | 13,722,993 | 28,598 | 0.21% |
| 3 | 10,877,115 | 27,913 | 0.26% |
| 4 | 5,533,358 | 28,598 | 0.52% |
| 5 | 9,124,858 | 28,331 | 0.31% |
| 6 | 5,830,258 | 26,967 | 0.46% |
| 7 | 10,229,258 | 33,979 | 0.33% |
| 8 | 8,537,298 | 28,863 | 0.34% |

Every round keeps about a third of one percent. But no single row is the
finding. The finding is that round 8 is summarising a summary of a summary,
seven layers deep. Content from early in that session has been rewritten
eight times, each rewrite made by something that could not know which detail
would matter later. By the end the model is working from roughly 28 KB of a
68.6 MB session — about 0.04%.

## What kind of thing is dropped

Bytes are the easy measurement. We wanted to know whether the loss is even,
so we counted sentence shapes.

A decision-shaped sentence is assistant text between 25 and 400 characters
matching one of three deterministic patterns — no model involved. *Decision*:
"decided", "chose", "opted for". *Rejected*: "instead of", "ruled out",
"will not". *Reason*: "because". For each round we counted matches in the
span about to be dropped, and in the summary that replaced it.

Across the eight rounds: 52 decisions, 182 rejected alternatives and 174
reasons went in. 8, 17 and 3 came out.

| class | survival |
| :--- | ---: |
| decisions | 15.4% |
| rejected alternatives | 9.3% |
| **reasons** | **1.7%** |

Counting distinct sentences rather than pattern matches, because one
sentence can hit two patterns: **402 went in and 28 came out.** 374
sentences where the session decided something, and no longer has.

Round 2 is the clearest single row. 63 decision-shaped sentences in, three
out, and not one of them a reason.

## Why this is the expensive shape

A session that has compacted keeps roughly one in six of its conclusions and
one in fifty-eight of its reasons. That is exactly the shape that makes it
re-litigate a settled question: it kept the answer's shadow and lost the
argument.

You have watched this happen. The session confidently re-proposes something
it already tried and rejected three hours ago. You remember the rejection.
It does not, because the turn where it happened was summarised away. Then
you retype the reason. Then it happens again.

One more measurement, because we expected the opposite: summaries do not
quote. The highest word overlap between any dropped sentence and any
surviving one, across all eight rounds, is 0.29 — and five of the eight
rounds peak below 0.10. Summaries paraphrase. Nothing of the original
wording is there to find.

## How common, and what to do

Compaction is rare. Of 543 sessions over 50 KB, 22 had compacted at least
once. The median compacted session is 23.2 MB; the median session that never
compacted is 0.3 MB. So this serves the 4% of sessions that are roughly
seventy times the size of a normal one — which is also the 4% where a day of
work is at stake.

It is not a routine event. It is what happens when a session should have
ended and did not.

The practical version: do not auto-compact every few turns, because each
round is a lossy generation and compacting often means many generations. One
compaction is survivable. After two, you are reading a summary of a summary
— start fresh with a written handoff instead. A handoff costs ten minutes,
you choose what it keeps, it can be re-read, and it does not degrade.

We shipped the recovery path in
[#83](https://github.com/unfoundbox-crew/agentworth/pull/83):
`forgotten_context` and `agentworth forgotten` hand back the dropped
sentences verbatim, with a sequence number and what happened next, so a
stated decision that was acted on can be told from one that was only
claimed. Verbatim on purpose — a model summarising the dropped span would
just be a second summariser, which is the step this is meant to undo.

```
npx -y agentworth scan
```
