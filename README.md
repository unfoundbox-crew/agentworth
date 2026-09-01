# AgentWorth

[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md)

[![npm](https://img.shields.io/npm/v/agentworth?style=flat-square&color=000000)](https://www.npmjs.com/package/agentworth)
[![License](https://img.shields.io/badge/license-Apache--2.0-000000?style=flat-square)](LICENSE)
[![Privacy](https://img.shields.io/badge/telemetry-zero%20(100%25%20local)-000000?style=flat-square)](#100-offline-local-sqlite-architecture)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-000000?style=flat-square)](#quickstart)
[![Website](https://img.shields.io/badge/website-agentworth.dev-000000?style=flat-square)](https://agentworth.dev)

**Your agents left receipts.**  
See what your AI coding agents have actually been doing on your machine.

AgentWorth is a local-first, native Rust engine that discovers, normalizes, and indexes the AI agent histories already sitting in your computer's dotfiles.

It turns gigabytes of unreadable JSONL into clean metrics, trajectories, and verified outcomes—showing you exactly how many tokens you spent, which tasks succeeded, where loops got stuck, and which agent edited each line of code.

```text
┌─────────────────────────────────────────────────────────────┐
│                   * * * FLIGHT RECEIPT * * *                │
│                                                             │
│ PROVENANCE ................................. [flown] LOCAL  │
│ TOTAL TOKENS BURNT ............................. 8,420,000  │
│   ├─ INPUT TOKENS .............................. 6,100,000  │
│   ├─ OUTPUT TOKENS ............................. 1,820,000  │
│   └─ PROMPT CACHE READS .......................... 500,000  │
│ ESTIMATED EXPENDITURE .......................... $124.50    │
│ INDEXED SESSIONS ............................... 695        │
│ DETECTED ADAPTERS .............................. 20         │
│ HIGHEST VERIFIED OUTCOME ....................... Commit     │
│   └─ VERIFIED RATIO ............................ 412 (59%)  │
│ TOP MODEL ...................................... Claude 3.7 │
│                                                             │
│ [flown]: 100% measured on local disk (SHA-256 verified)     │
└─────────────────────────────────────────────────────────────┘
```

---

## ⚡ Instant Agent Skill Install

Equip your AI agent (Claude Code, Antigravity `agy`, Cursor, Goose, etc.) with AgentWorth in a single command:

```bash
npx skills add unfoundbox-crew/agentworth -g
```

---

## 🚀 1-Line CLI Quickstart

Run immediately with zero prior installation:

```bash
npx -y agentworth scan
```

### Installation Methods

| Method | Command | Description |
| :--- | :--- | :--- |
| **Agent Skill** | `npx skills add unfoundbox-crew/agentworth -g` | Installs global Agent Skill for AI coding agents. |
| **NPX (Zero Install)** | `npx -y agentworth scan` | Instant runner that executes the native binary on demand. |
| **Standalone Script** | `curl -fsSL https://agentworth.dev/install.sh \| sh` | Installs pre-built native binary to `~/.local/bin`. |
| **Homebrew** | `brew install unfoundbox-crew/tap/agentworth` | Installs via official Homebrew tap. |
| **Cargo (Native)** | `cargo install agentworth-cli` | Compiles native binaries (`agentworth` & `agwt`) into `~/.cargo/bin`. |

---

## Workflow

```bash
# 1. Scan and index all local agent histories across 20 adapters
agentworth scan

# 2. View machine-wide token burn, top models, and expenditures
agentworth stats

# 3. Inspect daily token burn and 5-hour rolling pacing windows
agentworth usage --period day
agentworth usage --pacing

# 4. Blame file edits back to the agent session and prompt that authored them
agentworth blame src/main.rs

# 5. Launch the local interactive receipt explorer dashboard
agentworth serve --open
```

> **Tip:** You can also use the shorthand alias `agwt` for all commands (e.g. `agwt usage`, `agwt blame`, `agwt stats`).

---

## What It Does

AgentWorth runs **100% offline** on your machine. It discovers histories from Claude Code, Cursor, Antigravity (`agy`), Codex, Goose, Aider, Windsurf, DeepSeek, and 12 other agents, providing:

* **Token Expenditure & Burn Rate**: Input, output, reasoning, and prompt cache hit ratios translated into USD costs, daily/weekly/monthly rollups, and rolling 5-hour pacing windows.
* **AI Code Lineage (`blame`)**: Trace file modifications back to the exact agent session, model, timestamp, and user prompt that produced them.
* **Verified Outcomes**: Deterministic detection of whether the agent actually accomplished the goal or just claimed it was done.
* **Autonomous Recovery Loops**: Detecting when an agent encountered a compiler/runtime error, diagnosed it, and recovered without human intervention.
* **Timeline Archaeology**: Step-by-step interactive inspection of prompts, thinking blocks, tool calls, and file diffs.
* **Safe ATIF v1.0 Export**: 13-rule offline privacy scrubber (API keys, `.env` secrets, user paths) with export to standard Agent Trajectory Interchange Format (ATIF v1.0).

---

## Typed Provenance

AgentWorth applies strict, typed provenance to every metric:

```text
┌─────────────────────────────────────────────────────────────┐
│                     TYPED PROVENANCE                        │
├────────────┬────────────────────────────────────────────────┤
│   [flown]  │ Measured directly on your local machine        │
│            │ (SHA-256 fingerprint verified from disk logs)  │
├────────────┼────────────────────────────────────────────────┤
│ [on paper] │ Cited external claims (vendor pricing sheets,  │
│            │ published model benchmarks, token specs)       │
├────────────┼────────────────────────────────────────────────┤
│  [unflown] │ Unverified or speculative claims               │
└────────────┴────────────────────────────────────────────────┘
```

> **Invariant:** Blending `flown` telemetry with `on paper` assumptions without explicit annotation is a type error.

---

## Outcome Evidence Hierarchy

AgentWorth does not trust self-reported success. Traces are evaluated through a strict evidence ladder:

```text
┌──────────────────────────────┐
│  CiOrDeploymentVerified      │  ▲ Highest confidence (CI check / deploy artifact)
├──────────────────────────────┤  │
│  CommitObserved              │  │ Git commit created
├──────────────────────────────┤  │
│  TestOrBuildPassed           │  │ Compiler / test runner exited with status 0
├──────────────────────────────┤  │
│  ArtifactChanged             │  │ Files modified or created on disk
├──────────────────────────────┤  │
│  DoneClaimed                 │  │ Lowest confidence (Agent said "I am done")
└──────────────────────────────┘
```

Every score provides an explainable 5-factor breakdown:
1. **Outcome Score**: Strength of highest verified outcome.
2. **Verifiability**: Ratio of empirical evidence to self-claimed "done" statements.
3. **Complexity**: Trajectory depth, tool breadth, and files modified.
4. **Recovery Signal**: Bonus for recovering autonomously from tool or compiler failures.
5. **Provenance**: Completeness and cryptographic integrity of source logs.

---

## 100% Offline Local SQLite Architecture

AgentWorth is designed around strict privacy, performance, and local-first invariants:

* **Zero Telemetry & Offline Execution**: Scanning, indexing, scoring, and UI serving function completely offline. Zero network calls or telemetry.
* **Raw Histories Remain Source of Truth**: Original agent transcripts are NEVER modified and NEVER duplicated into database storage. SQLite stores only metadata, SHA-256 fingerprints, derived features, and outcome indexes. Full trajectories are streamed lazily on demand.
* **Streaming Parsers & Bounded Memory**: Multi-gigabyte JSONL files are processed as bounded streams with incremental rescanning. Unchanged files are skipped based on `(path, size, mtime, SHA-256)`.
* **SQLite with WAL Mode**: High-performance concurrent reads and batched transactions with zero background daemon required.

```text
┌────────────────────────────────────────────────────────┐
│                   Source Discovery                     │
│       (~/.claude, ~/.cursor, ~/.gemini, ~/.codex)      │
└───────────────────────────┬────────────────────────────┘
                            │
┌───────────────────────────▼────────────────────────────┐
│               Streaming Rust Adapters                  │
│       (JSONL / JSON / Event stream deserializers)      │
└───────────────────────────┬────────────────────────────┘
                            │
┌───────────────────────────▼────────────────────────────┐
│                  NormalizedEvent[]                     │
│        (Unified user, tool, diff & model AST)          │
└───────────────────────────┬────────────────────────────┘
                            │
┌───────────────────────────▼────────────────────────────┐
│                  AgentWorthTrace                       │
│        (Canonical memory & trajectory model)           │
└───────────────────────────┬────────────────────────────┘
                            │
┌───────────────────────────▼────────────────────────────┐
│          Outcomes, Recovery & Scoring Engine           │
│     (Evidence ladder, compiler correlation, score)     │
└───────────────────────────┬────────────────────────────┘
                            │
┌───────────────────────────▼────────────────────────────┐
│                  SQLite Local Index                    │
│     (Fingerprints, scores, metadata, WAL storage)      │
└───────────────────────────┬────────────────────────────┘
                            │
        ┌───────────────────┴───────────────────┐
        ▼                                       ▼
┌────────────────────────┐             ┌─────────────────┐
│     CLI & REST API     │             │ Web UI Explorer │
│ (Inspect, stats, ATIF) │             │ (Thermal Paper) │
└────────────────────────┘             └─────────────────┘
```

---

## Safe ATIF v1.0 Export

AgentWorth natively supports the **Agent Trajectory Interchange Format (ATIF v1.0)** for sharing anonymized, high-signal trajectories with benchmark suites, evaluators, and research harnesses.

Before exporting, every trace passes through a deterministic **13-rule offline redaction pipeline**:
* API keys & bearer tokens (`sk-ant-*`, `sk-*`, `ghp_*`, AWS credentials)
* Environment variable assignments (`.env` secrets, password strings)
* Organization names and private domain URLs
* User home directory absolute paths (`/Users/username/...` → `~/...`)
* Git author emails and personal signatures

```bash
# Export a redacted session in standard ATIF v1.0 format
agentworth export <SESSION_ID> --format atif --redact > trajectory_atif.json
```

---

## CLI Reference

| Command | Description |
| :--- | :--- |
| `agentworth scan [PATHS...]` | Discovers and incrementally indexes agent sessions into local SQLite. |
| `agentworth stats [--json]` | Displays machine-wide token totals, top models, top tools, and session counts. |
| `agentworth usage [--period ...]` | Usage rollups by `day`, `week`, or `month`, or rolling 5-hour pacing (`--pacing`). |
| `agentworth blame <FILE>` | AI code lineage: finds all agent sessions and prompts that modified a given file. |
| `agentworth traces [OPTIONS]` | Tabular directory of indexed sessions (`--limit`, `--adapter`, `--model`, `--json`). |
| `agentworth inspect <ID>` | Interactive ASCII trajectory timeline of prompts, thoughts, tool calls, and diffs. |
| `agentworth serve [OPTIONS]` | Boots the local API server and monochrome receipt explorer UI (`--port 3000`, `--open`). |
| `agentworth export <ID>` | Exports a session as JSON, ATIF v1.0, or a Flight Receipt (`--format json\|atif\|receipt\|svg`), with optional privacy scrubbing (`--redact`). |
| `agentworth receipt <ID>` | Renders a Flight Receipt for a session: an ANSI box for the terminal or a shareable 1200x630 SVG card (`--format terminal\|svg\|json`, `--output <PATH>`). |
| `agentworth doctor [--json]` | Diagnoses system health, SQLite WAL status, and detected adapter roots. |

---

## Supported Agent Adapters

AgentWorth isolates proprietary log formats inside native streaming adapters. Includes **20 native adapters**:

| Agent / Framework | Adapter ID | Supported History Sources |
| :--- | :--- | :--- |
| **Claude Code** | `claude_code` | `~/.claude/projects/`, `~/.claude/sessions/` |
| **Google Antigravity** | `antigravity` | `~/.gemini/antigravity/`, `~/.gemini/history/`, `~/.antigravity/` |
| **DeepSeek Code** | `deepseek` | `~/.deepseek/`, `~/.deepseek-coder/` (R1 & V3 reasoning tokens) |
| **Kimi Code** | `kimi` | `~/.kimi-code/`, `~/.kimi/sessions/wire.jsonl` (Moonshot AI) |
| **MiniMax** | `minimax` | `~/.minimax/`, `~/.minimax-agent/` (Coding plan trajectories) |
| **Qwen Code / Qwen-Agent** | `qwen` | `~/.qwen/`, `~/.qwen-agent/` (Alibaba Qwen 2.5) |
| **Zhipu / CodeGeeX** | `zhipu` | `~/.codegeex/`, `~/.zhipu/` (GLM-4 & IDE extensions) |
| **Aider** | `aider` | `.aider.chat.history.md`, `~/.aider/` (Git-paired trajectories) |
| **Cline & Roo-Code** | `cline` | VS Code `globalStorage/saoudrizwan.claude-dev/tasks/`, `roo-cline/` |
| **Windsurf / Cascade** | `windsurf` | `~/.codeium/windsurf/`, `~/.windsurf/` (Cascade execution caches) |
| **Manus** | `manus` | `~/.manus/` (Autonomous browser & coding action trajectories) |
| **Cursor Composer** | `cursor` | `~/.cursor/`, `~/Library/Application Support/Cursor/User/workspaceStorage/` |
| **OpenAI Codex** | `codex` | `~/.codex/sessions/` |
| **Block Goose** | `goose` | `~/.config/goose/`, `~/.local/share/goose/sessions/` |
| **Pi** | `pi` | `~/.pi/`, `~/.pi/tasks/` |
| **Herdr** | `herdr` | `~/.config/herdr/` (Multi-agent orchestration DAGs) |
| **Nous Hermes** | `hermes` | `~/.hermes/sessions/` |
| **OpenClaw** | `openclaw` | `~/.openclaw/` |
| **xAI Grok** | `grok` | `~/.grok/sessions/` |
| **OpenCode** | `opencode` | `~/.opencode/`, `~/.local/share/opencode/` |

---

## The Fleet

AgentWorth is part of the Unfoundbox autonomous agent tooling collective:

* ⚡ [**STFU Opus** (`stfuopus.lol`)](https://stfuopus.lol) — Claude Opus token burn & model pacing reality checker.
* 🌐 [**WorldTrainer** (`worldtrainer.xyz`)](https://worldtrainer.xyz) — Open-weights dataset and decentralized model training collective.
* 🤝 [**CommonGain** (`commongain.xyz`)](https://commongain.xyz) — Public commons and autonomous collective tools.

---

## License

Apache-2.0
