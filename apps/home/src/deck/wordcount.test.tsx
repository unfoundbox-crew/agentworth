import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { dispatch } from '../model/store';
import { plain } from '../model/theme';
import type { Direction, Harness, HomeEnv, ServerFrame } from '../protocol';
import { Cruise } from './Cruise';
import { Alert } from './Alert';
import { FirstRun } from './FirstRun';
import { FirstDirection } from './FirstDirection';
import { FirstRide } from './FirstRide';

const here = path.dirname(fileURLToPath(import.meta.url));
const fixture = JSON.parse(readFileSync(path.join(here, '../mock/fixture.json'), 'utf8'));

const HARNESSES: Harness[] = [
  { id: 'claude', label: 'Claude', bin: 'claude' },
  { id: 'codex', label: 'Codex', bin: 'codex' },
];

const ENV: HomeEnv = {
  cwd: '/repo',
  repo: '/repo',
  harnesses: HARNESSES,
  herdr: 'ok',
  budgetDefaultTokens: 5_000_000,
};

function hello(directions: Direction[]): ServerFrame {
  return { t: 'hello', protocol: 2, personas: fixture.personas, spaces: fixture.spaces, directions, env: ENV };
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

  it('first run stays under 30 visible words', () => {
    const html = renderToStaticMarkup(<FirstRun env={ENV} onSubmit={() => {}} />);
    const count = visibleWordCount(html);
    // eslint-disable-next-line no-console
    console.log(`first run visible words: ${count}`);
    expect(count).toBeLessThan(30);
  });

  it('first run without herdr stays under 30 visible words too', () => {
    const html = renderToStaticMarkup(<FirstRun env={{ ...ENV, herdr: 'missing' }} onSubmit={() => {}} />);
    const count = visibleWordCount(html);
    expect(count).toBeLessThan(30);
  });

  it('first direction stays under 40 visible words before the decision', () => {
    const html = renderToStaticMarkup(<FirstDirection goal="ship it" env={ENV} onPick={() => {}} />);
    const beforeDecision = html.split('who rides?')[0];
    const count = visibleWordCount(beforeDecision);
    // eslint-disable-next-line no-console
    console.log(`first direction visible words before the decision: ${count}`);
    expect(count).toBeLessThan(40);
  });

  it('first ride renders the seated rider with a working dock', () => {
    dispatch({ type: 'frame', frame: hello([]) });
    const direction: Direction = {
      id: 'd1',
      goal: 'make the scan under a second',
      area: '/repo',
      done: 'test',
      reached: null,
      budgetTokens: 5_000_000,
      spentTokens: 0,
      riders: ['claude-abcd1234'],
      state: 'riding',
      exception: null,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };
    const persona = fixture.personas[0];
    const html = renderToStaticMarkup(
      <FirstRide direction={direction} persona={persona} theme={plain} dockRef={{ current: null }} />,
    );
    expect(html).toContain('reading the repo');
    expect(html).toContain('riding');
  });
});
