---
title: Why nobody hands an agent its receipts
slug: why-nobody-hands-an-agent-its-receipts
date: 2026-09-07
description: Every coding harness can resume a session. We read their docs to see what an agent actually gets back, and found two answers, both without evidence. That gap is why session_wake exists.
tags: [wake, resume, compaction, harnesses]
author: AgentWorth
---

Every coding harness can resume a session. We read the official docs on
2026-09-06 to find out what the agent gets back when it does. Two answers
came up, and neither carries evidence.

## Two ways to resume

| Harness | What the agent receives | Checkout, tests, promises |
| :--- | :--- | :--- |
| Claude Code | The full transcript, or a model-written `/compact` summary plus the last few files read | The git branch goes into the system prompt at startup. Test results and open promises: not a field |
| Gemini CLI | The full conversation, every tool call and output. Checkpointing reverts files to a git snapshot | The snapshot restores files; it does not report what passed |
| Codex CLI | Replays the session's rollout file | The docs do not say |
| Cursor | "Load prior context" | The docs do not say |
| OpenCode | `--continue`, `--session`, `--fork` | The docs do not say |

Sources, all fetched 2026-09-06:

- Claude Code: [sessions](https://code.claude.com/docs/en/sessions), [context window](https://code.claude.com/docs/en/context-window)
- Gemini CLI: [session management](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/session-management.md), [checkpointing](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/checkpointing.md)
- [Codex CLI](https://learn.chatgpt.com/docs/codex/cli), [Cursor CLI](https://cursor.com/docs/cli/using), [OpenCode CLI](https://opencode.ai/docs/cli/)

Amp and Aider are not in the table: we did not read their own pages, and a
search snippet is not a receipt.

So you get one of two things: the whole transcript again, at the price of
the whole context, or a summary a model wrote. Claude Code's docs say what the summary
keeps: intent, files, errors, pending tasks. They also say what it drops:
"full tool outputs and intermediate reasoning are gone." The exit code of
the last test run is a tool output. The line where the agent promised to
re-run it is intermediate reasoning. Those are the two things a waking agent
needs, and they are the two things the summary cannot keep.

## What resume restores, and what it never reads

Every harness above restores *state*: the conversation, the files, the
branch. None of them reads its own transcript as *evidence*. The transcript
holds the command, the `is_error` flag on its result, the edit to each
file, and the sentence that said "I'll do that next", each with a position.
A resume feature replays those to the model. It does not answer from them.

That is the gap. As of their own docs on that date, nobody fills it.

## Why we built it from the index

AgentWorth already indexes every session on the machine, across harnesses,
receipts intact. So `session_wake` does not summarise anything. It reads
the newest session for the repo back from its transcript and reports the
last test that passed, the last that failed and whether anything re-ran it,
the promises with no later evidence, and the checkout as it is right now,
each line with the sequence number to go and check. Under 30 lines, no
model in the loop, and a fact the index does not hold is named as a gap
rather than written around.

A summary is what a model believes happened. A receipt is what the
transcript says happened. An agent waking up cold should be handed the
second.

```
npx -y agentworth@latest scan
```
