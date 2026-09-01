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

### 1. `agentworth scan`
Scans and indexes agent histories from local standard paths or custom directories.

```bash
agentworth scan [PATHS]... [OPTIONS]
```

**Options:**
- `[PATHS]...`: Optional specific file paths or directories to scan (e.g. `~/.claude/projects`). Defaults to every adapter's standard discovery paths when omitted.
- `-f, --force`: Force re-indexing of unchanged files bypassing mtime/hash cache.
- `--json`: Output raw structured JSON scan summary.

**Example:**
```bash
agentworth scan ~/.claude/projects --force
```

---

### 2. `agentworth blunder`
Discovers, scores, and extracts catastrophic agent blunders (e.g. unconstrained `rm -rf`, leaked shell variables like `$d`, multi-thousand-dollar token burns, and groveling remorse loops) with formatted thermal receipt cards.

```bash
agentworth blunder [OPTIONS]
```

**Options:**
- `-t, --top <N>`: Number of top blunder exhibits to display (default: `5`).
- `-s, --submit`: Anonymize, redact, and submit the top exhibit to the public Hall of Blunders (`stfuopus.lol`).
- `--json`: Output full structured blunder exhibits as JSON.

**Example:**
```bash
agentworth blunder --top 3 --json
```

---

### 3. `agentworth audit`
Performs safety and threat vector auditing across every indexed trajectory, catching dangerous commands, credential leaks, unconstrained sweeps, and false success claims.

```bash
agentworth audit [OPTIONS]
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
agentworth audit --safety
```

---

### 4. `agentworth export`
Safely sanitizes, redacts, and exports session trajectories into JSON, ATIF, or Flight Receipt format.

```bash
agentworth export <SESSION_ID> [OPTIONS]
```

**Options:**
- `<SESSION_ID>`: The session identifier to export (required).
- `-r, --redact`: Apply deterministic redaction masking secrets, API keys, tokens, emails, org names, and home paths.
- `-f, --format <json|atif|receipt|svg>`: Export format standard. Default: `json`. Use `atif` for the official Agent Trajectory Interchange Format, `receipt` for the ANSI Flight Receipt (`terminal`/`ansi` are accepted aliases), or `svg` for the shareable 1200x630 dark-mode receipt card.
- `-o, --output <PATH>`: Write export to a specific file instead of stdout.

**Example:**
```bash
agentworth export session_8f12ac --format atif --redact --output ./trace.atif.json
```

---

### 5. `agentworth receipt`
Renders a Flight Receipt for one session: an ANSI ASCII box for the terminal, or a standalone 1200x630 dark-mode SVG card for sharing. Surfaces Typed Provenance, the composite score and its five dimensions, token spend, the Apology Tax, and Autonomous Resilience in one canonical record.

```bash
agentworth receipt <SESSION_ID> [OPTIONS]
```

**Options:**
- `<SESSION_ID>`: The session identifier to render (required).
- `-f, --format <terminal|ansi|svg|receipt|json>`: Output format. Default: `terminal`. `terminal`, `ansi`, and `receipt` all render the same ANSI box; `svg` renders the shareable dark-mode card; `json` emits the structured receipt data instead of a rendered view.
- `-o, --output <PATH>`: Write the receipt to a file instead of stdout.

**Example:**
```bash
agentworth receipt session_8f12ac --format svg --output ./receipt.svg
```

---

### 6. Complementary Utility Commands

- `agentworth stats`: Machine-wide summary of indexed sessions, token usage, verified outcome rates, and top models.
- `agentworth traces [--limit N] [--adapter <name>] [--model <name>]`: List indexed sessions with outcome badges and composite scores.
- `agentworth search <QUERY> [--kind <category>] [--min-score <0.0-1.0>]`: Semantic vector search across trajectory turns.
- `agentworth usage [--period <day|week|month>] [--pacing] [--hours N]`: Detailed token burn rate and 5-hour pacing analysis.
- `agentworth blame <FILE_PATH>`: Reverse-traces file modifications back to the exact agent prompt and session that authored them.
- `agentworth doctor`: Validates local adapter source paths, SQLite schema integrity, and parser health.
- `agentworth serve [--port 3000] [--open]`: Launches the local forensic API server and Web UI.

---

## 6. Verifiable ASCII Output Receipts

AgentWorth prioritizes clear, verifiable terminal receipts. Never use raw unstructured logs when receipts are requested.

### Machine Experience Summary (`agentworth stats`)

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

### Hall of Blunders Exhibit Card (`agentworth blunder`)

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
