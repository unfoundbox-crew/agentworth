# AgentWorth design language

The written spec. Cards live in [`packages/ui/design-system/`](../packages/ui/design-system/index.html) — start there for anything visual; this file is the rules behind them.

## Identity

AgentWorth is the purple one. Zinc neutrals, one violet accent, light default, three-state theming (System / Light / Dark). Never borrow MotionVector's black-and-white chrome or SpacePilot's gold/silver — same rigor, different palette.

| | MotionVector | SpacePilot | AgentWorth |
| --- | --- | --- | --- |
| Neutrals | zinc | metallic gray | zinc |
| Accent | violet (`#7c6bb3`) | gold + silver | violet (`#7c6bb3`) |
| Chrome | black/white, Claude-Code / Slack-like | metallic | zinc + one accent |
| Default theme | light | light | light |
| Motion source | studio motion-language.md | own | same tokens, this repo's own surfaces |
| Signature surface | film canvas | flight deck | the receipt |

The token names still carry an `--mv-*` prefix — that's the shared naming convention across products in this codebase, not a MotionVector import. Every value under that prefix in `packages/ui/tokens.css` is AgentWorth's own.

## The one violet rule

`--mv-accent` is the only accent color anywhere in the system. It carries selection, focus, links, and the receipt's total line — nothing else. Chart series, status, and CLI roles each get their own reserved tokens (see below) so a second color never has to borrow the accent's meaning.

| Theme | Accent | Contrast |
| --- | --- | --- |
| Light | `#7c6bb3` | `#ffffff` |
| Dark | `#a396d6` | `#0d0d0f` |

Dark gets a *lighter* violet, not the same hex — a saturated violet that reads on white goes muddy on true black.

## Theming — three states, one contract

Bare `:root` (light, the default) → `@media (prefers-color-scheme: dark)` guarded by `:root:not([data-theme="light"])` → `:root[data-theme="dark"]`, unguarded. A token never gets its only definition inside a media query or `[data-theme]` block. Every card in `design-system/` ships the visible toggle and demonstrates this live — see `foundations/theming.html`.

## Type

Geist (sans) and Geist Mono, loaded from Google Fonts in every app's own `index.html`. Mono carries eyebrows, labels, CLI/code blocks, and tabular numerals; sans carries everything else. Fallback stacks and weights: `foundations/fonts.html`.

## Glyph set — the CLI's hard boundary

`apps/cli/src/ui/mod.rs` enforces this with a test. Nothing outside it, ever:

| Set | Glyphs |
| --- | --- |
| ASCII | the full range |
| Box drawing | U+2500–257F |
| Block elements | U+2580–259F |
| Exactly five more | `● ○` (the evidence meter), `·`, `—`, `→` |

No emoji, anywhere, in any surface — CLI, dashboard, marketing site, this document. Number sections with mono eyebrows instead. Full color-role table: `diagrams/ascii-conventions.html`.

## Chart rules

1. **Sequential for magnitude, categorical for identity, semantic for state.** Never mix jobs on one palette.
2. **Categorical is a known gap.** The 8-slot `--mv-cat-*` palette was split per-theme in #93 but still fails the dataviz skill's validator on chroma floor and CVD separation for several slots (`charts/categorical-palette.html` has the full run). Until it's re-stepped: cap active series at 4 (`cat-1`..`cat-4`, the widest pairwise gaps) with direct labels, or drop to `:root[data-palette='mono']`, which already ships a validated zinc ramp.
3. **A rate never ships without its `n`.** Below a confidence floor (n<10 in this system), suppress the rate rather than show it with false confidence — `charts/outcome-rate-table.html`.
4. **A share that can't be measured renders nothing, not a false zero** — `charts/cache-warmth-meter.html`'s real component behavior.
5. **Legend for 2+ series, direct labels too at ≤4.** No chart hides identity behind color alone.

## Motion grammar

Same duration/easing tokens as the MotionVector studio's approved motion-language (`--motion-fast/base/slow/slower/exit`, `--ease-out`, `--ease-in-out`) — this repo's own copy in `packages/ui/tokens.css`.

| Rule | Value |
| --- | --- |
| Entrances travel | fade + 10px rise (`--mv-rise`), `--motion-slow` |
| Exits don't | fade only, in place, `--motion-exit` (120ms — as fast as `--motion-fast`, on purpose) |
| Reduced motion zeroes distance, keeps time | strips `--mv-rise`, `--motion-enter-scale`, `--stagger-step` — duration is untouched, so a crossfade still happens |
| Never `ease-in` on UI | it holds back the exact frame the user is watching for |
| Bounce is a license, not a default | reserved for the patch-card / modal-arrival class of surface, not buttons |

Two signature entrances live only in this product: the receipt printing its line items top to bottom, and a determinate scan bar filling left to right. Both are demoed with a replay button in `motion/motion-rules.html`.

## Components, icons, diagrams

Full inventory and usage rules are on the cards, not duplicated here — see the index. Two things worth stating once:

- **Two icon grids, never mixed on one screen**: the marketing set (20×20, round caps, `packages/ui/icons.tsx`) and the dashboard set (24×24, square caps, `apps/dashboard/src/shell/dsIcons.tsx`) are different constructions on purpose.
- **Diagrams**: solid line = built, dashed accent line = proposed — the one convention that separates fact from plan, in both the CLI's ASCII output and the SVG diagrams used in docs and the blog.

## Archie

Settled 2026-09-02: the D2 hound, redrawn the same day as **M3** — the jaw is merged
into the head outline, so the head is one round shape with a nose dot near the bottom,
and the light is a torch he carries in a front paw rather than a lamp strapped to his
head. Poses, colourways, motion and the terminal short form live in
`packages/ui/brand/archie/` — read that README before drawing him anywhere. Eight rules
from the placement board:

- **Where he belongs**: the ⌘K empty state, the scan line, the 404, the dashboard's kit
  picker, and `agentworth.dev/archie`. Nowhere else.
- **Where he never goes**: the landing hero, the receipt, `--json` or `--quiet` output.
  He is a state, not a texture.
- **Once per screen** — two Archies are two states and one of them is lying. The one
  exception is `agentworth.dev/archie`, where he is the subject rather than the
  furniture and the page is the kit.
- **He arrives bare.** The default accessory is `none` (Saurabh, 2026-09-02); the lamp
  and the goggles are optional costumes inside `#accessory`, not how he shows up. The
  torch is not one of them — it lives in `#base`, in his paw, in every pose.
- **Under 40px he swaps to the lit dot.** The torch is unreadable at that size, so the
  SVG root takes `data-size="small"`: the torch hides, a lit disc and halo show at the
  paw, and the nose grows so it cannot read as an open mouth. `Archie.tsx` sets it from
  the `size` prop; anything rendering the files directly sets it itself.
- **The terminal form is nine columns, not seven** — `,---.` / `( o o )` / `-*  '._.'`,
  with the torch glyph at the paw as the state. Cost, accepted on purpose: the beam dash
  sits in front of it, so "nothing" prints `--` and "off" prints `-.`.
- **C3 is the default** colourway and the only one that ships on the site. C4 is for
  dense chrome, where he must not out-shout the data.
- **Inline the SVG, never an `<img>`**: the colourways are CSS custom properties, and a
  document's custom properties do not reach inside an image. `packages/ui/Archie.tsx`
  is the React wrapper; the SVG files stay the only place the drawing exists.

## Open items

- Categorical palette re-step (chart rule 2, above) — tracked here until it's fixed in `tokens.css`, which this bundle doesn't own.
