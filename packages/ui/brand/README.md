# AgentWorth brand pack

One mark, derived from the tagline: the product is a receipt for work an agent
claims it did. `mark.svg` is that receipt — two neutral line items over one
heavier violet bar (the total, the number that got checked) with a four-tooth
torn bottom edge; `mark-mono.svg` is the same drawing in a single
`currentColor` with every bar knocked out of the body, so it reverses onto any
ground without a second file. `favicon.svg` sets the mark reversed out of an
ink rounded square (the app icon; use `mark-mono.svg` below 20 px, where the
container muddies). `wordmark.svg` is AGENTWORTH in Geist Mono converted to
outlines — AGENT at weight 500, WORTH at 700, tracked 0.08 em — so no font is
needed at render time. `lockup-h.svg` (default) and `lockup-v.svg` pair the two;
`og.svg` is the 1200×630 social card. `archie-idle.svg` and `archie-dig.svg` are
the mascot, built from the same receipt geometry — his torso *is* the mark.
**Clear space: leave free space equal to one line-item bar height — 1/8 of the
mark height — on every side of the mark or lockup, including against a page
edge.** Colours come straight from `packages/ui/tokens.css` (`--mv-ink`,
`--mv-accent`); each file carries a `prefers-color-scheme` block so it works on
white and on black unaided. Nothing here is wired into `apps/web` or
`apps/dashboard` yet — those still ship three different marks, which this pack
exists to replace.

Design canvas, with construction grids, size ladders, the CLI and motion
systems and Archie's expression and animation sheets:
<https://claude.ai/code/artifact/f7219b89-162f-4f64-8737-30009f66fb93>
