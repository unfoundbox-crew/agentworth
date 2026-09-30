# App-merge

Status: product call locked 2026-09-30. P1 and P2 have merged (#204, #205).
P3 is next when scheduled. **P4 and P5 are blocked** until after P3 dogfood
and an explicit COS relaunch of P4.

This file is the durable home for the merge of `apps/dashboard` into
`apps/home`. Code comments and PR bodies already name the phases; keep them
honest here so a later session does not re-open a closed product call.

## Product call (COS / Saurabh)

1. **Keep archive-as-phase through P3.** The archive (session list + inspector)
   stays a phase inside the home deck (`a` / `/home/s/<id>`), not a separate
   root app and not a reason to delete the dashboard early.
2. **Do not delete `apps/dashboard`.** It remains in the tree and in the binary
   until after P3 is dogfooded and COS relaunches P4.
3. **Do not flip the serve root** (`/` → home deck, drop legacy dashboard HTML)
   until that same relaunch. Closed attempt: PR #182 ("Remove legacy v0.1.27
   dashboard") — closed, not merged.
4. **P4 and P5 stay blocked** on that relaunch. Do not start flip-root, delete
   dashboard, or promote-as-default work under another name.

## Phases

| Phase | What | State |
| :--- | :--- | :--- |
| P1 | Path deep links inside the deck: `/home/insights`, `/home/s/<id>` | Merged — #204 |
| P2 | Archive phase in the deck: `SessionList` + `InspectorPane`, `a` key | Merged — #205 |
| P3 | Parity / kill duplicates: rail overview, coverage, archaeology, exports, command palette, shared-package dedupe. Archive remains a phase. Detail: `docs/specs/app-merge-p3-parity.md`. | SPEC proposed 2026-09-30; implement not started |
| P4 | Flip root / delete `apps/dashboard` | **Blocked** — needs P3 dogfood + COS relaunch |
| P5 | Follow-ons after P4 (e.g. voice / #144, promote-to-default paths) | **Blocked** on P4 |

Out of scope for any app-merge PR until COS says otherwise: merging the PR by
the author, push to `main`, Show HN, live-tail/matrix follow-ons that assume
the dashboard is gone.

## Where the pieces live today

| Surface | Path | Notes |
| :--- | :--- | :--- |
| Home deck | `apps/home` | Served at `/home/` via `archie serve --home` / `archie home` |
| Archive phase | `apps/home/src/archive/` | Port of dashboard sessions view; see `Archive.tsx` header |
| Legacy dashboard | `apps/dashboard` | Still embedded and served; still the home for rail / overview / coverage / archaeology / exports / palette until P3 |
| Design | `apps/home/DESIGN.md`, `docs/specs/home.md` | Product shape of the deck, not the merge plan |

## Why this call exists

P1/P2 deliberately left dashboard-only surfaces on `apps/dashboard` ("until
P3" in `apps/home/src/archive/Archive.tsx`). Deleting the dashboard or flipping
`/` before that parity lands would strand those surfaces or force a rushed
port. #182 tried the delete early and was closed. The archive phase is the
interim shape on purpose — keep it through P3.
