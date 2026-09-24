import { useEffect, useState } from 'react';
import { AggregateStats } from '../types';
import { fetchAggregateStats, EMPTY_AGGREGATE_STATS } from '../services/api';
import { VerdictBoard } from '../components/VerdictBoard';
import { CacheCliffWidget } from '../components/CacheCliffWidget';
import { FleetStrip } from './FleetStrip';

/**
 * Rail "Overview" view — the aggregate evidence ladder plus the cache-cliff
 * cost widget, fetched once on mount. Same components the old scrolling
 * dashboard rendered inline; here they fill the view-region pane instead.
 */
export interface OverviewPaneProps {
  /** Opens a session in the inspector; the shell switches the rail view. */
  onOpenSession?: (sessionId: string) => void;
}

export function OverviewPane({ onOpenSession }: OverviewPaneProps) {
  const [stats, setStats] = useState<AggregateStats>(EMPTY_AGGREGATE_STATS);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    fetchAggregateStats()
      .then((data) => {
        if (!cancelled) setStats(data);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div className="view-region">
      {loading && <div className="shell-inspector-loading">Loading overview…</div>}
      <div className="view-stack">
        <FleetStrip onOpenSession={onOpenSession} />
        <VerdictBoard stats={stats} />
        <CacheCliffWidget />
      </div>
    </div>
  );
}
