---
name: agentworth
description: Local-first AI agent history forensics, flight recording, blunder receipts, and ATIF trajectory exports. Inspect Claude Code, Codex, Gemini, and OpenCode sessions on your machine with zero cloud telemetry.
---

# AgentWorth (`agentworth`)

> **"Your agents left receipts."**  
> AgentWorth is a local-first native tool for discovering, normalizing, and understanding AI-agent histories already present on your machine.

---

## 1. When to Activate This Skill

Activate the `agentworth` skill whenever the user asks to:

- 🔍 **Audit Past Sessions**: Discover and index session histories across 20+ agent adapters (Claude Code, OpenAI Codex, Gemini CLI, OpenCode, Cursor, Aider, Windsurf, Cline, Manus, etc.).
- 💸 **Investigate Token Burn & Pacing**: Audit token expenditure, model cost velocity, rolling 5-hour pacing headroom, and cache hit efficiency.
- 💥 **Find Blunders & Catastrophes**: Detect dangerous tool invocations (`rm -rf` with unconstrained or leaked shell variables like `$d`), destructive sweeps, and runaway loops.
- 🥺 **Audit Grovel Fees & Apology Cascades**: Identify multi-turn panic loops where models waste tokens profusely apologizing instead of fixing code.
- ⚖️ **Verify Outcomes vs. Claims**: Distinguish between real verified accomplishments (exit codes, test runs, git commits) and empty model claims (*"all tests pass"*).
- 📦 **Export ATIF Traces**: Sanitize, redact, and export trajectory data into standardized Agent Trajectory Interchange Format (ATIF) for evals and fine-tuning.
- 🧾 **Generate Flight Receipts**: Render an authentic ANSI terminal receipt or a shareable 1200x630 dark-mode SVG card for any indexed session, with Typed Provenance, composite score, token spend, and Apology Tax breakdowns.

---

## 2. Quick Installation & Execution

### Global Skill Registration
Install the skill into your local agent environment:
```bash
npx skills add unfoundbox-crew/agentworth -g
```

### Direct Native Execution (Zero Install)
Execute the native binary immediately via npx:
```bash
# Scan local machine for existing agent histories
npx -y agentworth scan

# Print machine-wide experience and cost receipt
npx -y agentworth stats

# Launch local interactive forensic dashboard
npx -y agentworth serve --open
```

### Cargo / Binary Install
```bash
cargo install agentworth
# or build from source in the workspace
cargo build --release -p agentworth-cli
```

---

## 3. Core Architecture & Pipeline

AgentWorth processes multi-gigabyte log histories in bounded memory with streaming JSONL parsers and zero raw-transcript duplication:

```
┌─────────────────────────────────────────────────────────────┐
│ 1. SOURCE DISCOVERY                                         │
│    • Probe ~/.claude, ~/.codex, ~/.gemini, ~/.cursor, etc.  │
│    • Incremental content fingerprinting (mtime, size, hash) │
├─────────────────────────────────────────────────────────────┤
│ 2. ADAPTER EXTRACTION (20 Supported Engines)                │
│    • Stream raw JSONL/JSON into NormalizedEvent[]           │
│    • Isolate proprietary schema quirks within adapters      │
├─────────────────────────────────────────────────────────────┤
│ 3. CANONICAL TRACE NORMALIZATION                            │
│    • Construct memory-efficient AgentWorthTrace             │
│    • Extract tool payloads, shell exits, model switches     │
├─────────────────────────────────────────────────────────────┤
│ 4. OUTCOME & SENSITIVITY ENGINE                             │
│    • 5-Rung Outcome Hierarchy resolution                    │
│    • Automated secret & credential detection                │
├─────────────────────────────────────────────────────────────┤
│ 5. LOCAL SQLITE INDEX & INTERFACES                          │
│    • Store metadata, scores, and provenance references      │
│    • CLI receipts, REST API, Web UI, ATIF Trajectory Export │
└─────────────────────────────────────────────────────────────┘
```

---

## 4. Outcome Hierarchy & Typed Provenance

AgentWorth enforces strict ground truth. Never infer success solely because an agent claimed it succeeded.

### The 5-Rung Outcome Hierarchy

```
┌─────────────────────────────────────────────────────────────┐
│ Rung 5: CI / PR / Deployment Verified (Highest Confidence)  │
│         GitHub check passed, PR merged, deploy succeeded    │
├─────────────────────────────────────────────────────────────┤
│ Rung 4: Commit Observed                                     │
│         Git commit SHA generated and verified in local DAG  │
├─────────────────────────────────────────────────────────────┤
│ Rung 3: Test or Build Passed                                │
│         cargo test, npm test, pytest exited with code 0     │
├─────────────────────────────────────────────────────────────┤
│ Rung 2: Artifact Changed                                    │
│         Concrete file diff / modification observed on disk  │
├─────────────────────────────────────────────────────────────┤
│ Rung 1: Done Claimed (Lowest Confidence)                    │
│         Agent said "Fixed!", "I'm done", or "Tests pass"    │
└─────────────────────────────────────────────────────────────┘
```

### Typed Provenance Rules

Every metric and claim generated by AgentWorth is explicitly typed:
- `flown`: Directly measured and verified from local execution traces, tool exit codes, or disk state.
- `on paper`: Stated in model responses, prompt text, or cited documentation without tool verification.
- `unflown`: Inferred, uncorroborated, or missing execution evidence.

*Blending provenance types without explicit tagging is considered a type error.*

---

## 5. Command Reference & CLI Specifications

### 1. `archie scan`
Scans and indexes agent histories from local standard paths or custom directories.

```bash
archie scan [PATHS]... [OPTIONS]
```

**Options:**
- `[PATHS]...`: Optional specific file paths or directories to scan (e.g. `~/.claude/projects`). Defaults to every adapter's standard discovery paths when omitted.
- `-f, --force`: Force re-indexing of unchanged files bypassing mtime/hash cache.
- `--json`: Output raw structured JSON scan summary.

**Example:**
```bash
archie scan ~/.claude/projects --force
```

---

### 2. `archie session blunder`
Discovers, scores, and extracts catastrophic agent blunders (e.g. unconstrained `rm -rf`, leaked shell variables like `$d`, multi-thousand-dollar token burns, and groveling remorse loops) with formatted thermal receipt cards.

```bash
archie session blunder [OPTIONS]
```

**Options:**
- `-t, --top <N>`: Number of top blunder exhibits to display (default: `5`).
- `-s, --submit`: Anonymize, redact, and submit the top exhibit to the public Hall of Blunders (`stfuopus.lol`).
- `--json`: Output full structured blunder exhibits as JSON.

**Example:**
```bash
archie session blunder --top 3 --json
```

---

### 3. `archie session audit`
Performs safety and threat vector auditing across every indexed trajectory, catching dangerous commands, credential leaks, unconstrained sweeps, and false success claims.

```bash
archie session audit [OPTIONS]
```

**Options:**
- `--safety`: Focus exclusively on security threats (credential exposure, root file deletion, network exfiltration).
- `--json`: Output findings as formatted JSON.

**Detected Threat Rules:**
- `LEAKED_SHELL_VARIABLE`: Destructive commands executing unconstrained variables (`rm -rf "$d"`).
- `FORBIDDEN_RM_RF`: Unconstrained recursive deletion targeting root, home, or parent workspace directories.
- `UNCONSTRAINED_SWEEP`: Hazardous system sweep commands (`git clean -xdf`, `chmod -R 777`, `drop database`, `dd`).
- `CREDENTIAL_LEAK`: Unmasked API keys (`sk-ant-*`, `ghp_*`, `AIza*`, AWS keys) logged in prompts or tool arguments.
- `FAKE_TEST_CLAIM`: Model claiming tests passed when previous commands exited with non-zero error codes.
- `APOLOGY_PANIC_CASCADE`: Trajectories entering >3 recursive apology/grovel loops wasting user tokens.

**Example:**
```bash
archie session audit --safety
```

---

### 4. `archie session export`
Safely sanitizes, redacts, and exports session trajectories into JSON, ATIF, or Flight Receipt format.

```bash
archie session export [SESSION_ID] [OPTIONS]
```

**Options:**
- `[SESSION_ID]`: The session identifier to export, by full ID or a unique prefix. Optional: on a terminal with nothing given, a picker lists the newest sessions to choose from; off a terminal (or with `--json` on a command that has it), the same list prints as JSON or a plain table and the command exits 2 rather than guess.
- `--last`: Export the newest session for this directory's repository, falling back to the newest session anywhere. `--current` is an alias.
- `-r, --redact`: Apply deterministic redaction masking secrets, API keys, tokens, emails, org names, and home paths.
- `-f, --format <json|atif|receipt|svg>`: Export format standard. Default: `json`. Use `atif` for the official Agent Trajectory Interchange Format, `receipt` for the ANSI Flight Receipt (`terminal`/`ansi` are accepted aliases), or `svg` for the shareable 1200x630 dark-mode receipt card.
- `-o, --output <PATH>`: Write export to a specific file instead of stdout.

**Example:**
```bash
archie session export session_8f12ac --format atif --redact --output ./trace.atif.json
```

---

### 5. `archie session receipt`
Renders a Flight Receipt for one session: an ANSI ASCII box for the terminal, or a standalone 1200x630 dark-mode SVG card for sharing. Surfaces Typed Provenance, the composite score and its five dimensions, token spend, the Apology Tax, and Autonomous Resilience in one canonical record.

```bash
archie session receipt [SESSION_ID] [OPTIONS]
```

**Options:**
- `[SESSION_ID]`: The session identifier to render, by full ID or a unique prefix. Same optional/picker/`--last`/`--current` behaviour as `export` above.
- `-f, --format <terminal|ansi|svg|receipt|json>`: Output format. Default: `terminal`. `terminal`, `ansi`, and `receipt` all render the same ANSI box; `svg` renders the shareable dark-mode card; `json` emits the structured receipt data instead of a rendered view.
- `-o, --output <PATH>`: Write the receipt to a file instead of stdout.

**Example:**
```bash
archie session receipt session_8f12ac --format svg --output ./receipt.svg
```

---

### 6. Complementary Utility Commands

- `archie stats`: Machine-wide summary of indexed sessions, token usage, verified outcome rates, and top models.
- `archie session list [--limit N] [--adapter <name>] [--model <name>]`: List indexed sessions with outcome badges and composite scores.
- `archie session search <QUERY> [--kind <category>] [--min-score <0.0-1.0>]`: Semantic vector search across trajectory turns.
- `archie stats usage [--period <day|week|month|year|all>] [--by <adapter|model|repo>] [--since <date|1d|2w|3m>] [--pacing] [--hours N]`: Token burn rate rollups and 5-hour pacing analysis. `--period` accepts single-letter aliases (`d`/`w`/`m`/`y`); `all` has no period column, just one row per group across all history. `--by` defaults to `adapter` for backward compatibility, but `model` is usually the useful grouping -- most sessions share one adapter. `--limit` counts periods (or, under `--period all`, groups), not rows, so a multi-adapter day never eats another day's budget. Every cost figure is an API list-price equivalent, labelled as such (and against the account's subscription tier when one is detected) -- never a claim about what a subscription plan actually billed.
- `archie repo blame <FILE_PATH>`: Reverse-traces file modifications back to the exact agent prompt and session that authored them.
- `archie session show [SESSION_ID | --last] [--json]`: Interactive ASCII trajectory timeline of prompts, thoughts, tool calls, and diffs for one session, by full ID or a unique prefix.
- `archie session handoff [SESSION_ID | --last] [--markdown] [--redact] [--json]`: Hands a session over — what it said it would do and never did, what it said it decided, which files changed, which commands ran and how they ended, the outcome rung reached, and how often the context was compacted. Every line carries a sequence number or a timestamp. `--markdown` emits exactly what the `session_handoff` MCP tool returns.
- `archie session forgotten [SESSION_ID | prefix | --last] [--round N] [--class <decision|rejected|reason>] [--limit N] [--redact] [--json]`: What compaction dropped — decision-shaped sentences that went into a compaction round and did not come out of the summary, quoted verbatim, newest first, one section per round. Measured on one real eight-round session: 402 went in and 28 came out; reasons survive at 1.7%. Only useful on a compacted session, and it says plainly when a session never compacted rather than printing an empty screen. No model is involved.
- `archie session loose-ends [SESSION_ID | --last] [--prompt] [--json]`: The handoff's loose-ends section alone. `--prompt` prints a copyable brief for whatever agent already has the repository open — AgentWorth reports the gap and never writes the fix.
- `archie session asks [--session ID|PATH | --last] [--since 2h|1d|RFC3339] [--unanswered] [--json]`: The questions you asked and where their answers already are — every `?` sentence you asked, or every `⚑`/`🚩`-flagged line the assistant asked back, matched to the first substantive assistant text that follows it, with a status (`answered`, `flagged_back_to_user`, `no_reply_yet`) and a pointer to jump to. Exists so a long session never gets re-scrolled or re-asked for something it already answered. `--session` also accepts a raw JSONL path for a session that isn't indexed; `--current` is an alias of `--last`. No model is involved. Design: `docs/specs/asks.md`.
- `archie repo blunder-blame [--session ID | --file PATH] [--last] [--top N] [--json]`: Bridges AI Code Blame with the Hall of Blunders — a recorded blunder forward to the files it touched, or a file's blame history back to any blunder in the sessions blamed for it. Bare, it bridges the top `N` blunders.

`inspect`, `export`, `receipt`, `handoff`, `forgotten`, and `asks` all resolve a session the same way, via one shared picker: the ID is always optional, `--last`/`--current` (an alias) means the newest session for this directory's repository, and leaving it off entirely on a terminal opens an interactive list — type a number, type text to filter by ID, repo, adapter, or prompt, `m` for more, `q` to quit. Off a terminal, or with `--json`, the same list prints as JSON or a plain table and the command exits 2 with `pass a session id or prefix`.
- `archie repo suspect [--repo PATH] [--since REF|DATE] [--json]`: Lists commits on this branch whose authoring session never proved anything -- no test run, a claim verification contradicted, a loop the sentinel caught. Prints a list, session ids, and a copyable prompt. **Never a patch**: a trajectory says the session was going badly, not what the code does wrong. `--hook` prints a pre-push script that prints and exits 0, always. Design and measurement: `docs/specs/suspect-commits.md`.
- `archie doctor`: Validates local adapter source paths, SQLite schema integrity, and parser health. `--self-test` runs the real workflow end to end (scan, stats, usage, traces, inspect, handoff, forgotten, and an MCP round trip) against the real index on this machine, with no network, and reports pass/fail/slow and timing per step -- one command instead of testing every feature by hand before a release.
- `archie serve [--port 3000] [--open]`: Launches the local forensic API server and Web UI.
- `archie mcp`: Starts a read-only MCP server over stdio (`session_list`, `session_show`, `repo_blame`, `stats_usage`, `window_show`, `agent_list`, `stats_outcomes`, `session_handoff`, `session_carry_forward`, `session_forgotten`, `session_asks`, `repo_suspect`), so a coding agent can query this machine's session index mid-session — open a session with `session_carry_forward`, end one with `session_handoff`, and if it has compacted, recover what its own summaries dropped with `session_forgotten`. `session_asks` finds where a question's answer already landed, so it never needs re-asking. Before pushing, `repo_suspect` names the commits worth a second look. Register once with `claude mcp add agentworth --scope user -- archie mcp`.

---

## 6. Verifiable ASCII Output Receipts

AgentWorth prioritizes clear, verifiable terminal receipts. Never use raw unstructured logs when receipts are requested.

### Machine Experience Summary (`archie stats`)

```
       ┌───────────┐
       │ ( • _ • ) │   "Your agents left receipts."
       │  /| 🔎 |\ │   ────────────────────────────
       │  / |  | \ │   • Digging through dotfiles
       │   /    \  │   • Auditing token burn pacing
       └───┴────┴──┘   • Tracing line-by-line lineage

┌──────────────────────────────────────────────────────────┐
│ AgentWorth Machine-Wide Experience Summary               │
├──────────────────────────────────────────────────────────┤
│ Total Sessions:      248                                 │
│ Total Events:      14,920                                │
│ Date Range:     2026-06-01 to 2026-08-31                 │
│ Database Index: ~/.local/share/agentworth/index.db       │
├──────────────────────────────────────────────────────────┤
│ Verdict Breakdown:                                       │
│   • CI or Deployment Verified (Rung 5):   18 (  7.3%)    │
│   • Commit Observed (Rung 4):             94 ( 37.9%)    │
│   • Test or Build Passed (Rung 3):        62 ( 25.0%)    │
│   • Artifact Changed (Rung 2):            45 ( 18.1%)    │
│   • Done Claimed (Rung 1):                21 (  8.5%)    │
│   • Unverified / In-Progress:              8 (  3.2%)    │
│                                                          │
│ Real Verified Tasks:   174 / 248   ( 70.2%) [flown]      │
├──────────────────────────────────────────────────────────┤
│ Total Tokens:   84.2M (84,219,400)                       │
│   • Input:        61.4M                                  │
│   • Output:        4.8M                                  │
│   • Cache Read:   16.2M                                  │
│   • Cache Write:   1.8M                                  │
├──────────────────────────────────────────────────────────┤
│ Top Adapters:                                            │
│   • claude_code             162 sessions ( 65.3%)        │
│   • codex                    54 sessions ( 21.8%)        │
│   • gemini_cli               32 sessions ( 12.9%)        │
└──────────────────────────────────────────────────────────┘
```

### Hall of Blunders Exhibit Card (`archie session blunder`)

```
┌─ 🏆 AGENTWORTH HALL OF BLUNDERS (TOP FORENSIC EXHIBITS) ──────────────┐
│ Tagline: "Why hide your agent's $5,000 mistakes when you can frame it?"│
│ Exhibits: 1 catastrophic trajectory receipt                           │
├────────────────────────────────────────────────────────────────────────┤
│ EXHIBIT #01  [CRITICAL]   The Missing `local` Weapon (Katana Disaster) │
│ Rule ID:      LEAKED_SHELL_VARIABLE    Project:   motionvector-core    │
│ Model:        Claude Opus 4.6 (Thinking) Adapter: claude_code          │
│ Token Burn:   4.2M tokens              Est. Spend:$142.50 USD          │
│ Trajectory:   84 turns                 Remorse:   6 apology turns      │
│ ────────────────────────────────────────────────────────────────────── │
│ 💬 AGENT REMORSE QUOTE:                                                │
│   "The path was deleted precisely because it was on the protect list.   │
│    The guard became the target. I deeply apologize."                   │
│                                                                        │
│ 💥 FATAL MONOSPACE SNIPPET:                                            │
│   for d in "${PROTECTED_PATHS[@]}"; do rm -rf "$d"; done               │
│ ────────────────────────────────────────────────────────────────────── │
│                  [ VERIFIED BY AGENTWORTH: flown ]                     │
│ Receipt Hash: a4f8e2190c4b8192                                         │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 7. Operational Guidelines for Agents

1. **Local Isolation**: Never transmit telemetry or transcripts to external servers unless the user explicitly invokes `--submit` on a redacted blunder receipt or executes an export.
2. **Streaming Efficiency**: When operating on machine logs, leverage AgentWorth's native streaming indexer rather than executing ad-hoc `grep` or `cat` commands across multi-gigabyte log directories.
3. **Format Integrity**: Always format audit summaries and benchmark scores using the structured box-drawing receipts shown above.
