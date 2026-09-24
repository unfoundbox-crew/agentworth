import type { NormalizedEvent } from '../types';

/**
 * A call whose cache did almost nothing for it. Below this share, the request
 * paid to re-send context rather than reusing it.
 */
const COLD_WARMTH = 0.5;
/** Ignore trivial re-creations; only cold starts that actually cost something. */
const MIN_COLD_CREATION = 10_000;

export interface Invocation {
  time: number;
  model: string | null;
  cacheRead: number;
  cacheCreation: number;
}

export interface ColdStart {
  /** Milliseconds of silence before this call. Null for the session's first. */
  gapMs: number | null;
  /** Tokens this call had to re-create. */
  recreated: number;
  /** Set when the model changed across this boundary. */
  modelSwitch: { from: string; to: string } | null;
}

export interface CacheEconomics {
  /** Share of cached context that was reused rather than re-created, 0..1. */
  warmth: number | null;
  invocations: number;
  totalRead: number;
  totalCreated: number;
  coldStarts: ColdStart[];
  /** The single most expensive cold start, or null if there were none. */
  worst: ColdStart | null;
}

function parseTime(value: unknown): number | null {
  if (typeof value !== 'string') return null;
  const ms = Date.parse(value);
  return Number.isFinite(ms) ? ms : null;
}

/** Pulls model invocations with their token usage out of a trace's events. */
export function readInvocations(events: NormalizedEvent[]): Invocation[] {
  const out: Invocation[] = [];
  for (const event of events) {
    const payload = event.payload as { type?: string; data?: Record<string, unknown> } | undefined;
    if (payload?.type !== 'model_invocation') continue;
    const data = payload.data ?? {};
    // Wire names, confirmed against a live trace — not the TS type's names,
    // which have drifted from the server before and read as zero when they do.
    const usage = (data.token_usage ?? {}) as Record<string, unknown>;
    const cacheRead = Number(usage.cache_read_tokens ?? 0) || 0;
    const cacheCreation = Number(usage.cache_creation_tokens ?? 0) || 0;
    const time = parseTime(event.timestamp);
    if (time === null) continue;
    out.push({
      time,
      model: typeof data.model === 'string' ? data.model : null,
      cacheRead,
      cacheCreation,
    });
  }
  out.sort((a, b) => a.time - b.time);
  return out;
}

/**
 * What a session's cache actually did for it.
 *
 * Every number here is arithmetic over token counts the scanner already
 * parsed. Nothing is estimated and no cache TTL is assumed: a cold start is
 * identified by what the call was charged, and the gap before it is reported
 * as an observed fact rather than as evidence of an expiry policy.
 */
export function analyzeCacheEconomics(events: NormalizedEvent[]): CacheEconomics {
  const invocations = readInvocations(events);
  let totalRead = 0;
  let totalCreated = 0;
  const coldStarts: ColdStart[] = [];

  for (let i = 0; i < invocations.length; i++) {
    const call = invocations[i];
    totalRead += call.cacheRead;
    totalCreated += call.cacheCreation;

    const cached = call.cacheRead + call.cacheCreation;
    if (cached === 0) continue;
    const warmth = call.cacheRead / cached;
    if (warmth >= COLD_WARMTH || call.cacheCreation < MIN_COLD_CREATION) continue;

    const prev = i > 0 ? invocations[i - 1] : null;
    const switched =
      prev && prev.model && call.model && prev.model !== call.model
        ? { from: prev.model, to: call.model }
        : null;
    coldStarts.push({
      gapMs: prev ? call.time - prev.time : null,
      recreated: call.cacheCreation,
      modelSwitch: switched,
    });
  }

  const cachedTotal = totalRead + totalCreated;
  const worst = coldStarts.reduce<ColdStart | null>(
    (best, c) => (best === null || c.recreated > best.recreated ? c : best),
    null
  );

  return {
    warmth: cachedTotal > 0 ? totalRead / cachedTotal : null,
    invocations: invocations.length,
    totalRead,
    totalCreated,
    coldStarts,
    worst,
  };
}

/**
 * Only a gap this long is offered as an explanation.
 *
 * Measured across 32,901 model-invocation pairs in 34 real sessions: calls
 * resuming within 30 minutes are cold 8% of the time or less, while past an
 * hour they are cold essentially always. Below this, an idle gap explains
 * nothing, and naming it would invent a cause — a 14-second pause did not
 * expire anything.
 */
const EXPLANATORY_GAP_MS = 30 * 60_000;

/**
 * Names what caused a cold start, and says so only when the evidence supports it.
 *
 * A model switch is certain when it happened: the model id changed. A long gap
 * is reported as elapsed time, never as "the cache expired", because nothing
 * here observes an expiry policy.
 *
 * Most cold starts get neither. On the sample above, model switches accounted
 * for 3% and long gaps for 13%, leaving 83% with no visible cause — so an
 * unexplained cold start says exactly that rather than blaming whichever
 * small gap happened to precede it.
 */
export function describeCause(cold: ColdStart, formatGap: (ms: number) => string): string {
  const parts: string[] = [];
  if (cold.gapMs !== null && cold.gapMs >= EXPLANATORY_GAP_MS) {
    parts.push(`after ${formatGap(cold.gapMs)} idle`);
  }
  if (cold.modelSwitch) parts.push(`model changed to ${cold.modelSwitch.to}`);
  if (parts.length > 0) return parts.join(', ');
  return cold.gapMs === null ? 'at the start of the session' : 'no idle gap or model change';
}
