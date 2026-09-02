import { useMemo } from 'react';
import type { NormalizedEvent } from '../types';
import { analyzeCacheEconomics, describeCause } from '../utils/cacheEconomics';
import { formatDuration } from '../utils/timeAxis';
import { formatTokens } from '../utils/formatters';

export interface CacheWarmthProps {
  events: NormalizedEvent[] | null | undefined;
  /** False while background pages are still loading — warmth is a share of
   * every model invocation, so a partial event set understates it. */
  eventsComplete?: boolean;
}

/**
 * One line: how much of this session ran warm, and what the worst cold start
 * cost.
 *
 * Warmth is not a verdict. A cold session is not a failure — it is a session
 * that was resumed, which is a Tuesday. So this uses `--mv-warn` only on the
 * re-created portion, which is money actually spent, and never `--mv-danger`.
 */
export function CacheWarmth({ events, eventsComplete = true }: CacheWarmthProps) {
  const economics = useMemo(() => analyzeCacheEconomics(events ?? []), [events]);

  if (!eventsComplete) {
    return (
      <div className="cache-warmth">
        <div className="cache-warmth-line">
          <span className="cache-warmth-label">Cache</span>
          <span className="cache-warmth-value">Loading…</span>
        </div>
      </div>
    );
  }

  // No model invocations carrying cache counts: say nothing rather than
  // render 0% warmth, which would read as "your cache did nothing".
  if (economics.warmth === null) return null;

  const warmPct = economics.warmth * 100;
  const { worst } = economics;

  return (
    <div className="cache-warmth">
      <div className="cache-warmth-line">
        <span className="cache-warmth-label">Cache</span>
        <span className="cache-warmth-bar" aria-hidden="true">
          <span className="cache-warmth-fill" style={{ width: `${warmPct}%` }} />
        </span>
        <span className="cache-warmth-value">{warmPct.toFixed(1)}% warm</span>
        <span className="cache-warmth-detail" title={`${economics.totalRead.toLocaleString()} tokens reused`}>
          {formatTokens(economics.totalRead)} reused
        </span>
        <span className="cache-warmth-sep">·</span>
        <span
          className="cache-warmth-detail"
          title={`${economics.totalCreated.toLocaleString()} tokens re-created`}
        >
          {formatTokens(economics.totalCreated)} re-created
        </span>
      </div>

      {worst && (
        <p className="cache-warmth-worst">
          Largest cold start re-created{' '}
          <strong>{formatTokens(worst.recreated)}</strong> tokens — {describeCause(worst, formatDuration)}.
          {economics.coldStarts.length > 1 && (
            <span className="cache-warmth-count">
              {' '}
              {economics.coldStarts.length} cold starts in {economics.invocations.toLocaleString()} calls.
            </span>
          )}
        </p>
      )}
    </div>
  );
}
