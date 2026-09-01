# Trajectory scrubber

Status: draft spec, not yet built. Written for someone implementing this in a
fresh session with no memory of how this doc came to exist.

## The problem

`apps/dashboard/src/shell/TimelineStrip.tsx` draws three rows of ticks
(messages, model, tools) across the width of the inspector. Bucket count comes
from measured container width, not event count:

```
PX_PER_TICK = 4
MIN_BUCKETS = 40
MAX_BUCKETS = 400
```

At the default 700px container width that's up to 175 buckets. A session
with 6,978 events — real, from the owner's own machine — spreads roughly 40
events into every bucket. Every bucket saturates its opacity range and the
strip reads as a near-solid bar. Above 5,000 events it's worse, and there's
no ceiling on session size: `AGENTS.md` assumes files can be multi-GB.

What a user cannot currently do:

- Tell a 20-minute stall from a burst of 40 tool calls. The strip shows
  "there was a lot," nothing about density, order, or timing.
- Click a specific moment. A tick already represents dozens of events; there
  is no way to get to one.
- See the shape of a long session at all — it's one texture, undifferentiated
  from start to end.

`TrajectoryView.tsx` already virtualizes the event list below the strip
(`@tanstack/react-virtual`, ~500-1000+ events assumed), so the list itself
scales. The strip is what's stuck at a fixed 700-ish pixels trying to
represent an unbounded number of events.

## The axis decision

Today the x-axis is event *sequence*, evenly spaced — one bucket-width per
equal slice of the event index, regardless of how much wall-clock time that
slice took.

**This should be time**, not sequence. `NormalizedEvent.timestamp` already
exists on every event (`apps/dashboard/src/types/index.ts:109`), so this is a
mapping change, not a new data requirement.

The case for time: gaps are the information sequence throws away. An agent
that stalls 20 minutes waiting on a human, or on a slow test run, should look
visibly different from one that fires 40 tool calls in ten seconds. On a
sequence axis those two situations render identically — same bucket width,
same tick spacing — because sequence has already discarded the very thing
that distinguishes them. A scrubber whose entire job is showing session
*shape* is not doing that job if it's blind to the one dimension that
actually varies.

The cost: time is not uniform density. A rapid tool-call loop can produce
dozens of events inside one second, which on a linear time axis compresses to
a sliver of pixels — harder to click precisely than the current evenly-spaced
ticks. Zoom (below) is what makes that cost tolerable: you zoom into the
sliver and it un-compresses.

**Decision: time only, not both.** A sequence/time toggle would mean two
bucketing code paths, two sets of edge cases, and a UI control to explain —
for a use case (browsing pure event order regardless of clock time) that
"zoom into the cluster, then scroll the filtered stream in order" already
covers. The one place sequence has to come back is the malformed-timestamp
fallback below, and that's a degrade path, not a user-facing toggle.

## The design

Two coupled controls plus the existing stream, replacing today's single
`TimelineStrip`:

```
┌────────────────────────────────────────────────────────────────┐
│ Overview rail (full session span, always unzoomed)              │
│ ░░░▓▓░░░░░▓▓▓▓▓░░░░░[■■■■■■■■■■■■■]░░░▓░░░░░░░░░░░░░░░░░░░░░░░░  │
│                      ^ drag body = pan, drag edges = zoom        │
├────────────────────────────────────────────────────────────────┤
│ Detail strip (zoomed window, three rows, full width)             │
│ Msgs   █ █    ██  █     █  ██                                    │
│ Model    ▓  ▓   ▓    ▓                                           │
│ Tools █ ██ █  █    █  ███   █                                    │
│        └──drag = brush-select a sub-range, filters stream──┘     │
├────────────────────────────────────────────────────────────────┤
│ Event stream (existing virtualized list) — shows the brushed     │
│ range if one is set, else the full zoomed window                 │
└────────────────────────────────────────────────────────────────┘
```

This is a two-part split — overview rail for navigation, detail strip for
selection — because "drag to pan" and "click-drag to select a range" are the
same physical gesture and need separate surfaces to not collide. The shape
(a small always-visible overview plus a time-range picker on the detail view)
is behaviorally the same idea as Modal's function-call histogram; nothing
about its visual language is reused here — colors, type, and spacing come
entirely from `~/code/design.md`.

### States

| State | Trigger | What renders |
| --- | --- | --- |
| Full range | default on session load | overview rail with no window highlight (window = full span); detail strip shows the whole session bucketed to fit |
| Zoomed | `+`, edge-drag, or window-drag on the overview rail | overview rail shows a highlighted window rect; detail strip re-buckets to just that window, ticks spread wider |
| Brushed | click-drag on the detail strip | an accent-tinted overlay spans the dragged sub-range across all three rows; stream filters to it |
| Selection outside window | an event gets selected (stream click, deep link, keyboard) that falls outside the current zoom | window auto-expands the minimum amount needed to include it, same instinct as the existing `virtualizer.scrollToIndex` behavior in `TrajectoryView.selectEvent` |
| Zoomed-empty | window (or brush) contains zero events | detail strip shows "No events in this range" + a "Reset zoom" action, distinct from the all-time empty state |
| No events | session has zero events | unchanged from today's `traj-strip-empty` |
| Timestamps unusable | any event's `timestamp` fails to parse, or events don't monotonically distinguish (adapter quirk — `AGENTS.md`: "treat agent formats as unstable," "malformed records should degrade gracefully") | fall back to today's sequence bucketing for the whole session, with a visible note: "Time data unavailable for this session — showing sequence order." Partial time axes (time for some events, sequence for others) are not offered — that would misrepresent gaps as real when they're actually missing data. |

### Interactions

**Overview rail**
- Drag the highlighted window body: pan.
- Drag its left/right edge: resize the window (zoom).
- Click outside the window: jump the window there, centered, same width.
- Double-click: reset to full range, clear any brush.

**Detail strip**
- Click a tick: select that event (unchanged from today — still scrolls the
  stream to it).
- Click-drag across ticks: draw a brush; releasing sets the stream filter to
  that sub-range. Does not change zoom level.
- Click without drag on empty strip background: clears an existing brush.
- An explicit "Clear range" chip appears next to the event count header
  whenever a brush is active, so clearing never depends on remembering the
  click-empty-space gesture.

**Zoom controls**
- `+` / `−` buttons in the detail strip header, zoom centered on the current
  window's midpoint (or the brush's midpoint, if one exists). Disabled at
  min/max zoom.
- `0` resets to full range.
- These are visible buttons, not keyboard-only — `+`/`−` also work as
  keyboard shortcuts when the scrubber has focus, but the buttons are the
  primary affordance per the brief.

**Keyboard** (only while the scrubber container has focus — a wrapping
`tabIndex={0}` element, matching the pattern `traj-expand` already uses for
its own focus ring)

| Key | Effect |
| --- | --- |
| `+` / `=` | zoom in, 2x, centered on window midpoint |
| `−` | zoom out, 2x |
| `0` | reset to full range |
| `←` / `→` | pan by 10% of window width |
| `Shift+←` / `Shift+→` | pan by one full window width |

`ArrowLeft`/`ArrowRight` are unclaimed at the shell level today —
`apps/dashboard/src/hooks/useShellKeys.ts` binds `j`/`ArrowDown`,
`k`/`ArrowUp`, `Enter`, `/`, and `Escape`, document-wide. The scrubber's own
`onKeyDown` handler must call `stopPropagation()` on the keys it consumes so
they don't also reach that document listener — it doesn't check focus
containment, only whether the target is a text input.

### Empty and error states

- No events at all: existing `traj-strip-empty` message, unchanged.
- Zoomed or brushed into a range with nothing in it: "No events in this
  range" + Reset action (new).
- Unusable timestamps: sequence-order fallback banner (new, described above).
- Stream and strip disagree (a defensive case — e.g. a brush references
  events that got filtered out elsewhere): the stream is the source of truth
  for what's actually shown; the strip's brush overlay should always be
  computed from the same event set the stream renders, never a separate copy.

### Design tokens

| Use | Token(s) |
| --- | --- |
| Row color (messages / model / tools) | `--mv-cat-1`, `--mv-cat-2`, `--mv-cat-3` — one hue per row, replacing today's single `--mv-ink` fill. Each row's label in the detail strip stays the swatch's color at low weight, matching the "add a legend when a diagram carries more than one meaning" convention. |
| Selected event, brush overlay, overview-rail window highlight | `--mv-accent` / `--mv-accent-soft` / `--mv-accent-border` — the one earned use of the accent here; nothing else in this component reaches for violet |
| Gap glyph (idle stretch with zero events) | `--mv-faint` fill, `--mv-border-soft` hairline |
| Panel chrome (strip backgrounds, rail track) | `--mv-surface`, `--mv-surface-2`, `--mv-border`, `--mv-border-soft` |
| Zoom-level readout, `+`/`−`/`0` buttons, time-range label | `--font-mono`, `font-variant-numeric: tabular-nums` (hard rule 5 — these are aligned digits) |
| Button/handle hover and press feedback | `--motion-fast` (120ms), `--ease-out` — same recipe as `.chip`/`.theme-toggle` in `index.css`, color/background transitions only, no transform |
| Focus rings | reuse the existing global `:focus-visible` recipe (`box-shadow` with `--mv-accent-soft` / `--mv-accent-border`) — no new focus style |

No entrance/exit animation is specified for this component — it's UI chrome
the user is already looking at when it changes, not content revealing on
scroll or an overlay opening. That also means it needs no
`prefers-reduced-motion` handling beyond what's already global, since nothing
here uses `--mv-rise`, `--motion-enter-scale`, or `--stagger-step`.

### Gap handling

A long idle stretch (an agent waiting on a human, or on a slow CI run)
rendered at literal proportional width would let one multi-hour gap
dominate the whole strip and squeeze every actual event into a few pixels.
Cap it: any zero-event stretch beyond a threshold renders at a fixed minimum
width with a distinct gap glyph (a dashed hairline in `--mv-faint`, not a
tick), rather than growing with wall-clock time. This keeps the gap visible
— which is the entire point of moving to a time axis — without letting it
eat the view. Exact threshold is a tunable, not decided here (see Open
questions).

## Not in scope

- A sequence-axis toggle. See axis decision above.
- Scroll-wheel zoom. Adds `preventDefault` fights with page scroll for a
  marginal gain over the `+`/`−`/drag controls already specified.
- Multiple simultaneous brushed ranges — one brush at a time.
- Persisting zoom/pan state when switching sessions. Every session opens at
  full range.
- Any behavior change while `liveTail` is on. Whether the window should
  follow new events as they arrive isn't decided here — see Open questions.

## How you'd know it worked

- Zooming a 6,978-event session's detail strip down to a few hundred events
  produces individually clickable ticks, not a saturated bar.
- A 20-minute stall is visible as a gap on the overview rail without opening
  the event list.
- Finding one specific event in a 5,000+ event session takes zoom + a click,
  not scrolling a virtualized list by hand.

## Decisions made here

- Time axis, not sequence, with a sequence fallback only for unusable
  timestamps.
- Pan lives on the overview rail; range-select (brush, filters the stream)
  lives on the detail strip — deliberately different surfaces, not a
  modifier key on the same gesture.
- Idle gaps get a minimum-width cap rather than literal proportional width.
- The three event-kind rows get distinct `--mv-cat-*` colors instead of one
  flat `--mv-ink` fill.

## Open questions

- Exact gap-collapse threshold and minimum zoom-window width. Both are
  product judgment calls that should be checked against real session data,
  not the numbers in this doc.
- Whether the zoomed window should auto-follow the tail when `liveTail` is on
  and new events arrive, versus holding the user's chosen window steady.
- Whether brush-select needs a keyboard path (e.g. `Shift+←`/`Shift+→` to
  extend a brush) or whether mouse-only is acceptable for v1.
- Copy for the zoomed-empty and unusable-timestamps messages needs a product
  pass — the wording above is placeholder.
