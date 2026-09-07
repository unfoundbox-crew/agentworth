import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { dispatch } from '../model/store';
import type { Direction, ServerFrame } from '../protocol';
import { Cruise } from './Cruise';
import { Alert } from './Alert';

const here = path.dirname(fileURLToPath(import.meta.url));
const fixture = JSON.parse(readFileSync(path.join(here, '../mock/fixture.json'), 'utf8'));

function hello(directions: Direction[]): ServerFrame {
  return { t: 'hello', protocol: 2, personas: fixture.personas, spaces: fixture.spaces, directions };
}

/**
 * Strips tags and sr-only (visually hidden) content, then counts words -- what
 * someone actually reads on screen. A bare separator like "·" is punctuation,
 * not a word, so a token needs at least one letter or digit to count.
 */
function visibleWordCount(html: string): number {
  const withoutSrOnly = html.replace(/<[^>]*class="sr-only"[^>]*>[\s\S]*?<\/[^>]+>/g, ' ');
  const text = withoutSrOnly.replace(/<[^>]+>/g, ' ');
  return text
    .split(/\s+/)
    .map((w) => w.trim())
    .filter((w) => w.length > 0 && /[\p{L}\p{N}]/u.test(w)).length;
}

describe('word budgets', () => {
  it('cruise stays under 30 visible words', () => {
    const noExceptions = fixture.directions.map((d: Direction) => ({ ...d, exception: null }));
    dispatch({ type: 'frame', frame: hello(noExceptions) });
    const html = renderToStaticMarkup(<Cruise />);
    const count = visibleWordCount(html);
    // eslint-disable-next-line no-console
    console.log(`cruise visible words: ${count}`);
    expect(count).toBeLessThan(30);
  });

  it('alert stays under 40 visible words before the first action', () => {
    dispatch({ type: 'frame', frame: hello(fixture.directions) });
    const html = renderToStaticMarkup(<Alert />);
    const beforeActions = html.split('open the pane')[0];
    const count = visibleWordCount(beforeActions);
    // eslint-disable-next-line no-console
    console.log(`alert visible words before first action: ${count}`);
    expect(count).toBeLessThan(40);
  });
});
