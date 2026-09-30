# packages/shell

Shared archive/dashboard shell widgets extracted during app-merge P3
(`docs/specs/app-merge-p3-parity.md`).

## Authoritative modules (safe-delete twins)

| Module | Was |
| :--- | :--- |
| `TrajectoryScrubber.tsx` | Identical in `apps/dashboard/src/shell/` and `apps/home/src/archive/shell/` |
| `OutcomeLadder.tsx` | Same |

Supporting pieces owned here because the twins import them: `types.ts`
(minimal), `eventGroups.ts`, `timeAxis.ts`, `ladderIcons.tsx`.

Import via the `@shell/*` alias (mirrors `@ui/*`). Do **not** delete
`TrackScrubber` / `VerdictBoard` / `FleetStrip` variants without a keep-winner.

## Still app-local (temporary forks — extract next)

Overview / Exports / palette landed in `apps/home/src/archive/` for this
stack. Dashboard copies remain until a follow-up moves presentational panes
(`VerdictBoard`, `CacheCliffWidget`, `ExportModal`, …) here or deletes the
dashboard path at P4. Do not add further home copies of shell widgets —
extend this package instead.
