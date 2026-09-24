import { useEffect, useState } from 'react';
import { ArchaeologyData } from '../types';
import { fetchArchaeology } from '../services/api';
import { ArchaeologyPanel } from '../components/ArchaeologyPanel';

/** True once at least one highlight has something to show — an all-absent, all-zero
 * result (see EMPTY_ARCHAEOLOGY_DATA) means the index has no sessions to dig through yet. */
function hasAnyFinding(data: ArchaeologyData): boolean {
  return (
    data.most_expensive_unsolved != null ||
    data.longest_recovery_loop != null ||
    data.most_frequent_model_switches != null ||
    data.token_carbon_dating.timeline.length > 0
  );
}

/**
 * Rail "Archaeology" view. Fetches its own data straight from /api/archaeology rather than
 * riding along on /api/stats: computing archaeology walks full traces for its candidate
 * sessions, so it only runs when this pane actually mounts, not on every stats poll.
 */
export function ArchaeologyPane() {
  const [data, setData] = useState<ArchaeologyData | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    fetchArchaeology()
      .then((result) => {
        if (!cancelled) setData(result);
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

  if (!data || !hasAnyFinding(data)) {
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
