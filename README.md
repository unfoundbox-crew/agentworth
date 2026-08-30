# AgentWorth

[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md)

[![npm](https://img.shields.io/npm/v/agentworth?style=flat-square&color=000000)](https://www.npmjs.com/package/agentworth)
[![License](https://img.shields.io/badge/license-Apache--2.0-000000?style=flat-square)](LICENSE)
[![Privacy](https://img.shields.io/badge/telemetry-zero%20(100%25%20local)-000000?style=flat-square)](#privacy--local-first-guarantees)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-000000?style=flat-square)](#quickstart)
[![Website](https://img.shields.io/badge/website-agentworth.dev-000000?style=flat-square)](https://agentworth.dev)

**Your agents left receipts.**  
See what your AI coding agents have actually been doing on your machine.

AgentWorth is a local-first, native Rust tool that discovers, normalizes, and indexes the AI agent histories already sitting in your computer's dotfiles.

It turns gigabytes of unreadable JSONL into clean metrics, trajectories, and verified outcomes—showing you exactly how many tokens you spent, which tasks succeeded, where loops got stuck, and which agent edited each line of code.

```text
┌─────────────────────────────────────────────────────────────┐
│                      * * * RECEIPT * * *                    │
│ TOTAL TOKENS BURNT ............................. 8,420,000  │
│ ESTIMATED EXPENDITURE .......................... $124.50    │
│ INDEXED SESSIONS ............................... 695        │
│ DETECTED AGENTS ................................ 11         │
│ VERIFIED OUTCOMES .............................. 412 (59%)  │
│ PRIMARY ADAPTER ................................ Cursor     │
└─────────────────────────────────────────────────────────────┘
```

---

## Quickstart

| Method | Command | Description |
| :--- | :--- | :--- |
| **Standalone Script** | `curl -fsSL https://agentworth.dev/install.sh \| sh` | Installs the pre-built native binary directly to `~/.local/bin`. |
| **Homebrew** | `brew install unfoundbox-crew/tap/agentworth` | Installs via official Homebrew tap. |
| **Cargo (Native)** | `cargo install agentworth-cli` | Compiles and installs `agentworth` & `agwt` to `~/.cargo/bin`. |
| **NPX (Instant)** | `npx agentworth` | Zero-install runner that detects or downloads the native binary. |

```bash
# 1. Scan and index all local agent histories
agentworth scan

# 2. View machine-wide token and model stats
agentworth stats

# 3. Inspect daily token expenditure and 5-hour rolling burn rate
agentworth usage --period day
agentworth usage --pacing

# 4. Blame file edits back to the agent session that authored them
agentworth blame src/main.rs

# 5. Launch the local interactive receipt explorer UI
agentworth serve --open
```

> **Tip:** You can also use the shorthand alias `agwt` for all commands (e.g. `agwt usage`, `agwt blame`).

---

## What It Does

AgentWorth runs 100% offline on your machine. It discovers logs from tools like Claude Code, Cursor, Antigravity (`agy`), Codex, and Goose, extracting:

* **Token Expenditure & Burn Rate**: Input, output, and prompt cache hit ratios translated into USD costs, daily/weekly/monthly rollups, and rolling pacing windows.
* **AI Code Lineage (`blame`)**: Trace file modifications back to the exact agent session, model, timestamp, and user prompt that produced them.
* **Verified Outcomes**: Deterministic detection of whether the agent actually accomplished the goal or just claimed it was done.
* **Autonomous Recovery Loops**: Detecting when an agent encountered a compiler/runtime error, diagnosed it, and recovered.
* **Timeline Archaeology**: Step-by-step interactive inspection of prompts, thinking blocks, tool calls, and file diffs.
* **Safe ATIF Export**: 13-rule offline privacy scrubber (API keys, `.env` secrets, user paths) with export to standard Agent Trajectory Interchange Format (ATIF v1.0).

---

## Privacy & Local-First Guarantees

* **Zero telemetry, zero network uploads.** Nothing leaves your machine unless you explicitly export a redacted file.
* **Read-only by default.** AgentWorth never alters or writes to original transcript files.
* **No transcript duplication.** AgentWorth maintains a lightweight SQLite index of metadata, outcomes, and scores. Raw transcripts are loaded lazily on demand.
* **Streaming parsers.** Large multi-gigabyte JSONL files are streamed with bounded memory consumption and instant SHA-256 fingerprint deduplication.

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
| `agentworth export <ID>` | Exports a session in JSON or ATIF v1.0 format with optional privacy scrubbing (`--redact`). |
| `agentworth doctor [--json]` | Diagnoses system health, SQLite WAL status, and detected adapter roots. |

---

## Supported Agent Adapters

AgentWorth isolates proprietary log formats inside native streaming adapters. V0 includes 11 adapters:

| Agent / Framework | Adapter ID | Supported History Sources |
| :--- | :--- | :--- |
| **Claude Code** | `claude_code` | `~/.claude/projects/`, `~/.claude/sessions/` |
| **Google Antigravity** | `antigravity` | `~/.gemini/antigravity/`, `~/.gemini/history/`, `~/.antigravity/` |
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

## Outcome Evidence Hierarchy

AgentWorth does not trust self-reported success. Traces are evaluated through a strict evidence ladder:

```text
DoneClaimed               (Agent said "I have completed the task")
    ▲
ArtifactChanged           (Files modified / created on disk)
    ▲
TestOrBuildPassed         (test runner or compiler exited 0)
    ▲
CommitObserved            (git commit created)
    ▲
CiOrDeploymentVerified    (CI check passed / build artifact deployed)
```

Every score provides an explainable 5-factor breakdown:
1. **Outcome Score**: Strength of highest verified outcome.
2. **Verifiability**: Ratio of empirical evidence to self-claimed done statements.
3. **Complexity**: Trajectory depth, tool breadth, and files modified.
4. **Recovery Signal**: Bonus for recovering autonomously from tool or compiler failures.
5. **Provenance**: Completeness and cryptographic integrity of source logs.

---

## Architecture

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

### Repository Structure

```text
crates/
  schema/        Canonical data model (AgentWorthTrace, NormalizedEvent, TokenUsage)
  adapter-sdk/   Common traits and scan options for adapter implementations
  adapters/      11 streaming agent history parsers
  core/          Scanning orchestrator, incremental SHA-256 fingerprinting
  storage/       SQLite index, transactions, B-tree queries, and pagination
  outcomes/      Evidence hierarchy detection and failure-recovery loop extraction
  scoring/       Explainable 5-factor TraceScore engine
  redaction/     13-rule offline privacy engine
  export-atif/   Standard Agent Trajectory Interchange Format (ATIF v1.0) serializer

apps/
  cli/           Rust CLI binary (agentworth, agwt) and embedded Axum API server
  web/           React + Vite + Tailwind monochrome receipt explorer dashboard

packages/
  agentworth/    Official agentworth npm & npx package distribution
```

---

## License

Apache-2.0
