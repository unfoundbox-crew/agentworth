import { useEffect, useState } from 'react';
import { AgentWorthTrace } from '../types';
import { fetchTraceDetail } from '../api';
import { ExportModal } from '@shell/ExportModal';
import { performClientSideRedaction, convertToAtif } from '../api';

export interface ExportsPaneProps {
  /** The currently selected session (Sessions view's selection), if any. */
  sessionId: string | null;
}

/**
 * Rail "Exports" view. ExportModal needs a trace — reuses the selected session.
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
      <ExportModal
        trace={trace}
        embedded
        performClientSideRedaction={performClientSideRedaction}
        convertToAtif={convertToAtif}
      />
    </div>
  );
}
