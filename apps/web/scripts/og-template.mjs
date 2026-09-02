// Per-post social cards, 1200x630, rendered with the same Geist Mono and the
// same brand lockup as public/og.svg.
//
// The lockup is not re-typed here — it is lifted out of og.svg at build time,
// so the mark and wordmark have exactly one source on disk. If og.svg's
// structure changes, this throws rather than silently drawing a card with no
// logo on it.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import { Resvg } from '@resvg/resvg-js';
import { webRoot, escapeXml } from './content.mjs';

const INK = '#111113';
const ACCENT = '#7c6bb3';
const MUTED = '#52525b';
const FAINT = '#e4e4e7';
const MONO = 'Geist Mono, ui-monospace, SF Mono, Menlo, monospace';

const fontDir = path.join(webRoot, 'node_modules/geist/dist/fonts/geist-mono');
const FONT_FILES = [
  path.join(fontDir, 'GeistMono-Regular.ttf'),
  path.join(fontDir, 'GeistMono-Bold.ttf'),
];

/** The two <g> blocks in og.svg: the brand mark, then the wordmark. */
export function readLockup() {
  const svg = readFileSync(path.join(webRoot, 'public/og.svg'), 'utf8');
  const groups = svg.match(/<g[^>]*transform="translate\([^"]*\)[^"]*"[^>]*>[\s\S]*?<\/g>/g);
  if (!groups || groups.length < 2) {
    throw new Error('og.svg: could not find the brand mark and wordmark groups');
  }
  return groups.slice(0, 2).join('\n  ');
}

const wrap = (title, perLine) => {
  const lines = [];
  let line = '';
  for (const word of title.split(/\s+/)) {
    const next = line ? `${line} ${word}` : word;
    if (next.length > perLine && line) {
      lines.push(line);
      line = word;
    } else {
      line = next;
    }
  }
  if (line) lines.push(line);
  return lines;
};

/**
 * Geist Mono is monospace, so line length is arithmetic rather than a
 * measurement: 0.62em per character, measured against the rendered card
 * rather than taken from the nominal 0.6 advance, which overflowed.
 *
 * Steps the size down until the title fits three lines, then narrows the
 * measure until the last line is not a lone orphan word — "…now means exit"
 * over "0" is worse than two balanced lines.
 */
function layoutTitle(title, maxWidth = 1040) {
  for (const size of [58, 52, 46, 40, 36]) {
    const perLine = Math.floor(maxWidth / (size * 0.62));
    let lines = wrap(title, perLine);
    if (lines.length > 3) continue;

    for (let narrow = perLine; narrow > perLine * 0.6; narrow--) {
      const candidate = wrap(title, narrow);
      if (candidate.length > lines.length) break;
      const last = candidate[candidate.length - 1];
      lines = candidate;
      if (candidate.length === 1 || last.length >= narrow * 0.45) break;
    }
    return { size, lines };
  }
  return { size: 36, lines: wrap(title, Math.floor(maxWidth / (36 * 0.62))).slice(0, 3) };
}

export function postCardSvg({ title, kicker, footer }, lockup) {
  const { size, lines } = layoutTitle(title);
  const lineHeight = Math.round(size * 1.18);
  // Bottom-anchored block, so one-line and three-line titles both sit on the
  // same baseline above the rule instead of drifting up the card.
  const firstBaseline = 452 - 34 - (lines.length - 1) * lineHeight;

  const titleTspans = lines
    .map(
      (l, i) =>
        `<text x="80" y="${firstBaseline + i * lineHeight}" font-family="${MONO}" ` +
        `font-size="${size}" font-weight="700" letter-spacing="-1.2" fill="${INK}">` +
        `${escapeXml(l)}</text>`
    )
    .join('\n  ');

  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1200 630" width="1200" height="630" role="img" aria-label="${escapeXml(title)}">
  <rect width="1200" height="630" fill="#ffffff"/>
  <rect x="0" y="0" width="1200" height="10" fill="${INK}"/>
  ${lockup}
  <text x="80" y="252" font-family="${MONO}" font-size="21" letter-spacing="2" fill="${ACCENT}">${escapeXml(
    kicker.toUpperCase()
  )}</text>
  ${titleTspans}
  <rect x="80" y="500" width="1040" height="1.5" fill="${FAINT}"/>
  <text x="80" y="556" font-family="${MONO}" font-size="21" fill="${MUTED}">${escapeXml(
    footer
  )}</text>
  <text x="1120" y="556" font-family="${MONO}" font-size="21" text-anchor="end" fill="${ACCENT}" font-weight="700">agentworth.dev</text>
</svg>`;
}

export function renderSvgToPng(svg, outPath) {
  const resvg = new Resvg(svg, {
    fitTo: { mode: 'original' },
    font: {
      fontFiles: FONT_FILES,
      loadSystemFonts: false,
      defaultFontFamily: 'Geist Mono',
    },
  });
  const png = resvg.render();
  if (png.width !== 1200 || png.height !== 630) {
    throw new Error(`${outPath}: expected 1200x630, got ${png.width}x${png.height}`);
  }
  mkdirSync(path.dirname(outPath), { recursive: true });
  writeFileSync(outPath, png.asPng());
  return { width: png.width, height: png.height };
}
