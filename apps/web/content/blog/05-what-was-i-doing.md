---
title: What was I doing?
slug: what-was-i-doing
date: 2026-09-06
description: A coding agent that starts cold, or comes back from a compaction, burns thousands of tokens re-reading its own past. session_wake answers the four questions it is really asking in under 30 lines, from the local index, with no model in the loop.
tags: [wake, handoff, mcp, tokens]
author: AgentWorth
---

An agent that wakes up cold has four questions. What was I doing? What
state is the checkout in? What passed, and what failed? What is still
open? It answers them the slow way: list the directory, grep the
transcripts, read whatever handoff markdown the last session left, parse
`git diff`. In our own transcripts that warm-up runs to 15k–30k tokens
before the first real edit. Every one of those reads is a fact the index
already holds, or a `git` command that costs nothing.

`session_wake` is one call that answers all four. It ships in v0.1.19 as an
MCP tool and as `archie session wake` on the CLI.

## What it returns

```
# Wake · unfoundbox/agentworth · 2026-09-05 15:57Z
Checkout ~/code/unfoundbox/agentworth · detached HEAD · HEAD 2ca7539 "feat(onboarding): every first-run surface prints the same brand line, a…" · 1 file dirty
Index scanned 2026-09-05 15:57Z · source unchanged since scan

## Last session 7f3c9a2e · 2026-09-04 05:32–16:17 · 252.4K tokens · 40 events · 1 compaction
**Task** Add a session_wake MCP tool: one call that tells a cold agent what it was doing, under 30 lines, from the index and the…
**Last asked** Now rebuild the docs site and push, then open the PR.
**Ran in** /Users/x/code/unfoundbox/agentworth/.claude/worktrees/session-wake on claude/session-wake
**Outcome** rung 4, commit_observed
**Proof** last passed `cargo test -p agentworth-storage` 05:36 · last failed `cd apps/web && npm run build` 15:41, not re-run
**Changed** 3 files · Terminal.tsx (1) · lib.rs (1) · provenance.rs (1)
**Loose ends** (1)
- "I'll fix that next and then push the branch and open the PR."  [seq 39]
**Said it decided** "I decided to keep the wake renderer in apps/cli beside handoff rather than a new crate."  [seq 27]
**Forgotten** 1 decision dropped by compaction — `session_forgotten` has them

## Next
Blocker `cd apps/web && npm run build` failed at 15:41 and was not run again.
Next "I'll fix that next and then push the branch and open the PR."  [seq 39]
Not here: PR and CI state, open decisions. `gh pr list` for the first.

---
session 7f3c9a2e-1b4d-4c8e-9a0f-2d6e8b1c3a5f · claude_code · generated 2026-09-05T15:57Z
```

That is the whole answer: 23 lines, about 500 tokens (1,522 characters,
499 by the cl100k tokenizer). The ceiling is 30 lines, enforced by the
renderer.

Five blocks, each from a different place:

| Block | Where it comes from |
| :--- | :--- |
| Checkout | `git`, run read-only in the directory you name, with a five-second deadline |
| Last session | the newest session for the repo in the index, re-parsed from its own transcript |
| Proof | the last test or build that passed, the last that failed, and whether the failure was ever re-run |
| Loose ends | sentences that promised something, with no later evidence it happened |
| Next | the failure nobody re-ran, and the newest promise |

No model writes any of it. Every line is a row, a `git` read, or a stat
call, with the sequence number to go and check. A fact the index does not
hold is named in `gaps`, never padded. Pull request and CI state are not
in the index, and the last line says so.

## How fast

Measured on the fixture index with a release build, the whole CLI process
including startup: 240–340 ms across ten runs, and about 200 ms with no
git checkout to probe. No inference: the cost is a SQLite read, one
transcript parse, and a handful of `git` calls.

## Two things the build fixed on the way

**Subagents used to answer for their parent.** A subagent transcript
starts after the session that spawned it, so it won "newest session" every
time. On our own index, 127 of 131 sessions under one repo key were
subagents; the old catch-up call returned three of them and never the
parent. Wake, carry-forward, and `--last` now resolve to primary sessions.

**The adapter was dropping the checkout.** Claude Code writes `cwd` and
`gitBranch` on nearly every record. The adapter parsed past both. They now
land in the trace, which is where the "Ran in" line comes from.

## What's next

The repo key is derived two ways: from the directory you are standing in,
and from the project slug the harness wrote. For a repo that sits directly
under `code/` they disagree, and wake finds nothing unless you pass `repo`
by hand. Aligning the two is the next change.

```
npx -y agentworth@latest scan
```
