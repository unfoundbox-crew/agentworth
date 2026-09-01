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
| SSE / live-tail endpoint | **Done, merged into integration branch.** | `GET /api/live-tail`, `notify`-based watcher + broadcast channel, no polling. 25/25 tests pass on lenovo (7 new). See SSE findings below |
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

Integration branch: `integrate/handoff-batch-1`, working tree clean, ahead of `origin/main`. Check `git log --oneline -1` for the current tip — it moves fast today.

## SSE findings (2026-09-01)

Built `GET /api/live-tail`: a `notify`-based filesystem watcher (real OS events, no polling) feeding a `tokio::broadcast` channel, consumed by an SSE handler. Watch roots come from each adapter's own `.detect()` (same logic `/api/matrix` already uses, so the watch set can't drift from what's actually scanned). Lag turns into a `lagged` SSE event rather than silently dropping the client. 25/25 tests pass on lenovo.

While verifying, the branch's own isolated test run showed the `agentworth`/`agwt` CLI binaries failing to compile — the same 8 files from the batch-2 findings above. **False alarm, not a new bug**: that branch (`feat/sse-live-tail`) was forked from the integration tip *before* the batch-2 fixes landed (merge-base `a2342ff`, batch-2 fixes start at `8f60e3e`), so it was testing against a stale, pre-fix snapshot. Confirmed resolved on the real integration tip (which already has 211 passing tests, workspace-wide, binaries included). A task chip flagging this as a live break was raised and has been withdrawn as stale.

## Batch-2b — dispatched today

| Item | Branch | Status |
| --- | --- | --- |
| Independent outcome verification | `feat/outcome-independent-verification` | Building |
| Per-model attribution downstream in scoring | `feat/scoring-per-model-attribution` | Done — `TraceScore` gains `per_model`/`total_estimated_cost_usd`, keyed to `per_model_token_usage`; 3 new tests incl. multi-model fixture; 214/214 pass on lenovo (commit `bf5a5ff`) |
| High-entropy secret detector | `feat/secret-detector-entropy` | Building |
| ModelSwitch events across 20 adapters | `feat/adapter-modelswitch-events` | Building |
| Persisted CLI config/defaults | `feat/cli-persisted-config` | Building |
| Recovery-loop human-vs-agent distinction | `feat/recovery-loop-human-vs-agent` | Done. Built clean + `cargo test --workspace` green on lenovo (0 failed, 221 passed, incl. 7 new/extended `watch::` tests). Heuristic: race from the alert forward — whichever comes first, a `UserMessage` event or the loop's own pattern breaking on the agent's side. Confidence: moderate, not high — it's correlation, not causation (an unrelated user message ahead of the break still reads as "rescued"), it has no time/distance bound, and it silently misses a human interrupt that never lands as a `UserMessage` (e.g. a raw Ctrl+C an adapter doesn't parse). Full reasoning and limitations are in the doc comment on `classify_resolution` in `apps/cli/src/commands/watch.rs`. |
| Context-Rot Marker | `feat/context-rot-marker` | Building |

**Deferred, not dispatched today:**
- **Threat Digest** — depends on the secret detector above actually landing first. Next wave.
- **Blunder-to-Blame Bridge** — depends on blunder detection + blame both being stable under the same integration; glue work, do after this wave folds in.
- **Personal Leaderboard** — product-fit question, not just an engineering one: this is a local-first, zero-telemetry, single-player tool — "leaderboard" implies comparing against other people, which doesn't obviously fit. Worth asking Saurabh before building rather than guessing.
- **Hall of Blunders Share Pack** — same product-fit question: a "share pack" implies exporting for others to see, in tension with local-first/zero-telemetry positioning. Ask before building.

## New punch-list items (not yet actioned)

- `verdict_breakdown.real_verified_rate` can exceed 1.0 — triage the scoring crate for a wrong denominator or double-count.
- `models_usage_count` / `tools_usage_count` zero-seeding (low priority, see Decisions below).
- Leftover dirty state in 9 local worktrees + `lane3-bundle` on lenovo — Saurabh's call.
- `Cargo.lock` carries a stale package version (`0.1.3`) against the workspace's declared `0.1.5` — confirmed pre-existing (reproduces on unmodified `f45c6b1` too, not caused by any batch-2b item). Worth a clean `cargo update`/regenerate pass before the final PR, not urgent before then.

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
