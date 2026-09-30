import { useEffect, useState } from 'react';
import { AggregateStats } from '../types';
import { fetchAggregateStats, EMPTY_AGGREGATE_STATS } from '../api';
import { VerdictBoard } from '../components/VerdictBoard';
import { CacheCliffWidget } from '../components/CacheCliffWidget';
import { FleetStrip } from './FleetStrip';

/**
 * Rail "Overview" view — aggregate evidence ladder + cache-cliff + fleet strip.
 * Same contracts as apps/dashboard OverviewPane; deck navigates via onOpenSession.
 */
export interface OverviewPaneProps {
  onOpenSession?: (sessionId: string) => void;
  liveTailRefreshSignal?: number;
}

export function OverviewPane({ onOpenSession, liveTailRefreshSignal = 0 }: OverviewPaneProps) {
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
        {onOpenSession ? (
          <FleetStrip onOpenSession={onOpenSession} refreshSignal={liveTailRefreshSignal} />
        ) : null}
        <VerdictBoard stats={stats} />
        <CacheCliffWidget />
      </div>
    </div>
  );
}
