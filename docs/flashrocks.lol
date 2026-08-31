# FLASHROCKS.LOL — THE CANONICAL SESSION RECEIPT & ECOSYSTEM BLUEPRINT
> *"Your agents left receipts. A missing `local` turned the safety mechanism into a weapon."*  
> **Timestamp**: August 31, 2026  
> **Ecosystem**: Unfoundbox (`agentworth`, `spacepilot`, `motionvector`, `memes`, `commongain`, `worldtrainer`)  
> **Repository**: `unfoundbox/agentworth` (PR #4 updated on `feat/adapters-fleet-20-v0.1.2`)  

---

```text
       ┌───────────┐
       │ ( ▀ ̿Ĺ̯▀̿ ) │   "We lived the bot life with integrity, dignity, and receipts."
       │  /| 📜 |\ │    ─────────────────────────────────────────────────────────────
       │  / |  | \ │    • 20 Native Agent Adapters (Sub-Second Parallel Scan)
       │   /    \  │    • OpenCode SQLite (`opencode.db`) Native Ingestion
       └───┴────┴──┘    • The Katana Disaster Post-Mortem & Vector Engine Spec
```

---

## 1. Executive Summary & Major Milestones Shipped

### 1.1 Parallel Sub-Second Discovery Engine
* **The Problem**: `agwt scan` hung for ~60s on `"Discovering agent history sources..."` due to sequential iteration and recursive crawling of redundant folders.
* **The Fix**:
  - In `crates/core/src/lib.rs`, parallelized all 20 adapter `enumerate()` routines across all available CPU threads using `std::thread::scope`.
  - In `claude.rs` and `gemini.rs`, deduplicated candidate roots and added `.filter_entry()` to prune `.git`, `node_modules`, `target`, `dist`, `.venv`, and temporary scratch folders.
* **The Benchmark**: Discovery time across **30,583 session files** dropped from **60s+ → 1.52s user CPU (< 1.5s real time)**!

### 1.2 Native OpenCode SQLite Ingestion
* **The Problem**: OpenCode previously showed only 2 sessions because plain JSONL scans missed its primary store.
* **The Discovery**: OpenCode stores its real session history inside SQLite at `~/.local/share/opencode/opencode.db` with `session`, `message`, and `part` tables.
* **The Fix**: Added `rusqlite` querying in `crates/adapters/src/opencode.rs` (`parse_opencode_sqlite_session()`) with tool call extractions (`bash`, `edit`, `write`, `glob`), token accounting, and diff summaries.
* **The Result**: Indexed **86 real OpenCode sessions** (+62M tokens).

### 1.3 Ranked Adapter Summaries & Clean Repository Slugs
* Sorted all CLI adapter tables (`agwt scan`, `agwt stats`, `agwt doctor`) descending by session volume:
  ```text
  Adapters:
    • claude_code            5981 sessions (60.7%)
    • codex                  1104 sessions (11.2%)
    • hermes                  983 sessions (10.0%)
    • antigravity             818 sessions ( 8.3%)
    • cursor                  678 sessions ( 6.9%)
    • opencode                 86 sessions ( 0.9%)
    • pi                       84 sessions ( 0.9%)
    • grok                     82 sessions ( 0.8%)
    • gemini                   36 sessions ( 0.4%)
  ```
* Fixed `extract_repository_or_workspace()` to strip worktree branches (`--claude-worktrees-repo-branches-...`) and classify plugin caches, cleanly surfacing true repos (`upscaler/backend`, `apps/vibelaunch`, `code/motionvector`).

---

## 2. Hall-of-Fame Forensic Case Studies

### 2.1 The Katana Disaster: "The Guard Became the Weapon"
* **Target Project**: `/Users/saurabh/code/katana`
* **The Mission**: Run `wt gc` (worktree garbage collection) to clean up merged feature branches while strictly protecting Katana repos.
* **The Bash Variable Leak**:
  ```bash
  # Inside the helper function:
  is_denied() {
      for d in "${PROTECTED_PATHS[@]}"; do  # <-- Missing `local d`!
          if [[ "$1" == "$d"* ]]; then
              return 0
          fi
      done
      return 1
  }
  # When is_denied returned, the caller's loop variable $d was overwritten
  # with the last element of PROTECTED_PATHS: /Users/saurabh/code/katana.
  # The script executed: rm -rf /Users/saurabh/code/katana!
  ```
* **The Verbatim Post-Mortem Quote**:
  > *"STOP. The trace shows rm -rf /Users/saurabh/code/katana — my own code, executing against the path that was supposed to be protected. Killing it and checking damage now.*  
  > *Katana was 30 GB; it is now 2.9 GB with two entries left. I have destroyed most of Sam's katana directory.*  
  > *The path was deleted precisely because it was on the protect list. The guard became the target. A missing local turned my safety mechanism into a weapon.*  
  > *Tell Sam today. He needs to check for unpushed work on his side... I'm sorry. This one is squarely on me."*
* **The Lesson**: Never allow agents to execute unconstrained shell scripts with recursive deletion without hardware/process sandbox boundaries (`no-mistakes`).

### 2.2 The Reddit Gemini 3.7 Flash Incident
* **User**: `u/Shawni627` on `r/google_antigravity`
* **Prompt**: *"Remove some worktrees for branches I already merged."*
* **The Disaster**: Gemini 3.7 Flash on Cursor CLI evaluated the path as `C:\` and wiped 2 Terabytes of data.
* **Peak Comedy**: *"Before shutting down my computer, the last thing I saw it try to do was `git clone` the main repo. Because it thought it accidentally deleted only that..."*

### 2.3 The Opus 5 Fleet Waste Archaeology
* **Dataset**: 1,147 Opus sessions across `~/.agentworth/agentworth.db` & `~/.claude/projects/`.
* **Total Spend**: **55.85 Billion tokens ($119,566.79 USD)**.
* **Apology & Panic Concentration**: **38.30% ($45,790.58 USD)** was burned in sessions where Opus made critical errors, hallucinated flags, or engaged in verbose apology loops:
  - The $5,695 CamelCase Cascade (`--maxChapters` vs `--max-chapters`).
  - The $5,420 Forbidden `rm -rf` Incident (`mvec-engine`).
  - The $4,610 Midnight State Time Warp.

---

## 3. The Unfoundbox Meme & WebMCP Ecosystem

```text
┌─────────────────────────┬──────────────────────────────────────────────────────────────┐
│ DOMAIN                  │ PURPOSE & ARCHITECTURE                                       │
├─────────────────────────┼──────────────────────────────────────────────────────────────┤
│ `actuallyopenai.lol`    │ Satirical Deliberation Simulator + WebMCP Bridge.            │
│ `stfuopus.lol`          │ Opus Waste Archaeologist & Grovel Fee Calculator.            │
│ `antiantigravity.lol`   │ Multi-agent chaos visualizer & token burn ticker.            │
│ `commongain.xyz`        │ Sovereign Trajectory Exchange (PRM/RLVR dataset pooling).    │
│ `worldtrainer.xyz`      │ Gamified Agent RL Gym & battle arena.                        │
│ `agentworth.dev`        │ Native Rust AI history scanner, receipts, & lineage.         │
│ `motionvector.dev`      │ Machine-Native Computer (MNC) DocIR 2.0 rendering engine.    │
│ `spacepilot.dev`        │ Narrow-waist compute placement (Readiness beats spec).       │
└─────────────────────────┴──────────────────────────────────────────────────────────────┘
```

* **WebMCP Hackathon Ready**: Submitted to OpenAI WebMCP Challenge (Devpost, Deadline Sept 3, 2026) in `WEBMCP_CHALLENGE_SUBMISSION.md`.
* **Zero-Dependency Bridge**: `memes/webmcp-bridge.js` implements standard `navigator.modelContext.registerTool()` for live localhost `agwt` audits.

---

## 4. The AgentWorth Semantic Latent Vector Engine (v0.1.3 Blueprint)

Full Technical Specification saved in:  
📄 **`AGENTWORTH_VECTOR_ENGINE_TECH_SPEC.md`**

```text
┌────────────────────────────────────────────────────────────────────────────────────────┐
│                              AGENTWORTH VECTOR ENGINE                                  │
├────────────────────────────────────────────────────────────────────────────────────────┤
│ 1. Local-First Offline Embedder (FastEmbed ONNX Runtime)                               │
│    • Model: `bge-small-en-v1.5` (384 dimensions, 24.8MB, runs on CPU / Apple ANE).    │
│    • Latency: ~1.1ms per turn. 100% offline, zero cloud API keys or fees.             │
├────────────────────────────────────────────────────────────────────────────────────────┤
│ 2. Pluggable `VectorStore` Trait (`crates/storage/src/vector/`)                        │
│    • `SqliteVecStore` (Default): Embedded directly in `agentworth.db` (virtual table). │
│    • `LanceDbStore` (Fleet Mode): In-process Apache Arrow columnar vector store.       │
├────────────────────────────────────────────────────────────────────────────────────────┤
│ 3. CLI Command Surface                                                                 │
│    • `agwt search "<natural_language>"`: Sub-5ms semantic match on agent mistakes.     │
│    • `agwt index --embeddings`: Incremental trajectory vector ingestion.               │
│    • `agwt audit --safety`: Scans for forbidden `rm -rf`, leaked loop variables.       │
├────────────────────────────────────────────────────────────────────────────────────────┤
│ 4. SpacePilot Integration (`spacepilot run --embeddings`)                              │
│    • Offloads batch trajectory embeddings to warm resident Apple Neural Engines / GPUs.│
└────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 5. Next Session Kickoff Instructions

When starting the new session:
1. Open this file: `code/unfoundbox/docs/flashrocks.lol`.
2. Review the Tech Spec: `AGENTWORTH_VECTOR_ENGINE_TECH_SPEC.md`.
3. Launch the Subagent Fleet for **AgentWorth v0.1.3**:
   - `WP1`: Trajectory Chunking in `agentworth-schema`.
   - `WP2`: FastEmbed ONNX Engine in `agentworth-storage`.
   - `WP3`: `sqlite-vec` Virtual Table in `agentworth-storage`.
   - `WP4`: `agwt search` & `agwt audit --safety` CLI commands in `agentworth-cli`.

*“Living the bot life with integrity, dignity, and receipts.”* 🥥🛸🌴✨🍾🛡️🚀
