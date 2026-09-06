---
title: "The loop"
description: "Archie stops reading the tape after the fact and starts hearing what's happening while it happens — and can finally answer what moved under you, and who moved it."
---

## The problem

A flight recorder reads the tape after the flight lands. Every other tool in
this product does the same thing: it opens a transcript someone already
finished writing and tells you what happened. Useful, but it means the
answer to "what is my other agent doing right now" is always "let me go
check," never "here."

And there's a sharper problem underneath that one. Several agents can share
a machine and a checkout. When a file you read five minutes ago is different
now, was that you, another one of your own sessions, or something else
entirely? A transcript alone can't say — it only knows what its own agent
did.

## The return path

The fix is a return path from the harness back into Archie, while the agent
is still working:

```
  harness hook ──stdin JSON──▶ archie hook ──socket──▶ archie serve
   (async, exit 0 always)        │  no socket?              │
                                  └──▶ spool/               state + SQLite
                                         ▲                     │
                                   archie scan ingests    agent_status
                                                           session_drift
```

`archie hook` reads one event as JSON on stdin, tries to write it to
`~/.agentworth/archie.sock` with a 50ms budget, and falls back to appending
a line to `~/.agentworth/spool/<session_id>.jsonl` if nothing is listening.
Either way it exits 0. The hook can never slow the agent down and can never
make a tool call fail — that's not a nice-to-have, it's the one rule the
whole design answers to.

## Efference copy, in one paragraph

Biology has a name for this: efference copy. When your brain sends a motor
command, it also keeps a copy of that command, so it can tell the difference
between "the world moved" and "I moved and the world moved because of it."
Archie does the same thing with tool calls. `PreToolUse` is the copy of the
command — which tool, what input, and (for `Edit`/`Write`/`NotebookEdit`) the
path it's about to touch. `PostToolUse` is what came back. At `Stop`, Archie
diffs the checkout against every path this session predicted touching since
its last `Stop`. Anything that changed and nobody in this session predicted
is exafference — the world moved on its own, and Archie names the mover when
another session's own predictions cover that path.

## The support set and drift

Beside the write set, Archie tracks a narrower "support set": the files this
session actually read, hashed each time. `session_drift` re-hashes them and
reports which ones changed, when, and — if another session on this machine
wrote them — which one. That's the question a harness summary can't answer:
did my ground move because of me, or because of someone else.

`archie session anchors [id]` lists the join keys a session's own tool
results carried along the way — a `run_id`, a file's sha256, a pane id —
so this session's work can be tied back to what SpacePilot measured or what
MotionVector rendered, without either product needing to know the other
exists.

## Install

```bash
archie hook print claude   # prints the settings.json snippet
# paste it into ~/.claude/settings.json under "hooks"
# restart the session
```

`archie hook print claude` only prints. It never writes `settings.json` for
you — you decide what goes into your own hook config.

## What stays true

The loop never uploads anything, works offline through the spool, never
calls a model, and never blocks or slows the agent down — the hook has a
50ms budget and always exits 0. Raw histories stay the source of truth; the
spool is just one more of them. Cursor and Codex hook shapes aren't built
yet — Claude Code is the only harness wired up so far.
