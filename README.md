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

AgentWorth isolates proprietary log formats inside native streaming adapters. AgentWorth ships **20 native adapters**:

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
