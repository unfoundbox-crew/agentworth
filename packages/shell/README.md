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

Supporting pieces owned here because the twins import them: `types.ts`
(minimal), `eventGroups.ts`, `timeAxis.ts`, `ladderIcons.tsx`, `formatters.ts`
(token/USD helpers for CacheCliff).

Import via the `@shell/*` alias (mirrors `@ui/*`). Do **not** delete
`TrackScrubber` / `VerdictBoard` / `FleetStrip` variants without a keep-winner.

## Still app-local (temporary forks)

`VerdictBoard`, `FleetStrip`, `CommandPalette`, and related rail chrome still
live under each app. Do not add further home copies of shell widgets —
extend this package instead.
