# home: design

Status: draft, 2026-09-07. This is agentworth's own design doc for `apps/home`.
It is not MotionVector's design system, and it is not the dashboard's.

## What it is for

One window to run a fleet of coding agents for a whole day without a terminal
grid. It replaces reading panes, not the agents. Every choice below is judged
by one test: is this easier on the eyes and the hands at hour six.

## Surfaces

```
┌──────────┬────────────────────────────────────┬──────────────┐
│ spaces   │ floor                              │ drawer       │
│          │                                    │              │
│ rooms    │ speech only. one bubble per turn.  │ artifacts:   │
│ offices  │ a persona's work collapses to one  │ diff, file,  │
│          │ line that opens the drawer.        │ command,     │
│          │                                    │ note, link.  │
│          ├────────────────────────────────────┤              │
│          │ dock: prompt, @mentions            │              │
└──────────┴────────────────────────────────────┴──────────────┘
```

- **Spaces**: rooms first, then offices. An office is one persona. A room is a
  cast. Each row: presence dot, name, one-line summary, unread count.
- **Floor**: the stream. Speech renders. Work renders as one line. Raw tool
  output never renders here. Nothing scrolls the user; new messages append
  below and a "new below" pill appears if they have scrolled up.
- **Drawer**: slides in from the right, never inline. Diffs live here.
- **Dock**: floats over the floor bottom. Enter sends, Shift+Enter newline,
  `@` completes persona names from the active theme.

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
