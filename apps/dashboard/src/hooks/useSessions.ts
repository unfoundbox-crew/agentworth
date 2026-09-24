import { useCallback, useEffect, useState } from 'react';
import { SessionSummary } from '../types';

export interface UseSessionsResult {
  sessions: SessionSummary[];
  loading: boolean;
  error: string | null;
  refetch: () => void;
}

/**
 * Fetches the whole session summary list once from /api/traces and holds it.
 * Stub sessions (total_events <= 1 or zero tokens) are excluded server-side,
 * so this count is lower than /api/stats total_sessions by design.
 * Callers filter client-side — the summaries are small, so re-querying the
 * API per keystroke would just add latency to a search box that should feel
 * instant.
 *
 * Fetches directly (rather than through services/api.ts's fetchTraces,
 * which swallows fetch failures into an empty list) so a genuine API/network
 * error can be told apart from "the API is up and there are just zero
 * sessions indexed" — the two need different UI.
 */
export function useSessions(reloadSignal = 0): UseSessionsResult {
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [reloadToken, setReloadToken] = useState(0);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);

    (async () => {
      try {
        // The server defaults to 50. Without an explicit limit the list showed
        // the newest 50 of thousands, and every client-side filter, search and
        // sort silently operated on that slice. Summaries are small — the whole
        // non-stub set is ~1.3 MB and comes back in ~12ms over localhost — so
        // fetch it all and keep filtering instant.
        const res = await fetch('/api/traces?limit=100000');
        if (!res.ok) {
          throw new Error(`/api/traces returned ${res.status}`);
        }
        const data = await res.json();
        const traces: SessionSummary[] = Array.isArray(data)
          ? data
          : Array.isArray(data?.traces)
            ? data.traces
            : [];
        if (cancelled) return;
        setSessions(traces);
        setLoading(false);
      } catch (_err) {
        if (cancelled) return;
        setError('Could not reach the AgentWorth API.');
        setLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [reloadToken, reloadSignal]);

  const refetch = useCallback(() => {
    setReloadToken((t) => t + 1);
  }, []);

  return { sessions, loading, error, refetch };
}
