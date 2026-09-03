---
title: "MCP setup"
description: "Register the read-only stdio server once, and the next session can ask the index directly instead of waiting for you to open a dashboard."
---

A dashboard needs a human to open it, read it, and retype what mattered. That
human is the bottleneck. A nicer chart does not remove a person from the loop —
it gives them a nicer screen to transcribe from.

`archie mcp` is a read-only stdio MCP server. The next session asks
directly, in the same turn it needs the answer, and gets structured data back.

## Claude Code

```bash
claude mcp add agentworth --scope user -- archie mcp
```

`--scope user` matters more here than for a typical MCP server. The point is a
session in *any* repo asking about *any* other repo's history — "what was I
doing in the other checkout yesterday", asked from somewhere else entirely. A
project-scoped entry is only live in one repo at a time, which defeats that.

The equivalent hand-written entry goes in `~/.claude.json` under the top-level
`mcpServers` key, or in `.mcp.json` at project scope:

```json
{
  "mcpServers": {
    "agentworth": {
      "type": "stdio",
      "command": "agentworth",
      "args": ["mcp"]
    }
  }
}
```

## Any other MCP client

The server is plain stdio: command `agentworth`, one argument, `mcp`. No
environment variables and no auth — it runs as your own user, the same trust
boundary as typing `agentworth` in a terminal.

Where that server definition goes differs per harness, and this repo does not
document the registration step for Codex, OpenCode or Gemini CLI. Use each
harness's own MCP configuration docs, with the command and args above.

## The 13 tools

All read-only. Nothing scans, nothing writes, nothing touches the original
session logs.

A client's `tools/list` shows 23, not 13. The extra 10 are the pre-0.1.16 tool
names, still registered as deprecated aliases forwarding to the same handlers,
so a client configured before the rename keeps working. They are removed in
v0.1.20. Use the 13 below.

| Tool | What it answers |
| :--- | :--- |
| `session_list` | Filter the index by adapter, model, repo, time or outcome. |
| `session_show` | One session with its events, tokens and outcome rung. |
| `repo_blame` | Which session, model and prompt produced a line of code. |
| `stats_usage` | Tokens and cost rolled up by day, week or month. |
| `window_show` | Throughput over a moving window. |
| `agent_list` | Which adapters are detected and what they yield. |
| `stats_outcomes` | Verified-outcome rate by model, adapter or repo, with an `n` floor. |
| `stats_ladder` | Of everything spent in a window, how much bought a verified outcome, and what it cost per model, repo or adapter. |
| `session_handoff` | What a session promised, decided and did not finish. |
| `session_carry_forward` | What the previous sessions in this repo left for this one. |
| `session_forgotten` | Decisions compaction dropped, returned verbatim with receipts. |
| `session_asks` | Where a question's answer already landed. |
| `repo_suspect` | Commits whose session never proved anything. |

Full schemas: the [Reference](/docs/reference/).

## Redaction

Redacted output is the default everywhere event or file content comes back.
`include_raw` is the only opt-in, and it is per call — never global.

## A session's first and last tool call

Open with `session_carry_forward`, so the session starts knowing what the last few did.
Close with `session_handoff`, so the next one does. If the session compacted in
between, `session_forgotten` recovers what its own summaries dropped.

No tool scans. Run `archie scan` yourself if the index looks stale.

## What to run

```bash
claude mcp add agentworth --scope user -- archie mcp
archie mcp
archie scan
archie doctor --self-test
```

`archie mcp` on its own speaks the protocol on stdin and stdout, which is
how you check the binary is reachable. `doctor --self-test` runs the real
workflow end to end, including an MCP round trip, and exits non-zero if any step
fails.
