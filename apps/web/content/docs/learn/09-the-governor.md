---
title: "The governor"
description: "A meter that only reports is a post-mortem with a shorter delay. The governor stops the next model call before it's paid for."
---

## Why a report is not enough

Every other tool in this product reads a transcript after the session ends
and tells you what it cost. That's useful right up until the moment a
session is thrashing on one file, right now, with nobody watching. By the
time a person opens the dashboard and sees the burn, the tokens are already
spent. A report that arrives after the money is gone is a receipt, not a
brake.

## The two rules

`~/.agentworth/policy.toml` (or a repo's own `.agentworth/policy.toml`)
turns on two rules. No file, nothing is governed.

- **Thrash halt.** The same file edited three times with no passing
  verification command in between stops the batch before the next model
  call.
- **Session spend cap.** Tokens or dollars over the line set in
  `policy.toml` stops the batch, then blocks every prompt after it.

## What the model sees when it's halted

The batch doesn't just stop — it hands back the ground truth, so the next
turn knows exactly why:

```
file: crates/adapters/src/claude.rs
edits: 3
last failing command: cargo test -p agentworth-adapters
last output line: assertion `left == right` failed
sequence: 412, 418, 425
```

That same text arrives again as context on the next prompt, so a person
re-prompting the session doesn't have to explain what already happened.

## The cap, and lifting it

Once a session crosses its cap, every prompt is blocked with the reason
until someone clears it:

```bash
archie policy lift <session>   # clears this one session
```

Raising the cap in `policy.toml` clears it for everyone.

## Fail open, and only open

`archie hook --gate` is one socket round trip with a 50ms budget. If
`archie serve` isn't there, the hook exits 0 and writes one line to the
spool so the miss is on record. There is no fail-closed mode — a governor
that can brick every agent on the machine when its own server dies is a
worse failure than one missed halt.

## Install

```bash
archie hook print claude --govern   # prints both the async recorders and the sync gates
# paste it into ~/.claude/settings.json under "hooks"
# restart the session
```

## What stays true

No model is called by Archie; the governor speaks to the model through the
harness's own channel. No routing, no upload. Every action is a row with
its evidence, in `governor_events`. The gate fails open. Writing a cap or a
threshold into `policy.toml` is your decision, not the tool's default.
