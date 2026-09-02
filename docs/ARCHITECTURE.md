# AgentWorth Architecture

## 1. Status

This document is the canonical engineering reference for AgentWorth. It supersedes:

- `AgentWorth — Technical Design Spec v0.1.md` (external, not in this repo — circulated as a Google Doc export). That spec designs a TypeScript/Node/pnpm monorepo with Rust explicitly deferred ("Why TypeScript rather than Rust initially?"). **The repo is Rust**: a 9-crate Cargo workspace plus an Axum-based CLI/API binary. Node appears only as a thin distribution launcher (`packages/agentworth/`). Where the old spec and the code disagree, the code wins.
- `docs/HANDOFF.md` ("Fable 5 Handoff"), where it disagrees with the code. It does not — with one exception: it says AgentWorth ships "11 adapters." The code ships 20 (see §4). `CHANGELOG.md` (`[0.1.2] - 2026-08-31`) documents the expansion from 11 to 20; HANDOFF.md was written against the older count and never updated.

**Verified:** 31 August 2026, by reading the source in this repo at commit `cb62ec0` (branch `docs/architecture`, forked from `main`). Every claim below is tagged:

- **Verified** — read directly from source or ran a command myself.
- **Inferred** — follows from something verified.
- **Assumed** — neither; flagged explicitly, and there are very few of these.

**Re-verify:** every claim here has a command next to it, or is covered in §9. Nothing here should be trusted past the next `cargo metadata` / `grep` that contradicts it.

## 2. What AgentWorth is, technically

AgentWorth is a local-first Rust tool that discovers AI coding-agent history files already on a user's machine (Claude Code, Cursor, Codex, Aider, and 16 others), parses them with per-agent adapters into one canonical trace format, deterministically detects whether each session actually accomplished something (not just claimed to), scores the trace on five explainable axes, indexes the result in a local SQLite database, and lets the user inspect, search, and export it — never uploading anything unless the user runs an explicit export/submit command. *(Verified against `AGENTS.md`, `crates/core/src/lib.rs`, and the CLI surface in `apps/cli/src/main.rs`.)*

## 3. Repository layout

Verified with `cargo metadata --no-deps` (9 Rust crates + 1 binary crate, matching `Cargo.toml`'s `[workspace] members`) plus direct reads of each crate's `src/lib.rs` doc comment.

| Path | Responsibility |
|---|---|
| `crates/schema` | Canonical data types: `AgentWorthTrace`, `NormalizedEvent`, `EventPayload`, `Provenance`, `TokenUsage`, plus a `vector` submodule for semantic-search types (`ChunkKind`, `TrajectoryChunk`, `VectorSearchResult`). No I/O, no logic — pure serde structs/enums. |
| `crates/adapter-sdk` | The `AgentAdapter` trait every adapter implements, plus `ScanOptions`, `SessionSource`, `DetectionResult`, `ParseResult`, and the SHA-256 fast-fingerprint function used for incremental rescans. |
| `crates/adapters` | 20 per-agent source parsers (one file each) plus `mcp.rs`, a shared tool-name normalizer used *by* those adapters (not a standalone adapter — see §4 and §8). |
| `crates/core` | `Scanner`: runs adapter discovery in parallel threads, does incremental skip-if-unchanged scanning, calls outcome detection and scoring per session, and writes results to storage. Also lazily reloads a full trace on demand (`load_trace`) by re-parsing the original file. |
| `crates/storage` | SQLite index (`Storage`, two tables: `sources`, `sessions`), a local-embedding engine (`embedder.rs`, FastEmbed/ONNX with a deterministic offline fallback), a semantic chunk extractor (`chunker.rs`), and a separate SQLite-backed vector store (`vector/`) for chunk search. |
| `crates/outcomes` | The outcome-evidence-hierarchy detector (`OutcomeDetector`) and the failure→recovery loop detector (`RecoveryDetector`), including regex-based correlation of compiler/test output back to the file that was edited to fix it. |
| `crates/scoring` | `TraceScorer`: the explainable 5-factor `TraceScore` (see §5, §8). |
| `crates/redaction` | `Redactor`: regex-based PII/secret scrubbing, applied to a *copy* of a trace before export (see §6, §8). |
| `crates/export-atif` | Converts `AgentWorthTrace` into the ATIF (Agent Trajectory Interchange Format) JSON shape for `archie session export --format atif`. |
| `apps/cli` | The `agentworth` / `archie` / `agwt` binary (`apps/cli/Cargo.toml`, `[[bin]]` x3, same `src/main.rs`). 13 subcommands (`scan`, `stats`, `traces`, `matrix`, `inspect`, `export`, `search`, `audit`, `blunder`, `serve`, `usage`, `blame`, `doctor`) plus an embedded Axum REST API (`apps/cli/src/server/routes.rs`: `/stats`, `/traces`, `/traces/:id`, `/usage`, `/pacing`, `/blame`, `/matrix`, `/archaeology`, `/scan`, `/export/:id`) that backs `archie serve`. |
| `apps/web` | Marketing site only. React + Vite + Tailwind, deploys to agentworth.dev via the Vercel CLI. Makes **no** API calls — anything fetching `/api/*` here ships a request that 404s in production. |
| `apps/dashboard` | The local app the CLI serves. Keyboard-first three-pane explorer, compiled **into** the binary with `rust-embed`, so `npm run build` here must run before `cargo` or the binary ships a stub instead of a UI. |
| `packages/ui` | Shared between the two: the `--mv-*` design tokens, `ThemeToggle`, `useTheme`, and icons ported from the SpacePilot design system. |
| `packages/agentworth` | The npm launcher package. See §7. |

Note: `AGENTS.md`'s own "Repository shape" section still says `packages/npm-wrapper/`; the real directory is `packages/agentworth/`. Minor, but a reader following AGENTS.md literally will look in the wrong place.

## 4. The adapter fleet

**Verified count: 20 adapters**, registered in `crates/core/src/lib.rs::Scanner::new()` and re-exported from `crates/adapters/src/lib.rs`. `mcp.rs` is a 21st module but exports a function (`normalize_mcp_tool_name`), not an `AgentAdapter` impl — it is not a 21st adapter. `apps/cli/src/main.rs` itself documents the `matrix` subcommand as covering "all 20 agent adapters," matching this count.

Each adapter implements `AgentAdapter::name()` (the canonical machine identifier stored in `AgentWorthTrace.adapter`) and a `candidate_roots()` method giving the directories it scans. Paths below are the primary ones; most adapters also check a handful of `~/.config/<tool>` fallbacks (verified per-file, not reproduced in full here — see the source for the exhaustive list per adapter).

| `name()` | Agent | Primary path(s) verified in source |
|---|---|---|
| `claude_code` | Claude Code | `~/.claude/projects/`, `~/.claude/sessions/` |
| `antigravity` | Google Antigravity / Gemini CLI (one adapter, two identities unified under `"antigravity"`) | `~/.gemini/antigravity-cli/brain/`, `~/.gemini/antigravity-ide/brain/`, `~/.gemini/history/`, `~/.antigravity/sessions/` |
| `codex` | OpenAI Codex | `~/.codex/`, `~/.codex/sessions/` |
| `cursor` | Cursor (Composer/Chat) | `~/.cursor/`, plus macOS `~/Library/Application Support/Cursor/User/{workspaceStorage,globalStorage}` |
| `cline` | Cline & Roo-Code (VS Code) | macOS `~/Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/tasks/` and `.../rooveterinaryinc.roo-cline/tasks/` |
| `windsurf` | Windsurf / Codeium Cascade | `~/.codeium/windsurf/`, `~/.codeium/windsurf/cascade/`, macOS `~/Library/Application Support/Windsurf/` |
| `goose` | Block Goose | `~/.config/goose/sessions/`, `~/.local/share/goose/sessions/` |
| `opencode` | OpenCode | `~/.local/share/opencode/opencode.db` (SQLite, not JSONL — the one adapter that reads a database directly), `~/.opencode/sessions/` |
| `aider` | Aider | `.aider.chat.history.md`, `~/.aider/sessions/`, `~/.aider/chats/` |
| `grok` | xAI Grok | `~/.grok/sessions/`, `~/.xai/sessions/` |
| `herdr` | Herdr | `~/.config/herdr/sessions/`, `~/.herdr/sessions/` |
| `hermes` | Nous Hermes | `~/.hermes/sessions/`, `~/.hermes/turns/` |
| `openclaw` | OpenClaw | `~/.openclaw/sessions/`, `~/.openclaw/logs/` |
| `pi` | Pi | `~/.pi/tasks/`, `~/.pi/sessions/` |
| `manus` | Manus | `~/.manus/sessions/`, `~/.manus-agent/sessions/` |
| `deepseek` | DeepSeek (Coder/R1/V3) | `~/.deepseek/sessions/`, `~/.deepseek-coder/sessions/` |
| `kimi` | Moonshot Kimi / Kimi-Code | `~/.kimi-code/sessions/`, `~/.kimi/wire/` (wire-protocol JSONL) |
| `minimax` | MiniMax (abab / MiniMax Agent) | `~/.minimax/sessions/`, `~/.minimax-agent/sessions/` |
| `qwen` | Alibaba Qwen-Agent / Qwen Coder | `~/.qwen/sessions/`, `~/.qwen-agent/sessions/` |
| `zhipu` | CodeGeeX / Zhipu GLM-4 | `~/.codegeex/sessions/`, `~/.zhipu/sessions/` |

All 20 read JSONL (or JSONL-like line-delimited logs) except `opencode`, which reads a SQLite database file directly.

**Contradiction not limited to the old spec:** the marketing site (`apps/web/index.html`, `apps/web/public/llms.txt`, `apps/web/public/llms-full.txt`) also advertised "11 native adapters" by name. The FAQ JSON-LD was corrected to 20 in 0.1.6; the `llms*.txt` files may still be stale and are worth checking. All three files are stale against the same `crates/adapters/src/` the rest of this doc is written from.

## 5. Data model

The canonical types live in `crates/schema/src/`. **None of the spec's named types — `TraceFeatures`, `SensitivityReport`, `PermissionEnvelope`, `AgentWorthTrace.schemaVersion = "awt-0.1"` — exist anywhere in the codebase** (verified: `grep -rn "TraceFeatures\|SensitivityReport\|PermissionEnvelope\|awt-0.1" crates/ apps/` returns nothing). The real shape is simpler and organized differently:

```rust
// crates/schema/src/trace.rs
pub struct AgentWorthTrace {
    pub session_id: String,
    pub adapter: String,                 // e.g. "claude_code", "antigravity"
    pub provenance: Provenance,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub stats: TraceStats,               // aggregate rollup, always present
    pub events: Vec<NormalizedEvent>,    // full event list; omitted on serialize when empty
    pub metadata: serde_json::Value,     // adapter-specific extras
}

pub struct TraceStats {
    pub total_events: usize,
    pub user_messages_count: usize,
    pub assistant_messages_count: usize,
    pub tool_calls_count: usize,
    pub token_usage: TokenUsage,
    pub models_used: Vec<String>,
    pub tools_used: BTreeMap<String, usize>,
    pub duration_seconds: Option<f64>,
}
```

```rust
// crates/schema/src/event.rs
pub struct NormalizedEvent {
    pub id: String,           // UUID v4
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    pub payload: EventPayload,
    pub raw_ref: Option<String>,   // lazy pointer back to the raw source line/offset
}

pub enum EventPayload {          // serde tag = "type", content = "data"
    UserMessage { content: String },
    AssistantMessage { content: String, thinking: Option<String> },
    ModelInvocation { model: String, token_usage: TokenUsage, cost_usd: Option<f64>, latency_ms: Option<u64> },
    ModelSwitch(ModelSwitch),     // { from_model: Option<String>, to_model: String, reason: Option<String> }
    ToolCall(ToolCall),           // { id: Option<String>, name: String, arguments: Value }
    ToolResult(ToolResult),       // { call_id: Option<String>, name: Option<String>, output: Value, is_error: bool }
    ShellCommand(ShellCommand),   // { command: String, cwd: Option<String>, exit_code: Option<i32>, output: Option<String> }
    FileAction { path: String, action: FileActionType, diff: Option<String>, lines_changed: Option<u64> },
    OutcomeEvidence(OutcomeEvidence), // { kind: OutcomeKind, summary: String, confidence: f32 }
    Error { message: String, is_recovered: bool },
    HumanIntervention(HumanIntervention), // { action: String, details: Option<String> }
    Custom { kind: String, data: Value },
}

pub enum FileActionType { Read, Write, Edit, Delete }

pub enum OutcomeKind {           // ranked, weakest to strongest — this hierarchy is real and matches every doc
    DoneClaimed,
    ArtifactChanged,
    TestOrBuildPassed,
    CommitObserved,
    CiOrDeploymentVerified,
}
```

```rust
// crates/schema/src/provenance.rs
pub struct Provenance {
    pub source_path: String,
    pub adapter_name: String,
    pub file_size_bytes: u64,
    pub mtime_epoch_secs: i64,
    pub content_fingerprint: String,  // SHA-256 over path + size + mtime + first 4KB of content
}

// crates/schema/src/tokens.rs
pub struct TokenUsage {   // Copy, Add/AddAssign with saturating arithmetic
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
}
```

Derived types produced by other crates, all operating on `&AgentWorthTrace`:

- `agentworth_outcomes::RecoverySignal` — `failure_sequence`, `failure_summary`, `recovery_sequence`, `recovery_summary`, `steps_to_recover`, `duration_seconds`, `corrective_actions_count`, `correlated_files: Vec<String>`.
- `agentworth_scoring::TraceScore` — `outcome_score`, `verifiability_score`, `complexity_score`, `recovery_score`, `provenance_score`, `composite_score`, `explanations: Vec<String>` (one human-readable sentence per dimension). See §8 for what's real vs. aspirational here.
- `agentworth_redaction::RedactionReport` — per-category counts (`api_keys_count`, `env_vars_count`, `paths_count`, `emails_count`, `credentials_count`, `jwt_tokens_count`, `ip_addresses_count`, `private_keys_count`, `custom_count`) plus `breakdown_by_category: BTreeMap<String, usize>`.
- `agentworth_export_atif::AtifTrajectory` — a **separate, external-only** shape (`schema_version: "atif-v1.0"`, `agent`, `environment`, `steps: Vec<AtifStep>`, `tools`, `metrics`, `tokens`). Built by `AtifTrajectory::from_trace(&AgentWorthTrace)` at export time; never stored internally (AGENTS.md is explicit: "Do not make ATIF the internal database schema" — verified true, ATIF types live only in `crates/export-atif`).

A second, independent schema exists for local semantic search and is not mentioned in either the old spec or HANDOFF.md at all:

```rust
// crates/schema/src/vector.rs
pub enum ChunkKind { SessionSummary, ErrorRecovery, ToolInvocation, ApologyPanic, CodeLineage }

pub struct TrajectoryChunk {
    pub chunk_id: String, pub session_id: String, pub adapter: String,
    pub kind: ChunkKind, pub turn_index: usize, pub timestamp: String,
    pub text_content: String, pub metadata_json: String,
}
```

## 6. Storage

SQLite via `rusqlite` (bundled), opened with `PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;` (verified, `crates/storage/src/lib.rs::initialize_schema`).

Two tables:

- **`sources`** — `source_path` (PK), `adapter`, `file_size`, `mtime`, `fingerprint`, `scanned_at`. One row per discovered file; this is the incremental-rescan cache (`should_scan_source` compares size/mtime/fingerprint and skips a re-parse if all three match).
- **`sessions`** — `session_id` (PK, FK → `sources.source_path` `ON DELETE CASCADE`), `adapter`, timestamps, `duration_seconds`, every `TraceStats` field flattened into columns (`total_events`, `*_count`, four token columns, `total_tokens`), `models_used`/`tools_used`/`metadata` as JSON-serialized TEXT, `scanned_at`, `primary_outcome`, `composite_score`. A `PRAGMA table_info` + `ALTER TABLE ADD COLUMN` fallback exists for the last two columns, for databases created before they existed.

Eight indices on `sessions`/`sources`, plus three SQL views (`v_daily_usage`, `v_weekly_usage`, `v_monthly_usage`) that pre-aggregate token/event/duration rollups grouped by day/week/month and adapter — this is what backs `archie stats usage`.

**What is and isn't copied out of raw transcripts:** confirmed — no message content, tool arguments, tool output, or diffs are ever written to the `sessions` table. Only counts, token numbers, model/tool name lists, and the small `metadata` JSON blob are persisted. `Scanner::load_trace()` reconstructs a full trace with events by looking up the source path in SQLite and **re-running the adapter's `parse()` on the original file** — the raw history file itself is the only copy of full event content, exactly as `AGENTS.md` requires ("Raw histories remain the source of truth").

One partial exception, not called out in any existing doc: the optional vector-search feature (`archie session search`) maintains a **second, separate SQLite store** (`SqliteVectorStore`, its own `sessions` shadow table plus a `trajectory_chunks` table: `chunk_id` PK, `session_id`, `adapter`, `kind`, `turn_index`, `timestamp`, `text_content`, `metadata_json`, `embedding BLOB`). `text_content` here **does** persist a short extracted excerpt of the transcript (session summaries, error/recovery snippets, destructive-command snippets, "apology/panic" turns, code diffs) — not the full transcript, but not nothing either. Similarity search is plain cosine similarity computed in Rust over the stored `f32` BLOBs (`crates/storage/src/vector/mod.rs::cosine_similarity`), not a SQLite vector extension (no `sqlite-vec`, no `CREATE VIRTUAL TABLE`). Embeddings come from a local ONNX model (BGE-Small-EN-v1.5, falling back to MiniLM, falling back to a fully offline deterministic embedder — `crates/storage/src/embedder.rs`) — no network call.

## 7. Distribution and release

**npm package is a thin JS launcher**, verified in full: `packages/agentworth/package.json` declares `bin: { agentworth, archie, agwt }` all pointing at `bin/agentworth.js`, which does nothing but `import { run } from '../lib/resolver.js'` and call it. All logic lives in `lib/resolver.js` (544 lines, read in full).

`resolveBinary()` search order, exactly as implemented (`lib/resolver.js`, comment block above the function plus the code itself):

1. `AGENTWORTH_BIN` environment variable, if set and executable.
2. A vendored platform package under `../vendor/<platform>-<arch>/agentworth` next to the launcher (for future `optionalDependencies`-style platform packages — not currently published as separate packages, just a resolution slot).
3. **System `PATH`** — "highest user priority for installed binaries." Skipped entirely when `AGENTWORTH_LAUNCHER_ACTIVE` is already set in the environment (i.e. this launcher is itself a child process — see the self-recursion fix below).
4. A `target/release/agentworth` or `target/debug/agentworth` found by walking up from the current working directory (`findCargoTargetBinary(cwd, ...)`).
5. The same cargo-target walk, but rooted at the launcher's own package directory instead of `cwd`.
6. `$CARGO_TARGET_DIR/release/` then `$CARGO_TARGET_DIR/debug/`, if that env var is set.
7. `~/.cargo/bin/agentworth`.
8. The on-demand download cache, `~/.agentworth/bin/v<version>/agentworth`.

If none of the 8 steps find a binary, `run()` shells out to a one-shot Node subprocess that calls `downloadAndExtractBinary()`: builds a GitHub Releases URL (`https://github.com/unfoundbox-crew/agentworth/releases/download/v<version>/agentworth-v<version>-<target-triple>.tar.gz`), downloads it (following up to 5 redirects), extracts with `tar --force-local -xzf`, `chmod 0o755`s it, and caches it at `~/.agentworth/bin/v<version>/`. Supported target triples (verified in `getTargetTriple()`): `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`. **Windows is not a supported platform** (dropped 2026-09-02, see §7) — `getTargetTriple('win32', ...)` returns `null`, and `downloadAndExtractBinary` throws a clear "Unsupported platform/architecture" error rather than attempting a download that would 404.

**Publishing is trusted publishing via OIDC — verified, no `NPM_TOKEN` anywhere in the workflow.** `.github/workflows/release.yml`'s `publish-npm` job requests `permissions: id-token: write`, pins `npm >= 11.5.1` (checked explicitly with a version-compare script, since that's the minimum for OIDC trusted publishing), and runs a bare `npm publish --access public` with no token — the job's own comment explains npm's OIDC exchange replaces the token, and that npm restricted bypass-2FA granular tokens on 2026-07-31 with a plan to stop accepting them for publish in January 2027. This requires a Trusted Publisher configured on npmjs.com for `unfoundbox-crew/agentworth` → workflow `release.yml` (asserted by the workflow's own error-handling branch, which detects "trusted publisher / oidc / not configured" in the publish log and prints setup instructions — I did not and cannot verify the npmjs.com-side configuration itself from this repo).

**Release pipeline** (`.github/workflows/release.yml`, triggered on `v*` tags), five jobs in sequence:

1. **`version-gate`** — fails the whole run unless the git tag, `Cargo.toml`'s `version`, and `packages/agentworth/package.json`'s `version` all match exactly. The workflow's own comment explains why: v0.1.3 was tagged once from a tree where `Cargo.toml` still said 0.1.2, producing a binary that reported the wrong version and could 404 against the npm launcher's download URL.
2. **`build-binaries`** — 4 targets: `aarch64-apple-darwin`, `x86_64-apple-darwin` (both `macos-latest`), `x86_64-unknown-linux-gnu` (`ubuntu-latest`), `aarch64-unknown-linux-gnu` (`ubuntu-24.04-arm`). Strips symbols, tars, and SHA-256-checksums each artifact. **No Windows target is built, and the launcher no longer resolves one either** (both changed together 2026-09-02) — a `windows-latest`/`x86_64-pc-windows-msvc` leg was briefly added and shipped in v0.1.10, but GNU tar on Windows kept misparsing the download cache's `C:\...` archive path as a remote `host:path` tar spec (a drive letter followed by a colon), breaking `npx agentworth` extraction for real Windows users across the runs that tried it. Dropped rather than chased further.
3. **`create-release`** — publishes a GitHub Release with the 4 archives + checksums attached, via `softprops/action-gh-release`.
4. **`publish-npm`** — OIDC trusted publish, as above.
5. **`smoke-test`** — runs on `ubuntu-latest` and `macos-latest` (not Windows, consistent with step 2). Waits for the new version to appear on the npm registry, then runs `npx -y agentworth@<version> --version` and `npx -y agentworth@<version> usage --pacing` in a genuinely clean room (no repo checkout, no cargo target dir, no `~/.agentworth` cache) — explicitly "the case that shipped broken and that nothing was checking."

**The self-recursion bug, fixed in 0.1.5** (per `resolver.js`'s own inline comment, dated to the bug's discovery): `npx`/`npm` put `node_modules/.bin` on `PATH`, and the `agentworth` entry there is a **symlink** back to `bin/agentworth.js`. `path.resolve()` does not follow symlinks, so a naive self-check comparing resolved paths missed the collision — the launcher's step-3 PATH search found itself, spawned itself, which found itself on PATH again, and so on until macOS refused to `fork()` (`EAGAIN`, in v0.1.4 per the comment). Fixed three ways, all present and verified in `resolver.js`:
- `isSelf(candidate, selfPath)` — compares `fs.realpathSync()` of both paths (falls back to `path.resolve()` comparison only if `realpathSync` throws), so the symlink is correctly recognized as pointing at the launcher itself.
- `looksLikeNodeScript(filePath)` — rejects any PATH candidate that is a `.js`/`.cjs`/`.mjs` file or whose first 64 bytes start with a `#!` shebang containing `node`, so even a copy (not just a symlink) of the launcher is rejected.
- `AGENTWORTH_LAUNCHER_ACTIVE` env sentinel — set on the child process before `spawnSync`, and checked at the top of the PATH-search step, so a child that somehow still re-enters the launcher skips PATH entirely rather than looping.

## 8. Honest gaps

Several things this repo's own docs (`AGENTS.md`, `docs/HANDOFF.md`) describe as open or planned turn out, on reading the code, to already be closed — and one thing assumed to be closed (per the task brief that produced this document) is actually open. Stated precisely:

- **TraceScore's components are fully defined — all 5 of them.** `AGENTS.md` lists 7 aspirational scoring properties: outcome quality, verifiability, trajectory richness, complexity, recovery signal, provenance, rarity. The implemented `ScoringWeights` struct (`crates/scoring/src/scorer.rs`) has exactly **5** fields — outcome, verifiability, complexity, recovery, provenance (default weights 0.30/0.25/0.20/0.15/0.10) — and every one of the 5 has a complete, concrete formula in `TraceScorer` (`compute_outcome_score`, `compute_verifiability_score`, `compute_complexity_score`, `compute_recovery_score`, `compute_provenance_score`), each returning a score and a human-readable explanation string. **The actual gap is narrower than "sub-formulas don't exist": two of AGENTS.md's seven named properties — `trajectory richness` as a distinct axis, and `rarity` — have no implementation anywhere in the scoring crate at all.** "Richness" is partially folded into the complexity formula (event count, tool breadth, token scale, file-action count); rarity has no code path whatsoever — no cross-session comparison, no percentile, nothing.
- **The privacy scrubber's detection mechanism is fully implemented, not just a category list.** `crates/redaction/src/rules.rs::default_rules()` returns 15 concrete `Regex`-backed rules (PEM private keys, JWTs, Anthropic/OpenAI/Google API keys, GitHub tokens, AWS access keys, bearer tokens, sensitive env-var assignments, credentialed URLs, three home-directory path patterns for macOS/Linux/Windows, email addresses, private IPv4 ranges), each with a category tag and a literal replacement string. The file's own numbered comments group the 3 home-directory-path rules under one heading ("11. User Home Directories"), which is why README.md's "13-rule" count and the actual 15-entry `Vec<RedactionRule>` both correctly describe the same thing from two counting conventions. There is no undefined "detection mechanism" here.
- **ATIF is expanded in code and in `README.md` — "Agent Trajectory Interchange Format"** (`crates/export-atif/src/lib.rs`, `models.rs`, and `README.md` line 72: "standard Agent Trajectory Interchange Format (ATIF v1.0)"). It is not, however, expanded in `docs/HANDOFF.md` or `AGENTS.md`, which both use the bare acronym. Separately and more interesting: the old spec describes an *external*, independently evolving "ATIF v1.7" standard; this repo's exporter emits `schema_version: "atif-v1.0"`, self-defined in `crates/export-atif/src/serializer.rs`. I found no code or doc in this repo connecting the two — no reference to an upstream ATIF spec, version negotiation, or compatibility claim. Whether an external "ATIF v1.7" standard actually exists is not something I verified (or could verify from this repo); treat the old spec's claim about it as unconfirmed, not as false.
- **MCP tool-call name normalization is real and already used by 14 of the 20 adapters** — `crates/adapters/src/mcp.rs::normalize_mcp_tool_name()`, called from `claude.rs`, `cline.rs`, `aider.rs`, `goose.rs`, `gemini.rs`, `deepseek.rs`, `kimi.rs`, `minimax.rs`, `manus.rs`, `opencode.rs`, `qwen.rs`, `windsurf.rs`, `zhipu.rs`. It rewrites tool names like `mcp__postgres__query`, `call_mcp_tool` (with `ServerName`/`ToolName` args), `developer__github__create_issue`, etc. into one canonical `mcp:<server>:<tool>` form on the `ToolCall.name` field. This is real, but it is a much smaller thing than the old spec's §24 "Future MCP collector" (an observability proxy sitting between an agent and its real MCP server, capturing every call/arg/timing/result live). That proxy does not exist. **Note on premise:** I could not find any mention of MCP anywhere in `docs/HANDOFF.md` — the specific claim attributed to "the handoff" isn't in the file that exists in this repo today. The underlying technical question is answered above regardless.
- **Windows is not supported, on purpose, as of 2026-09-02** (see §7) — not the stale-doc-vs-code mismatch this section otherwise catalogs. A `windows-latest` build leg shipped once (v0.1.10) and broke `npx agentworth` on real Windows machines (GNU tar misparsing a `C:\...` archive path as a remote tar spec); dropped from both the release matrix and the launcher's target-triple resolution rather than chased further. `getTargetTriple('win32', ...)` now returns `null` and the launcher fails with a clear "Unsupported platform/architecture" error instead of a 404 or a cryptic tar crash.
- **`CHANGELOG.md` stops at `[0.1.2] - 2026-08-31`.** `Cargo.toml` and `packages/agentworth/package.json` are both at `0.1.5`. Three releases (0.1.3, 0.1.4, 0.1.5 — including the self-recursion fix described in §7) are undocumented there.
- **No `DemandMatcher`/TokenBid interface exists in code.** This matches `AGENTS.md`'s explicit priority order (marketplace/bounty work is last, "the immediate product is not a marketplace") — not a bug, just confirming the deferral is real and not partially started.
- **"Sensitivity/redaction state" is not a stored field.** `AGENTS.md`'s conceptual description of what a trace "should represent" lists it alongside token usage and outcome evidence. In the actual `AgentWorthTrace` struct there is no such field — redaction is a pure transform (`Redactor::redact_trace(&trace) -> AgentWorthTrace`) that produces a new, separate, sanitized copy; nothing about sensitivity is persisted on the original trace or in SQLite.
- **Phase 2 (the routing/policy engine described in `docs/HANDOFF.md`) has no code.** Confirmed by absence — no `route`/`explain`/`run` subcommands in `apps/cli/src/main.rs`, no policy/routing crate in the workspace. HANDOFF.md is explicit that this is unstarted; the code agrees.

## 9. How to verify this document

Run from the repo root:

```bash
# Workspace shape: 9 crates + 1 CLI binary crate
cargo metadata --no-deps --format-version 1 | python3 -c \
  "import json,sys; [print(p['name'], p['version']) for p in json.load(sys.stdin)['packages']]"

# Adapter count and names
ls crates/adapters/src/*.rs | grep -v -e lib.rs -e mcp.rs | wc -l
grep -n 'fn name(&self)' -A1 crates/adapters/src/*.rs | grep -v mcp.rs

# Adapter registration matches the fleet above
grep -n 'Box::new(' crates/core/src/lib.rs

# Data model: confirm spec-only types are absent
grep -rn "TraceFeatures\|SensitivityReport\|PermissionEnvelope\|awt-0.1" crates/ apps/   # expect no output

# Storage schema
sed -n '/initialize_schema/,/^    }/p' crates/storage/src/lib.rs

# Redaction rule count and categories
grep -c 'RedactionRule::new' crates/redaction/src/rules.rs

# Scoring: 5 weighted components, confirm no "rarity" or "richness" field
grep -n 'pub struct ScoringWeights' -A6 crates/scoring/src/scorer.rs
grep -rn "rarity\|richness" crates/scoring/src/   # expect no output

# ATIF acronym expansion
grep -rn "Agent Trajectory Interchange Format" crates/export-atif/ README.md

# npm launcher resolution order and self-recursion guard
sed -n '/^export function resolveBinary/,/^}/p' packages/agentworth/lib/resolver.js
grep -n "isSelf\|looksLikeNodeScript\|AGENTWORTH_LAUNCHER_ACTIVE" packages/agentworth/lib/resolver.js

# Release pipeline: confirm 4 build targets, no Windows, OIDC (no NPM_TOKEN)
grep -n "target:\|NPM_TOKEN\|id-token" .github/workflows/release.yml

# CHANGELOG staleness
head -5 CHANGELOG.md; grep '^## \[' CHANGELOG.md
grep '^version' Cargo.toml
```

None of the above require a build. `cargo metadata --no-deps` reads `Cargo.toml`/`Cargo.lock` without compiling anything.
