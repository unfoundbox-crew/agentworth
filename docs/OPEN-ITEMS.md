# Open items

State at the end of 2026-09-01. Written so a fresh session can pick up without
re-deriving any of it. Delete lines as they land.

## Shipped today

`v0.1.9` is on npm and the binary finally contains a dashboard — every release
before `0.1.6` served a hardcoded stub, so no installed copy had ever shown a
UI. Along the way: the marketing site and the local dashboard became separate
builds, the dashboard became a keyboard-first three-pane app with real paths
and deep links, the inspector gained score breakdown, token economics,
provenance and a trajectory view, colour stopped claiming failure where the
data only says no evidence yet, and the icons now come from the SpacePilot
design system instead of being reinvented.

## Waiting on a human

| What | Where | Note |
| :--- | :--- | :--- |
| Merge the two docs PRs | #25, #26 | AGENTS.md facts, README workflow guide, changelog 0.1.2 to 0.1.9 |
| Decide on four older PRs | #5, #6, #9, #10, #11 | see below |
| Revoke the two dead npm classic tokens | npm settings | publishing moved to Trusted Publishing; nothing reads them |
| Delete the unused `NPM_TOKEN` repo secret | repo settings | same reason |
| Restart Claude Code | — | `theme` was set to `dark-ansi`; it takes effect on restart |

### The older PRs

- **#10 — Windows target.** Still real. `release.yml` builds four targets and
  none of them is Windows, but the npm launcher maps `win32-x64`, so a Windows
  user gets a launcher with nothing to download. Worth merging.
- **#9 — ARCHITECTURE.md.** Mergeable. Predates the app split, so check it
  describes `apps/dashboard` and `packages/ui` before merging.
- **#5 — social card, repo links, brew tap, version badge.** The version badge
  and social card were fixed independently today. Check what is left before
  merging; it may be down to the brew tap only.
- **#6 — version gate, npm auth, smoke tests.** Superseded. All of it shipped.
  Conflicting. Close it.
- **#11 — SKILL.md and flight receipts.** Untouched today. Needs a read.

## Backend, with the other session

| What | Status |
| :--- | :--- |
| `OutcomeKind` PascalCase in the DB vs snake_case everywhere else | taken, top of their queue |
| SSE endpoint, sharing a file watcher with `agentworth watch` | taken, queued |
| `/api/stats` counts 10,188 where `/api/traces` returns 2,903 | raised, unowned |
| `prompt_preview` is empty on every session | raised, unowned |

The last two matter more than they look. Until the enum lands, every session
reads as unresolved and the whole outcome layer is cosmetic. Until
`prompt_preview` is populated, sessions are unrecognisable — `agent-af702e89`
tells a human nothing, and the field built to fix that has never been filled.

## Specced, not built

- `docs/specs/trajectory-scrubber.md` — zoom and pan for the timeline strip. It
  is a solid bar at 6,978 events today.
- `docs/specs/fleet-view.md` — which agents are running right now, across every
  harness. The cheap version infers from file mtime and needs no streaming.
- `docs/specs/desktop-app.md` — Tauri, `.dmg`, and the menubar variant.

## Smaller, still open

- **No `busy_timeout` is set on any SQLite connection.** WAL is on, but every
  connection opens without a busy timeout, so a write collision returns
  `SQLITE_BUSY` immediately instead of waiting a few hundred milliseconds.
  `agentworth serve` and `agentworth scan` already open independent connections
  to the same file, so this is reachable today with no desktop app involved.
  One pragma plus a concurrency test.
- **The cargo-target fallback is unversioned.** Inside any repo checkout the
  launcher prefers `target/release/agentworth` regardless of its version, so a
  stale local build silently shadows the installed one. Same class as the PATH
  bug fixed in `0.1.8`, same fix would apply.
- **Session list and inspector each call `useSessions()`**, so the summary list
  is fetched twice on mount. Wants a shared store.
- **Resizable session column.** Asked for, not built. Drag handle, width
  persisted, frontend only.
- **Group sessions by repo, worktree, and subagent.** All three are derivable
  from `source_path` today with no NLP and no embeddings — 23 distinct repos
  across a 500-session sample, and 440 of those 500 were subagent runs.
- **A Harness adapter.** DeepSeek Harness is MIT with an append-only log at
  `~/.dsh-*/session.jsonl.zstd`. No adapter exists. It is the cheapest one
  left, and its users are the audience.
- **`~/code/CLAUDE.md` still documents the `GITHUB_TOKEN=$GITHUB_CREW_TOKEN gh`
  workaround.** Obsolete — `gh` holds both accounts now. The SSH section was
  corrected today; this line was missed.

## Decided today, so nobody relitigates it

Written up in `docs/DECISIONS.md`. The short version: AgentWorth is local-only
forever, because hosting means uploading users' traces. Nothing is built toward
worldtrainer inside this repo until agentworth.dev takes off. The dashboard is a
viewer plus one rescan button; the CLI is where you act. Marketing and dashboard
are separate builds. Real paths, not hash routing. No polling stands in for SSE.
