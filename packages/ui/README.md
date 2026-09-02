# packages/ui

Shared design tokens, brand assets, and React chrome components for `apps/web` and `apps/dashboard`, imported via the `@ui` alias.

| Path | What |
| --- | --- |
| `tokens.css` | The full token cascade — zinc neutrals, the violet accent, semantic, categorical, and motion tokens. Three-state theming (light `:root`, guarded dark media query, explicit `[data-theme="dark"]`). |
| `brand/` | SVG brand assets — mark, wordmark, lockups, favicon, OG card, Archie sketches. See `brand/README.md`. |
| `BrandMark.tsx` / `Wordmark.tsx` | React components inlining the brand SVGs with token-driven fills, so they never need a separate `-dark` file. |
| `icons.tsx` | The marketing/landing icon set — 20×20, round caps. |
| `ThemeToggle.tsx` / `useTheme.ts` | The System/Light/Dark control and its `localStorage`-backed hook. |
| `design-system/` | The full design system: preview cards for foundations, icons, motion, charts, diagrams, components, and composed pages. Open `design-system/index.html`. |

Written spec: [`docs/DESIGN.md`](../../docs/DESIGN.md) — identity, the one violet rule, theming contract, glyph set, chart rules, motion grammar, and how AgentWorth differs from MotionVector and SpacePilot.
