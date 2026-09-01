import { useCallback, useEffect, useRef, useState } from 'react';
import type { SessionSummary, UsageRollupResponse } from '../types';

/** How recently a session file must have been written to count as probably running. */
export const RUNNING_WINDOW_SECS = 5 * 60;
const POLL_MS = 30_000;
/** Enough to cover anything plausibly still running, without pulling the index. */
const POLL_LIMIT = 80;

/**
 * A value that may be genuinely absent.
 *
 * Absent is not zero. `fetchUsageRollups` currently answers a missing endpoint
 * with `total_cost_usd: 0`, which renders as "$0.00 spent" — a false statement
 * dressed as a measurement. Anything this hook cannot establish is reported as
 * unavailable so the UI can omit the line instead of asserting a number.
 */
export type Maybe<T> = { state: 'ok'; value: T } | { state: 'unavailable' };

const UNAVAILABLE: Maybe<never> = { state: 'unavailable' };

/**
 * Fetches JSON, treating a non-JSON body as absence rather than as an error.
 *
 * A route this build does not implement is not a 404 — the static handler
 * serves `index.html` with status 200, so `res.ok` is true and `res.json()`
 * throws on the HTML. Checking the content type turns that into a clean
 * "unavailable" instead of a parse exception.
 */
async function fetchJson<T>(url: string): Promise<Maybe<T>> {
  try {
    const res = await fetch(url);
    if (!res.ok) return UNAVAILABLE;
    const type = res.headers.get('content-type') ?? '';
    if (!type.includes('json')) return UNAVAILABLE;
    return { state: 'ok', value: (await res.json()) as T };
  } catch {
    return UNAVAILABLE;
  }
}

export interface RunningSession {
  session: SessionSummary;
  /** Seconds since the session file was last written. */
  ageSecs: number;
}

export interface FleetState {
  /** Sessions whose file was written inside the window, newest write first. */
  running: RunningSession[];
  /**
   * False when no session carries `source_mtime_epoch_secs` — the field is not
   * on every build, and without it "nothing is running" would be a guess
   * rather than an answer.
   */
  mtimeAvailable: boolean;
  indexedCount: number;
  spend: Maybe<UsageRollupResponse>;
  loading: boolean;
  /** Set when the most recent refresh failed but earlier data is still shown. */
  staleSince: number | null;
}

/**
 * Polls a small slice of the index for sessions that look like they are still
 * running, plus today's spend.
 *
 * Polling pauses while the tab is hidden and refetches immediately on return,
 * and a failed refresh keeps the last good data rather than blanking the strip.
 */
export function useFleet(enabled: boolean): FleetState {
  const [state, setState] = useState<FleetState>({
    running: [],
    mtimeAvailable: false,
    indexedCount: 0,
    spend: UNAVAILABLE,
    loading: true,
    staleSince: null,
  });
  const lastGood = useRef<number | null>(null);

  const poll = useCallback(async () => {
    const [traces, usage] = await Promise.all([
      fetchJson<SessionSummary[]>(`/api/traces?limit=${POLL_LIMIT}`),
      fetchJson<UsageRollupResponse>('/api/usage?period=day'),
    ]);

    setState((prev) => {
      if (traces.state !== 'ok') {
        // Keep whatever was last shown; only note that it has aged.
        return { ...prev, loading: false, staleSince: lastGood.current };
      }
      const rows = Array.isArray(traces.value) ? traces.value : [];
      const nowSecs = Date.now() / 1000;
      const withMtime = rows.filter(
        (s) => typeof s.source_mtime_epoch_secs === 'number' && s.source_mtime_epoch_secs > 0
      );
      const running = withMtime
        .map((s) => ({ session: s, ageSecs: nowSecs - (s.source_mtime_epoch_secs as number) }))
        .filter((r) => r.ageSecs >= 0 && r.ageSecs <= RUNNING_WINDOW_SECS)
        .sort((a, b) => a.ageSecs - b.ageSecs);

      lastGood.current = Date.now();
      return {
        running,
        mtimeAvailable: withMtime.length > 0,
        indexedCount: rows.length,
        spend: usage,
        loading: false,
        staleSince: null,
      };
    });
  }, []);

  useEffect(() => {
    if (!enabled) return;
    let timer: number | undefined;
    let cancelled = false;

    const tick = () => {
      if (cancelled || document.hidden) return;
      void poll();
    };

    void poll();
    timer = window.setInterval(tick, POLL_MS);

    const onVisibility = () => {
      if (!document.hidden) tick();
    };
    document.addEventListener('visibilitychange', onVisibility);

    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearInterval(timer);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [enabled, poll]);

  return state;
}
