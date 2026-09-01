import { useEffect, useMemo, useRef, useState } from 'react';
import { NormalizedEvent } from '../types';
import { EventGroup, getEventGroup } from './EventDetail';

const ROWS: { key: EventGroup; label: string }[] = [
  { key: 'messages', label: 'Msgs' },
  { key: 'model', label: 'Model' },
  { key: 'tools', label: 'Tools' },
];

// Target ~4px per tick (3px tick + 1px gap). Bucket count comes from the
// measured container width, not from event count — that's what keeps the
// strip legible whether the session has 50 events or 5000: more events per
// bucket just reads as a denser tick, the tick grid itself doesn't grow.
const PX_PER_TICK = 4;
const MIN_BUCKETS = 40;
const MAX_BUCKETS = 400;
const DEFAULT_WIDTH = 700;

interface Bucket {
  count: number;
  firstId: string | null;
}

export interface TimelineStripProps {
  /** Must already be sorted ascending by `sequence` — TrajectoryView owns that sort so this and the row list agree on order. */
  events: NormalizedEvent[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}

/** A thin per-kind tick strip spanning the whole session, so activity shape
 * and clustering are visible before scrolling anything. Ticks are bucketed
 * by sequence range, not rendered one-per-event — see PX_PER_TICK above. */
export function TimelineStrip({ events, selectedId, onSelect }: TimelineStripProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(DEFAULT_WIDTH);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const observer = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width;
      if (w && w > 0) setWidth(w);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const bucketCount =
    events.length === 0 ? 0 : Math.max(MIN_BUCKETS, Math.min(MAX_BUCKETS, Math.round(width / PX_PER_TICK)));

  const { buckets, maxCount, selectedBucket } = useMemo(() => {
    const empty = { buckets: {} as Record<EventGroup, Bucket[]>, maxCount: {} as Record<EventGroup, number>, selectedBucket: {} as Record<EventGroup, number> };
    if (events.length === 0 || bucketCount === 0) return empty;

    const byGroup: Record<EventGroup, Bucket[]> = {
      messages: Array.from({ length: bucketCount }, () => ({ count: 0, firstId: null })),
      model: Array.from({ length: bucketCount }, () => ({ count: 0, firstId: null })),
      tools: Array.from({ length: bucketCount }, () => ({ count: 0, firstId: null })),
    };
    const minSeq = events[0].sequence;
    const maxSeq = events[events.length - 1].sequence;
    const range = Math.max(1, maxSeq - minSeq);

    const indexFor = (sequence: number) =>
      Math.min(bucketCount - 1, Math.max(0, Math.floor(((sequence - minSeq) / range) * bucketCount)));

    let selectedEventSeq: number | null = null;
    let selectedGroup: EventGroup | null = null;

    for (const evt of events) {
      const type = evt.payload?.type ?? 'unknown';
      const group = getEventGroup(type);
      const idx = indexFor(evt.sequence);
      const bucket = byGroup[group][idx];
      bucket.count += 1;
      if (bucket.firstId === null) bucket.firstId = evt.id;
      if (evt.id === selectedId) {
        selectedEventSeq = evt.sequence;
        selectedGroup = group;
      }
    }

    const maxCountByGroup: Record<EventGroup, number> = { messages: 0, model: 0, tools: 0 };
    (Object.keys(byGroup) as EventGroup[]).forEach((g) => {
      maxCountByGroup[g] = byGroup[g].reduce((m, b) => Math.max(m, b.count), 0);
    });

    const selBucket: Record<EventGroup, number> = { messages: -1, model: -1, tools: -1 };
    if (selectedGroup && selectedEventSeq !== null) {
      selBucket[selectedGroup] = indexFor(selectedEventSeq);
    }

    return { buckets: byGroup, maxCount: maxCountByGroup, selectedBucket: selBucket };
  }, [events, bucketCount, selectedId]);

  if (events.length === 0) {
    return (
      <div className="traj-strip traj-strip-empty" ref={containerRef}>
        <span className="traj-empty-text">No events.</span>
      </div>
    );
  }

  return (
    <div className="traj-strip" ref={containerRef}>
      {ROWS.map((row) => {
        const rowBuckets = buckets[row.key] ?? [];
        const rowMax = maxCount[row.key] ?? 0;
        const selIdx = selectedBucket[row.key] ?? -1;
        return (
          <div className="traj-strip-row" key={row.key}>
            <span className="traj-strip-label">{row.label}</span>
            <div className="traj-strip-ticks">
              {rowBuckets.map((bucket, i) =>
                bucket.count > 0 ? (
                  <button
                    type="button"
                    key={i}
                    className={`traj-tick${i === selIdx ? ' is-selected' : ''}`}
                    style={{ opacity: rowMax > 0 ? 0.28 + (bucket.count / rowMax) * 0.72 : 0.28 }}
                    title={`${bucket.count} event${bucket.count === 1 ? '' : 's'}`}
                    onClick={() => bucket.firstId && onSelect(bucket.firstId)}
                  />
                ) : (
                  <span key={i} className="traj-tick traj-tick-empty" aria-hidden="true" />
                )
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}
