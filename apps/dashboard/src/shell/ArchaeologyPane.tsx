import { useEffect, useState } from 'react';
import { ArchaeologyData } from '../types';
import { fetchAggregateStats } from '../services/api';
import { ArchaeologyPanel } from '../components/ArchaeologyPanel';

/**
 * Rail "Archaeology" view. ArchaeologyPanel needs a populated
 * ArchaeologyData — the aggregate-stats index either has one (a scan ran
 * and found notable sessions) or it doesn't, so this pane owns the
 * loading/empty states ArchaeologyPanel itself has no opinion about.
 */
export function ArchaeologyPane() {
  const [data, setData] = useState<ArchaeologyData | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    fetchAggregateStats()
      .then((stats) => {
        if (!cancelled) setData(stats.archaeology ?? null);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (loading) {
    return (
      <div className="view-region">
        <div className="shell-inspector-loading">Loading archaeology…</div>
      </div>
    );
  }

  if (!data) {
    return (
      <div className="view-region">
        <div className="shell-inspector-empty">
          No archaeology on file yet. Run <code>agentworth scan</code> to index sessions.
        </div>
      </div>
    );
  }

  return (
    <div className="view-region">
      <div className="view-stack">
        <ArchaeologyPanel data={data} />
      </div>
    </div>
  );
}
