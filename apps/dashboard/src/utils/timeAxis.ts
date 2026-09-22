import type { NormalizedEvent } from '../types';

/**
 * Idle stretches longer than this contribute a fixed width instead of their
 * real duration.
 *
 * Measured, not chosen: on a real 7.15-hour Codex session, gaps over 120s
 * account for 73.6% of the wall-clock span, and the largest single gap is 84
 * minutes. Rendered proportionally, three quarters of the strip would be
 * blank and every actual event would be crushed into the remaining quarter.
 * 90s sits above the p90 inter-event gap of an actively working session
 * (~19s) and below the scale at which a human has clearly walked away.
 */
const GAP_THRESHOLD_MS = 90_000;

/**
 * What a compressed gap is worth on the axis. Equal to the threshold, so a
 * long pause still reads as wider than ordinary think-time but can never
 * dominate: an 84-minute gap and a 2-minute gap both occupy 90s of axis.
 */
const GAP_WIDTH_MS = GAP_THRESHOLD_MS;

export interface TimePoint {
  event: NormalizedEvent;
  /** Milliseconds since epoch. */
  time: number;
  /** Position on the compressed axis, 0..1. */
  position: number;
}

export interface AxisGap {
  /** Compressed-axis span this gap occupies. */
  from: number;
  to: number;
  /** Real elapsed milliseconds the gap represents. */
  durationMs: number;
}

export interface TimeAxis {
  kind: 'time';
  /** Events sorted ascending by timestamp, each with its axis position. */
  points: TimePoint[];
  /** Idle stretches that were compressed, for drawing gap glyphs. */
  gaps: AxisGap[];
  startTime: number;
  endTime: number;
  /** Compressed-axis position -> wall-clock milliseconds. */
  timeAt: (position: number) => number;
}

export interface SequenceAxis {
  kind: 'sequence';
  points: TimePoint[];
  gaps: [];
  /** Why the time axis was not usable. Shown to the user, so it must be true. */
  reason: 'unparseable' | 'no-span';
}

export type Axis = TimeAxis | SequenceAxis;

function parseTime(value: unknown): number | null {
  if (typeof value !== 'string') return null;
  const ms = Date.parse(value);
  return Number.isFinite(ms) ? ms : null;
}

/**
 * Builds the axis a strip is drawn against.
 *
 * Events are ordered by **timestamp**, not by `sequence`. Those two orders
 * genuinely differ: a top-level session resumed across 17 days carried 2,069
 * backward steps in 29,641, because replayed and subagent content keeps its
 * original timestamps while appearing later in the file. Sorting by time is
 * the fix for that, not a reason to distrust the timestamps — eight other
 * sampled sessions had zero backward steps and 100% parseable times.
 *
 * So the fallback to sequence order fires only when the time axis genuinely
 * cannot be drawn: no event carries a parseable timestamp, or every event
 * shares one instant so there is no span to spread them across. Ordering
 * anomalies alone are not a fallback trigger — they are what sorting exists
 * for.
 */
export function buildAxis(events: NormalizedEvent[]): Axis {
  if (events.length === 0) {
    return { kind: 'sequence', points: [], gaps: [], reason: 'unparseable' };
  }

  const timed: { event: NormalizedEvent; time: number }[] = [];
  for (const event of events) {
    const time = parseTime(event.timestamp);
    if (time !== null) timed.push({ event, time });
  }

  const sequenceFallback = (reason: SequenceAxis['reason']): SequenceAxis => {
    const ordered = [...events].sort((a, b) => a.sequence - b.sequence);
    const last = ordered.length - 1;
    return {
      kind: 'sequence',
      gaps: [],
      reason,
      points: ordered.map((event, i) => ({
        event,
        time: Number.NaN,
        position: last === 0 ? 0 : i / last,
      })),
    };
  };

  // A partial time axis would draw real gaps for some events and invented
  // ones for the rest, which misrepresents missing data as idle time.
  if (timed.length !== events.length || timed.length === 0) {
    return sequenceFallback('unparseable');
  }

  timed.sort((a, b) => a.time - b.time);
  const startTime = timed[0].time;
  const endTime = timed[timed.length - 1].time;
  if (endTime === startTime) return sequenceFallback('no-span');

  // Walk the sorted events accumulating "axis milliseconds", where an idle
  // stretch past the threshold is worth a flat GAP_WIDTH_MS however long it
  // really was. Each step records the real and axis interval so the mapping
  // stays invertible.
  const steps: { realFrom: number; realTo: number; axisFrom: number; axisTo: number }[] = [];
  const rawGaps: { axisFrom: number; axisTo: number; durationMs: number }[] = [];
  let axis = 0;

  for (let i = 1; i < timed.length; i++) {
    const realFrom = timed[i - 1].time;
    const realTo = timed[i].time;
    const elapsed = realTo - realFrom;
    const compressed = elapsed > GAP_THRESHOLD_MS;
    const width = compressed ? GAP_WIDTH_MS : elapsed;
    steps.push({ realFrom, realTo, axisFrom: axis, axisTo: axis + width });
    if (compressed) rawGaps.push({ axisFrom: axis, axisTo: axis + width, durationMs: elapsed });
    axis += width;
  }

  const total = axis;
  // Every interval collapsed to nothing (all events inside the same
  // millisecond bar one): spread them evenly rather than divide by zero.
  if (total <= 0) return sequenceFallback('no-span');

  const points: TimePoint[] = timed.map((t, i) => ({
    event: t.event,
    time: t.time,
    position: i === 0 ? 0 : steps[i - 1].axisTo / total,
  }));

  const gaps: AxisGap[] = rawGaps.map((g) => ({
    from: g.axisFrom / total,
    to: g.axisTo / total,
    durationMs: g.durationMs,
  }));

  const timeAt = (position: number): number => {
    const target = Math.min(1, Math.max(0, position)) * total;
    if (target <= 0) return startTime;
    if (target >= total) return endTime;
    // Steps are ordered and contiguous on the axis, so a binary search lands
    // on the interval containing `target`.
    let lo = 0;
    let hi = steps.length - 1;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (steps[mid].axisTo < target) lo = mid + 1;
      else hi = mid;
    }
    const step = steps[lo];
    const width = step.axisTo - step.axisFrom;
    const ratio = width <= 0 ? 0 : (target - step.axisFrom) / width;
    return step.realFrom + ratio * (step.realTo - step.realFrom);
  };

  return { kind: 'time', points, gaps, startTime, endTime, timeAt };
}

/** Compact elapsed time — "4m", "1h 12m", "820ms". For gap labels. */
export function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return '—';
  if (ms < 1000) return `${Math.round(ms)}ms`;
  const seconds = Math.round(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours < 24) return rest > 0 ? `${hours}h ${rest}m` : `${hours}h`;
  const days = Math.floor(hours / 24);
  return `${days}d ${hours % 24}h`;
}

/** Clock time for an axis label. Local, seconds included — sessions are short. */
export function formatClock(ms: number): string {
  if (!Number.isFinite(ms)) return '—';
  return new Date(ms).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

function sameLocalDay(a: number, b: number): boolean {
  const x = new Date(a);
  const y = new Date(b);
  return (
    x.getFullYear() === y.getFullYear() &&
    x.getMonth() === y.getMonth() &&
    x.getDate() === y.getDate()
  );
}

/**
 * Labels a window's endpoints.
 *
 * Dates appear whenever the two ends fall on different days. Without that, a
 * real session resumed across 17 days rendered as "23:54:54 – 13:49:41" — an
 * end that reads as earlier than its start.
 */
export function formatRange(from: number, to: number): string {
  if (!Number.isFinite(from) || !Number.isFinite(to)) return '—';
  if (sameLocalDay(from, to)) return `${formatClock(from)} – ${formatClock(to)}`;
  const date = (ms: number) =>
    new Date(ms).toLocaleDateString([], { month: 'short', day: 'numeric' });
  return `${date(from)} ${formatClock(from)} – ${date(to)} ${formatClock(to)}`;
}
