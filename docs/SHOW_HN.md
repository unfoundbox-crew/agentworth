# Show HN: AgentWorth – Your AI coding agents left receipts (100% offline Rust tool)

## Submission Details

* **Title**: `Show HN: AgentWorth – Your AI coding agents left receipts (100% offline Rust tool)`
* **URL / Link**: `https://github.com/unfoundbox-crew/agentworth` (or `https://agentworth.dev`)
* **Author**: `unfoundbox-crew`

---

## Post Body

Hey HN,

Over the past year, many of us started running multiple AI coding agents locally—Claude Code, Cursor Composer, Google Antigravity (`agy`), OpenAI Codex, Goose, Aider, Windsurf, DeepSeek, and others.

Every one of these tools writes extensive, non-standard JSONL transcripts into dotfiles across `~/.claude/projects/`, `~/.cursor/`, `~/.gemini/antigravity/`, and `~/.config/`. Soon you have gigabytes of unreadable logs, but no easy way to answer simple questions:
- *How many millions of tokens did I actually burn today across all my agents?*
- *What is my rolling 5-hour pacing consumption vs prompt caching efficiency?*
- *Which agent session and prompt authored lines 45–90 of `src/main.rs`?*
- *Did the agent actually pass the test suite, or did it just claim "Done!" before quitting?*

We built **AgentWorth** (`agwt`) to solve this. It is an open-source, 100% offline native Rust engine that discovers, normalizes, and indexes your machine's AI coding history into a local SQLite database.

### ⚡ 1-Line Quickstart

You can scan and index your local histories with zero install:

```bash
npx -y agentworth scan
```

Or equip your agent with the AgentWorth skill:

```bash
npx skills add unfoundbox-crew/agentworth -g
```

You can also install via Homebrew, Cargo, or direct binary:
```bash
# Homebrew
brew install unfoundbox-crew/tap/agentworth

# Cargo
cargo install agentworth-cli

# Standalone binary
curl -fsSL https://agentworth.dev/install.sh | sh
```

---

### What You Get: The Flight Receipt

When you run `agentworth stats` or inspect your sessions, AgentWorth generates an instant ASCII flight receipt:

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

### Core Engineering Invariants

1. **Zero Telemetry & 100% Offline**: AgentWorth never phones home, uploads transcripts, or requires an internet connection.
2. **Never Duplicate Raw Logs**: Raw logs remain the source of truth on disk. SQLite only stores lightweight metadata, SHA-256 content fingerprints, derived features, and outcome indexes. Full trajectories are streamed lazily on demand.
3. **Typed Provenance**: Every metric is strictly typed:
   - `[flown]`: Measured directly from disk logs with cryptographic SHA-256 verification.
   - `[on paper]`: External cited reference (e.g. vendor token pricing tables).
   - `[unflown]`: Unverified or speculative claims. Blending `flown` and `on paper` without explicit distinction is treated as a type error.
4. **Outcome Evidence Ladder**: Agents frequently hallucinate success. We evaluate traces through an empirical hierarchy:
   `DoneClaimed < ArtifactChanged < TestOrBuildPassed < CommitObserved < CiOrDeploymentVerified`
5. **AI Code Lineage (`agentworth blame <FILE>`)**: Like `git blame`, but traces code modifications back to the agent session, model, timestamp, and user prompt that authored them.
6. **Safe ATIF v1.0 Export**: Includes a deterministic 13-rule offline privacy scrubber that strips API keys (`sk-*`, `ghp_*`), `.env` secrets, absolute home directory paths, and org URLs before serializing to standard Agent Trajectory Interchange Format.
7. **20 Native Streaming Adapters**: Claude Code, Cursor, Google Antigravity (`agy`), DeepSeek, Kimi, MiniMax, Qwen, Zhipu, Aider, Cline/Roo-Code, Windsurf, Manus, OpenAI Codex, Goose, Pi, Herdr, Hermes, OpenClaw, Grok, and OpenCode.

---

### Local Thermal Explorer UI

If you prefer a visual interface, `agentworth serve --open` boots a local monochrome thermal-receipt explorer UI on `http://localhost:3000`:
- Interactive prompt, thought, tool-call, and diff archaeology
- Rolling burn-rate charts & pacing monitors
- Redaction previews and one-click ATIF exports

---

### Ecosystem Fleet

AgentWorth is part of the Unfoundbox open-source collective:
- [STFU Opus (`stfuopus.lol`)](https://stfuopus.lol) — Claude Opus token burn & model pacing reality checker
- [WorldTrainer (`worldtrainer.xyz`)](https://worldtrainer.xyz) — Open-weights dataset & decentralized model training collective
- [CommonGain (`commongain.xyz`)](https://commongain.xyz) — Public commons & autonomous tooling collective

Code: https://github.com/unfoundbox-crew/agentworth  
Website: https://agentworth.dev

We'd love to hear your feedback on edge cases in messy agent logs, missing adapters you'd like added, or features you'd find useful for your local workflows!
