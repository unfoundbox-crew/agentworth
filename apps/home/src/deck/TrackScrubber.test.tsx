import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import type { Stop, TimelineMoment } from '../protocol';
import { momentToEvent } from '../model/trackTimeline';
import { TrackScrubber } from './TrackScrubber';

const T0 = '2026-09-23T10:00:00Z';

function t(sec: number): string {
  return new Date(Date.parse(T0) + sec * 1000).toISOString();
}

function moment(seq: number, kind: TimelineMoment['kind'], text: string, at: string): TimelineMoment {
  return { seq, kind, text, at };
}

function stop(id: string, at: string, rung: Stop['rung'] = 'test'): Stop {
  return { id, directionId: 'd1', from: 'sen', rung, artifactId: null, at };
}

function events(ms: TimelineMoment[]) {
  return ms.map(momentToEvent);
}

function base(stops: Stop[] = [], truncated = false) {
  return { stops, truncated, onScrub: () => {}, onLive: () => {} };
}

describe('TrackScrubber (pure view layer)', () => {
  it('renders the scrub surface with slider semantics when moments exist', () => {
    const html = renderToStaticMarkup(
      <TrackScrubber
        events={events([moment(1, 'speech', 'a', t(0)), moment(2, 'work', 'b', t(10))])}
        scrub={null}
        {...base()}
      />,
    );
    expect(html).toContain('data-track="scrubber"');
    expect(html).toContain('role="slider"');
    expect(html).toContain('aria-valuemax="1"');
    expect(html).toContain('aria-label="session timeline scrubber"');
    expect(html).toContain('>now</span>');
  });

  it('renders no marks when the timeline has no handoffs or errors (honest degrade)', () => {
    const html = renderToStaticMarkup(
      <TrackScrubber
        events={events([moment(1, 'speech', 'a', t(0)), moment(2, 'work', 'b', t(10))])}
        scrub={null}
        {...base()}
      />,
    );
    expect(html).not.toContain('data-marker="handoff"');
    expect(html).not.toContain('data-marker="fork"');
  });

  it('renders a handoff arrow only where the feed reports a handoff', () => {
    const html = renderToStaticMarkup(
      <TrackScrubber
        events={events([
          moment(1, 'speech', 'a', t(0)),
          moment(2, 'handoff', 'opus → fable', t(10)),
        ])}
        scrub={null}
        {...base()}
      />,
    );
    expect(html).toContain('data-marker="handoff"');
    expect(html).toContain('handoff · opus → fable');
    expect(html).not.toContain('data-marker="fork"');
  });

  it('renders fork marks with retried/open state from the synthetic error feed', () => {
    const html = renderToStaticMarkup(
      <TrackScrubber
        events={events([
          moment(1, 'error', 'boom', t(0)),
          moment(2, 'work', 'retry run', t(10)),
          moment(3, 'error', 'boom again', t(20)),
        ])}
        scrub={null}
        {...base()}
      />,
    );
    expect((html.match(/data-marker="fork"/g) ?? []).length).toBe(2);
    expect(html).toContain('data-state="retried"');
    expect(html).toContain('data-state="open"');
  });

  it('a scrubbed state shows the clock and the way back to live, plus the playhead', () => {
    const html = renderToStaticMarkup(
      <TrackScrubber
        events={events([moment(1, 'speech', 'a', t(0)), moment(2, 'work', 'b', t(10))])}
        scrub={0}
        {...base()}
      />,
    );
    expect(html).toContain('data-track="scrub-clock"');
    expect(html).toContain('data-track="live-button"');
    expect(html).toContain('data-track="playhead"');
    // Evidence earned after the playhead dims: none after t(0) here.
  });

  it('dims stops earned after the playhead and keeps earlier ones solid', () => {
    const stops = [stop('s1', t(5)), stop('s2', t(25))];
    const at = (scrub: number) =>
      renderToStaticMarkup(
        <TrackScrubber
          events={events([moment(1, 'speech', 'a', t(0)), moment(2, 'speech', 'b', t(30))])}
          scrub={scrub}
          {...base(stops)}
        />,
      );
    // playhead at t=0: both stops land after it
    expect((at(0).match(/opacity: ?0\.35/g) ?? []).length).toBe(2);
    // playhead at the last moment (t=30s): everything earned is behind it
    expect((at(1).match(/opacity: ?0\.35/g) ?? []).length).toBe(0);
    expect(at(0)).toContain('test · sen');
  });

  it('stops-only strip when the session has no timeline yet (paused agent)', () => {
    const html = renderToStaticMarkup(
      <TrackScrubber
        events={[]}
        scrub={null}
        {...base([stop('s1', t(5))])}
      />,
    );
    expect(html).toContain('no session timeline yet');
    expect(html).not.toContain('role="slider"');
  });

  it('says nothing happened when there is no timeline and no stops', () => {
    const html = renderToStaticMarkup(<TrackScrubber events={[]} scrub={null} {...base()} />);
    expect(html).toContain('no evidence yet');
  });

  it('notes a truncated timeline instead of silently losing the session head', () => {
    const html = renderToStaticMarkup(
      <TrackScrubber events={events([moment(1, 'speech', 'a', t(0))])} scrub={null} {...base([], true)} />,
    );
    expect(html).toContain('data-track="truncated-note"');
    expect(html).toContain('most recent of a long session');
  });
});
