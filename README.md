# AgentWorth

See what your AI agents have been doing.

AgentWorth is a local-first tool that scans coding-agent histories, normalizes them into a common trace format, and shows useful statistics about your agent activity.

```bash
npx agentworth
```

Other install paths will include:

```bash
brew install agentworth

curl -fsSL https://agentworth.dev/install.sh | sh
```

All install methods run the same native AgentWorth binary.

## What it does

AgentWorth discovers histories from tools such as:

* Claude Code
* Codex
* Gemini CLI
* OpenCode
* more adapters over time

It extracts:

* sessions and token usage
* models and tools used
* languages and repositories
* failures and recoveries
* tests, builds, diffs, and other outcome evidence
* long-horizon and unusual trajectories

Example:

```text
Claude Code       1,842 sessions
Codex             1,103
Gemini              428

8.7B tokens
3,373 sessions
1,281 verified outcomes
216 failure → recovery traces
```

## Local first

Scanning and analysis happen locally.

```text
agent histories
      ↓
Rust adapters
      ↓
normalized traces
      ↓
local SQLite index
      ↓
CLI / localhost UI
```

Nothing should leave the machine unless the user explicitly chooses to export or share it.

## Architecture

The core is a native Rust application.

```text
Claude ─┐
Codex ──┤
Gemini ─┼─→ NormalizedEvent[] → AgentWorthTrace
OpenCode┤
MCP ────┤
WebMCP ─┘
```

Repository shape:

```text
crates/
  core/
  schema/
  adapters/
  storage/
  outcomes/
  scoring/
  redaction/

apps/
  cli/
  web/

packages/
  npm-wrapper/
```

* Rust: scanning, parsing, normalization, SQLite, scoring, redaction, CLI
* React/TypeScript: optional localhost UI
* npm wrapper: enables `npx agentworth`

## Commands

```bash
agentworth scan
agentworth stats
agentworth traces
agentworth inspect <trace-id>
agentworth serve
agentworth export <trace-id>
agentworth doctor
```

## Principles

1. Local-first.
2. Native and dependency-light.
3. Read-only by default.
4. No raw-log duplication.
5. Streaming parsers for large histories.
6. Evidence-backed outcome detection.
7. Explainable scoring.
8. Open adapter layer.
9. Explicit consent before anything leaves the machine.

## Status

Early development.

The first milestone:

> Reliably scan real Claude Code, Codex, Gemini CLI, and OpenCode histories from large, messy developer machines.

Everything else comes after that.
