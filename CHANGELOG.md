# Changelog

All notable changes to **AgentWorth** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.1.14] - 2026-09-02

### Added

- **The handoff, written by the machine.** `agentworth handoff`, `agentworth loose-ends`, and the MCP tools `session_handoff` and `carry_forward`: what a session promised and did not do, what it decided, files touched, commands and how they ended, the outcome rung, compactions. Every line carries a receipt; nothing is summarised (#77).
- **`forgotten_context`.** The decisions compaction dropped, handed back with receipts, plus `agentworth forgotten`. Measured on one 8-round session: 405 decision sentences in, 28 out (#83).
- **`suspect_commits`** and `agentworth suspect --hook`: commits whose authoring session had no exit-0 test, a demoted done-claim, or a loop; anchored to the repo so relative blame paths cannot suffix-match every repo on disk (#84).
- **`outcome_rate`**: verified-outcome rate by model, adapter or repo, with an n floor (#75).
- `inspect` accepts a session-id prefix; `doctor` and `matrix` render in the design system (#76).
- CI runs tests on ubuntu and macOS, clippy gates, path-aware jobs; the dead GitHub Pages deploy is gone (#79).

### Fixed

- **"Tests passed" now means exit 0.** The Claude Code adapter never parsed tool results on real transcripts (they nest under `message.content`), so no exit code ever reached the outcome engine, which granted rung 3 from the command string alone. Exit codes are parsed from `is_error` and "Exit code N"; a test, build, CI or deploy command with no exit code cannot reach a verified rung. Aider's markdown path no longer hardcodes success (#81, #85).
- Every real session gets scored; a parser version per adapter triggers a one-time reparse when parsing changes; backfill runs once per version and never loops on sessions with no prompt (#85, #89).
- `stats` and `usage` count the same sessions; usage views apply the non-stub predicate (#78).
- Live-tail adapter attribution works on macOS (FSEvents reports canonical paths) (#87).
- The archaeology pane fetches its data (#80). Trace responses gzip and paginate; the inspector streams large sessions (#72, #73). prompt_preview backfills; the matrix derives from the adapter registry (#74).

---

## [0.1.13] - 2026-09-02

### Added

- **MCP server.** `agentworth mcp` (stdio) exposes the index to any coding agent: `sessions_find`, `session_get`, `blame_find`, `usage_summary`, `pacing_window`, `coverage_stats`. Redaction on by default. Register with `claude mcp add agentworth -- agentworth mcp` (#56).
- **Per-session compaction tracking** in the schema, storage and API, and a compaction pane in the dashboard: rounds, context before and after, what survived, marked on the trajectory (#57, #62).
- **CLI output redesigned.** One design system for `stats`, `usage`, `traces`, `blame`, `scan` and the receipt: the evidence ladder with its evidence line, right-aligned numbers, 100-column max, no emoji, `--plain`, `--no-color`, `NO_COLOR` and non-TTY honoured. `--json` payloads unchanged (#67).
- Real vendor logos in the coverage matrix, and one brand mark across the site, dashboard, favicons and social card (#59, #63, #65, #66).
- `curl -fsSL https://agentworth.dev/install.sh | sh` is now a real installer (#55).

### Fixed

- The index stored hundreds of config, telemetry and other non-session files as sessions with zero events. Discovery rules for seven adapters now reject them, the scanner skips zero-event parses, and a full scan prunes existing stubs (#68).
- `agentworth serve --dist <path>` silently served the embedded dashboard when the path was wrong; it now fails at startup naming the path (#60).
- `GET /api/traces` dropped `primary_outcome` and `composite_score` when null, so every outcome dot rendered grey; the keys are always present now (#60).
- Release tarballs ship `agwt` as well as `agentworth`; the npm launcher resolves whichever name it was invoked as (#61).
- CI now runs `cargo test --workspace` and the npm launcher tests; a test that had been red on main is fixed; one mutex `unwrap` that could wedge `serve` recovers (#54, #61).
- Dashboard formatters no longer throw on a field the server stopped sending (#53).

---

## [0.1.12] - 2026-09-02

### Fixed

- **`npx agentworth` works on macOS again.** 0.1.11 passed `--force-local` to `tar` on every platform. That flag exists only in GNU tar; macOS ships BSD tar, which rejected it, so every fresh install on a Mac failed with "native binary not found". The flag was a Windows-only workaround and Windows is no longer supported, so it is gone (#51).

---

## [0.1.11] - 2026-09-02

### Fixed

- **Windows dropped as a supported platform.** 0.1.10 shipped a Windows build for the first time, and it broke `npx agentworth` on real Windows machines within minutes: GNU tar parses a `C:\Users\...\agentworth.tar.gz` archive path as a remote `host:path` tar spec (the drive letter before the colon reads as a hostname), so extraction fails instead of just opening the local file. Rather than chase this again, Windows is no longer built or resolved — `agentworth` on Windows now fails with a clear "unsupported platform" message instead of a 404 or a tar crash. Building from source still works on any platform.
- The npm launcher no longer lets a stale binary in a cargo target directory or `~/.cargo/bin` silently answer for every later version forever — the same version check already applied to `PATH` now applies to those sources too.

---

## [0.1.10] - 2026-09-02

### Fixed

- **The outcome ladder now actually renders.** `primary_outcome` was written to the index in hand-rolled PascalCase (`"CommitObserved"`) while the API contract and the whole frontend expect snake_case — every session with a real outcome silently read as unresolved, and `verified_outcomes_count` was 0 for everyone. A data migration corrects every row already on disk the first time this version opens your index; nothing to run by hand.
- No `busy_timeout` was set on the SQLite connection — `agentworth serve` and `agentworth scan` running at the same time could return `SQLITE_BUSY` immediately on a write collision instead of waiting.
- `prompt_preview` was always empty. It's now populated from each session's first real user message, truncated past 200 characters.
- `/api/stats` and `/api/traces` disagreed on how many sessions existed (one counted near-empty stub sessions, the other didn't) — several report/dashboard numbers were quietly dividing against the wrong population. Fixed at the source rather than patched per call site.
- Several commands (`audit`, `threat-digest`, `stats`, `archaeology`, `blind_spots`, and more) silently capped how many sessions they looked at while presenting the result as "all sessions" — on an index above the cap, that read as a clean bill of health when it wasn't. All now scan the real total.
- `agwt search`'s background indexing only ever ran once per process; any session scanned after the first successful run was never indexed for search again. Rebuilt as a real incremental indexer.
- Every dollar figure in `agentworth`'s reports was priced as Claude 3.5 Sonnet regardless of which model actually ran the session.

### Added

- `agentworth version` / `agentworth update` — checks whether a newer release exists and tells you how to get it (never replaces the binary itself).
- `agentworth threat-digest` and `agwt blunder-blame` — rank indexed sessions by real secret exposure, and connect a blunder back to the files it touched (or a file back to the sessions that touched it).
- A real-time `GET /api/live-tail` (SSE) endpoint — filesystem-watch-based, no polling.
- Outcome claims are now cross-checked against real trace state before being trusted — a bare tool-call request with no observed result no longer counts as verified.
- `ModelSwitch` events, tracked across all 20 supported harnesses.
- `agentworth config` — persisted defaults for `json`/`limit`/`period`, explicit flags always win.
- A context-rot marker and a human-vs-agent recovery-loop classifier, both intentionally conservative (weak-to-moderate confidence, documented as such) rather than dressed up as more certain than the underlying signal supports.
- Redaction now covers a session's own repository/project name (previously only a home directory's username was stripped, so `export --redact` still leaked project identity) and reaches outcome/recovery evidence, not just raw event content — closing a gap flagged ahead of the read-only MCP server spec in `docs/specs/mcp-server.md`.

---

## [0.1.9] - 2026-09-01

### Added

- **Trajectory view in the dashboard inspector** — a timeline strip (bucketed ticks across three rows), a virtualized event stream, and a detail panel. Before this, the inspector scored a session (`80/100`) without ever showing what the agent actually did; `trace.events` was fully populated and rendered nowhere.
- **Categorical colour palette** for the dashboard's data views — eight identity hues, full light and dark sets, with a colour/monochrome toggle. A stacked token-economics bar in one hue plus greys wasn't readable; cache read vs. cache creation is the whole point of that chart.
- **SpacePilot icon sprite** replaces most hand-drawn icons in the dashboard topbar, rail, and outcome ladder (27 icons ported).
- **Landing state now shows an overview** ("how am I doing") instead of "Select a session to inspect it."

### Fixed

- Escape now actually collapses the expanded trajectory view. Its tooltip had promised `Collapse (esc)` since the explorer shell shipped, but no handler existed — and once one was wired up, a stale-closure bug in the key handler's dependency array kept it doing nothing anyway.
- Danger red no longer marks sessions that simply lack evidence yet. `OutcomeKind` has no failure state — every value is a degree of evidence or its absence — so red is now reserved for an actual cost signal (a cache-invalidation spike), and unverified work reads as neutral or hollow instead of a wall of alarms.

---

## [0.1.8] - 2026-09-01

### Fixed

- **`npx agentworth@<version>` now runs the version you asked for.** Every release before this deferred to any `agentworth` already on `PATH`, so a pinned request silently ran whatever was installed locally instead — four releases stale, in the case that surfaced it. This is the fix 0.1.7 was tagged for; see below — it didn't actually ship until now.
- The release smoke test no longer races the npm registry. A good release could previously report itself broken because the check ran before the published version had propagated.

---

## [0.1.7] - 2026-09-01

This release shipped no code changes. The tag and PR are titled "restore the substance the shell replaced" — the same title as the previous release's PR — but the commit changes zero files against 0.1.6. Whatever it was meant to carry landed on the wrong branch and never reached `main` before this tag was cut; the real fix (above) shipped for real in 0.1.8. If you're already on 0.1.6, there is nothing here for you.

---

## [0.1.6] - 2026-09-01

### Added

- **MotionVector design system** applied across the marketing landing page and the new dashboard.
- **Keyboard-first, three-pane explorer shell** for the dashboard, splitting `apps/web` into a marketing site and a standalone `apps/dashboard`.
- Inspector detail restored after the shell's first pass thinned it out: a five-component score breakdown with the audit explanations underneath, token economics shown as a proportion (cache read vs. cache creation), a provenance block (source path, fingerprint, on-disk-verified chip), and recovery signals. The session list regained its seven sort modes, cross-field search, and a density toggle.
- `ci.yml` — the repo's first Rust CI. Before this, only page-deploy and the tag-triggered release existed, so nothing compiled the workspace on a pull request.

### Fixed

- **The installed binary now actually contains the dashboard.** Every release back to 0.1.0 built no web app in `release.yml` and looked for `apps/dashboard/dist` by a relative path that only resolved inside a repo checkout — so every npm and cargo install served a hardcoded stub page, silently, for months.
- The session list was showing 50 of 2,903 sessions, because `/api/traces` defaults to `limit=50` and nothing asked for more — every filter, sort, and search ran against just the newest 50 and looked normal doing it.
- Every deep link was a blank page: Vite's `base: './'` resolves asset URLs against the current route, so `/s/<id>` requested `/s/assets/...`, got the SPA fallback's `index.html` back, and died on a MIME check before React ever mounted.
- Five TypeScript field names never matched the server's actual response shape (`cache_read_tokens`/`cache_creation_tokens` vs. `cache_*_input_tokens`; `adapter_name`, `mtime_epoch_secs`, `content_fingerprint` vs. `adapter`, `modified_timestamp`, `fingerprint`) — cache economics silently read as zero and provenance as em-dashes even though the real data was there.
- The adapter column was 54px, truncating `claude_code` — the single most common value in any real index — down to `claude_c`.
- `agwt blame` now persists file modifications so lineage lookups actually match; previously edits went unrecorded and blame silently came up empty.

---

## [0.1.5] - 2026-08-31

### Fixed

- **`npx agentworth` no longer spawns itself into an `EAGAIN` crash loop on macOS.** The launcher's PATH-resolution step found npm's `node_modules/.bin/agentworth` symlink — which points back at the launcher itself — before the GitHub-release binary downloader ever got a chance to run. The old anti-recursion guard compared unresolved paths and didn't follow the symlink, so it missed it. Fixed with three independent guards: realpath-based self-detection, rejecting `.js`/`.cjs`/`.mjs` shim files outright, and an `AGENTWORTH_LAUNCHER_ACTIVE` flag that skips PATH lookup once already inside a launcher.

---

## [0.1.4] - 2026-08-31

### Fixed

- **`npm install agentworth` actually installs something again.** 0.1.3's `npm publish` failed with a 404 that looked like a missing package but was really an npm auth failure, so every `npx agentworth` invocation — including every landing-page CTA — had been silently stuck on 0.1.1 since. Publishing now goes over trusted publishing (OIDC) instead of a token; classic and even bypass-2FA granular tokens both got a hard 403 when tested against a real publish, which matches npm's own policy of retiring token-based publishing for this case.
- A version gate now blocks a release where the git tag, `Cargo.toml`, and `package.json` versions disagree. 0.1.3 was tagged from a tree where `Cargo.toml` still said 0.1.2, so the binary it built reported the wrong version.
- `npm whoami` now runs before publish, so a broken publish fails in seconds naming the real cause instead of ending in a confusing 404.

### Added

- A clean-room smoke test: `npx -y agentworth@<version> --version` and `usage --pacing`, run on a fresh Ubuntu and macOS runner with no checkout and no cargo cache — the exact conditions 0.1.3 shipped broken under.

### Removed

- `brew install agentworth` and the `curl | sh` installer, from the missing-binary message. Neither exists: there's no Homebrew tap, and that install URL just returns the site's HTML with a 200 — so the advertised command would have piped a webpage into a shell. Points at the GitHub releases page instead.

---

## [0.1.3] - 2026-08-31

Tagged, but never published to npm — `npm view agentworth versions` jumps straight from `0.1.1` to `0.1.4`. Same root cause as the 0.1.4 fix above: `npm publish` returned a 404 here that looked like a missing package but was an auth failure.

This commit is a large squash that also carries the full adapter-fleet-to-20 change already described under [0.1.2] below — there's no separate 0.1.2 tag; the two shipped as one commit. What's new beyond that:

### Added

- Local semantic search (`agwt search`), backed by a FastEmbed ONNX embedding engine that runs fully offline.
- Forensic safety auditor (`agwt audit --safety`).
- Five-rung outcome ladder indexing in SQLite, plus a cache-cliff visualizer and `agwt matrix` in the (not-yet-shipped) dashboard UI.
- `agwt blunder` — dispatches a redacted incident report to stfuopus.lol, stripping secrets before anything leaves the machine.
- Adapter discovery now runs across multiple cores in parallel, with adapter stats sorted; native SQLite ingestion for OpenCode sessions.

### Fixed

- `primary_outcome` and `composite_score` now migrate before index creation in `initialize_schema`, instead of after.

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
