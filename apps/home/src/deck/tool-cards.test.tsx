import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import type { Artifact, Direction, Message } from '../protocol';
import { deriveToolEntries, MAX_TOOLS_SHOWN, retryAvailability } from './tools';
import { ToolCards } from './ToolCards';

function msg(over: Partial<Message> = {}): Message {
  return {
    id: 'm1',
    spaceId: 'office-sen',
    from: 'sen',
    kind: 'work',
    text: 'ran 1 command',
    at: '2026-09-07T10:08:00Z',
    artifacts: ['a1'],
    ...over,
  };
}

function artifact(over: Partial<Artifact> = {}): Artifact {
  return {
    id: 'a1',
    spaceId: 'office-sen',
    from: 'sen',
    kind: 'command',
    title: 'cargo test',
    ref: 'cargo test',
    body: 'ok. 4 passed; 0 failed',
    at: '2026-09-07T10:05:00Z',
    ...over,
  };
}

function direction(over: Partial<Direction> = {}): Direction {
  return {
    id: 'd1',
    goal: 'ship it',
    area: '/repo',
    done: 'test',
    reached: null,
    budgetTokens: 5_000_000,
    spentTokens: 0,
    riders: ['sen'],
    state: 'riding',
    exception: null,
    createdAt: '2026-09-07T10:00:00Z',
    updatedAt: '2026-09-07T10:00:00Z',
    ...over,
  };
}

describe('tool cards (lane 2/3)', () => {
  it('renders every tool call in the timeline as a card', () => {
    const entries = deriveToolEntries([msg()], { a1: artifact() });
    expect(entries).toHaveLength(1);
    const html = renderToStaticMarkup(
      <ToolCards
        messages={[msg()]}
        artifacts={{ a1: artifact() }}
        direction={direction()}
        riderPresence="working"
        onRetry={() => {}}
      />,
    );
    expect(html).toContain('cargo test');
  });

  it('renders failures as error cards with message + retry', () => {
    const failed = artifact({ body: 'exit 1: cargo test FAILED: 1 failed, 3 passed\nassertion at foo.rs:12' });
    const html = renderToStaticMarkup(
      <ToolCards
        messages={[msg()]}
        artifacts={{ a1: failed }}
        direction={direction()}
        riderPresence="working"
        onRetry={() => {}}
      />,
    );
    expect(html).toContain('role="alert"');
    expect(html).toContain('FAILED');
    // retry affordance exists and is enabled (wired to onRetry / steer path)
    expect(html).toContain('retry');
  });

  it('caps long runs with a count summary at the stated N', () => {
    const arts: Record<string, Artifact> = {};
    const ids: string[] = [];
    for (let i = 0; i < MAX_TOOLS_SHOWN + 5; i++) {
      const id = `a${i}`;
      ids.push(id);
      arts[id] = artifact({ id, title: `cmd ${i}`, ref: `cmd ${i}` });
    }
    const html = renderToStaticMarkup(
      <ToolCards
        messages={[msg({ id: 'm-big', artifacts: ids })]}
        artifacts={arts}
        direction={direction()}
        riderPresence="working"
        onRetry={() => {}}
      />,
    );
    expect(html).toContain(`Showing ${MAX_TOOLS_SHOWN} of ${MAX_TOOLS_SHOWN + 5}`);
  });

  it('retry is visibly disabled with a reason when no retry path exists (never a dead button)', () => {
    // No direction selected: no steer path to wire to.
    const avail = retryAvailability(null, 'working');
    expect(avail.enabled).toBe(false);
    expect(avail.reason.length).toBeGreaterThan(0);

    const html = renderToStaticMarkup(
      <ToolCards
        messages={[msg()]}
        artifacts={{ a1: artifact({ body: 'exit 1: FAILED' }) }}
        direction={null}
        riderPresence="working"
        onRetry={() => {}}
      />,
    );
    expect(html).toContain('disabled');
    expect(html).toContain(avail.reason);

    // Blocked rider: steer is refused gateway-side, so retry must say so.
    const blocked = retryAvailability(direction(), 'blocked');
    expect(blocked.enabled).toBe(false);
    expect(blocked.reason).toMatch(/blocked/i);
  });

  it('enabled retry exposes a live button (wired to the steer path)', () => {
    expect(retryAvailability(direction(), 'working').enabled).toBe(true);
    const html = renderToStaticMarkup(
      <ToolCards
        messages={[msg()]}
        artifacts={{ a1: artifact({ body: 'exit 1: FAILED' }) }}
        direction={direction()}
        riderPresence="working"
        onRetry={() => {}}
      />,
    );
    // A live retry is a button WITHOUT the disabled attribute.
    expect(html).toContain('retry');
    expect(html).not.toContain('disabled');
    expect(html).not.toContain('select a direction to retry');
  });
});
