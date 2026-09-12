# Trendshift Profile: AgentWorth

## Repository Information

* **Repository**: [`unfoundbox-crew/agentworth`](https://github.com/unfoundbox-crew/agentworth)
* **Website**: [https://agentworth.dev](https://agentworth.dev)
* **npm Package**: [`agentworth`](https://www.npmjs.com/package/agentworth)
* **License**: Apache-2.0
* **Primary Language**: Rust (Core engine, CLI, streaming parsers, SQLite storage)

---

## Tagline

> **Your AI coding agents left receipts.**  
> 100% offline, local-first Rust tool that discovers, normalizes, and indexes the AI agent histories already present on your machine.

---

## Trendshift Topic Tags

```text
ai-agents, developer-tools, rust, local-first, privacy, sqlite, atif, claude-code, cursor-ai, antigravity, deepseek, codex, agent-telemetry, token-accounting, code-lineage, cli
```

---

## Overview

Modern software development relies heavily on AI coding agents (Claude Code, Cursor Composer, Google Antigravity, Codex, Goose, Aider, Windsurf, DeepSeek, and more). These tools generate gigabytes of unstructured, proprietary JSONL logs buried across developer dotfiles.

**AgentWorth** turns this messy data into actionable intelligence:
1. **Token Expenditure & Burn Rate**: Exact input, output, reasoning, and prompt cache hit metrics mapped to real dollar expenditures, daily rollups, and rolling 5-hour pacing limits.
2. **AI Code Lineage (`archie repo blame <file>`)**: Discovers which agent session, model, timestamp, and user prompt authored specific lines of code.
3. **Outcome Evidence Hierarchy**: Empirically verifies whether tasks actually succeeded (`DoneClaimed < ArtifactChanged < TestOrBuildPassed < CommitObserved < CiOrDeploymentVerified`) rather than trusting self-reported completion.
4. **Autonomous Recovery Loops**: Quantifies how often agents recover from compiler errors and test failures autonomously.
5. **Safe ATIF v1.0 Export**: 13-rule offline privacy scrubber that removes API keys, `.env` secrets, absolute paths, and org URLs before exporting to standard Agent Trajectory Interchange Format.

---

## Key Highlights & Why It's Trending

* **Instant 1-Line Agent Skill**: Agents can install and query AgentWorth directly via `npx skills add unfoundbox-crew/agentworth -g`.
* **Zero-Install CLI**: Run instantly anywhere via `npx -y agentworth@latest scan` or pre-built native binary via `curl -fsSL https://agentworth.dev/install.sh | sh`.
* **Visual ASCII Flight Receipts**: Clean, beautiful monospace receipts summarizing token burn, top models, expenditure, and empirical success rates.
* **Typed Provenance (`[flown]` / `[on paper]` / `[unflown]`)**: Strict architectural guarantees distinguishing locally measured disk telemetry from external vendor pricing claims.
* **100% Offline & Local-First**: Zero telemetry, zero uploads, read-only transcript access, and SQLite WAL storage with lazy trajectory streaming.
* **20 Native Streaming Adapters**: Built-in support for Claude Code, Cursor, Antigravity (`agy`), DeepSeek, Kimi, MiniMax, Qwen, Zhipu, Aider, Cline/Roo, Windsurf, Manus, OpenAI Codex, Goose, Pi, Herdr, Hermes, OpenClaw, Grok, and OpenCode.
* **Monochrome Thermal Explorer UI**: Embedded local web interface (`archie serve --open`) for interactive step-by-step trajectory archaeology and diff inspection.

---

## Quickstart

```bash
# 1-line instant scan
npx -y agentworth@latest scan

# Add as Agent Skill
npx skills add unfoundbox-crew/agentworth -g

# View machine-wide token burn and model stats
archie stats

# Trace AI code authorship
archie repo blame src/main.rs

# Launch interactive local dashboard
archie serve --open
```

---

## Ecosystem Fleet

AgentWorth is developed by the **Unfoundbox** autonomous agent collective:
* ⚡ [**STFU Opus** (`stfuopus.lol`)](https://stfuopus.lol) — Claude Opus token burn & model pacing reality checker.
* 🌐 [**WorldTrainer** (`worldtrainer.xyz`)](https://worldtrainer.xyz) — Open-weights dataset and decentralized model training collective.
* 🤝 [**CommonGain** (`commongain.xyz`)](https://commongain.xyz) — Public commons and autonomous collective tools.
