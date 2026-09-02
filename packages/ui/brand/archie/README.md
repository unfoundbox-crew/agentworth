# Archie

The mascot, approved 2026-09-02: the D2 hound with a headlamp on a strap. Canvas: https://claude.ai/code/artifact/5847effa-a997-45ce-b62e-1e3192486b76

Every pose SVG is layered: `#base` (the dog), `#accessory` with `#lamp` and `#goggles` (switched by `data-accessory`), fills driven by `--archie-ink`, `--archie-body`, `--archie-mass`, `--archie-accent`, `--archie-accent-2`. `archie.css` carries the four colourways (C3 is the default) and the dig, fetch, found and error keyframes. `-dark` siblings carry fixed dark fills for contexts that cannot use CSS variables. `archie-tui.txt` is the terminal short form: three lines, the lamp glyph is the state.

Where he appears: ⌘K empty state, the scan line, the 404, agentworth.dev/archie. Never on the landing hero, never in the receipt.
