import type { Message, Stop, TimelineMoment } from '../protocol';

/**
 * Pure scrub math for the course track's scrubber: the timeline model, its positions
 * along the track, drag/snap, and the rewound prefixes the track renders. View layer
 * never re-derives any of this.
 *
 * The timeline is built from two sources that arrive by different paths and must not
 * duplicate each other: `timeline` frames (the whole session, distilled server-side) and
 * live `message` frames (speech/work only) the deck receives while the timeline fetch is
 * already in flight. Merge rule: a live message whose `at|kind|text` key already exists
 * in the distilled moments is skipped; everything else appends after them.
 */

export const TRACK_POS_MIN = 8;
export const TRACK_POS_MAX = 92;

/** The same distillation kinds the backend ships; nothing else reaches the track. */
export type TrackEventKind = TimelineMoment['kind'];

export interface TrackEvent {
  /** The trace's own event sequence, so seek order is deterministic, not clock-dependent. */
  seq: number;
  at: string;
  atMs: number;
  kind: TrackEventKind;
  text: string;
  /** Stable dedupe id: one moment of speech/work is one event even if seen twice. */
  key: string;
}

export function trackEventKey(e: Pick<TrackEvent, 'at' | 'kind' | 'text'>): string {
  return `${e.at}|${e.kind}|${e.text}`;
}

function toEvent(seq: number, m: { at: string; kind: TrackEventKind; text: string }): TrackEvent {
  return { seq, at: m.at, atMs: Date.parse(m.at), kind: m.kind, text: m.text, key: trackEventKey(m) };
}

export function momentToEvent(m: TimelineMoment): TrackEvent {
  return toEvent(m.seq, m);
}

/**
 * The live-message twin of `momentToEvent`: kind maps straight over, and the seq sits
 * after every distilled moment so a scrub seek keeps the live tail ahead of history.
 * System lines are not moments, so they return null.
 */
export function messageToTrackEvent(m: Message, afterSeq: number): TrackEvent | null {
  if (m.kind !== 'speech' && m.kind !== 'work') return null;
  return toEvent(afterSeq, { at: m.at, kind: m.kind, text: m.text });
}

/**
 * The seekable timeline: distilled moments plus the live tail, deduped and sorted by
 * time (stable, so equal timestamps keep sequence order).
 */
export function buildTrackEvents(moments: TimelineMoment[], liveMessages: Message[]): TrackEvent[] {
  const out: TrackEvent[] = moments.map(momentToEvent);
  const seen = new Set(out.map((e) => e.key));
  const maxSeq = moments.reduce((acc, m) => Math.max(acc, m.seq), 0);
  let next = maxSeq + 1;
  for (const m of liveMessages) {
    const e = messageToTrackEvent(m, next);
    if (!e) continue; // system lines aren't moments
    next += 1;
    if (seen.has(e.key)) continue;
    seen.add(e.key);
    out.push(e);
  }
  return out.sort((a, b) => a.atMs - b.atMs || a.seq - b.seq);
}

/**
 * Percent position of each event along the track, by time fraction — the same math the
 * stop strip already uses (8%..92% padding). Monotonic in event order.
 */
export function positionsByTime(events: TrackEvent[]): number[] {
  if (events.length === 0) return [];
  const min = events[0].atMs;
  const max = events[events.length - 1].atMs;
  const span = Math.max(1, max - min);
  return events.map((e) => TRACK_POS_MIN + ((e.atMs - min) / span) * (TRACK_POS_MAX - TRACK_POS_MIN));
}

/**
 * Which event a drag at fraction `frac` of the track snaps to: the nearest in time,
 * clamped. Empty timeline returns 0 — the playhead sits at the start, harmlessly.
 */
export function fracToIndex(events: TrackEvent[], frac: number): number {
  if (events.length === 0) return 0;
  const min = events[0].atMs;
  const max = events[events.length - 1].atMs;
  const target = min + Math.min(Math.max(frac, 0), 1) * Math.max(1, max - min);
  let best = 0;
  let bestDist = Infinity;
  for (let i = 0; i < events.length; i++) {
    const d = Math.abs(events[i].atMs - target);
    if (d < bestDist) {
      bestDist = d;
      best = i;
    }
    // Equal distance resolves toward the later moment (ties favor the future).
    else if (d === bestDist && events[i].atMs >= target) {
      best = i;
    }
  }
  return best;
}

/** Keyboard scrub: one event per step, clamped at both ends. */
export function stepIndex(events: TrackEvent[], index: number, dir: 1 | -1): number {
  const next = index + dir;
  if (events.length === 0) return 0;
  return Math.min(Math.max(next, 0), events.length - 1);
}

/** The timeline state at the playhead: the inclusive prefix up to `index`. */
export function rewoundEvents(events: TrackEvent[], index: number): TrackEvent[] {
  return events.slice(0, Math.min(Math.max(index + 1, 0), events.length));
}

/**
 * Fork-on-retry state of moment `i`: an error moment is a fork point; it has `retried`
 * when any later moment exists (the branch the session took after the error) and is
 * `open` when nothing follows yet. Non-error moments are not forks.
 */
export function forkState(events: TrackEvent[], index: number): 'retried' | 'open' | null {
  if (index < 0 || index >= events.length) return null;
  if (events[index].kind !== 'error') return null;
  return index < events.length - 1 ? 'retried' : 'open';
}

/** Stops the rewound track shows: every evidence crossing at or before the playhead. */
export function rewoundStops(stops: Stop[], atMs: number): Stop[] {
  return stops.filter((s) => Date.parse(s.at) <= atMs);
}

/** The evidence the rewound track headlines, or null before any stop. */
export function latestStopBefore(stops: Stop[], atMs: number): Stop | null {
  let latest: Stop | null = null;
  for (const s of stops) {
    if (Date.parse(s.at) <= atMs && (!latest || Date.parse(s.at) >= Date.parse(latest.at))) {
      latest = s;
    }
  }
  return latest;
}
