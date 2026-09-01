# Decision inbox

Fix outcomes and open items that haven't made it into `docs/DECISIONS.md` yet.
One entry per task. Newest first.

---

## 2026-09-01 — Dashboard crash fix, re-applied after the apps/dashboard restructure

**Task:** the token-usage crash fix was originally built against the old
single-app `App.tsx` layout. PRs #13/#15 then split that into
`apps/web` (marketing) + `apps/dashboard` (the real app) + `packages/ui`. This
re-applies the fix's intent onto the new files — no git rebase, read the new
tree and check by hand.

**Claims checked against the actual code and a live repro**, not assumed:

| Claim | Actual |
| --- | --- |
| `SessionInspector.tsx` guards came along verbatim | **False.** Token Economics panel still called `.toLocaleString()` straight on `cache_read_input_tokens`/`cache_creation_input_tokens`, no guard. Reproduced live: blank white screen, `Cannot read properties of undefined (reading 'toLocaleString')`. |
| `SessionInspector.tsx` is the live session-detail view | **False.** It's dead code — nothing imports it except a code comment in `ExportModal.tsx`. The real click-through view is `shell/InspectorPane.tsx`, written fresh in the restructure. |
| `fetchTraceDetail()` in `api.ts` normalizes token field names | **False.** Only `fetchAggregateStats()` did. Two real consumers were affected: `InspectorPane` (own duplicate inline fetch) and `ExportsPane`/`ExportModal` (via `fetchTraceDetail`). Reproduced live: session inspect showed "3.0k tokens" and the ATIF export showed `cache_read_tokens: 0` against a true total of 3.8k (500/300 cache read/creation) — silent data loss rather than a crash, since both already used `?? 0` / `\|\| 0` guards. |
| An `ErrorBoundary` exists | **False.** Confirmed by repo-wide grep and by the live crash — React's own console warning said "Consider adding an error boundary". |

**Changed:**

| File | Change |
| --- | --- |
| `apps/dashboard/src/services/api.ts` | `fetchTraceDetail()` now normalizes `token_usage` the same way `fetchAggregateStats()` already did (short backend names -> long frontend names). |
| `apps/dashboard/src/shell/InspectorPane.tsx` | Dropped its own duplicate inline fetch; now calls `fetchTraceDetail()` so the two can't drift apart again. |
| `apps/dashboard/src/components/SessionInspector.tsx` | Added the optional-chaining + short-name-fallback guard (same pattern already used elsewhere in that file) to all four token fields in Token Economics. Dead code today, but it's in the `tsc` build and one accidental import away from live. |
| `apps/dashboard/src/components/ErrorBoundary.tsx` | New. Class component, catches a render error in one panel, shows an inline message, leaves the rest of the shell running. |
| `apps/dashboard/src/shell/ExplorerShell.tsx` | Wrapped each top-level panel (session list, inspector, overview, coverage, archaeology, exports) in its own `ErrorBoundary` — one panel's crash can't take its neighbor down. |

**How it was verified:** a mock `/api/*` server returning only short-form
token fields (matching the real Rust server's serialization), `vite dev`
against it, driven through an actual browser — click a session, open Exports,
mount `SessionInspector` directly. Confirmed the crash and the data loss
before the fix, confirmed both gone after. Separately forced a real render
error inside a wrapped panel and confirmed the inline fallback shows and
sibling panels keep working. `npm run build` (`tsc && vite build`, `strict` +
`noUnusedLocals` on) passes clean.
