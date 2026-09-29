import { describe, expect, it } from 'vitest';
import type { Message, Stop, TimelineMoment } from '../protocol';
import {
  TRACK_POS_MAX,
  TRACK_POS_MIN,
  buildTrackEvents,
  forkState,
  fracToIndex,
  latestStopBefore,
  messageToTrackEvent,
  momentToEvent,
  positionsByTime,
  rewoundEvents,
  rewoundStops,
  stepIndex,
} from './trackTimeline';

const T0 = '2026-09-23T10:00:00Z';

function t(sec: number): string {
  return new Date(Date.parse(T0) + sec * 1000).toISOString();
}

function moment(seq: number, kind: TimelineMoment['kind'], text: string, at: string): TimelineMoment {
  return { seq, kind, text, at };
}

function message(text: string, at: string, kind: Message['kind'] = 'speech'): Message {
  return { id: `m-${at}-${text}`, spaceId: 'office-sen', from: 'sen', kind, text, at };
}

function stop(id: string, at: string, rung: Stop['rung'] = 'test'): Stop {
  return { id, directionId: 'd1', from: 'sen', rung, artifactId: null, at };
}

function event(seq: number, kind: TimelineMoment['kind'], text: string, at: string) {
  return momentToEvent(moment(seq, kind, text, at));
}

describe('positionsByTime', () => {
  it('maps events linearly between the 8% and 92% bounds, oldest first', () => {
    const pos = positionsByTime([
      event(1, 'speech', 'a', t(0)),
      event(2, 'work', 'b', t(50)),
      event(3, 'speech', 'c', t(100)),
    ]);
    expect(pos[0]).toBeCloseTo(TRACK_POS_MIN, 5);
    expect(pos[1]).toBeCloseTo((TRACK_POS_MIN + TRACK_POS_MAX) / 2, 5);
    expect(pos[2]).toBeCloseTo(TRACK_POS_MAX, 5);
  });

  it('keeps a single event at the start bound', () => {
    expect(positionsByTime([event(1, 'speech', 'a', t(5))])).toEqual([TRACK_POS_MIN]);
  });

  it('is empty-safe', () => {
    expect(positionsByTime([])).toEqual([]);
  });

  it('stays monotonic across a 50k-moment session (huge-session math)', () => {
    const events = Array.from({ length: 50_000 }, (_, i) => event(i + 1, 'work', `w${i}`, t(i)));
    const pos = positionsByTime(events);
    expect(pos).toHaveLength(50_000);
    expect(pos[pos.length - 1]).toBe(TRACK_POS_MAX);
    for (let i = 1; i < pos.length; i++) {
      expect(pos[i]).toBeGreaterThanOrEqual(pos[i - 1]);
    }
  });
});

describe('fracToIndex (drag/snap)', () => {
  const events = [
    event(1, 'speech', 'a', t(0)),
    event(2, 'work', 'b', t(40)),
    event(3, 'speech', 'c', t(80)),
    event(4, 'handoff', 'opus → fable', t(120)),
  ];

  it('snaps to the nearest event in time', () => {
    expect(fracToIndex(events, 0.4)).toBe(1); // 0.4 of 120s = 48s -> 40s
    expect(fracToIndex(events, 1.2)).toBe(3); // past the end clamps
    expect(fracToIndex(events, -0.5)).toBe(0);
  });

  it('resolves an exact midpoint toward the later moment', () => {
    expect(fracToIndex(events, 0.5)).toBe(2); // 60s, tied between 40s and 80s
  });

  it('degrades on an empty timeline', () => {
    expect(fracToIndex([], 0.5)).toBe(0);
  });
});

describe('stepIndex (keyboard scrub)', () => {
  const events = [event(1, 'speech', 'a', t(0)), event(2, 'work', 'b', t(10))];

  it('steps one event and clamps at both ends', () => {
    expect(stepIndex(events, 0, 1)).toBe(1);
    expect(stepIndex(events, 1, 1)).toBe(1);
    expect(stepIndex(events, 1, -1)).toBe(0);
    expect(stepIndex(events, 0, -1)).toBe(0);
  });

  it('stays 0 on an empty timeline', () => {
    expect(stepIndex([], 0, 1)).toBe(0);
  });
});

describe('rewoundEvents (the track rewinds)', () => {
  const events = [
    event(1, 'speech', 'a', t(0)),
    event(2, 'error', 'boom', t(10)),
    event(3, 'work', 'retry', t(20)),
    event(4, 'speech', 'after', t(30)),
  ];

  it('slices to the inclusive prefix at the playhead', () => {
    expect(rewoundEvents(events, 1).map((e) => e.text)).toEqual(['a', 'boom']);
    expect(rewoundEvents(events, 3)).toEqual(events);
    expect(rewoundEvents(events, 99)).toHaveLength(4);
  });
});

describe('forkState (fork-on-retry)', () => {
  const events = [
    event(1, 'error', 'boom', t(10)),
    event(2, 'work', 'retry', t(20)),
    event(3, 'error', 'boom again', t(30)),
  ];

  it('an error followed by later moments is a retried fork', () => {
    expect(forkState(events, 0)).toBe('retried');
  });

  it('a trailing error is an open fork (retry pending)', () => {
    expect(forkState(events, 2)).toBe('open');
  });

  it('non-error moments are not forks', () => {
    expect(forkState(events, 1)).toBeNull();
    expect(forkState([], 0)).toBeNull();
  });
});

describe('stops under the playhead', () => {
  const stops = [stop('s1', t(5), 'artifact'), stop('s2', t(15), 'test'), stop('s3', t(25), 'commit')];

  it('rewoundStops slices to at-or-before the playhead', () => {
    expect(rewoundStops(stops, Date.parse(t(15))).map((s) => s.id)).toEqual(['s1', 's2']);
    expect(rewoundStops(stops, Date.parse(t(0)))).toEqual([]);
  });

  it('latestStopBefore is the last stop at or before, or null before any', () => {
    expect(latestStopBefore(stops, Date.parse(t(20)))?.id).toBe('s2');
    expect(latestStopBefore(stops, Date.parse(t(4)))).toBeNull();
  });
});

describe('buildTrackEvents (distilled moments + live tail)', () => {
  it('starts from the distilled moments in sequence order', () => {
    const events = buildTrackEvents(
      [moment(1, 'speech', 'a', t(5)), moment(2, 'handoff', 'opus → fable', t(10))],
      [],
    );
    expect(events.map((e) => e.kind)).toEqual(['speech', 'handoff']);
    expect(events[0].seq).toBe(1);
  });

  it('appends the live tail the timeline fetch could not have seen', () => {
    const events = buildTrackEvents([moment(1, 'speech', 'a', t(5))], [message('fresh', t(20))]);
    expect(events).toHaveLength(2);
    expect(events[1].seq).toBe(2); // one past the distilled max seq
    expect(events[1].text).toBe('fresh');
  });

  it('does not duplicate a live message the moments already carry', () => {
    const events = buildTrackEvents([moment(1, 'speech', 'a', t(5))], [message('a', t(5))]);
    expect(events).toHaveLength(1);
  });

  it('drops system messages: they are not moments', () => {
    const events = buildTrackEvents([], [message('notice', t(5), 'system')]);
    expect(events).toEqual([]);
    expect(messageToTrackEvent(message('notice', t(5), 'system'), 1)).toBeNull();
  });

  it('sorts merged events by time with a stable sequence tiebreak', () => {
    const events = buildTrackEvents(
      [moment(9, 'speech', 'late-distilled', t(30))],
      [message('early-live', t(10))],
    );
    expect(events.map((e) => e.text)).toEqual(['early-live', 'late-distilled']);
  });

  it('degrades honestly on an empty session: no events, nothing to scrub', () => {
    expect(buildTrackEvents([], [])).toEqual([]);
  });
});
