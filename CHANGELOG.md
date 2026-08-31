# Changelog

All notable changes to **AgentWorth** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.1.2] - 2026-08-31

### Added

- **Expanded Native Adapter Fleet from 11 to 20 Agents**:
  - 🐋 **DeepSeek Code (`deepseek`)**:
    - Discovers `~/.deepseek/`, `~/.deepseek-coder/`, and `.deepseek/` traces.
    - Full reasoning token stream accounting for DeepSeek R1 and V3 (`reasoning_content`, `thought`).
    - Tracks prompt cache hits (`prompt_cache_hit_tokens`) and cache creation (`prompt_cache_miss_tokens`).
    - Normalizes file editing (`str_replace_editor`, `write`, `edit`) and shell execution (`bash`).
  - 🌙 **Kimi Code (`kimi`)**:
    - Discovers Moonshot Kimi Code sessions in `~/.kimi-code/` and `~/.kimi/sessions/wire.jsonl`.
    - Parses streaming wire JSONL protocols, subagent delegations (`subagent_delegation`), and tool calls.
  - ⚡ **MiniMax (`minimax`)**:
    - Discovers `~/.minimax/` and `~/.minimax-agent/` coding plan trajectories.
    - Normalizes multi-step planning milestones, tool executions, and token expenditures.
  - 🐉 **Qwen Code / Qwen-Agent (`qwen`)**:
    - Discovers Alibaba Qwen Code and Qwen-Agent trajectories in `~/.qwen/` and `~/.qwen-agent/`.
    - Extracts reasoning CoT, `code_interpreter` executions, and tool calls.
  - 🧠 **Zhipu / CodeGeeX (`zhipu`)**:
    - Discovers `~/.codegeex/` and `~/.zhipu/` IDE extension and GLM-4 session histories.
  - 🛠️ **Aider (`aider`)**:
    - Discovers `.aider.chat.history.md` and `~/.aider/` git-driven trajectory markdown/JSON logs.
    - Maps git diff edits and commit messages directly into verified outcome evidence (`OutcomeKind::CommitObserved`).
  - 👁️ **Cline & Roo-Code (`cline`)**:
    - Discovers VSCode global storage task logs (`saoudrizwan.claude-dev/tasks/` and `rooveterinaryinc.roo-cline/tasks/`).
    - Parses task UI messages, API conversation histories, token cache metrics, and tool execution trees.
  - 🌊 **Windsurf / Cascade (`windsurf`)**:
    - Discovers `~/.codeium/windsurf/` and Cascade execution caches.
    - Normalizes multi-turn code edits, terminal outputs, and test validations.
  - 🦾 **Manus (`manus`)**:
    - Discovers `~/.manus/` autonomous agent browser actions and coding trajectories.

---

## [0.1.1] - 2026-08-30

### Added

- **`agwt usage` Command & Rollups**:
  - Deep usage analytics by timeframe with `--period day|week|month` and `--limit`.
  - Aggregates sessions, input tokens, output tokens, prompt cache reads, and estimated USD spend.
  - Real-time rolling pacing telemetry with `--pacing` (default 5-hour window via `--hours 5`):
    - Token burn velocity (tokens/hour).
    - Prompt cache hit ratio percentage.
    - Active agent adapters and models within the pacing window.
    - Estimated dollar expenditure tracking.
  - Machine-readable JSON output via `--json`.
- **`agwt blame <file_path>` (AI Code Lineage)**:
  - Trace code alterations and file edits back to the specific AI agent session, model, sequence timestamp, and user prompt that created or modified them.
  - Full support across all 11 supported agent adapter ecosystems.
- **SQL Analytics Views**:
  - Added native SQLite aggregation views: `v_daily_usage`, `v_weekly_usage`, and `v_monthly_usage` with pre-computed token math and cache metrics.
- **`agwt` CLI Alias & Binary Distribution**:
  - Added native `agwt` command-line alias alongside `agentworth`.

### Fixed

- **Date Range Epoch Anomaly**:
  - Fixed an issue where corrupt session timestamps or epoch zeroes (`1970-01-01`) distorted aggregate date bounds by enforcing `MIN(CASE WHEN started_at > '2020-01-01' THEN started_at END)` in SQLite aggregate queries.
- **SQLite Concurrency & WAL Performance**:
  - Configured optimized SQLite WAL mode, `busy_timeout = 5000ms`, `synchronous = NORMAL`, and `cache_size = -64000` (64MB) to prevent database locking during rapid parallel parsing.

### Improved

- **Incremental Rescan Performance**:
  - Optimized SHA-256 fingerprint checks to instantly skip unchanged multi-gigabyte session JSONL transcripts.
- **Unified Multi-Platform Installation**:
  - Standalone script: `curl -fsSL https://agentworth.dev/install.sh | sh`
  - Homebrew: `brew install unfoundbox/tap/agentworth`
  - Cargo: `cargo install agentworth-cli`
  - NPX: `npx agentworth` or `npx agwt`

---

## [0.1.0] - 2026-08-25

### Added

- **Core Agent History Normalization Pipeline**:
  - 100% offline, local-first discovery and streaming JSONL ingestion engine.
  - Unified `AgentWorthTrace` and `NormalizedEvent` canonical schema.
- **11 Native Streaming Agent Adapters**:
  - Claude Code (`claude_code`)
  - Cursor Composer (`cursor`)
  - Google Antigravity (`antigravity`)
  - OpenAI Codex (`codex`)
  - Block Goose (`goose`)
  - Pi (`pi`)
  - Herdr (`herdr`)
  - Nous Hermes (`hermes`)
  - OpenClaw (`openclaw`)
  - xAI Grok (`grok`)
  - OpenCode (`opencode`)
- **Outcome Evidence Ladder & Scoring Engine**:
  - Deterministic outcome verification (`DoneClaimed` < `ArtifactChanged` < `TestOrBuildPassed` < `CommitObserved` < `CiOrDeploymentVerified`).
  - Explainable 5-factor `TraceScore` rating.
- **CLI Commands**:
  - `agentworth scan` — Discovers and indexes local session logs.
  - `agentworth stats` — Machine-wide token expenditures and top model usage.
  - `agentworth traces` — Tabular session directory with filters.
  - `agentworth inspect` — Step-by-step ASCII trajectory timeline.
  - `agentworth doctor` — System health and adapter discovery diagnostics.
  - `agentworth export` — ATIF v1.0 and JSON export with 13-rule offline privacy scrubber.
  - `agentworth serve` — Local embedded Axum API server and monochrome receipt explorer UI.
