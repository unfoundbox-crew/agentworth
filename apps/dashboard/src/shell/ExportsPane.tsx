import { useEffect, useState } from 'react';
import { AgentWorthTrace } from '../types';
import { fetchTraceDetail } from '../services/api';
import { ExportModal } from '../components/ExportModal';

export interface ExportsPaneProps {
  /** The currently selected session (Sessions view's selection), if any. */
  sessionId: string | null;
}

/**
 * Rail "Exports" view. ExportModal needs a trace to export — reuses
 * whatever session is selected rather than duplicating a picker; if
 * nothing is selected yet, says so instead of rendering an empty modal.
 */
export function ExportsPane({ sessionId }: ExportsPaneProps) {
  const [trace, setTrace] = useState<AgentWorthTrace | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!sessionId) {
      setTrace(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    fetchTraceDetail(sessionId)
      .then((data) => {
        if (!cancelled) setTrace(data);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId]);

  if (!sessionId) {
    return (
      <div className="view-region">
        <div className="shell-inspector-empty">
          Select a session from Sessions, then come back here to export it.
        </div>
      </div>
    );
  }

  if (loading || !trace) {
    return (
      <div className="view-region">
        <div className="shell-inspector-loading">Loading session…</div>
      </div>
    );
  }

  return (
    <div className="view-region view-region-flush">
      <ExportModal trace={trace} embedded />
    </div>
  );
}
