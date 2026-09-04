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
npx -y agentworth@latest scan
```

### Installation Methods

| Method | Command | Description |
| :--- | :--- | :--- |
| **Claude Code plugin** | `/plugin marketplace add unfoundbox-crew/agentworth` then `/plugin install agentworth@agentworth` | MCP server plus skill in one step; the binary comes down through `npx` on first use. |
| **Agent Skill** | `npx skills add unfoundbox-crew/agentworth -g` | Installs global Agent Skill for AI coding agents. |
| **NPX (Zero Install)** | `npx -y agentworth@latest scan` | Instant runner that executes the native binary on demand. |
| **Standalone Script** | `curl -fsSL https://agentworth.dev/install.sh \| sh` | Installs pre-built native binary to `~/.local/bin`. |

The standalone script draws its own progress, so a 22 MB download over a slow link no
longer looks like a hang:

```
 (*) archie  resolving    v0.1.18  aarch64-apple-darwin
 (o) archie  downloading  ─────────────────────·······   75%  16.7 / 22.2 MB
 (*) archie  verifying    sha256 matches
 (*) archie  extracting   agentworth-v0.1.18-aarch64-apple-darwin.tar.gz
 (*) archie  installed    agentworth, archie, agwt in ~/.local/bin

  Next  archie --version   confirm the install
```

Piped into a file or a CI log it prints one line per step and no bar — a loop that
scrolls is a loop that lies.

---

## Workflow

```bash
# 1. Scan and index all local agent histories across 20 adapters
archie scan

# 2. View machine-wide token burn, top models, and expenditures
archie stats

# 3. Inspect daily token burn, per-model spend, and 5-hour rolling pacing windows
archie stats usage --period day
archie stats usage --period month --by model
archie window show

# 4. Blame file edits back to the agent session and prompt that authored them
archie repo blame src/main.rs

# 5. Launch the local interactive receipt explorer dashboard
archie serve --open
```

> **Tip:** `archie` is the short name for every command (e.g. `archie stats usage`, `archie repo blame`, `archie stats`). The older `agwt` still works and is no longer documented.

---

## What It Does

AgentWorth is **local-first**: your data never leaves your machine. It discovers histories from Claude Code, Cursor, Antigravity (`agy`), Codex, Goose, Aider, Windsurf, DeepSeek, and 12 other agents, providing:

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
  cli/           Rust CLI binary (agentworth, archie, agwt) and embedded Axum API server
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
archie session export <SESSION_ID> --format atif --redact > trajectory_atif.json
```

---

## CLI Reference

| Command | Description |
| :--- | :--- |
| `archie scan [PATHS...]` | Discovers and incrementally indexes agent sessions into local SQLite. |
| `archie stats [--json]` | Displays machine-wide token totals, top models, top tools, and session counts. |
| `archie stats usage [--period ...]` | Usage rollups by `day`, `week`, `month`, `year`, or `all` (no period column); group with `--by adapter\|model\|repo` (`model` is usually the useful one -- most sessions share one adapter); filter with `--since <date\|1d\|2w\|3m>`; or rolling 5-hour pacing (`--pacing`). Cost is always an API list-price equivalent, labelled as such, not what a subscription plan actually billed. |
| `archie stats ladder [--period ...]` | The evidence ladder over a window, the API-equivalent spend that sits below the evidence line, what a verified outcome costs per model (or `--by repo\|adapter\|effort`), and the newest sessions that left evidence. Filter with `--repo`, `--adapter`, `--model`; `--min-n` sets the floor below which a rate and a cost render blank. |
| `archie repo blame <FILE>` | AI code lineage: finds all agent sessions and prompts that modified a given file. |
| `archie session list [OPTIONS]` | Tabular directory of indexed sessions (`--limit`, `--adapter`, `--model`, `--json`). |
| `archie agent list [--json]` | Extraction capability and coverage matrix across all 20 agent adapters. |
| `archie session show [ID]` | Interactive ASCII trajectory timeline of prompts, thoughts, tool calls, and diffs. `ID` accepts any unique session-ID prefix, not just the full ID. |
| `archie serve [OPTIONS]` | Boots the local API server and monochrome receipt explorer UI (`--port 3000`, `--open`). |
| `archie session export [ID]` | Exports a session as JSON, ATIF v1.0, or a Flight Receipt (`--format json\|atif\|receipt\|svg`), with optional privacy scrubbing (`--redact`). |
| `archie session receipt [ID]` | Renders a Flight Receipt for a session: an ANSI box for the terminal or a shareable 1200x630 SVG card (`--format terminal\|svg\|json`, `--output <PATH>`). |
| `archie session handoff [ID]` | What a session promised, decided, changed, ran, and proved — the same report the `session_handoff` MCP tool returns (`--markdown`, `--redact`, `--json`). |
| `archie session loose-ends [ID \| --last]` | The handoff's loose-ends section alone: what a session said it would do and didn't (`--prompt` prints a copyable brief). |
| `archie session asks [--session ID \| --last] [--since 2h] [--unanswered]` | The questions you asked and where their answers already are, so you never re-scroll or re-ask (`--session` also accepts a raw JSONL path; `--current` is an alias of `--last`; `--json` for the structured list). |
| `archie session forgotten [ID]` | What compaction dropped: decisions a session made that its own summaries didn't keep (`--round N`, `--class CLASS`, `--json`). |
| `archie repo blunder-blame [--session ID \| --file PATH]` | Bridges a recorded blunder forward to the files it touched, or a file's blame history back to any blunder in the sessions blamed for it (`--top N`, `--json`). |
| `archie repo suspect [OPTIONS]` | Lists commits on this branch whose authoring session never proved anything, so you know where to look twice before pushing (`--repo`, `--since`, `--json`). `--hook` prints a pre-push script that prints and never blocks. |
| `archie doctor [--json]` | Diagnoses system health, SQLite WAL status, and detected adapter roots. |
| `archie doctor --self-test` | Runs the real workflow end to end — scan, stats, usage, traces, inspect, handoff, forgotten, an MCP round trip — against the real index on this machine, with no network. Prints pass/fail/slow and timing per step; exits non-zero if any step fails. |
| `archie mcp` | Starts the read-only MCP server over stdio, so a coding agent can query this machine's session index mid-session (see below). |
| `archie` / `archie tui` | Opens the cockpit: the same grammar, full screen, with a cursor (see below). Off a terminal it prints the overview and exits 0. |

Every command accepts `--plain` (no colour, ASCII-only glyphs, same column positions as the colour output) and `--no-color`; setting `NO_COLOR` in the environment has the same effect as `--no-color`.

`inspect`, `export`, `receipt`, `handoff`, and `forgotten` all take the session ID the same way, and it's always optional. Every one of them also takes `--last` (the newest session for this directory's repository, falling back to the newest session anywhere) and `--current` (an alias of `--last`); `blunder-blame` and `asks` take the same two flags for `--session`. Leave the ID off entirely on a terminal and a picker lists the newest sessions to choose from — type a number, type text to filter by ID, repo, adapter, or prompt, `m` for more, `q` to quit. Off a terminal, or with `--json`, the same list prints as JSON or a plain table and the command exits 2 with `pass a session id or prefix` — nothing is guessed for a script.

### Where the spend went

`archie stats ladder` is the one screen that answers "of everything I spent, how
much of it bought something a test, a commit or a CI run can be pointed at."
Captured at 80 columns from the test fixture — one session on every rung, so the
whole ladder has something to say. This is the `--plain` rendering, which is what
a pipe or a redirect gets; on a terminal the meter draws as `● ○` and the rungs
above the line carry the accent:

```text
$ archie stats ladder --plain --period all --min-n 1

archie stats ladder                                      all time - 6 sessions
------------------------------------------------------------------------------

  cost: API-equivalent at list prices; your account is on
  default_claude_max_20x, so this is not what you paid

  EVIDENCE LADDER                  SESS  SHARE MED TOK   MED $   SPEND  %SPEND
  ----------------------------------------------------------------------------
  ##### CI or deployment verified     1  16.7%  652.0K   $0.45
  ####. commit observed               1  16.7%  652.0K   $0.45
  ###.. test or build passed          1  16.7%  652.2K   $0.12
  ---------------------------- the evidence line -----------------------------
  ##... artifact changed              1  16.7%  652.0K   $0.12   $0.12    8.7%
  #.... done claimed                  1  16.7%  652.0K   $0.12   $0.12    8.7%
  ..... unflown                       1  16.7%  652.0K   $0.12   $0.12    8.7%
  ----------------------------------------------------------------------------
  BELOW THE LINE                      3  50.0%                   $0.36   26.1%

  COST PER VERIFIED OUTCOME                                 by model - min n 1
  ----------------------------------------------------------------------------
  MODEL                                 N VERIFIED  MED TOK  STEPS  $/VERIFIED
  ----------------------------------------------------------------------------
  claude-3-5-haiku-20241022             3      33%   652.0K      1       $0.36
  claude-3-5-sonnet-20241022            2     100%   652.0K      1       $0.45
  claude-unknown                        1     100%      230      2      <$0.01
  ----------------------------------------------------------------------------

  RECENT VERIFIED                                                      3 shown
  ----------------------------------------------------------------------------
  WHEN        REPO                  MODEL            EVIDENCE  TOKENS     COST
  ----------------------------------------------------------------------------
  08-27 10:00 tmp/ladder-fixture    3-5-haiku           ###..  652.2K    $0.12
  08-26 10:00 tmp/ladder-fixture    3-5-sonnet          ####.  652.0K    $0.45
  08-25 10:00 tmp/ladder-fixture    3-5-sonnet          #####  652.0K    $0.45
  ----------------------------------------------------------------------------

  26.1% of spend this period sits below the evidence line.
  Next  archie session list --unproven   spend that bought nothing provable
```

(`--min-n 1` is only there because six sessions is a small fixture. The real
floor is 20 — `agentworth_storage::OUTCOME_RATE_DEFAULT_MIN_N`, the one floor in
the product: below it a group's rate and cost render **blank**, never a zero and
never a dash, and the row still carries its `n`.)

The bottom row is *unflown* — no outcome evidence of any kind was found. It is
not a failure. `OutcomeKind` has no failure value, so nothing on this screen is
ever coloured as one.

---

## MCP Server

`archie mcp` exposes the local session index to any MCP client (Claude Code, Codex, Cursor) as a stdio server, so a session can ask "what was I doing in this repo yesterday" or "which sessions touched `api.ts`" directly, without a human opening the dashboard first. In Claude Code the plugin registers it, with the skill, in one step:

```
/plugin marketplace add unfoundbox-crew/agentworth
/plugin install agentworth@agentworth
```

Anywhere else, or without the plugin, register it once:

```bash
claude mcp add agentworth --scope user -- archie mcp
```

`--scope user` matters here: the point is asking about *any* repo's history from *any* other repo, so a project-scoped entry would only be live in one checkout at a time.

13 read-only tools: `session_list`, `session_show`, `repo_blame`, `stats_usage`, `window_show`, `agent_list`, `stats_outcomes`, `stats_ladder`, plus the two handoff tools, `session_forgotten`, `session_asks`, and `repo_suspect` below. A client's `tools/list` shows 23: the 10 pre-0.1.16 names are still registered as deprecated aliases of these, forwarding to the same handlers, and are removed in v0.1.20. Redacted output is the default everywhere event or file content is returned; `include_raw` is the only opt-in to raw content, and it's per-call, never global. No tool scans or writes anything -- run `archie scan` first if the index looks stale. Full design: `docs/specs/mcp-server.md`, `docs/specs/verified-outcome-rate.md`.

### The handoff, over MCP

| Tool | What it answers |
| :--- | :--- |
| `session_handoff(session_id?, max_lines?, include_loose_ends?, include_raw?)` | "What did this session actually do?" — what it said it would do and never did, what it said it decided, which files changed, which commands ran and how they ended, the outcome rung reached, and how often the context was compacted. Returns markdown under a line budget (default 60, ceiling 120), the receipt every claim traces back to, and `gaps`. Defaults to the newest session for the repo the server runs in. |
| `session_carry_forward(repo, n?, since?, max_lines?, include_raw?)` | "What happened in this repo recently?" — the last `n` handoffs (default 3, ceiling 10), newest first, so a session's *first* tool call can be the catch-up. A repo's worktrees all answer to one `repo` key. |

Two things these deliberately do not do. They never write a file — where a handoff lands is the caller's business. And they never summarise: every line is a fact from a row, quoted verbatim with a sequence number or a timestamp, because the moment a model writes the prose the receipt stops meaning anything.

What they cannot answer is stated in the output rather than filled in: open decisions, PR and CI state, and environment traps are not in the index. The machine owns the inventory; the judgment is still yours. Full design: `docs/specs/handoff.md`.

### What compaction dropped

When a session runs out of context, the harness replaces the conversation with a summary. The model's view is gone; the transcript on disk is not. So the dropped span and the summary that replaced it both exist, and can be diffed.

Measured on one real eight-round session: **402 decision-shaped sentences went into the compaction rounds and 28 came out.** Conclusions survive at about 15%, reasons at 1.7% — which is exactly the shape that makes a session confidently re-propose something it already tried and rejected.

| Tool | What it answers |
| :--- | :--- |
| `session_forgotten(session_id?, round?, classes?, limit?, include_raw?)` | "What did I decide and forget?" — decision-shaped sentences dropped by this session's own compaction rounds, quoted verbatim, newest first. Each carries its round, source sequence, and what the session did in the next few events, so a decision that was acted on reads differently from one that was only stated. `classes` is any of `decision`, `rejected`, `reason`; `limit` defaults to 20, ceiling 200. |

Three answers stay distinct and none is padded: never compacted, compacted with nothing decision-shaped dropped, and a real list. A session whose transcript has since been deleted gets a refusal, not a diff assembled from index rows.

**No model, on purpose.** Three regexes return the sentence verbatim with a sequence number. A model paraphrasing the dropped span would make this a second summariser — the exact lossy step the feature exists to undo — and the receipt would stop pointing at words anyone said. Full design: `docs/specs/compaction-diff.md`.

On the CLI, the same diff is `archie session forgotten [SESSION_ID | prefix] [--round N] [--class CLASS] [--json]`, and a compacted session's handoff carries it as its first section.

### Where the answer already is

In a long session you ask a question, the answer lands several messages later
among tool notifications, and you re-ask it because scrolling costs time and
re-asking costs tokens. This index finds it instead: every `?` sentence you
asked, or every `⚑`/`🚩`-flagged line the assistant asked back, matched to the
first substantive assistant text that follows it.

| Tool | What it answers |
| :--- | :--- |
| `session_asks(session_id?, since?, unanswered_only?, limit?, include_raw?)` | "Where's the answer to that?" — every question in the session with a status (`answered`, `flagged_back_to_user`, `no_reply_yet`), an excerpt of the answer when one was found, and a pointer (event sequence and timestamp) to jump to. `limit` defaults to 50, ceiling 500. |

No model reads the transcript -- three deterministic patterns, the same `regex_v1` posture `session_forgotten` uses. Full design: `docs/specs/asks.md`.

On the CLI this is `archie session asks [--session ID | --current] [--since 2h] [--unanswered] [--json]`; `--session` also accepts a raw JSONL path for a session that isn't indexed.

### Which commits to look at twice

`repo_suspect(repo, branch?, base?, since?, window_hours?)` walks `git log` over a range, joins each commit's changed paths to the sessions that touched them, and reports which of those sessions never proved anything -- no test run, a claim verification contradicted, a loop the sentinel caught. It returns a list, session ids, and a copyable prompt. **Never a patch**: a trajectory can say the session was going badly, but only the diff says what the code does wrong.

Two counts in its answer are load-bearing and worth reading every time: `unattributed` (commits with no indexed session at all -- unknown, never clean) and `unanchored_blame_rows` (evidence that could not be placed in any repository). Measured on this repo's own main, anchoring the join to the repo root takes the flag rate from 28.8% to 2.3%, and every one of the first ten flags it removes is false. Full design and measurement: `docs/specs/suspect-commits.md`.

On the CLI this is `archie repo suspect [--repo PATH] [--since REF|DATE] [--json]`, and `archie repo suspect --hook` prints a pre-push script that prints and exits 0, always.

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
archie serve --port 3250 --dist apps/dashboard/dist --open
```

`--dist` points the installed binary at any local build, so UI work needs no
Rust compile. Everything is served from `127.0.0.1`; nothing leaves the machine.

### Cutting a release

There is no publish step to run by hand. Pushing a `v*` tag does everything —
builds four targets, creates the GitHub Release, publishes to npm, then smoke
tests `npx agentworth@<version>` on clean Ubuntu and macOS.

Five files carry the version and `version-gate` fails the release if the tag
disagrees with any of them:

| File | What |
| :--- | :--- |
| `Cargo.toml` | workspace version |
| `Cargo.lock` | the ten `agentworth-*` workspace crates |
| `packages/agentworth/package.json` | the npm package |
| `apps/web/src/version.ts` | the badge on the marketing site |
| `.claude-plugin/plugin.json` | the plugin, twice: `version` and the `agentworth@<version>` pin in the `npx` args |

```bash
git checkout -b release/vX.Y.Z origin/main
# bump all five, then:
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
