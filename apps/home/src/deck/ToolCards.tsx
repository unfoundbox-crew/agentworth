import type { Artifact, Direction, Message, Presence } from '../protocol';
import {
  MAX_TOOLS_SHOWN,
  deriveToolEntries,
  retryAvailability,
  summarizeTools,
  type ToolEntry,
} from './tools';

function BanIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
      <circle cx="6" cy="6" r="5" stroke="currentColor" strokeWidth="1.2" />
      <line x1="3" y1="9" x2="9" y2="3" stroke="currentColor" strokeWidth="1.2" />
    </svg>
  );
}

function WrenchIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
      <circle cx="6" cy="6" r="4.4" stroke="currentColor" strokeWidth="1.2" />
      <circle cx="6" cy="6" r="1.4" fill="currentColor" />
    </svg>
  );
}

export function ToolCard({ entry }: { entry: ToolEntry }) {
  return (
    <div className="rounded-md border border-line bg-[var(--mv-ground)] px-2.5 py-1.5" data-card="tool">
      <div className="flex items-center gap-1.5 text-[11px] text-text">
        <span className="text-muted" aria-hidden>
          <WrenchIcon />
        </span>
        <span className="font-medium truncate">{entry.title}</span>
        {entry.status === 'running' && <span className="text-[10px] text-dim italic">running…</span>}
      </div>
      <div className="mt-0.5 truncate text-[10px] text-dim">{entry.ref}</div>
      {entry.detail && <div className="mt-0.5 truncate text-[10px] text-muted">{entry.detail}</div>}
    </div>
  );
}

export function ToolErrorCard({
  entry,
  direction,
  riderPresence,
  onRetry,
}: {
  entry: ToolEntry;
  direction: Direction | null | undefined;
  riderPresence: Presence | string | undefined;
  onRetry: (entry: ToolEntry) => void;
}) {
  const avail = retryAvailability(direction, riderPresence);
  return (
    <div
      role="alert"
      className="rounded-md border border-[var(--mv-warn)] bg-[var(--mv-ground)] px-2.5 py-1.5"
      data-card="tool-error"
    >
      <div className="flex items-center gap-1.5 text-[11px] text-text">
        <span className="text-warn" aria-hidden>
          <BanIcon />
        </span>
        <span className="font-medium truncate">{entry.title}</span>
        <span className="text-[10px] text-warn shrink-0">failed</span>
      </div>
      {entry.errorMessage && (
        <div className="mt-0.5 text-[10px] text-text">{entry.errorMessage}</div>
      )}
      <div className="mt-1 flex items-center gap-2">
        {avail.enabled ? (
          <button
            type="button"
            onClick={() => onRetry(entry)}
            className="rounded border border-line px-2 py-0.5 text-[10px] text-text hover:text-ink"
          >
            retry
          </button>
        ) : (
          <>
            <button
              type="button"
              disabled
              title={avail.reason}
              aria-describedby={`retry-why-${entry.id}`}
              className="rounded border border-line px-2 py-0.5 text-[10px] text-dim opacity-60 cursor-not-allowed"
            >
              retry
            </button>
            <span id={`retry-why-${entry.id}`} className="text-[10px] text-dim">
              {avail.reason}
            </span>
          </>
        )}
      </div>
    </div>
  );
}

export function ToolCountSummary({ total, shown }: { total: number; shown: number }) {
  const hidden = total - shown;
  if (hidden <= 0) return null;
  return (
    <div className="text-center text-[10px] text-dim" data-card="tool-count">
      Showing {shown} of {total} tool calls — {hidden} more
    </div>
  );
}

/**
 * The session timeline's tool layer: one card per tool call (derived read-only
 * from `work` messages + their linked artifacts), failures as error cards with
 * a retry wired to the existing steer path, long runs capped at MAX_TOOLS_SHOWN
 * with a count summary. Pure rendering -- no fetching, no index writes.
 */
export function ToolCards({
  messages,
  artifacts,
  direction,
  riderPresence,
  onRetry,
  limit = MAX_TOOLS_SHOWN,
}: {
  messages: Message[];
  artifacts: Record<string, Artifact>;
  direction: Direction | null | undefined;
  riderPresence: Presence | string | undefined;
  onRetry: (entry: ToolEntry) => void;
  limit?: number;
}) {
  const entries = deriveToolEntries(messages, artifacts);
  if (entries.length === 0) return null;
  const summary = summarizeTools(entries, limit);
  const shown = entries.slice(0, summary.shown);
  return (
    <div className="flex flex-col gap-1.5" aria-label="tools">
      {shown.map((e) =>
        e.status === 'error' ? (
          <ToolErrorCard
            key={e.id}
            entry={e}
            direction={direction}
            riderPresence={riderPresence}
            onRetry={onRetry}
          />
        ) : (
          <ToolCard key={e.id} entry={e} />
        ),
      )}
      <ToolCountSummary total={summary.total} shown={summary.shown} />
    </div>
  );
}
