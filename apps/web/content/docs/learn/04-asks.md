---
title: "Asks"
description: "Every question in a session, matched to where its answer landed — so nothing gets re-asked because it scrolled past."
---

In a long session you ask a question. The answer lands several messages later,
after tool calls, subagent notifications and other assistant text. Buried, not
missing.

So you re-ask it. Scrolling costs time and re-asking costs tokens.

`agentworth asks` builds the index instead: every question in the session, and
a pointer to the message that answered it.

## What counts as a question

Two things, both detected without a model.

- **A `?` sentence in one of your turns.** The turn is split into sentences;
  each qualifying sentence becomes its own entry.
- **A flag-prefixed line in an assistant turn.** A line starting with a flag
  glyph, once a markdown list or bold prefix is stripped. That is the assistant
  asking *you* something.

## Three statuses

| Status | Meaning |
| :--- | :--- |
| `answered` | A substantive reply was found, and it does not read as a question itself. |
| `flagged_back_to_user` | The question was a flag line, or the first reply handed the question back. |
| `no_reply_yet` | No assistant text before the next user turn, or before the trace ends. |

An answer is the first assistant message carrying a line of at least 20
characters that is not filler — "waiting", "on it", "one moment". Tool calls,
tool results and compaction records do not stop the scan; they are just not
candidates. The excerpt runs from that line, trimmed to 200 characters.

Every entry carries a pointer — an event sequence and a timestamp. When there
is an answer, it points at the answer. When there is not, it points at the
question. There is always somewhere to jump to, even when the answer is nowhere
yet.

No model reads the transcript. Three regexes and a line-length check.

## Reading it

`--unanswered` narrows to the entries that are not `answered` — still open, or
flagged back to you. That is the list worth reading before you close a session.

`--since` takes RFC 3339, `YYYY-MM-DD`, or a relative duration like `2h`, `30m`,
`1d`, `3w`.

`--session` also accepts a raw JSONL file path. If it is not an indexed session,
it is parsed directly.

Over MCP the same index is `session_asks`.

## What to run

```bash
agentworth asks --last
agentworth asks --last --unanswered
agentworth asks --last --since 2h
agentworth asks --session 33122482
agentworth asks --last --json
```
