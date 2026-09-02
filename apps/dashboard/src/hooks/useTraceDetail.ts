import { useEffect, useState } from 'react';
import { TraceDetailResponse } from '../types';
import { fetchTraceDetail, fetchTraceEventsPage } from '../services/api';

/** Big enough that the inspector's header, score, and first screenful of the
 * trajectory paint immediately; small enough that a 64MB session doesn't
 * block on it. */
const FIRST_PAGE_LIMIT = 500;
/** Background pages are bigger — nothing is waiting on them, so fewer round
 * trips beats a granular progress bar. */
const BACKGROUND_PAGE_LIMIT = 2000;

export interface UseTraceDetailResult {
  trace: TraceDetailResponse | null;
  /** True only while the first page is in flight — the state a spinner should key on. */
  loading: boolean;
  error: string | null;
  eventsLoaded: number;
  /** Null until the first response comes back; stays null forever against a
   * pre-#72 server, which never reports a total because it never paginates. */
  eventsTotal: number | null;
  /** True once `trace.events` holds everything — either because background
   * paging finished, or because the server returned it all up front. */
  eventsComplete: boolean;
}

/**
 * Loads a trace in two phases: a fast first page for the inspector to paint
 * immediately, then the rest fetched from /events in the background and
 * appended in place. Panes that need the complete event set (context
 * composition, compaction, cache warmth, loose ends) key their headline
 * numbers off `eventsComplete` rather than trusting a partial `trace.events`.
 */
export function useTraceDetail(sessionId: string | null): UseTraceDetailResult {
  const [trace, setTrace] = useState<TraceDetailResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [eventsTotal, setEventsTotal] = useState<number | null>(null);
  const [eventsComplete, setEventsComplete] = useState(false);

  useEffect(() => {
    if (!sessionId) {
      setTrace(null);
      setError(null);
      setLoading(false);
      setEventsTotal(null);
      setEventsComplete(false);
      return;
    }

    const controller = new AbortController();
    let cancelled = false;

    setLoading(true);
    setError(null);
    setTrace(null);
    setEventsTotal(null);
    setEventsComplete(false);

    (async () => {
      const first = await fetchTraceDetail(
        sessionId,
        { offset: 0, limit: FIRST_PAGE_LIMIT },
        controller.signal
      );
      if (cancelled) return;

      if (!first) {
        setError('Could not load this session.');
        setLoading(false);
        return;
      }

      setTrace(first);
      setLoading(false);

      const loadedSoFar = first.events?.length ?? 0;
      // No events_total means a pre-#72 server, which never paginates and
      // already handed back every event in `first.events` — done.
      if (first.events_total == null) {
        setEventsTotal(loadedSoFar);
        setEventsComplete(true);
        return;
      }

      const total = first.events_total;
      setEventsTotal(total);

      if (loadedSoFar >= total) {
        setEventsComplete(true);
        return;
      }

      let offset = loadedSoFar;
      while (offset < total && !cancelled) {
        const page = await fetchTraceEventsPage(
          sessionId,
          offset,
          BACKGROUND_PAGE_LIMIT,
          controller.signal
        );
        if (cancelled) return;
        if (!page || page.events.length === 0) break;

        offset += page.events.length;
        setTrace((prev) =>
          prev ? { ...prev, events: [...prev.events, ...page.events] } : prev
        );
      }
      if (!cancelled) setEventsComplete(true);
    })();

    return () => {
      cancelled = true;
      controller.abort();
    };
  }, [sessionId]);

  return {
    trace,
    loading,
    error,
    eventsLoaded: trace?.events?.length ?? 0,
    eventsTotal,
    eventsComplete,
  };
}
