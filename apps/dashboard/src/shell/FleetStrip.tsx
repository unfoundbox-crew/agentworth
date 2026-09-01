import { RUNNING_WINDOW_SECS, useFleet } from '../hooks/useFleet';
import { useRoute } from '../hooks/useRoute';
import { getAdapterBadge, formatTokens, formatUSD } from '../utils/formatters';

export interface FleetStripProps {
  /**
   * Opens a session in the inspector. Optional because the strip also renders
   * inside the sessions view, where the rail is already where it needs to be
   * and plain navigation is enough.
   */
  onOpenSession?: (sessionId: string) => void;
}

const MAX_CHIPS = 8;

function relativeAge(secs: number): string {
  if (secs < 60) return 'just now';
  const minutes = Math.round(secs / 60);
  return `${minutes}m ago`;
}

/**
 * What is running, and what it is costing. Two lines, above the aggregate
 * widgets.
 *
 * Everything here is inferred from when a session file was last written, not
 * observed from a running process — there is no watcher and no stream. The
 * wording, the dashed chip borders and the tooltips all say that rather than
 * implying certainty the data does not carry.
 */
export function FleetStrip({ onOpenSession }: FleetStripProps) {
  const fleet = useFleet(true);
  const { navigate } = useRoute();
  const open = (id: string) =>
    onOpenSession ? onOpenSession(id) : navigate(`/s/${encodeURIComponent(id)}`);

  // Without the mtime field there is no signal at all, and an empty strip
  // would read as "nothing is running" — an answer this build cannot give.
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
                onClick={() => open(session.session_id)}
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

      {/* Spend is omitted rather than shown as zero when the route is absent —
          "$0.00 today" would be a measurement this build never made. */}
      {fleet.spend.state === 'ok' && (
        <p className="fleet-spend">
          <span className="fleet-spend-label">Today</span>
          <span className="fleet-spend-value">{formatUSD(fleet.spend.value.total_cost_usd)}</span>
          <span className="fleet-spend-sep">·</span>
          <span className="fleet-spend-value">{formatTokens(fleet.spend.value.total_tokens)} tokens</span>
        </p>
      )}
    </section>
  );
}
