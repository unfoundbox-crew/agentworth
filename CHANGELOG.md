# Changelog

All notable changes to **AgentWorth** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

## [0.1.18] - 2026-09-03

### Changed

- **`update --json`'s `advice` values are arrays, not strings.** Each channel's advice used to print as one string; it now carries a list of lines, matching how the screen wraps and joins them. A script reading a channel's advice as a single string needs to join the array itself (#129).

### Fixed

- **The installer no longer waits on a size probe.** `install.sh` asked GitHub for the asset size with a HEAD request that followed the redirect to the object CDN, with no timeout — on a slow link the download line could sit for tens of seconds, or forever, before it ever appeared. The size now comes from the header dump curl writes while the download streams, so the bar appears at once and fills in the total when the CDN answers. Every request now carries a connect timeout (#128).
- **`archie serve` prints a screen, not a box.** The banner was the last command still drawing its own frame, with a rocket emoji in it and "Explorer Server" for a thing the product calls the dashboard — it was never in the two audit passes. It now renders through the ui module like every other screen: the command and version at column 0, a leaders block for the local URL, the two API routes and the index path with the home directory folded to `~`, and a `Next` line saying how to stop it. Same glyph set, same width clamp, same `--plain`/`--no-color` columns. The banner is a function returning a string, so the snapshot sweep holds it to the same four rules without binding a port (#129).
- **`archie update` names the channel you actually installed from.** It could only recognise the npm launcher, and called everything else "unknown" — including a binary from `curl -fsSL https://agentworth.dev/install.sh | sh`, which is now detected by its install location (`$AGENTWORTH_INSTALL_DIR`, or `~/.local/bin`). The channel this binary came from is listed first and tagged `this install`; the rest follow under `other ways`, where before an npm-launched binary was shown npm and nothing else. `--json` gains a `channel` field (#129).
- **Every doc says `npx -y agentworth@latest`.** A bare `npx agentworth` reuses whatever npx already cached, which is how a months-old binary answers for what reads like a fresh install. The npm channel on the `update` screen prints both `npm install -g agentworth@latest` and `npx -y agentworth@latest`, and says why. The release workflow's smoke test still pins an exact version, on purpose (#129).
- **The npm launcher says when it is about to run an older binary than itself.** `AGENTWORTH_BIN` and a vendored binary were the two resolution paths that never checked the version they handed back; they now print one line when the binary is older than the package. It cannot see a stale *package* from npx's own cache — that is what the `@latest` change above is for (#129).

## [0.1.17] - 2026-09-03

### Added

- **The cockpit.** A bare `archie` on a terminal opens a full-screen reader over the same data: overview, sessions, one session, agents, repos and windows, with `j`/`k`, `Enter`, `/`, `1`-`6`, `h`/`a`/`f`/`r`, `?` and `q`. `archie tui` is the explicit spelling. Off a terminal, under `--plain`, under `TERM=dumb`, or with JSON output, both print the overview — what `archie stats` prints plus the current window — and exit 0. Read-only, permanently: no writes, no scan, no config changes, no model calls. Every screen is a string a printed command already produces; the cockpit adds a viewport, a cursor and key handling and nothing else. Behaviour change: bare `archie` used to exit 2 with a usage message; it now exits 0 (#121).
- **Onboarding, one line everywhere.** `install.sh` draws its own download progress — curl's own bars land outside the glyph set, so bytes are read straight from the file it's writing and redrawn in place. The npm launcher and the first `archie scan` on an empty index now print that same three-line Archie form instead of an emoji or their own text (#123).
- **`archie stats ladder`** — one screen for the question `docs/specs/archie-bench.md` set out to answer locally: of everything spent in a window, how much of it bought something a test, a commit or a CI run can be pointed at. Three blocks — every rung of the outcome ladder with the spend that sits below the evidence line, what a verified outcome costs by model / repo / adapter / effort, and the newest sessions that got past the line — closing on one sentence with the share of spend that proved nothing. `--period`, `--repo`, `--adapter`, `--model`, `--by`, `--min-n`, `--json`, and `stats_ladder` over MCP with the same parameters and the same JSON. Reads the index only; no transcript is reparsed (#124).
- The bottom row is *unflown* — no outcome evidence of any kind was found. It is not a failure state and is never danger-coloured; `OutcomeKind` has no failure value to draw one from. Every screen that draws the ladder now uses that one set of words (`unflown`, `done claimed`, `artifact changed`, `test or build passed`, `commit observed`, `CI or deployment verified`), where `stats`, `session show` and `repo suspect` used to have their own shorter set (#124).
- The sample floor below which a rate is suppressed now lives in one place, `agentworth_storage::OUTCOME_RATE_DEFAULT_MIN_N` = 20, read by `stats outcomes`, the `stats_outcomes` tool and the new ladder. `docs/DESIGN.md`'s chart rule 3 said 10 and now names that constant, so one product has one floor. Under it, a rate and a cost render blank — never a zero, never a dash pretending to be data (#124).

### Fixed

- **The second audit pass.** Seven more commands — session autopsy, session recall, session search, session show, config, version, docs — render through the ui module and get a status line on their slow path. `session recall` no longer reparses a transcript per match; the per-model spend it needs already lives in `session_model_usage`, written by the same scan that parsed the session. `update` prints its install link on one full line. The suite now passes on a machine with real session history, not just a fresh one (#122).

## [0.1.16] - 2026-09-02

### Added

- **A grammar instead of a list.** Five nouns — `session`, `agent`, `repo`, `window`, `stats` — carry everything that acts on indexed data; `scan`, `serve`, `mcp`, `doctor`, `docs`, `config`, `version`, `update`, `completions` and `merge` stay top-level. Every pre-0.1.16 spelling still runs, hidden from `--help`, until v0.1.18 (#118).
- New verbs: `archie agent show <adapter>`, `archie repo list`, `archie window list`, and `archie stats outcomes` — the verified-outcome rate, on the CLI at last (#118).
- **`archie`**, the short name, shipped everywhere `agwt` shipped: a third binary, the npm bin map, `install.sh`, and the release tarball. `agwt` keeps working and is out of the docs (#118).
- **Shell completions.** `archie completions <shell>` for bash, zsh, fish, powershell and elvish, plus live completion of session ids, repositories and models through `COMPLETE=<shell> archie`. One bounded read on a read-only connection: a missing or locked index offers nothing rather than blocking a Tab (#118).
- **Archie, on four surfaces.** A terminal short form on the scan line (three lines, collapsing to `(*) archie` under 46 columns), a settings picker in the dashboard, a page at agentworth.dev/archie, and a branded 404 — driven by two new config keys (`archie.accessory`, `archie.colourway`) read and written through `GET`/`POST /api/config` (#114).
- **The M3 redraw.** The muzzle folds into one closed head curve instead of a separate shape, and the light moves from a headlamp into a hand-held torch carried in a front paw — sleeping sets it on the ground, error drops it. The terminal form goes to three lines, nine columns (#119).
- **A docs home at `/docs/`.** A card grid over Learn (7 guides), Reference, Specs (24), Research (2) and Changelog, plus a client-side search palette (Cmd/Ctrl-K) over a prebuilt index — no new dependency, no service (#117).
- Two measured specs: **archie-bench**, a local leaderboard keyed on model x effort x repo and ranked by verified rate (#112), and **convergence**, on when a session stops making verified progress — measured over 3,046 real sessions, shipped as a token budget and a coverage warning rather than a stop switch, because the data didn't support one (#113).

### Changed

- `blind-spots` is now `archie session list --unproven` — a filter, not a command of its own (#118).
- Every show-style verb resolves a session identically: unique prefix, `--last`/`--current`, the picker on a TTY. An ambiguous prefix exits 2 with the candidates instead of guessing; `session cache` and `session bisect` gained all of this (#118).
- MCP tools renamed to match the CLI (`sessions_find` → `session_list`, `session_get` → `session_show`, and eight more). Every old name stays registered and reaches the same handler until v0.1.18 (#118).

### Fixed

- The Codex adapter reads the fields Codex rollouts actually carry: model and effort from `turn_context`, tokens from the cumulative `total_token_usage` (including a restarted counter), and the real workspace from `session_meta.cwd` instead of the home directory. `PARSER_VERSION` 1 → 2, so a normal scan reparses every Codex session once (#116).
- Model pricing now matches current models. Claude Sonnet 5/Opus 5/Fable 5.x, Sonnet/Opus 4.5-4.8, Haiku 4.5, Gemini 3.x, DeepSeek V4 and GLM 5.x/4.7 were silently priced at the Claude 3.5 Sonnet default rate card; `agentworth usage`'s dollar figures are now right for the models actually in use (#115).
- The local API's CORS allowed any origin to read a person's whole session history through `/api/traces` and write their preferences through `/api/config`. It now passes only a page served from this machine (localhost, 127.0.0.1 or `[::1]`, any port). `GET /api/config` no longer returns `config_path`, which named the user's home directory for no reason a browser needed it (#114).

---

## [0.1.15] - 2026-09-02

### Fixed

- Fixed: `scan` panicked on transcripts containing non-ASCII text (a byte-index slice inside recovery detection). Every text truncation now cuts on character boundaries, and a workspace clippy lint denies byte slicing of strings so this class cannot ship again.

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
