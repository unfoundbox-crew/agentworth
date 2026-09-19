import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import type { Artifact, Direction, Stop } from '../protocol';
import { buildSessionReview, type SessionReviewData } from '../model/sessionReview';
import { SessionReview } from './SessionReview';

const DIRECTION: Direction = {
  id: 'd1',
  goal: 'ship the thing',
  area: '/repo/proj',
  done: 'commit',
  reached: 'test',
  budgetTokens: 5_000_000,
  spentTokens: 1_200_000,
  riders: ['sen'],
  state: 'riding',
  exception: null,
  createdAt: '2026-09-07T09:10:00Z',
  updatedAt: '2026-09-07T11:46:00Z',
};

const STOPS: Stop[] = [
  { id: 's1', directionId: 'd1', from: 'sen', rung: 'artifact', artifactId: 'a-diff-1', at: '2026-09-07T10:00:00Z' },
  { id: 's2', directionId: 'd1', from: 'sen', rung: 'test', artifactId: 'a-test-log', at: '2026-09-07T11:00:00Z' },
];

const ARTIFACTS: Record<string, Artifact> = {
  'a-diff-1': {
    id: 'a-diff-1', spaceId: 'office-sen', from: 'sen', kind: 'diff',
    title: 'crates/adapters/claude.rs', ref: 'crates/adapters/claude.rs',
    body: '@@ -1,2 +1,3 @@\n context\n-old line\n+new line\n+added line\n',
    at: '2026-09-07T10:00:00Z',
  },
  'a-diff-2': {
    id: 'a-diff-2', spaceId: 'office-sen', from: 'sen', kind: 'file',
    title: 'apps/home/src/deck/SessionReview.tsx', ref: 'apps/home/src/deck/SessionReview.tsx',
    body: '@@ -10,2 +10,2 @@\n-before\n+after\n',
    at: '2026-09-07T10:30:00Z',
  },
  'a-cmd': {
    id: 'a-cmd', spaceId: 'office-sen', from: 'sen', kind: 'command',
    title: 'cargo test', ref: 'cargo test -p agentworth-home',
    body: 'ok\n',
    at: '2026-09-07T11:00:00Z',
  },
  'a-test-log': {
    id: 'a-test-log', spaceId: 'office-sen', from: 'sen', kind: 'command',
    title: 'cargo test -p agentworth-home', ref: 'cargo test -p agentworth-home',
    body: 'test result: ok. 12 passed\n',
    at: '2026-09-07T11:00:00Z',
  },
};

function review(): SessionReviewData {
  return buildSessionReview(DIRECTION, STOPS, ARTIFACTS);
}

describe('session review', () => {
  it('lists every file the session changed, and nothing else', () => {
    const html = renderToStaticMarkup(<SessionReview review={review()} />);
    expect(html).toContain('crates/adapters/claude.rs');
    expect(html).toContain('apps/home/src/deck/SessionReview.tsx');
    expect(html).not.toContain('cargo test -p agentworth-home</button>');
  });

  it('carries proof on every file: outcome rung plus test citation', () => {
    const html = renderToStaticMarkup(<SessionReview review={review()} />);
    const badges = html.match(/test ok/g) ?? [];
    expect(badges.length).toBeGreaterThanOrEqual(2);
    expect(html).toContain('cargo test -p agentworth-home');
  });

  it('shows the selected file diff body', () => {
    const html = renderToStaticMarkup(
      <SessionReview review={review()} selectedPath="apps/home/src/deck/SessionReview.tsx" />,
    );
    expect(html).toContain('-before');
    expect(html).toContain('+after');
  });

  it('states honestly when the session changed no files', () => {
    const empty = buildSessionReview(DIRECTION, [], {});
    const html = renderToStaticMarkup(<SessionReview review={empty} />);
    expect(html).toMatch(/no files changed/i);
  });

  it('states honestly when no proof exists yet', () => {
    const unflown = buildSessionReview({ ...DIRECTION, reached: null }, [], ARTIFACTS);
    const html = renderToStaticMarkup(<SessionReview review={unflown} />);
    expect(html).toMatch(/no proof yet/i);
  });

  it('proof takes the strongest rung from stops or reached, whichever is higher', () => {
    const r = buildSessionReview(
      { ...DIRECTION, reached: 'artifact' },
      [...STOPS, { id: 's3', directionId: 'd1', from: 'sen', rung: 'commit', artifactId: null, at: '2026-09-07T12:00:00Z' }],
      ARTIFACTS,
    );
    expect(r.proof?.rung).toBe('commit');
  });

  it('infers added and deleted file status from the diff body', () => {
    const r = buildSessionReview(DIRECTION, STOPS, {
      added: {
        id: 'added', spaceId: 'office-sen', from: 'sen', kind: 'diff',
        title: 'new.md', ref: 'new.md', body: '--- /dev/null\n+++ b/new.md\n+hello\n',
        at: '2026-09-07T10:00:00Z',
      },
      deleted: {
        id: 'deleted', spaceId: 'office-sen', from: 'sen', kind: 'diff',
        title: 'old.md', ref: 'old.md', body: '--- a/old.md\n+++ /dev/null\n-bye\n',
        at: '2026-09-07T10:00:00Z',
      },
    });
    expect(r.files.find((f) => f.path === 'new.md')?.status).toBe('added');
    expect(r.files.find((f) => f.path === 'deleted' || f.path === 'old.md')?.status).toBe('deleted');
  });
});
