#!/usr/bin/env node
// Renders public/og.svg to public/og.png at 1200x630 with the real Geist
// Mono font (regular + bold, matching the weights the SVG actually uses) via
// resvg's native font loader. sharp was ruled out here: it rasterizes SVG
// text through the system's fontconfig, which falls back silently to a
// generic mono font when Geist Mono isn't installed as a system font.
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { Resvg } from '@resvg/resvg-js';

const here = path.dirname(fileURLToPath(import.meta.url));
const webRoot = path.resolve(here, '..');
const fontDir = path.join(webRoot, 'node_modules/geist/dist/fonts/geist-mono');
const svgPath = path.join(webRoot, 'public/og.svg');
const pngPath = path.join(webRoot, 'public/og.png');

const svg = readFileSync(svgPath, 'utf8');

const resvg = new Resvg(svg, {
  fitTo: { mode: 'original' },
  font: {
    fontFiles: [
      path.join(fontDir, 'GeistMono-Regular.ttf'),
      path.join(fontDir, 'GeistMono-Bold.ttf'),
    ],
    loadSystemFonts: false,
    defaultFontFamily: 'Geist Mono',
  },
});

const png = resvg.render();
writeFileSync(pngPath, png.asPng());

console.log(`Rendered ${pngPath} (${png.width}x${png.height})`);
if (png.width !== 1200 || png.height !== 630) {
  throw new Error(`Expected 1200x630, got ${png.width}x${png.height}`);
}
