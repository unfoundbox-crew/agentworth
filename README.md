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
* **Timeline Archaeology**: Step-by-step interactive inspection of prompts, thinking blocks, tool calls, and file diffs. Large sessions stream into the dashboard instead of blocking: the inspector paints the first 500 events immediately and streams the rest in behind them.
* **Compaction Awareness**: A dedicated pane shows when and why a session's context got compacted, and the session list carries a per-session compaction count.
* **Safe ATIF v1.0 Export**: 16-rule offline privacy scrubber (API keys, `.env` secrets, user paths, repository identity) with export to standard Agent Trajectory Interchange Format (ATIF v1.0).

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

```text
crates/
  schema/        Canonical data model (AgentWorthTrace, NormalizedEvent, TokenUsage)
  adapter-sdk/   Common traits and scan options for adapter implementations
  adapters/      20 streaming agent history parsers
  core/          Scanning orchestrator, incremental SHA-256 fingerprinting
  storage/       SQLite index, transactions, B-tree queries, and pagination
  outcomes/      Evidence hierarchy detection and failure-recovery loop extraction
  scoring/       Explainable 5-factor TraceScore engine
  redaction/     15-rule offline privacy engine
  export-atif/   Standard Agent Trajectory Interchange Format (ATIF v1.0) serializer

apps/
  cli/           Rust CLI binary (agentworth, agwt) and embedded Axum API server
  dashboard/     The local app. Keyboard-first three-pane explorer, compiled
                 INTO the binary via rust-embed — build it before cargo or the
                 binary ships a stub instead of a UI.
  web/           Marketing site only. Deploys to Pages; makes no API calls.

packages/
  agentworth/    Official agentworth npm & npx package distribution
  ui/            Design tokens, theme toggle and icons shared by both apps
```

---

## Safe ATIF v1.0 Export

AgentWorth natively supports the **Agent Trajectory Interchange Format (ATIF v1.0)** for sharing anonymized, high-signal trajectories with benchmark suites, evaluators, and research harnesses.

Before exporting, every trace passes through a deterministic **16-rule offline redaction pipeline**:
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
| `agentworth matrix [--json]` | Extraction capability and coverage matrix across all 20 agent adapters. |
| `agentworth inspect <ID>` | Interactive ASCII trajectory timeline of prompts, thoughts, tool calls, and diffs. `<ID>` accepts any unique session-ID prefix, not just the full ID. |
| `agentworth serve [OPTIONS]` | Boots the local API server and monochrome receipt explorer UI (`--port 3000`, `--open`). |
| `agentworth export <ID>` | Exports a session as JSON, ATIF v1.0, or a Flight Receipt (`--format json\|atif\|receipt\|svg`), with optional privacy scrubbing (`--redact`). |
| `agentworth receipt <ID>` | Renders a Flight Receipt for a session: an ANSI box for the terminal or a shareable 1200x630 SVG card (`--format terminal\|svg\|json`, `--output <PATH>`). |
| `agentworth handoff [ID \| --last]` | What a session promised, decided, changed, ran, and proved — the same report the `session_handoff` MCP tool returns (`--markdown`, `--redact`, `--json`). |
| `agentworth loose-ends [ID \| --last]` | The handoff's loose-ends section alone: what a session said it would do and didn't (`--prompt` prints a copyable brief). |
| `agentworth doctor [--json]` | Diagnoses system health, SQLite WAL status, and detected adapter roots. |
| `agentworth mcp` | Starts the read-only MCP server over stdio, so a coding agent can query this machine's session index mid-session (see below). |

Every command accepts `--plain` (no colour, ASCII-only glyphs, same column positions as the colour output) and `--no-color`; setting `NO_COLOR` in the environment has the same effect as `--no-color`.

---

## MCP Server

`agentworth mcp` exposes the local session index to any MCP client (Claude Code, Codex, Cursor) as a stdio server, so a session can ask "what was I doing in this repo yesterday" or "which sessions touched `api.ts`" directly, without a human opening the dashboard first. Register it once:

```bash
claude mcp add agentworth --scope user -- agentworth mcp
```

`--scope user` matters here: the point is asking about *any* repo's history from *any* other repo, so a project-scoped entry would only be live in one checkout at a time.

Ten read-only tools: `sessions_find`, `session_get`, `blame_find`, `usage_summary`, `pacing_window`, `coverage_stats`, `outcome_rate`, plus the two handoff tools and `forgotten_context` below. Redacted output is the default everywhere event or file content is returned; `include_raw` is the only opt-in to raw content, and it's per-call, never global. No tool scans or writes anything -- run `agentworth scan` first if the index looks stale. Full design: `docs/specs/mcp-server.md`, `docs/specs/verified-outcome-rate.md`.

### The handoff, over MCP

| Tool | What it answers |
| :--- | :--- |
| `session_handoff(session_id?, max_lines?, include_loose_ends?, include_raw?)` | "What did this session actually do?" — what it said it would do and never did, what it said it decided, which files changed, which commands ran and how they ended, the outcome rung reached, and how often the context was compacted. Returns markdown under a line budget (default 60, ceiling 120), the receipt every claim traces back to, and `gaps`. Defaults to the newest session for the repo the server runs in. |
| `carry_forward(repo, n?, since?, max_lines?, include_raw?)` | "What happened in this repo recently?" — the last `n` handoffs (default 3, ceiling 10), newest first, so a session's *first* tool call can be the catch-up. A repo's worktrees all answer to one `repo` key. |

Two things these deliberately do not do. They never write a file — where a handoff lands is the caller's business. And they never summarise: every line is a fact from a row, quoted verbatim with a sequence number or a timestamp, because the moment a model writes the prose the receipt stops meaning anything.

What they cannot answer is stated in the output rather than filled in: open decisions, PR and CI state, and environment traps are not in the index. The machine owns the inventory; the judgment is still yours. Full design: `docs/specs/handoff.md`.

### What compaction dropped

When a session runs out of context, the harness replaces the conversation with a summary. The model's view is gone; the transcript on disk is not. So the dropped span and the summary that replaced it both exist, and can be diffed.

Measured on one real eight-round session: **402 decision-shaped sentences went into the compaction rounds and 28 came out.** Conclusions survive at about 15%, reasons at 1.7% — which is exactly the shape that makes a session confidently re-propose something it already tried and rejected.

| Tool | What it answers |
| :--- | :--- |
| `forgotten_context(session_id?, round?, classes?, limit?, include_raw?)` | "What did I decide and forget?" — decision-shaped sentences dropped by this session's own compaction rounds, quoted verbatim, newest first. Each carries its round, source sequence, and what the session did in the next few events, so a decision that was acted on reads differently from one that was only stated. `classes` is any of `decision`, `rejected`, `reason`; `limit` defaults to 20, ceiling 200. |

Three answers stay distinct and none is padded: never compacted, compacted with nothing decision-shaped dropped, and a real list. A session whose transcript has since been deleted gets a refusal, not a diff assembled from index rows.

**No model, on purpose.** Three regexes return the sentence verbatim with a sequence number. A model paraphrasing the dropped span would make this a second summariser — the exact lossy step the feature exists to undo — and the receipt would stop pointing at words anyone said. Full design: `docs/specs/compaction-diff.md`.

On the CLI, the same diff is `agentworth forgotten [SESSION_ID | prefix] [--round N] [--class CLASS] [--json]`, and a compacted session's handoff carries it as its first section.

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

## Working on AgentWorth

### Running the dashboard without rebuilding the CLI

```bash
cd apps/dashboard && npm run build
agentworth serve --port 3250 --dist apps/dashboard/dist --open
```

`--dist` points the installed binary at any local build, so UI work needs no
Rust compile. Everything is served from `127.0.0.1`; nothing leaves the machine.

### Cutting a release

There is no publish step to run by hand. Pushing a `v*` tag does everything —
builds four targets, creates the GitHub Release, publishes to npm, then smoke
tests `npx agentworth@<version>` on clean Ubuntu and macOS.

Four files carry the version and `version-gate` fails the release if the tag
disagrees with any of them:

| File | What |
| :--- | :--- |
| `Cargo.toml` | workspace version |
| `Cargo.lock` | the ten `agentworth-*` workspace crates |
| `packages/agentworth/package.json` | the npm package |
| `apps/web/src/version.ts` | the badge on the marketing site |

```bash
git checkout -b release/vX.Y.Z origin/main
# bump all four, then:
gh pr create --base main --title "chore(release): vX.Y.Z"
# merge once CI is green, then tag the merged commit:
git tag -a vX.Y.Z <merged-sha> -m "..." && git push origin vX.Y.Z
```

Tag the merge commit, not your local branch — and confirm the work is actually
on `main` first. A release has already been cut around a commit that changed
nothing because that check was skipped; see `[0.1.7]` in the changelog.

### npm publishing needs no token

Publishing uses npm **Trusted Publishing** over OIDC. There is no `NPM_TOKEN`
anywhere and adding one would be a step backwards — the granular bypass-2FA
tokens it replaced are deprecated by npm. The workflow requires
`id-token: write` and npm >= 11.5.1, both asserted in `release.yml`.

If a smoke test fails immediately after a successful publish with `ETARGET`,
the registry has not propagated yet. The workflow polls for resolvability
before concluding, so a red smoke test now means a real failure.

### GitHub accounts

This repo belongs to **unfoundbox-crew**, not the personal account. Both are
authenticated:

```bash
gh api user -q .login          # trust this
gh auth switch --user unfoundbox-crew
```

`gh auth status` can report one account as active while `gh api user` returns
the other, and a merge will then fail on permissions. Check with `gh api user`
before anything that writes. SSH is pinned per host in `~/.ssh/config`:
`github.com` is personal, `github.com-crew` is this repo.

### Release notes and the changelog

`release.yml` sets `generate_release_notes: true`, so GitHub writes the release
page from merged PR titles — which makes PR titles the release notes. Write
them for someone deciding whether to upgrade.

`CHANGELOG.md` is maintained by hand in Keep a Changelog format and is the place
for consequences rather than commit subjects. Add the entry in the release PR,
while you still remember what shipped.

### Public documentation

`apps/web` is the marketing site and deploys to GitHub Pages on every push to
`main` via `deploy-pages.yml`. It is a separate build from the dashboard and
must stay free of API calls — anything that fetches `/api/*` there ships a
request that 404s in production.

### CI

`ci.yml` runs on every pull request: builds both web apps, builds the Rust
workspace, and greps the binary to prove the dashboard is actually embedded in
it. ubuntu-latest only, to stay inside the free tier.

## License

Apache-2.0
