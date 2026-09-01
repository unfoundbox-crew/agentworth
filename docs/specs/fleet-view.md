# Fleet view

Status: built, #41 (fleet strip) and the SSE live-tail endpoint (v0.1.10).
Originally written as a draft spec for someone implementing this in a fresh
session with no memory of how this doc came to exist.

## The problem

Nobody can currently look at AgentWorth and answer "what's running right
now, across everything on this machine." `OverviewPane.tsx` shows
`VerdictBoard` (aggregate outcome distribution) and `CacheCliffWidget` (a
cost simulation) — both are about the past. `SessionList.tsx` is a sorted,
filtered archive. Nothing in the dashboard today is about *now*.

This matters specifically for AgentWorth because cross-harness aggregation
is the product's one differentiator. `services/api.ts`'s
`GROUNDED_CAPABILITY_MATRIX` lists 21 adapters — Claude Code, Codex, Cursor,
Antigravity, Gemini CLI, and 16 more. Any single vendor's own UI shows you
its own runs. Nothing else on the machine can show you Claude Code and
Cursor and Codex sessions in one place, live. Today that capability exists
in the adapter layer and is invisible in the UI — the product's core claim
is asserted in `AGENTS.md`, not shown anywhere a user looks.

## Two versions

| | Cheap first cut | Real |
| --- | --- | --- |
| Signal | file mtime, inferred | file watcher event, observed |
| Backend | one field added to an existing response | a new subsystem (queued, not built) |
| Freshness | as fresh as the last poll (proposed: 30s) | near-instant |
| Certainty | "probably running" | "is running" |
| Ships | now | later |

### Cheap first cut: inference from mtime

A session file modified in the last **N minutes** is probably still being
written to, which probably means the agent is still running. N = 5 minutes,
proposed. Reasoning: adapters that stream events incrementally (Claude Code,
Codex) write close to real-time, so a genuinely active session should
produce at least one write inside any 5-minute window even accounting for a
slow tool call. Long enough to survive a 2-3 minute test run without
flickering to "not running"; short enough that a session that actually ended
20 minutes ago doesn't still show as live. This number is a guess, not a
measurement — see Open questions.

**This is inference, not observation**, and the UI has to say so, not just
imply it through a label. Concretely:

- The strip's header reads `RUNNING NOW · INFERRED FROM RECENT ACTIVITY`,
  mono, uppercase, `--mv-faint` — an eyebrow, not a claim of fact.
  ("Inferred," not "Live" — "Live" is a promise this version can't keep.)
- Every chip's border is **dashed**, not solid. `design.md`'s diagram
  convention is "a solid line means built and true, a dashed line means
  proposed or undecided" — the same idea applies directly here: solid
  borders elsewhere in this app (`.status-pill`, `.ladder-node`) mean
  checked facts; a dashed border on the fleet chip is the same visual
  vocabulary saying "not checked, inferred."
- Hovering a chip shows the exact fact behind the inference: "Session file
  modified 2m ago" — not "Active 2m ago." The chip states what was actually
  observed (a file write), and lets the user do the inferring.
- No streaming endpoint exists (`AGENTS.md`'s pipeline ends at "SQLite index
  → CLI / local UI / export"; the only mutation the dashboard triggers is
  `POST /api/scan`). This version cannot and does not claim otherwise.

**Required backend work.** `SessionSummary` (`crates/storage/src/lib.rs:70`)
has no mtime field today — only the full per-session `Provenance` object
(`apps/dashboard/src/types/index.ts:23`, fetched via `/api/traces/:id`) has
`mtime_epoch_secs`, and fetching every indexed session's full trace just to
check freshness is the wrong shape of query. The value already exists
server-side, though: the `sources` table (`crates/storage/src/lib.rs:220`)
has a `mtime INTEGER NOT NULL` column, populated on every scan for
incremental-fingerprinting purposes. The work is surfacing it, not computing
it new:

1. Add `source_mtime_epoch_secs: Option<i64>` to `SessionSummary`.
2. Join it in from `sources.mtime` wherever `list_sessions_filtered` /
   `row_to_session_summary` build the response (`crates/storage/src/lib.rs`,
   around `:683` and `:1059`).
3. Serialize it on `/api/traces` — no route signature change, `routes.rs:279`
   already returns `Vec<SessionSummary>`.

A `SessionOrderBy::ModifiedAtDesc` variant would make the frontend query
cleaner but isn't required: fetching a modest slice ordered by the existing
`StartedAtDesc` (say, the newest 50) and filtering client-side by
`source_mtime_epoch_secs` is sufficient, because a session that's actually
still running is with near-certainty also one of the more recently started
ones.

**Frontend polling.** No polling loop exists anywhere in this codebase today
— `useSessions` fetches once (plus on manual rescan); `liveTail` is a UI
toggle that InspectorPane currently renders as "Live tail — awaiting stream,
not yet wired" (`InspectorPane.tsx:142`) and does nothing else. The fleet
strip needs its own interval: poll a small `/api/traces` slice every 30s
while `OverviewPane` is mounted, paused via
`document.visibilitychange` when the tab isn't visible, with an immediate
refetch on regaining visibility. On a failed poll, keep showing the last
good data rather than blanking the strip, with a small `--mv-warn` note
("last updated 2m ago") if the most recent attempt failed — the same
resilience posture `useSessions` already uses for its own fetch failures,
applied to a recurring case instead of a one-shot one.

### Real: file watcher → SSE

Queued on the backend, not built. What it adds over the inference version:

- **Observation instead of inference.** A watcher sees an agent write the
  instant it happens; mtime-polling sees it up to 30 seconds late and can't
  distinguish "still running" from "wrote once and stopped."
- **The dashed border goes away.** Once the signal is actually observed, the
  chip can carry the same solid-border, `--mv-accent` treatment the rest of
  the app reserves for checked facts.
- **`liveTail`'s banner gets wired up.** The same stream this version needs
  is what `InspectorPane`'s placeholder is waiting for.
- **No polling waste.** The 30s interval and its request volume go away
  entirely; the fleet strip becomes push-driven.

This section is directional, not a build spec — there's no SSE endpoint in
this repo's API surface (`/api/traces`, `/api/stats`, `/api/traces/:id`,
`/api/matrix`, `/api/blame`, `/api/usage`, `/api/pacing`,
`/api/archaeology`, `POST /api/scan` is the complete list) and designing one
is out of scope for this doc.

## The forensics-vs-monitoring tension

AgentWorth's stated V0 question, verbatim from `AGENTS.md`, is "What useful
AI experience already exists on this machine?" — a question about
accumulated history, not present activity. The outcome hierarchy in the same
file ranks "agent says done" as the *weakest* evidence tier, below artifact
changes, test passes, commits, and CI. A session that's still running has,
almost by definition, produced the least verifiable evidence in the entire
index — nothing has landed yet. Putting "what's running now" on the homepage
would mean opening the product on its least-checked content, in a tool whose
whole premise is evidence-based grading.

There's also a concrete fact worth naming: `ExplorerShell.tsx` defaults
`activeView` to `'sessions'`, not `'overview'`
(`useState<RailViewId>('sessions')`, `ExplorerShell.tsx:37`). The archive —
`SessionList` — is already the actual homepage today. Making fleet view
*the* homepage wouldn't just mean growing it inside Overview; it would mean
also promoting Overview itself to be the default rail view, which is a
second, separate change nobody has asked for.

**I agree with keeping the fleet strip small, inside Overview, not the
homepage.** Reasoning above, plus: forensics wants density and history —
2,903 sessions is the actual asset, and burying that ranking under three
"probably running" chips would read as AgentWorth deciding recency matters
more than evidence, which contradicts the outcome hierarchy it otherwise
enforces everywhere else.

One real tension I won't paper over: keeping the strip inside Overview means
a user has to navigate to the Overview tab to ever see it, which sits
against "make the differentiator visible instead of asserted" — if it's only
visible one click deep, it's still mostly asserted. I'm not resolving this
by deciding it myself; see Open questions.

## The design

```
┌ Rail ┬──────────────────────────────────────────────────────────┐
│      │ Overview                                                  │
│      │ ┌ RUNNING NOW · INFERRED FROM RECENT ACTIVITY ───────────┐│
│      │ │ ┌─────────┐ ┌─────────┐ ┌─────────┐                    ││
│      │ │ │ claude_  │ │ codex    │ │ cursor   │   (dashed border)││
│      │ │ │ code 2m  │ │ 4m ago   │ │ 1m ago   │                  ││
│      │ │ └─────────┘ └─────────┘ └─────────┘                    ││
│      │ └──────────────────────────────────────────────────────┘│
│      │ ┌ VerdictBoard (unchanged) ──────────────────────────────┐│
│      │ ┌ CacheCliffWidget (unchanged) ───────────────────────────┐│
└──────┴──────────────────────────────────────────────────────────┘
```

`OverviewPane` gets a new top section, `FleetStrip`, above the existing
`view-stack`. It needs two new props threaded down from `ExplorerShell` that
`OverviewPane` doesn't take today: a way to navigate to a session
(`navigate`, already owned by `useRoute()` in `ExplorerShell`) and a way to
switch the active rail view to `'sessions'` (`setActiveView`, same as
`CommandPalette`'s existing `onNavigateView` prop) — clicking a chip should
land the user in the session inspector, not leave them looking at Overview.

### States

| State | Trigger | What renders |
| --- | --- | --- |
| Loading | first poll in flight | skeleton chips, same recipe as `shell-list-skeleton` in `list.css` |
| Populated | ≥1 session with recent mtime | one chip per session, dashed border, adapter badge, relative time |
| Empty (indexed, none recent) | 0 sessions modified inside the window, but the index isn't empty | small muted line: "Nothing modified in the last 5 minutes." Strip still renders — it's a real, checked answer, not a loading gap. |
| Nothing indexed | `total_sessions === 0` | strip doesn't render at all — this is `SessionList`'s existing "no sessions indexed yet" situation, already handled there; the fleet strip has nothing distinct to add to it |
| Poll failed | fetch error on a refresh (not the first load) | keep last good chips, add a small `--mv-warn` "last updated Xm ago" note |

### Interactions

- Click a chip: `setActiveView('sessions')` then `navigate('/s/:id')` for
  that session, same URL shape `SessionList` already uses.
- Hover a chip: title/tooltip with the exact fact — "Session file modified
  2m ago" — and the full session id (chips truncate it).
- Cap at 8 chips in one row; beyond that, a trailing "+N more" chip that
  behaves like clicking through to the Sessions view pre-filtered to
  recently-active (this filter doesn't exist in `SessionList` today — treat
  the "+N more" chip as linking to the plain Sessions view for v1, and treat
  a real recency filter as a follow-on, not part of this spec).
- No custom keyboard handling — chips are plain buttons in DOM order, Tab
  reaches them normally. A handful of chips doesn't need roving-focus
  arrow-key navigation; if the fleet regularly grows past ~8 concurrent
  sessions that's worth revisiting.

### Design tokens

| Use | Token(s) |
| --- | --- |
| Strip header | `.eyebrow` recipe — `--font-mono`, uppercase, `--mv-faint` |
| Chip border (inferred, not observed) | dashed, `--mv-border` — matches `.status-pill.is-bad`'s existing dashed-border-for-uncertainty convention already in `index.css` |
| Chip background / hover | `--mv-surface` idle, `--mv-surface-2` on hover, same transition recipe as `.chip` (`--motion-fast`, `--ease-out`) |
| "Possibly active" indicator dot | `--mv-accent` — the one earned use of the accent on this strip; deliberately not `--mv-success` (already means "verified outcome" elsewhere, e.g. `.status-pill.is-good`, and "running" is not "verified good" — reusing green here would borrow evidence-tier meaning this state hasn't earned) |
| Adapter badge inside each chip | reuse `getAdapterBadge()` from `apps/dashboard/src/utils/formatters.ts` — same per-adapter styling `SessionList` already uses, not a new palette |
| Relative time ("2m ago") | reuse `formatTimeAgo()` from the same file |
| Poll-failed note | `--mv-warn` / `--mv-warn-soft` |
| Chip layout | `.tag-pill`-style padding/radius, horizontal flex row, gap per `index.css` conventions |

Motion: `design.md`'s reduced-motion rule zeroes `--mv-rise`,
`--motion-enter-scale`, and `--stagger-step` — all one-shot entrance
mechanics. An infinitely-looping "possibly active" pulse on the accent dot
is a different animal design.md doesn't cover (it only specs one-shot
entrances and fast, in-place exits). Extension made here, consistent with
that document's intent rather than contradicting it: under
`prefers-reduced-motion: reduce`, the pulse collapses to a static filled dot
— no infinite animation runs regardless of reduced-motion preference, same
spirit as reduced-motion elsewhere in the system, applied to a case the
existing rules didn't anticipate.

## Not in scope

- The SSE/file-watcher backend itself — described directionally above, not
  specified for implementation here.
- A recency filter chip/toggle in `SessionList` (the "+N more" overflow
  points at plain Sessions for v1).
- Any change to which rail view is the default (`'sessions'` stays the
  homepage).
- Multi-machine or remote fleet views — this is single-machine, matching
  AgentWorth's local-only design (`AGENTS.md`: "Never upload user data
  without explicit user action").

## How you'd know it worked

- Someone running Claude Code and Cursor at the same time can look at one
  screen and see both, without opening either tool.
- The chip's hover text states a real, checkable fact (a file mtime), not a
  claim of certainty the backend can't back up.
- 2,903 archived sessions are still what the product opens to; the fleet
  strip is additive, not a replacement front door.

## Decisions made here

- N = 5 minutes for the mtime-inference window (proposed default, not
  measured — see Open questions).
- Dashed borders + explicit "inferred" labeling for the cheap version;
  solid + accent reserved for once the real (observed) version ships.
- `--mv-accent`, not `--mv-success`, for the activity indicator, to avoid
  borrowing the outcome-ladder's "verified" meaning.
- Fleet strip lives inside Overview, not as the app's default view — agreeing
  with the position given in this doc's brief, with reasoning above.
- 30s poll interval, paused on tab hidden, for the cheap version's frontend.

## Open questions

- Is 5 minutes the right window? This needs to be checked against real
  adapter write cadence (how often Claude Code / Codex / Cursor actually
  append to their session files mid-turn), not assumed.
- Should the fleet strip be visible from every rail view (a persistent
  top-bar element), not only inside Overview? That would resolve the
  visibility-vs-homepage tension noted above without touching which view is
  default — worth a real decision rather than defaulting silently either
  way.
- Is 30 seconds the right poll interval, and should it back off if nothing's
  changed across several polls in a row?
- What should the "+N more" overflow chip actually do once there's budget to
  build a real recency filter into `SessionList`?

---

## Addendum: what the owner actually opens this for

Added 2026-09-01, after the spec above was written. This reframes the homepage
and it comes from the real problem rather than from a competitor's layout.

The daily question is not "what happened yesterday". It is three things, in
this order:

1. **What is running right now**, across every harness.
2. **What is it costing**, minimal by default and expandable into detail.
3. **What credits are left where**, so the next run can be routed to the
   provider that still has room.

Today that is done by hand, every day, by reading handoff files. That is the
thing to remove.

### Why this changes the ordering above

The spec above argues the fleet strip should stay small and sit inside
Overview, so the indexed history is not demoted for the three sessions running
now. That argument holds for a forensics tool. It does not hold if the first
question a user has is operational.

Both can be true: the fleet strip is small *and* first. Running sessions, then
spend, then credits — each collapsed to one line, each expanding into the
detail that already exists. The 2,903 indexed sessions stay one keystroke away
and remain the bulk of the product.

### The part that is genuinely new: credits

Nothing in this repo knows a provider balance. It cannot be derived from
session logs, because it does not live there — it lives behind each provider's
API.

**Decided 2026-09-01: AgentWorth reads these itself.** It would be odd to parse
a harness's logs and then outsource its quota to another tool. Scope is the
harnesses this project already adapts — Claude Code, Codex, Cursor, Antigravity,
OpenCode — not every provider in existence.

This is not the first outbound call. `agwt search` already downloads an
embedding model and `agwt blunder --submit` posts opt-in. Reading your own
balance with your own credential sends nothing about you; it is a fetch, like
the model download. Local-only is about your traces, and they still never
leave.

If the answer is yes, the shape that keeps the guarantee intact:

- Off by default. The tool works exactly as it does now until someone enables it.
- Keys read from the environment or from an existing credential store. Never
  stored by AgentWorth, never written to the index.
- Balance only. No usage reporting, no telemetry, nothing that describes what
  you ran.
- The provider list is explicit and visible, so it is obvious what is being
  called and what is not.
- It fails quietly. A provider that cannot be reached shows as unknown, not as
  an error that blocks the page.

If the answer is no, the honest fallback is much weaker but still real: show
spend per provider from the local index and let the human hold the budget. That
does not solve the routing problem, and the spec should say so rather than
pretend otherwise.

### Worth taking from Modal, adapted

Four ideas survive translation. The rest either already exist here or assume a
cloud product.

| Idea | Why it fits | Note |
| :--- | :--- | :--- |
| A sparkline per session row | Spot a stuck loop or a recovery spiral without opening the trace | Must be CSS or a tiny canvas, not an SVG per row — the list is virtualized across thousands |
| Context window growth against the model limit | Shows a session approaching its ceiling before it gets there | Token usage per event already exists |
| Cost ramp marking where caching engaged or failed | The cache cliff as a moment in time rather than a static widget | This is the strongest of the four |
| Spend over time by model family | Answers where the money went | Needs the daily rollup that `/api/usage` already returns |

Not taken: coloured emoji event markers, which break the no-emoji rule and
would spend colour on categories that form already distinguishes; and a fixed
"90% cache discount" figure, which is asserted rather than measured.

### Open, and needs a human

- Does AgentWorth make outbound calls to read provider balances? Yes or no.
  Everything else in this addendum follows from that one answer.
- If yes, which providers first. Routing only helps if it covers the ones
  actually in rotation.
