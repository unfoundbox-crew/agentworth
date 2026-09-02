---
title: "The handoff"
description: "The handoff written by the machine that did the work: what a session promised, decided, changed, ran and proved, every line carrying its receipt."
---

You write a handoff file at the end of most working days, because the next
session starts blank and will otherwise redo what the last one finished.

The parts you re-type are mechanical: what changed, what ran, what passed, what
was promised and not done. `agentworth handoff` writes those from the
transcript. The judgment stays yours.

## What it prints

Under 60 lines by default, ceiling 120. Every claim comes from a row, quoted
verbatim with a sequence number or a timestamp. Nothing is summarised — the
moment a model writes the prose, the receipt stops meaning anything.

| Section | Source |
| :--- | :--- |
| What the task was | the session's first prompt |
| Files touched | the blame index |
| Commands run, and how they ended | tool calls with captured exit codes |
| Outcome rung reached | the ladder |
| Decided, then compacted away | the compaction diff |
| Loose ends | stated intents with no follow-through |
| Compaction count | how many times the context was replaced |

It also prints what it cannot know. Open decisions, PR and CI state, and
environment traps are not in the index, so they are named as gaps rather than
filled in.

## Loose ends

A loose end is an assistant stating an intent, emitting no tool call, and
handing control back to you. Measured on one 23 MB session, 7,144 events: 99
stated intents found, 55 of them with no tool call and a user turn next.

One of the 55, verbatim:

> Still owe you 03 and 04 over HTTP; I'll finish those next unless you want
> something else first.

That was never done. It surfaced hours later in a hand-written handoff, and only
because a human remembered.

`agentworth loose-ends` prints that section alone. `--prompt` gives you the
copyable text to hand to an agent that has the repository open.

## Picking a session

Leave the id off on a terminal and a picker lists the newest sessions — type a
number, type text to filter by id, repo, adapter or prompt. Off a terminal,
`--last` takes the newest session for this directory's repository. A unique
prefix of a session id works anywhere the full id does.

## Over MCP

`session_handoff` returns the same markdown to a coding agent mid-session.
`carry_forward(repo, n)` returns the last few handoffs for a repo, newest
first, so a session's *first* tool call can be the catch-up. Neither writes a
file — where the handoff lands is your business. See
[MCP setup](/docs/learn/mcp-setup/).

## What to run

```bash
agentworth handoff --last
agentworth handoff --last --markdown --max-lines 60
agentworth handoff --last --redact
agentworth handoff --json
agentworth loose-ends --last --prompt
```
