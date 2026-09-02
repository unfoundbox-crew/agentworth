---
title: "Getting started"
description: "Install AgentWorth, index the agent logs already on your disk, and read the first three answers it gives you."
---

Your coding agents already write everything down. Claude Code, Codex, Cursor,
Antigravity, OpenCode and sixteen others leave JSONL transcripts in your
dotfiles and never look at them again.

AgentWorth reads those files, indexes them into a local SQLite database, and
grades what each session claimed against what it left behind. Nothing is
uploaded. There is no account and no server.

## Install

Pick one. The first needs nothing installed.

```bash
npx -y agentworth scan
curl -fsSL https://agentworth.dev/install.sh | sh
cargo install agentworth-cli
```

`agwt` is a shorter alias for `agentworth`. Every command below accepts either
name.

## The first scan

```bash
agentworth scan
```

`scan` walks the adapter directories, fingerprints each source file, and indexes
what changed. Run it again tomorrow and it only reads the files that moved.
`--force` re-reads everything, which you want after an upgrade that adds a new
field.

If nothing shows up, `agentworth doctor` prints what it looked for and what it
found.

## The first three commands

**Where the tokens went.**

```bash
agentworth stats
```

One screen: sessions indexed, adapters detected, tokens in and out, cache
reads, estimated spend, and the highest outcome rung reached.

**Which sessions exist.**

```bash
agentworth traces --limit 20
agentworth traces --adapter claude_code --model opus
```

The newest sessions, with their repo, model, event count and outcome. Filter by
adapter or by a substring of the model name.

**The explorer.**

```bash
agentworth serve --open
```

A local server on port 3000, and a browser tab pointed at it. Prompts, thinking
blocks, tool calls, file diffs, compaction rounds — the whole session, step by
step.

## Where to go next

- [The outcome ladder](/docs/learn/the-outcome-ladder/) — what "verified" means here.
- [The handoff](/docs/learn/handoff/) — what a session promised, decided and dropped.
- [MCP setup](/docs/learn/mcp-setup/) — let the next session ask, instead of you.

## What to run

```bash
npx -y agentworth scan
agentworth stats
agentworth traces --limit 20
agentworth serve --open
agentworth doctor
```
