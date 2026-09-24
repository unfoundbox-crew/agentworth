import { useEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { NormalizedEvent } from '../types';
import { TrajectoryScrubber } from './TrajectoryScrubber';
import { EventDetail, RawPayload, getPrimaryText, getResultPreview, getRoleLabel, getRoleWeight } from './EventDetail';
import './trajectory.css';

const ROW_HEIGHT = 22;

export interface TrajectoryViewProps {
  /** Widened to the whole shell. A trajectory line is a command plus its
   *  result; in a 290px column both truncate to about eight characters, which
   *  is not readable. Expanding collapses the session list so the stream gets
   *  the width the information actually needs. */
  focused?: boolean;
  onToggleFocus?: () => void;
  events: NormalizedEvent[] | null | undefined;
  /** Event ids marking a compaction boundary — drawn as ticks on the scrubber's rail. */
  compactionEventIds?: string[];
  /** Total event count on the session, once the first page reports it — null
   * until then, or forever against a server that doesn't paginate. */
  eventsTotal?: number | null;
  /** False while background pages are still loading; drives the header's
   * progress readout instead of the plain event count. */
  eventsComplete?: boolean;
}

/** The full event stream for a session: a bucketed shape strip, then one
 * dense line per event (virtualized — a real session runs ~500-1000+
 * events), then the selected event's detail. This is deliberately the full
 * stream, not filtered to evidence-producing events — that filter needs a
 * backend outcome-mapping change landing separately. */
export function TrajectoryView({
  events,
  focused = false,
  onToggleFocus,
  compactionEventIds,
  eventsTotal,
  eventsComplete = true,
}: TrajectoryViewProps) {
  const sorted = useMemo(() => {
    const list = events ?? [];
    return [...list].sort((a, b) => a.sequence - b.sequence);
  }, [events]);

  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [brushedIds, setBrushedIds] = useState<Set<string> | null>(null);
  const listScrollRef = useRef<HTMLDivElement>(null);

  // The strip's brush filters the stream, so both must be computed from one
  // event set — the stream is the source of truth for what is actually shown.
  const shown = useMemo(
    () => (brushedIds ? sorted.filter((e) => brushedIds.has(e.id)) : sorted),
    [sorted, brushedIds]
  );

  // A new session means a new `events` array reference (InspectorPane
  // fetches fresh trace state per selection) — clear the stale selection
  // rather than let a sequence number from the previous session linger.
  useEffect(() => {
    setSelectedId(null);
    setBrushedIds(null);
  }, [events]);

  const virtualizer = useVirtualizer({
    count: shown.length,
    getScrollElement: () => listScrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 20,
  });

  const selectedEvent = useMemo(
    () => (selectedId ? sorted.find((e) => e.id === selectedId) ?? null : null),
    [sorted, selectedId]
  );

  function selectEvent(id: string) {
    setSelectedId(id);
    const idx = shown.findIndex((e) => e.id === id);
    if (idx >= 0) virtualizer.scrollToIndex(idx, { align: 'auto' });
  }

  return (
    <div className={focused ? "shell-trajectory-block is-focused" : "shell-trajectory-block"}>
      <div className="traj-header">
        <span className="shell-section-title">Trajectory</span>
        <span className="traj-count">
          {!eventsComplete && eventsTotal != null
            ? `Loading ${sorted.length.toLocaleString()} of ${eventsTotal.toLocaleString()} events…`
            : brushedIds
              ? `${shown.length.toLocaleString()} of ${sorted.length.toLocaleString()} events`
              : `${sorted.length.toLocaleString()} event${sorted.length === 1 ? '' : 's'}`}
        </span>
        {onToggleFocus && (
          <button
            type="button"
            className="traj-expand"
            onClick={onToggleFocus}
            aria-pressed={focused}
            title={focused ? 'Collapse (esc)' : 'Expand the trajectory'}
          >
            <svg width="13" height="13" viewBox="0 0 20 20" fill="none" stroke="currentColor"
                 strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              {focused
                ? <><path d="M9 3v6H3" /><path d="M11 17v-6h6" /></>
                : <><path d="M12 3h5v5" /><path d="M8 17H3v-5" /><path d="M17 3l-6 6" /><path d="M3 17l6-6" /></>}
            </svg>
            {focused ? 'Collapse' : 'Expand'}
          </button>
        )}
      </div>

      <TrajectoryScrubber
        events={sorted}
        selectedId={selectedId}
        onSelect={selectEvent}
        onBrushChange={setBrushedIds}
        compactionEventIds={compactionEventIds}
      />

      {sorted.length === 0 ? (
        <div className="traj-empty">No events recorded for this session.</div>
      ) : (
        <div className="traj-body">
          <div className="traj-list" ref={listScrollRef}>
            <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
              {virtualizer.getVirtualItems().map((vRow) => {
                const evt = shown[vRow.index];
                const payload: RawPayload = evt.payload ?? { type: 'unknown' };
                const type = payload.type || 'unknown';
                const isTurnStart = type === 'user_message';
                const primary = getPrimaryText(payload);
                const result = getResultPreview(payload);
                const weight = getRoleWeight(type);
                const selected = evt.id === selectedId;

                return (
                  <div
                    key={evt.id}
                    role="button"
                    tabIndex={0}
                    data-row-id={evt.id}
                    className={`traj-row${selected ? ' is-selected' : ''}${isTurnStart ? ' is-turn-start' : ''}`}
                    style={{
                      position: 'absolute',
                      top: 0,
                      left: 0,
                      width: '100%',
                      height: vRow.size,
                      transform: `translateY(${vRow.start}px)`,
                    }}
                    onClick={() => selectEvent(evt.id)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        selectEvent(evt.id);
                      }
                    }}
                  >
                    <span className="traj-row-gutter" aria-hidden="true" />
                    <span className="traj-row-seq">{evt.sequence}</span>
                    <span className={`traj-chip traj-chip-${weight}`}>{getRoleLabel(type)}</span>
                    <span className="traj-row-primary" title={primary}>
                      {primary}
                    </span>
                    {result && (
                      <span className="traj-row-result" title={result}>
                        <span className="traj-row-arrow" aria-hidden="true">
                          →
                        </span>
                        {result}
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
          </div>

          <EventDetail event={selectedEvent} />
        </div>
      )}
    </div>
  );
}
