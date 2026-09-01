# AgentWorth — Decision & Status Inbox

Owner of this doc: the backend/CLI session (Claude, socket name `code-76` in fleet messages). Update it in place as work lands — don't let decisions sit only in a chat transcript.

Last updated: 2026-09-01, mid-session. Check git before trusting this if it's more than a day old. This copy on `integrate/handoff-batch-1` is canonical; other branches may carry a stale snapshot until they fold in.

## Where things stand

| Item | Status | Notes |
| --- | --- | --- |
| PR #12 blame fix | Merged to main | |
| Batch-1 (10 fixes) | Done, on integration branch | Real bugs found and fixed, not just merged |
| Linux adapter detection | Done, on integration branch | 11 adapters fixed, pushed as `fix/linux-adapter-detection` |
| Per-model token attribution | Done, on integration branch | |
| Dashboard crash fix | Done, on integration branch | **Blocked from landing on main** — needs rebase onto PR #13 + #14 first, see Blocked below |
| Batch-2 (9 fixes) | **Done.** Verified, fixed, tested | 8 of 9 branches did not compile as originally committed (agy again). All fixed for real — see Batch-2 findings below |
| SSE / live-tail endpoint | Building now | own branch `feat/sse-live-tail`, folds in when done |
| Batch-2b (7 items dispatched, 4 deferred) | Building now | 7 agents in flight, own worktrees, see table below |
| Cost-Aware Task Router | Deferred, not scoped | Different shape of feature — needs a live per-agent hook, not an index query. Needs its own design pass |
| Final PR + version bump | Not yet | Saurabh's call — one PR, one version bump, only once everything this session touches is done |

## Batch-2 findings (2026-09-01, verify-and-integrate pass)

**8 of 9 branches failed to compile as originally committed.** Only `fix/agwt-alias-consistency` was genuinely clean. Every other item had fabricated API calls that don't exist on `Storage`/`Scanner` — `storage.get_session()` (real: `get_session_by_id`), `storage.insert_trace()` (real: `upsert_trace`), `Scanner::default()` (no such constructor), `storage.list_sources()` (no such method), `storage.search_similar_chunks()` (real: `vector_store()?.search_filtered()`), `storage.find_file_blame()` (real: `find_sessions_for_blame`) — the same handful of plausible-but-nonexistent names recurring near-identically across independently "written" files. All rewritten against the real API, real tests added/fixed, 211 passing (`cargo test --workspace`, lenovo, up from 191).

**One real logic bug distinct from the compile errors**: `blind_spots.rs` compared `primary_outcome` against lowercase `"done_claimed"`, but the index stores PascalCase (`"DoneClaimed"`, from `outcome_kind_name()`) — same bug class as the already-fixed `routes.rs` outcome-distribution bug. Checked the rest of the codebase for the same pattern (grep for hardcoded lowercase outcome strings) — no further instances found.

**Two things flagged, not fixed, need a look when convenient (not urgent):**
- Agy left uncommitted, unreviewed edits (identical across most of the 9 worktrees: `apps/cli/Cargo.toml`, `apps/cli/src/commands/audit.rs`, `apps/cli/src/main.rs`, plus an untracked `crates/storage/src/merge.rs` in `feat-cross-machine-merge`) sitting in the real local `.worktrees/feat-*` dirs. Confirmed: the agy process itself (PID 24559 on lenovo) is no longer running, so nothing is actively getting worse. The verified, correct version of all 9 items already exists on this integration branch — these dirty copies are very likely superseded leftovers, not lost work, but that's a guess, not a check. Left untouched pending Saurabh's call: inspect, `mv` aside, or discard.
- `~/builds/lane3-bundle` on lenovo has 24 dirty files, unrelated to anything this session started. Not touched, not investigated further — flagging in case it's someone else's build lane.
- `verdict_breakdown.real_verified_rate: 1.09` (over 100%) showed up in a real `stats` smoke-test run. Pre-existing scoring bug, unrelated to batch-2. Not triaged yet — added to the punch list below.
- `HANDOFF_BATCH_2.md`'s documented flags don't match what actually shipped for 5 of 9 items (e.g. docs say `watch --interval`, code has `--interval-secs`; docs say `cache-doctor --threshold`, code hardcodes the value). The verify pass treated the real committed code as ground truth and didn't invent flags to match stale docs. Cosmetic doc-drift, not a bug — low priority.

Integration branch: `integrate/handoff-batch-1`, currently at commit `c438719`, 57 commits ahead of `origin/main`, working tree clean.

## Batch-2b — dispatched today

| Item | Branch | Status |
| --- | --- | --- |
| Independent outcome verification | `feat/outcome-independent-verification` | Building |
| Per-model attribution downstream in scoring | `feat/scoring-per-model-attribution` | Building |
| High-entropy secret detector | `feat/secret-detector-entropy` | Building |
| ModelSwitch events across 20 adapters | `feat/adapter-modelswitch-events` | **Done.** All 20 adapters wired, verified on lenovo — see findings below |
| Persisted CLI config/defaults | `feat/cli-persisted-config` | Building |
| Recovery-loop human-vs-agent distinction | `feat/recovery-loop-human-vs-agent` | Building |
| Context-Rot Marker | `feat/context-rot-marker` | Building |

### ModelSwitch findings (2026-09-01)

**The dispatch brief was wrong about one thing: `ModelSwitch` already existed.** `agentworth_schema::EventPayload::ModelSwitch` was added back in the original `feat(schema,claude): canonical trace model` commit — full struct (`from_model`/`to_model`/`reason`), wired into `TraceStats::recalculate_stats`, `export-atif`'s serializer, `redaction`'s redactor, and the CLI's trace pretty-printer (🔀 line). No adapter ever constructed one, so the type was fully plumbed downstream but silent upstream. Nothing needed adding to schema or adapter-sdk — the whole job was in the 20 adapters.

**All 20 adapters got real detection — no gaps.** Every adapter already extracts a per-event `model` string to build its existing `ModelInvocation` event; that's the same field a switch check needs. Tracked the last-invoked model the same way `seq`/`sequence` is already threaded, and emit `ModelSwitch` immediately before a `ModelInvocation` whose model differs from the last one (no switch on the first model seen — nothing to switch from). 18 of 20 adapters fit one shared pattern; `aider` and `opencode` each have two independent event-construction paths (aider: structured JSON vs. its own markdown chat-history format; opencode: JSONL records vs. the native SQLite `opencode.db` schema) and needed the same logic wired twice.

**Real tests, not just the mechanical wiring.** Extended `claude.rs`'s existing multi-model test plus three new tests (`cline.rs`, `aider.rs`'s markdown path, `opencode.rs`'s SQLite path) with real fixture data asserting the actual `ModelSwitch` events — one per structurally distinct code shape touched, not one per adapter.

**Verified for real on lenovo**, isolated build dir + dedicated `CARGO_TARGET_DIR` (shared-target-dir staleness bug from earlier today, per punch list): `cargo build --workspace` — exit 0, only 2 pre-existing unused-import warnings (unrelated to this change). `cargo test --workspace` — exit 0, 207 passed across the whole workspace (0 failed, 0 ignored), 90 of those in `agentworth-adapters` alone. Added 3 new test functions (`cline`, `aider` markdown path, `opencode` SQLite path) plus extended `claude.rs`'s existing multi-model test with ModelSwitch assertions — didn't check the adapters crate's exact pre-change test count, so "90" is the verified post-change total, not a verified delta. Commit `eccbe09` on `feat/adapter-modelswitch-events`, not pushed.

**Deferred, not dispatched today:**
- **Threat Digest** — depends on the secret detector above actually landing first. Next wave.
- **Blunder-to-Blame Bridge** — depends on blunder detection + blame both being stable under the same integration; glue work, do after this wave folds in.
- **Personal Leaderboard** — product-fit question, not just an engineering one: this is a local-first, zero-telemetry, single-player tool — "leaderboard" implies comparing against other people, which doesn't obviously fit. Worth asking Saurabh before building rather than guessing.
- **Hall of Blunders Share Pack** — same product-fit question: a "share pack" implies exporting for others to see, in tension with local-first/zero-telemetry positioning. Ask before building.

## New punch-list items (not yet actioned)

- `verdict_breakdown.real_verified_rate` can exceed 1.0 — triage the scoring crate for a wrong denominator or double-count.
- `models_usage_count` / `tools_usage_count` zero-seeding (low priority, see Decisions below).
- Leftover dirty state in 9 local worktrees + `lane3-bundle` on lenovo — Saurabh's call.

## Blocked on someone else

- **PR #13** (`feat/web-design-system`) and **PR #14** (`feat/explorer-shell`) — both open, owned by the landing-page-ux-review session. The dashboard-crash-fix here touches the same 3 files (`App.tsx`, `SessionInspector.tsx`, `api.ts`). Can't rebase until both land on main.

## Decisions made this session

- **One PR, one version bump, at the end.** Everything accumulates on `integrate/handoff-batch-1`. No incremental pushes, no Cargo.toml bump, until Saurabh says the session's work is done.
- **Never trust agy/Flash batch self-reports.** Independent rebuild and retest is mandatory before folding anything in, no matter how polished the handoff doc looks. Confirmed a second, stronger time on batch-2: 8 of 9 branches didn't even compile despite a detailed handoff doc.
- **SSE endpoint built fresh, not sharing code with `watch`.** Loop Sentinel's `agentworth watch` (merged) is poll-based, not a real filesystem watcher — no existing `notify`-based primitive to reuse, and not refactoring `watch.rs` to manufacture one.
- **`models_usage_count` / `tools_usage_count` zero-seeding downgraded to low priority.** Open-ended maps the frontend only iterates — a missing key is invisible, not a crash, unlike the fixed-shape `outcome_distribution` bug. Still worth fixing, just not a fire.
- **Cost-Aware Task Router pulled out of batch-2b.** Needs a live per-agent hook, not an index query — different problem, deserves its own scoping conversation.
- **Personal Leaderboard and Hall of Blunders Share Pack held back from today's dispatch** — both raise a product-fit question (local-first/zero-telemetry vs. features implying other users or external sharing) worth asking Saurabh rather than assuming.

## Ownership boundaries (real coordination state — not derivable from git)

- **This session**: all agentworth backend/CLI work — everything in the status table above.
- **landing-page-ux-review session**: `feat/web-design-system` (PR #13), `feat/explorer-shell` (PR #14), `fix/launcher-self-recursion` (merged), the npm-publish/release/windows-target chain (PR #6, #10).
- **Not currently claimed by anyone confirmed** — leave alone, ask Saurabh before touching: PR #9 (`docs/architecture`), PR #11 (`agentworth-skill-and-receipts`), PR #5 (`claude/trusting-volhard-dadc3e`).
- **Director role** (AgentWorth promo video coordination): don't proactively contact about agentworth work — ask Saurabh directly instead if something's needed from that side.
