# AgentWorth brand pack

One mark, derived from the tagline: the product is a receipt for work an agent
claims it did.

| file | what it is |
| --- | --- |
| `mark.svg` / `mark-dark.svg` | The receipt mark — two neutral line items over one heavier violet bar (the total, the number that got checked), four-tooth torn bottom edge. Light and dark background fills. |
| `mark-mono.svg` | Same drawing, single `currentColor` fill, every bar knocked out of the body — reverses onto any ground the host page sets `color` to, no second file needed. |
| `favicon.svg` | The mark, dark-on-transparent by default, reversing to light via `prefers-color-scheme` — the one file that follows the OS, because a browser tab follows the OS, not the page. |
| `wordmark.svg` / `wordmark-dark.svg` | AGENTWORTH in Geist Mono converted to outlines — AGENT at weight 500, WORTH at 700, tracked 0.08em. No font needed at render time. Light and dark background fills. |
| `lockup-h.svg` / `lockup-h-dark.svg` | Mark beside the wordmark (default lockup). Light and dark background fills. |
| `lockup-v.svg` / `lockup-v-dark.svg` | Mark stacked above the wordmark. Light and dark background fills. |
| `og.svg` | The 1200×630 social card — a fixed composition, explicit colours, no theme switching. |
| `archie-idle.svg` / `archie-idle-dark.svg` | Archie mascot, idle: eyes level, trowel held up, ready. Light and dark background fills. |
| `archie-dig.svg` / `archie-dig-dark.svg` | Archie mascot, digging: leaning in, trowel lowered, spoil thrown clear. Light and dark background fills. |

**Apps pick the `-dark` file under `[data-theme=dark]`; never rely on
`prefers-color-scheme` inside a brand asset except `favicon.svg`.** A brand
asset used via `<img>` or as a favicon does not see the host page's
`data-theme` — only the OS/browser scheme reaches it through
`prefers-color-scheme`. So every asset except the favicon ships as a fixed
light file plus a fixed `-dark` sibling, and the app's own theme logic swaps
which one it references. The favicon is the deliberate exception: a browser
tab genuinely follows the OS/browser chrome, not the page, so it keeps its
`prefers-color-scheme` media query.

**Clear space:** leave free space equal to one line-item bar height — 1/8 of
the mark height — on every side of the mark or lockup, including against a
page edge. Colours come straight from `packages/ui/tokens.css` (`--mv-ink`,
`--mv-accent`). Nothing here is wired into `apps/web` or `apps/dashboard` yet
— those still ship three different marks, which this pack exists to replace.

Design canvas, with construction grids, size ladders, the CLI and motion
systems and Archie's expression and animation sheets:
<https://claude.ai/code/artifact/f7219b89-162f-4f64-8737-30009f66fb93>
