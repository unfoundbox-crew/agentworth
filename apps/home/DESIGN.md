# home: design

Status: draft, 2026-09-07. This is agentworth's own design doc for `apps/home`.
It is not MotionVector's design system, and it is not the dashboard's.

## What it is for

One window to run a fleet of coding agents for a whole day without a terminal
grid. It replaces reading panes, not the agents. Every choice below is judged
by one test: is this easier on the eyes and the hands at hour six.

## The shape: the Train (locked 2026-09-07)

The primary unit is a **direction**, not a message. A direction is a standing
intent the human set once: goal in one line, the area of the repo it owns, the
evidence rung that means done, a token budget, and the personas riding it.
Riders inherit it. Editing it steers every rider. Prior art is the RTS rally
point; no agent tool has built one. Research behind this lives in
`docs/specs/home.md`.

```
┌ strips ──────────────────────────────────────────────────────────┬─┐
│ [waiting on you · 14 min] [over budget · halted]  ░ ░ ░ ░ ░      │a│
├ standup · GO GO GO NO-GO GO GO GO ────────────────────────────────┤m│
├ track ────────────────────────────────────────────────────── now ┤b│
│ lane per direction · riders as initials · stops at the rung     │i│
│ earned · handoff arrows · fork on retry · scrubber rewinds       │e│
├ dock ────────────────────────────────────────────────────────────┤n│
│ steer the selected direction   [now | after this step]  1–9 0   │t│
└──────────────────────────────────────────────────────────────────┴─┘
```

- **Strip board**: one compact strip per direction, fixed fields, freely
  arrangeable, like an air-traffic flight strip. Default view is exceptions
  only: strips that are fine are dim and small; strips waiting on you or
  halted are large, first, and sorted by how long they have waited.
- **Track**: time left to right, one lane per direction. Riders are small
  markers. Stops are evidence at the rung it earned: diff, test, commit, CI.
  A stop opens the evidence, never a transcript. A scrubber rewinds.
- **Dock**: one input. Steering is explicit about timing: now, or after the
  rider's current step. Number keys select directions; zero selects all.
- **Ambient strip**: the right edge hums with fleet activity and burn. It is
  the only thing that moves when nothing needs you.
- **Standup**: a go/no-go poll across directions, one word each.
- **Quiet state**: when nothing needs you, the middle of the screen is one
  line. That is the interface tax at rest.

What you read per decision: one strip and one evidence rung.

Personas are riders. Solo or mute a persona to focus or silence it. The
codebase Map is a lens on the same data. Rooms from earlier sketches survive
only as theme-pack vocabulary.

## Personas and themes

A persona is a seat with a functional role: chief of staff, senior,
associate, executor, controller. The app knows roles. A theme is a JSON file
that maps roles to names, titles, badge colours and a soul file. Components
never import a theme's characters. Switching themes is a data swap.

The chief of staff role has a voice rule that applies under every theme:
sharp, short, human. No tool calls, no code, no monologue. See
`src/themes/donna.md` for the shape a soul file takes.

## Colour

Dark by default. This is a night-and-day-long tool, and the owner's eyes
picked these values from the terminal they already live in.

| token | value | use |
| --- | --- | --- |
| `--ground` | `#15191f` | page |
| `--panel` | `#1a1f26` | sidebar, drawer, dock |
| `--line` | `#272f3a` | 1px hairlines, only |
| `--ink` | `#cfd4dc` | text |
| `--muted` | `#8b949e` | secondary text, timestamps |
| `--dim` | `#586069` | disabled, placeholders |

Colour is spent only on persona badges, one hue each, at low saturation, and
on the four presence states. Nothing else is coloured. No neon, no gradients,
no glow. Presence: idle is a hollow dot, working is a slow pulse, blocked is
a filled amber dot, done is a filled dot in the badge hue, unknown is dashed.

A light palette is a token swap, not a redesign. Not built yet.

## Type

One face: `--font-mono`, PT Mono or SF Mono, 12px body, 11px meta, 13px for
the prompt. Line height 1.6 on the floor. No bold in the stream; names carry
the badge hue instead.

## Motion

120Hz smooth. Scroll is native, never JS-driven. Entering elements fade over
120ms; leaving elements go in place over 80ms. `prefers-reduced-motion`
keeps timing and zeroes distance. Presence pulse is 2s, opacity only.

## Never

- Diffs in the stream.
- A colour that is not a badge or a presence.
- A theme name in a component.
- A message from the app about itself unless something broke.
