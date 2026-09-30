# App-merge P3 — parity / kill duplicates

Status: SPEC on main (#217). Implement in flight (#218 Coverage/Archaeology; stacked Overview/Exports/palette + packages/shell twins). SPEC for the next app-merge phase after P1
(#204) and P2 (#205). Implements the P3 row already locked in
`docs/specs/app-merge.md` (product call #212). **SPEC first; implement on a
follow-up branch/PR stack. Do not merge to main without COS / Saurabh
approval. P4 and P5 stay blocked.**

## Parent product call (do not re-open)

From `docs/specs/app-merge.md` / #212:

1. Archive stays a **phase** inside the home deck through P3 (`a` /
   `/home/s/<id>`).
2. Do **not** delete `apps/dashboard`.
3. Do **not** flip the serve root (`/` → home deck).
4. P4 (flip root / delete dashboard) and P5 stay blocked until P3 dogfood +
   explicit COS relaunch of P4.

This SPEC is only the P3 parity cut. Closed early-delete attempt: #182.

## Goal

Bring the dashboard-only rail surfaces into the home deck so the archive
phase is no longer "sessions view only," then remove **safe** duplicate
codepaths. After P3 dogfood, the deck is the place a person lives for
Overview / Coverage / Archaeology / Exports / palette **and** the archive
phase — while `apps/dashboard` remains in the tree until P4.

## In scope (COS lock)

### 1. Port these panes into the home deck

| Surface | Dashboard home today | Deck target |
| :--- | :--- | :--- |
| Overview | `apps/dashboard/src/shell/OverviewPane.tsx` (pulls `VerdictBoard`, `FleetStrip`, cache-cliff) | A deck-reachable Overview pane (rail or equivalent phase / route — pick one navigation model in the implement PR and document it in `apps/home/DESIGN.md` / archive header) |
| Coverage | `apps/dashboard/src/shell/CoveragePane.tsx` + `components/CoverageMatrix.tsx` | Same |
| Archaeology | `apps/dashboard/src/shell/ArchaeologyPane.tsx` + `components/ArchaeologyPanel.tsx` | Same |
| Exports | `apps/dashboard/src/shell/ExportsPane.tsx` (+ embedded `ExportModal`) | Same |
| Command palette | `apps/dashboard/src/shell/CommandPalette.tsx` | Deck-global palette; commands must reach the new deck panes and existing archive / insights routes |

P2 left an explicit gap — `apps/home/src/archive/Archive.tsx` and
`InspectorPane.tsx` state that rail / overview / coverage / archaeology /
exports / palette **stay on the dashboard until P3**. This SPEC clears that
debt. Empty-inspector copy that says "no OverviewPane until P3" must be
updated when Overview lands.

API clients already partially mirrored: `apps/home/src/archive/api.ts`
already knows `fetchArchaeology` / `fetchCoverageMatrix` types. Prefer
wiring those rather than inventing new endpoints.

### 2. Shared-package dedupe

After the panes work in the deck, collapse duplicated modules into a shared
package (likely under `packages/ui` or a small new `packages/` sibling —
implement PR chooses based on existing import graph; do not create a parallel
design system). Goal: **one** implementation imported by deck (and, until P4,
optionally still by dashboard), not two drifting copies.

### 3. Safe delete after cutover — identical twins only

Measured 2026-09-30 (MD5):

| File | `apps/dashboard/src/shell/` | `apps/home/src/archive/shell/` | Identical? |
| :--- | :--- | :--- | :--- |
| `TrajectoryScrubber.tsx` | yes | yes | **Yes** (same MD5) |
| `OutcomeLadder.tsx` | yes | yes | **Yes** (same MD5) |

**Allowed after deck cutover + shared import:** delete one of the twin copies
(or both, once the shared package owns them) for **only** these two files,
once the deck (and dashboard, while it still embeds them) import the shared
module and tests pass.

### 4. Do **not** delete without a keep-winner

These exist as **variants**, not byte-identical twins. Leave them alone in
P3 unless a separate keep-winner decision is recorded:

| Component | Why it stays |
| :--- | :--- |
| `TrackScrubber` (`apps/home/src/deck/TrackScrubber.tsx`) | Deck track scrubber — different job from archive `TrajectoryScrubber` |
| `VerdictBoard` (`apps/dashboard/src/components/VerdictBoard.tsx`) | Overview building block; may move/share, but do not delete a "variant" blindly |
| `FleetStrip` (`apps/dashboard/src/shell/FleetStrip.tsx`) | Same — fleet strip vs deck ambient strip are not proven identical |

Porting Overview will **use** VerdictBoard / FleetStrip (or shared
extractions). Deleting the dashboard copies of those without an explicit
keep-winner is out of scope for P3.

## Success criteria

1. From the home deck (no legacy dashboard window required for daily use of
   these surfaces), a user can open **Overview**, **Coverage**,
   **Archaeology**, **Exports**, and the **command palette**, and each pane
   loads real API data (same contracts the dashboard uses today).
2. Archive phase still works (`a` / `/home/s/<id>`); product call #212
   unchanged.
3. Dual codepaths for the ported panes are either **deleted** or reduced to
   a **single shared package** import (dashboard may keep a thin re-export
   until P4).
4. `TrajectoryScrubber` + `OutcomeLadder` exist in at most one authoritative
   module after cutover (shared package or single app path).
5. `TrackScrubber` / `VerdictBoard` / `FleetStrip` variants are **not**
   deleted in this phase without a written keep-winner.
6. `apps/dashboard` remains in the tree and still builds; serve root
   unchanged.
7. Docs: this SPEC + `docs/specs/app-merge.md` phase table updated to
   "SPEC opened" / implement PR links; Archive.tsx header comment no longer
   says "until P3" once implement lands.
8. Ask-before-main: no merge without COS / Saurabh.

## Non-goals

- **P4:** delete `apps/dashboard`, flip `/` to the home deck.
- **P5:** voice (#144), promote-to-default, Show HN, dsh.
- Visual redesign of the deck or a new design system.
- Pi / OpenAI adapter work, live-tail (#213), honesty matrix (#215) — other
  trains.
- Deleting `TrackScrubber` / `VerdictBoard` / `FleetStrip` "because P3."
- Merging implement PRs to `main` without approval.

## Suggested implement stack (after this SPEC PR)

Prefer quality over speed. Suggested order for follow-up PRs (not this one):

| Step | Branch theme | Notes |
| :--- | :--- | :--- |
| A | Shared extract of identical `TrajectoryScrubber` + `OutcomeLadder` | Lowest risk; proves package wiring |
| B | Port Coverage + Archaeology (API already typed in archive) | Self-contained panes |
| C | Port Exports + Command palette | Palette must know new targets |
| D | Port Overview (VerdictBoard / FleetStrip — share or wrap, do not delete variants) | Highest product surface |
| E | Dedupe remaining dual imports; update Archive / DESIGN comments; dogfood checklist | Stop before P4 |

Each step is its own PR when possible. Do not batch a giant delete with the
first port.

## Dogfood checklist (before anyone asks for P4)

- [ ] Deck-only session: Overview / Coverage / Archaeology / Exports / palette
      all used once on a real index
- [ ] Archive phase still opens a session and scrubs trajectory
- [ ] Dashboard still serves for anyone who bookmarks it (until P4)
- [ ] No orphan imports / broken `rust-embed` asset build for either app

## References

- `docs/specs/app-merge.md` — locked product call (#212)
- `docs/specs/home.md` — deck product shape; app-merge note
- `apps/home/src/archive/Archive.tsx` — "until P3" comment to clear on implement
- `apps/dashboard/src/shell/{OverviewPane,CoveragePane,ArchaeologyPane,ExportsPane,CommandPalette,TrajectoryScrubber,OutcomeLadder,FleetStrip}.tsx`
- `apps/home/src/deck/TrackScrubber.tsx` — keep; not a twin of TrajectoryScrubber
- Closed early delete: #182
