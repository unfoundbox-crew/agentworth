import { RUNNING_WINDOW_SECS, useFleet } from '../hooks/useFleet';
import { getAdapterBadge, formatTokens, formatUSD } from '../utils/formatters';

export interface FleetStripProps {
  /**
   * Opens a session in the inspector. Required in the deck archive — there is
   * no dashboard useRoute() hash router here.
   */
  onOpenSession: (sessionId: string) => void;
  /**
   * Bumped by live-tail SSE when a watched session file changes. Optional —
   * the strip still works on poll alone when live tail is off / unwired.
   */
  refreshSignal?: number;
}

const MAX_CHIPS = 8;

function relativeAge(secs: number): string {
  if (secs < 60) return 'just now';
  const minutes = Math.round(secs / 60);
  return `${minutes}m ago`;
}

/**
 * What is running, and what it is costing. Two lines, above the aggregate
 * widgets. Inferred from session-file mtime — chips stay dashed.
 */
export function FleetStrip({ onOpenSession, refreshSignal = 0 }: FleetStripProps) {
  const fleet = useFleet(true, refreshSignal);

  if (!fleet.loading && !fleet.mtimeAvailable) return null;

  const shown = fleet.running.slice(0, MAX_CHIPS);
  const overflow = fleet.running.length - shown.length;

  return (
    <section className="fleet-strip" aria-label="Running now">
      <div className="fleet-head">
        <span className="fleet-eyebrow">Running now · inferred from recent activity</span>
        {fleet.staleSince !== null && (
          <span className="fleet-stale">
            last updated {relativeAge((Date.now() - fleet.staleSince) / 1000)}
          </span>
        )}
      </div>

      {fleet.loading ? (
        <div className="fleet-chips" aria-hidden="true">
          {Array.from({ length: 3 }).map((_, i) => (
            <span key={i} className="fleet-chip-skeleton" />
          ))}
        </div>
      ) : fleet.running.length === 0 ? (
        <p className="fleet-empty">
          No session file has been written in the last {Math.round(RUNNING_WINDOW_SECS / 60)} minutes.
        </p>
      ) : (
        <div className="fleet-chips">
          {shown.map(({ session, ageSecs }) => {
            const badge = getAdapterBadge(session.adapter);
            return (
              <button
                key={session.session_id}
                type="button"
                className="fleet-chip"
                onClick={() => onOpenSession(session.session_id)}
                title={`Session file modified ${relativeAge(ageSecs)} — ${session.session_id}`}
              >
                <span className="fleet-dot" aria-hidden="true" />
                <span className="fleet-chip-adapter">{badge.name}</span>
                <span className="fleet-chip-age">{relativeAge(ageSecs)}</span>
              </button>
            );
          })}
          {overflow > 0 && (
            <span className="fleet-chip fleet-chip-more" title={`${overflow} more recently written`}>
              +{overflow} more
            </span>
          )}
        </div>
      )}

      {fleet.spend.state === 'ok' && (
        <p
          className="fleet-spend"
          title={`${fleet.spend.value.total_tokens.toLocaleString()} tokens of context volume today (raw sum, cache reads at face value); ${Math.round(
            fleet.spend.value.total_weighted_tokens
          ).toLocaleString()} weighted — closer to spend (input + output + 1.25× cache writes + 0.1× cache reads)`}
        >
          <span className="fleet-spend-label">Today</span>
          <span className="fleet-spend-value">{formatUSD(fleet.spend.value.total_cost_usd)}</span>
          <span className="fleet-spend-sep">·</span>
          <span className="fleet-spend-value">{formatTokens(fleet.spend.value.total_tokens)} ctx</span>
          <span className="fleet-spend-sep">·</span>
          <span className="fleet-spend-value">
            {formatTokens(fleet.spend.value.total_weighted_tokens)} weighted
          </span>
        </p>
      )}
    </section>
  );
}
