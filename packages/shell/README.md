# packages/shell

Shared archive/dashboard shell widgets extracted during app-merge P3
(`docs/specs/app-merge-p3-parity.md`).

## Authoritative modules (safe-delete twins)

| Module | Was |
| :--- | :--- |
| `TrajectoryScrubber.tsx` | Identical in `apps/dashboard/src/shell/` and `apps/home/src/archive/shell/` |
| `OutcomeLadder.tsx` | Same |
| `CacheCliffWidget.tsx` | Identical in both apps' `components/` |
| `ExportModal.tsx` | Near-identical; apps inject `performClientSideRedaction` / `convertToAtif` |
| `VerdictBoard.tsx` | Byte-identical twins → `@shell/VerdictBoard` (keep-winner §4) |
| `FleetStrip.tsx` | Near-identical; **required** `onOpenSession`; co-located `useFleet.ts` |
| `useFleet.ts` | Polling hook for FleetStrip (was duplicated under each app's `hooks/`) |

Supporting pieces owned here because the twins import them: `types.ts`
(minimal), `eventGroups.ts`, `timeAxis.ts`, `ladderIcons.tsx`, `formatters.ts`
(token/USD helpers + `getAdapterBadge`).

Import via the `@shell/*` alias (mirrors `@ui/*`). Lucide is a peer dependency
resolved from each app's install (Vite + tsconfig path aliases — same pattern
as CacheCliff / ExportModal).

## Keep in the apps (do not delete / unify)

| Module | Why |
| :--- | :--- |
| `apps/home/src/deck/TrackScrubber.tsx` | Deck course track — different job from `TrajectoryScrubber` |
| `apps/home/src/deck/Ambient.tsx` | Deck ambient strip — not a FleetStrip twin |

`CommandPalette` and related rail chrome may still live under each app until a
later extract. Do not add further home copies of shell widgets — extend this
package instead.
