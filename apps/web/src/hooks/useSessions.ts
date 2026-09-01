import { useCallback, useEffect, useState } from 'react';
import { SessionSummary } from '../types';

export interface UseSessionsResult {
  sessions: SessionSummary[];
  loading: boolean;
  error: string | null;
  refetch: () => void;
}

/**
 * Fetches the session summary list once from /api/traces and holds it.
 * Callers filter client-side — the summaries are small, so re-querying the
 * API per keystroke would just add latency to a search box that should feel
 * instant.
 *
 * Fetches directly (rather than through services/api.ts's fetchTraces,
 * which swallows fetch failures into an empty list) so a genuine API/network
 * error can be told apart from "the API is up and there are just zero
 * sessions indexed" — the two need different UI.
 */
export function useSessions(): UseSessionsResult {
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
        const res = await fetch('/api/traces');
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
  }, [reloadToken]);

  const refetch = useCallback(() => {
    setReloadToken((t) => t + 1);
  }, []);

  return { sessions, loading, error, refetch };
}
