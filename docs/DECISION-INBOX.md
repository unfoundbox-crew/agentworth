# AgentWorth — Decision & Status Inbox

Owner of this doc: the backend/CLI session (Claude, socket name `code-76` in fleet messages). Update it in place as work lands — don't let decisions sit only in a chat transcript.

Last updated: 2026-09-01, mid-session. Check git before trusting this if it's more than a day old.

## Where things stand

| Item | Status | Notes |
| --- | --- | --- |
| PR #12 blame fix | Merged to main | |
| Batch-1 (10 fixes) | Done, on integration branch | Real bugs found and fixed, not just merged — see [[HANDOFF.md]] history |
| Linux adapter detection | Done, on integration branch | 11 adapters fixed, pushed as `fix/linux-adapter-detection` |
| Per-model token attribution | Done, on integration branch | |
| Dashboard crash fix | Done, on integration branch | **Blocked from landing on main** — needs rebase onto PR #13 + #14 first, see Blocked below |
| Batch-2 (9 fixes) | Merged into integration branch, verify pass running now | Same pattern as batch-1: agent independently rebuilds and fixes forward, doesn't trust the handoff doc |
| SSE / live-tail endpoint | Building now | own branch `feat/sse-live-tail`, folds into integration branch after batch-2 settles |
| Batch-2b (11 items) | Scoped, not started | Gated on batch-2 settling — touches the same files (adapters, redaction, outcomes, blunder.rs) |
| Cost-Aware Task Router | Deferred, not scoped | Different shape of feature — needs a live per-agent hook, not an index query. Needs its own design pass, not a bolt-on to batch-2b |
| Final PR + version bump | Not yet | Saurabh's call — standing decision is one PR, one version bump, only once everything this session touches is done |

## Blocked on someone else

- **PR #13** (`feat/web-design-system`) and **PR #14** (`feat/explorer-shell`) — both open, owned by the landing-page-ux-review session. The dashboard-crash-fix here touches the same 3 files (`App.tsx`, `SessionInspector.tsx`, `api.ts`). Can't rebase until both land on main. Not something this session can move — check PR state before assuming it's still blocked.

## Decisions made this session

- **One PR, one version bump, at the end.** Everything accumulates on `integrate/handoff-batch-1` (worktree `.worktrees/integrate-handoff-batch`). No incremental pushes, no Cargo.toml bump, until Saurabh says the session's work is done.
- **Never trust agy/Flash batch self-reports.** Independent rebuild and retest is mandatory before folding anything in, no matter how polished the handoff doc looks. Confirmed twice — see the memory note this points back to, or `HANDOFF.md` / `HANDOFF_BATCH_2.md` for the receipts.
- **SSE endpoint built fresh, not sharing code with `watch`.** Loop Sentinel's `agentworth watch` (already merged) is poll-based (`--interval-secs`, `--poll-once`), not a real filesystem watcher — there's no existing `notify`-based primitive to reuse. Not refactoring `watch.rs` to share one either; that's scope beyond what either feature actually asked for.
- **`models_usage_count` / `tools_usage_count` zero-seeding downgraded to low priority.** Same `entry(k).or_insert(0)` map pattern as the already-fixed `outcome_distribution` bug, but these are open-ended maps the frontend only iterates — a missing key is invisible, not a crash (unlike `outcome_distribution`, a fixed-shape record read by key). Still worth seeding at zero for contract honesty, just not urgent. Caught and correctly argued down by the landing-page-ux-review session.
- **Cost-Aware Task Router pulled out of batch-2b.** It's a live-intervention feature (needs a hook into agent execution as it happens) — not an after-the-fact analytics query like the rest of the list. Different problem, deserves its own scoping conversation with Saurabh rather than being sized as one item among eleven.

## Ownership boundaries (real coordination state — not derivable from git)

- **This session**: all agentworth backend/CLI work — everything in the status table above, plus batch-2b.
- **landing-page-ux-review session**: `feat/web-design-system` (PR #13), `feat/explorer-shell` (PR #14), `fix/launcher-self-recursion` (merged), the npm-publish/release/windows-target chain (PR #6, #10).
- **Not currently claimed by anyone confirmed** — leave alone, ask Saurabh before touching: PR #9 (`docs/architecture`), PR #11 (`agentworth-skill-and-receipts`), PR #5 (`claude/trusting-volhard-dadc3e`).
- **Director role** (AgentWorth promo video coordination): don't proactively contact about agentworth work — ask Saurabh directly instead if something's needed from that side.
