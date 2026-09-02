# Archie

The mascot, approved 2026-09-02: the D2 hound, redrawn 2026-09-02 with the **M3 head** and the torch moved off his head and into a front paw. Canvas: https://claude.ai/code/artifact/5847effa-a997-45ce-b62e-1e3192486b76

**M3** means the muzzle is not a separate shape. The head silhouette widens and drops into the jaw, so the whole head is one closed curve with a nose dot near the bottom — no second curve inside a closed curve to turn into a grey band under 32px. Ears, dot eyes, 2px round-joined ink, `viewBox 0 0 48 48`: unchanged.

**No head gear by default.** He arrives bare, carrying a hand-held torch (`#torch`, with `torch-body`, `torch-lens` and the animating `torch-beam`) in a front paw. The old headlamp and the goggles are still there as optional costumes inside `#accessory` (`#lamp`, `#goggles`), hidden until `data-accessory` says otherwise. Sleeping puts the torch on the ground beside him; error drops it, and both carry `data-lamp="off"`, which darkens the lens and hides the beam.

Every pose SVG is layered: `#base` (the dog and his torch), `#accessory` with `#lamp` and `#goggles`, fills driven by `--archie-ink`, `--archie-body`, `--archie-mass`, `--archie-accent`, `--archie-accent-2`. `archie.css` carries the four colourways (C3 is the default) and the dig, fetch, found and error keyframes. `-dark` siblings carry fixed dark fills for contexts that cannot use CSS variables.

## The size rule

Under **40px** the torch stops being a drawing. One SVG unit is half a pixel there, so its body is 2.2px tall with 0.8px of ink on each side, and what survives is a pale smear where the beam was. So the root SVG takes a `data-size` attribute:

| `data-size` | what changes |
| --- | --- |
| `full` (default) | the torch as drawn; the normal nose dot |
| `small` | `#torch` hides, a lit disc with a soft halo shows at the paw, and the nose grows (`rx 2.4` → `3.2`) so it cannot read as an open mouth |

`packages/ui/Archie.tsx` sets this for you: any `size` under `ARCHIE_SMALL_BELOW` (40) renders `data-size="small"`. Anything rendering the files directly — a favicon build, a static rasteriser — sets the attribute itself.

## The terminal

`archie-tui.txt` is the terminal short form: three lines, nine columns, the torch glyph at the paw is the state. `apps/cli/src/ui/mod.rs` is the implementation and the two must not drift. The one-line form stays `(*) archie`.

Where he appears: ⌘K empty state, the scan line, the 404, agentworth.dev/archie. Never on the landing hero, never in the receipt.
