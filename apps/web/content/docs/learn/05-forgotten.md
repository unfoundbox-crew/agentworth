---
title: "Forgotten context"
description: "Compaction replaces a session's memory with a summary. The transcript survives on disk, so the decisions the summary dropped can be handed back."
---

The session confidently re-proposes something it already tried and rejected
three hours ago. You remember the rejection. It does not, because the turn where
it happened was summarised away.

Then you retype the reason. Then it happens again.

## Why this is recoverable

Compaction is destructive from inside the harness and lossless from outside.
The model's view is replaced by a summary; the full JSONL is untouched on disk.

So the span that got dropped and the summary that replaced it both exist, side
by side, in a file nothing reads. `archie session forgotten` diffs them.

## What the loss looks like

Measured on one session: 19,955 JSONL lines, 29,642 events, 2.99M tokens, eight
compaction rounds.

**405 decision-shaped sentences went into the compaction rounds. 28 came out.**

Conclusions survive at roughly 15%. Reasons survive at 1.7%. That asymmetry is
exactly the shape that makes a session re-propose something it already ruled
out: it keeps the *what* and loses the *why*.

A sentence counts as surviving when it overlaps a summary sentence at 0.6
Jaccard over stopword-stripped tokens. On real data that threshold sits nowhere
near the boundary — across those eight rounds the highest overlap between any
dropped sentence and any surviving one is 0.29, and five rounds peak below 0.10.
Summaries paraphrase; they do not quote.

## Three classes

`--class` takes any of `decision`, `rejected`, `reason`, repeatable, and
defaults to all three.

| Class | What it catches |
| :--- | :--- |
| `decision` | Something the session settled on. |
| `rejected` | Something it ruled out. |
| `reason` | Why. The class that almost never survives. |

`--round` narrows to one 1-based compaction round. `--limit` defaults to 20 with
a ceiling of 200 — the output is read by a session that has a context budget,
while the totals still describe the whole session.

## When there is nothing to return

Three "I don't know" cases come back as named strings, not an empty array you
have to interpret:

- `no_compactions_in_this_session`
- `nothing_decision_shaped_was_dropped`
- `every_dropped_decision_survived_in_a_summary`

Over MCP the same diff is `session_forgotten`, and the handoff carries it as its
"Decided, then compacted away" section.

## What to run

```bash
archie session forgotten --last
archie session forgotten --last --class reason
archie session forgotten --last --round 3 --limit 20
archie session forgotten --last --redact
archie session forgotten --last --json
```
