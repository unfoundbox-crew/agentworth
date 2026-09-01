import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { NormalizedEvent } from '../types';
import { EventGroup, getEventGroup } from './EventDetail';
import { Axis, buildAxis, formatDuration, formatRange } from '../utils/timeAxis';

const ROWS: { key: EventGroup; label: string }[] = [
  { key: 'messages', label: 'Msgs' },
  { key: 'model', label: 'Model' },
  { key: 'tools', label: 'Tools' },
];

const PX_PER_BUCKET = 4;
const MIN_BUCKETS = 40;
const MAX_BUCKETS = 400;
const DEFAULT_WIDTH = 700;
const ZOOM_STEP = 2;
/** Below this the window is treated as the full range, absorbing float drift. */
const FULL_EPSILON = 1e-9;
/** A drag shorter than this is a click, not a brush. */
const DRAG_SLOP_PX = 3;

export interface Window {
  from: number;
  to: number;
}

interface Bucket {
  count: number;
  firstId: string | null;
  selected: boolean;
}

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v));
}

/** Moves a window without changing its width, keeping it inside [0,1]. */
function panned(win: Window, delta: number): Window {
  const width = win.to - win.from;
  const from = Math.min(Math.max(0, win.from + delta), 1 - width);
  return { from, to: from + width };
}

/** Scales a window about a fixed point, clamped to [0,1] and to `minWidth`. */
function zoomed(win: Window, factor: number, focus: number, minWidth: number): Window {
  const width = Math.min(1, Math.max(minWidth, (win.to - win.from) / factor));
  const from = Math.min(Math.max(0, focus - (focus - win.from) * (width / (win.to - win.from))), 1 - width);
  return { from, to: from + width };
}

export interface TrajectoryScrubberProps {
  /** Sorted ascending by `sequence` — the stream's order, which the scrubber re-sorts by time. */
  events: NormalizedEvent[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  /** Reports the brushed sub-range as the event ids the stream should show, or null for all. */
  onBrushChange?: (ids: Set<string> | null) => void;
}

/**
 * Overview rail plus zoomable detail strip.
 *
 * The two surfaces exist because "drag to pan" and "drag to select a range"
 * are the same gesture: panning lives on the rail, brushing on the strip, so
 * neither needs a modifier key.
 */
export function TrajectoryScrubber({ events, selectedId, onSelect, onBrushChange }: TrajectoryScrubberProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const stripRef = useRef<HTMLDivElement>(null);
  const railRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(DEFAULT_WIDTH);
  const [win, setWin] = useState<Window>({ from: 0, to: 1 });
  const [brush, setBrush] = useState<Window | null>(null);

  const axis = useMemo<Axis>(() => buildAxis(events), [events]);
  const points = axis.points;

  // Never zoom past roughly four events, however large the session — a window
  // holding nothing is a dead end the user has to escape from.
  const minWidth = points.length > 4 ? Math.max(1e-4, 4 / points.length) : 1;

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

  // A different session is a different axis; keep no zoom or brush across it.
  useEffect(() => {
    setWin({ from: 0, to: 1 });
    setBrush(null);
  }, [events]);

  const isFullRange = win.from <= FULL_EPSILON && win.to >= 1 - FULL_EPSILON;

  const visible = useMemo(
    () => points.filter((p) => p.position >= win.from && p.position <= win.to),
    [points, win]
  );

  const bucketCount = Math.max(MIN_BUCKETS, Math.min(MAX_BUCKETS, Math.round(width / PX_PER_BUCKET)));

  const { buckets, maxCount } = useMemo(() => {
    const byGroup: Record<EventGroup, Bucket[]> = {
      messages: Array.from({ length: bucketCount }, () => ({ count: 0, firstId: null, selected: false })),
      model: Array.from({ length: bucketCount }, () => ({ count: 0, firstId: null, selected: false })),
      tools: Array.from({ length: bucketCount }, () => ({ count: 0, firstId: null, selected: false })),
    };
    const span = Math.max(win.to - win.from, Number.MIN_VALUE);
    for (const p of visible) {
      const idx = Math.min(bucketCount - 1, Math.max(0, Math.floor(((p.position - win.from) / span) * bucketCount)));
      const group = getEventGroup(p.event.payload?.type ?? 'unknown');
      const bucket = byGroup[group][idx];
      bucket.count += 1;
      if (bucket.firstId === null) bucket.firstId = p.event.id;
      if (p.event.id === selectedId) bucket.selected = true;
    }
    const max: Record<EventGroup, number> = { messages: 0, model: 0, tools: 0 };
    (Object.keys(byGroup) as EventGroup[]).forEach((g) => {
      max[g] = byGroup[g].reduce((m, b) => Math.max(m, b.count), 0);
    });
    return { buckets: byGroup, maxCount: max };
  }, [visible, bucketCount, win, selectedId]);

  const brushedIds = useMemo(() => {
    if (!brush) return null;
    const lo = Math.min(brush.from, brush.to);
    const hi = Math.max(brush.from, brush.to);
    return new Set(points.filter((p) => p.position >= lo && p.position <= hi).map((p) => p.event.id));
  }, [brush, points]);

  useEffect(() => {
    onBrushChange?.(brushedIds);
  }, [brushedIds, onBrushChange]);

  // A selection made elsewhere (stream click, deep link) may sit outside the
  // current window; widen just enough to include it rather than leaving the
  // strip pointing somewhere the stream isn't.
  useEffect(() => {
    if (!selectedId || isFullRange) return;
    const hit = points.find((p) => p.event.id === selectedId);
    if (!hit) return;
    setWin((prev) => {
      if (hit.position >= prev.from && hit.position <= prev.to) return prev;
      const width = prev.to - prev.from;
      const from = clamp01(Math.min(Math.max(0, hit.position - width / 2), 1 - width));
      return { from, to: from + width };
    });
    // Only react to the selection moving, not to the window the user is driving.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId]);

  const resetView = useCallback(() => {
    setWin({ from: 0, to: 1 });
    setBrush(null);
  }, []);

  const zoomBy = useCallback(
    (factor: number) => {
      setWin((prev) => {
        const focus = brush ? (brush.from + brush.to) / 2 : (prev.from + prev.to) / 2;
        return zoomed(prev, factor, focus, minWidth);
      });
    },
    [brush, minWidth]
  );

  /** Pointer x -> position in [0,1] within an element. */
  const positionIn = (el: HTMLElement, clientX: number) => {
    const rect = el.getBoundingClientRect();
    return clamp01(rect.width <= 0 ? 0 : (clientX - rect.left) / rect.width);
  };

  const onRailPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    const rail = e.currentTarget;
    const start = positionIn(rail, e.clientX);
    const width = win.to - win.from;
    const edge = Math.max(0.01, 8 / Math.max(rail.getBoundingClientRect().width, 1));

    let mode: 'pan' | 'left' | 'right';
    if (!isFullRange && Math.abs(start - win.from) <= edge) mode = 'left';
    else if (!isFullRange && Math.abs(start - win.to) <= edge) mode = 'right';
    else if (!isFullRange && start > win.from && start < win.to) mode = 'pan';
    else {
      // Clicking outside the window jumps it there, centred, same width.
      const from = Math.min(Math.max(0, start - width / 2), 1 - width);
      setWin({ from, to: from + width });
      mode = 'pan';
    }

    const origin = start;
    const originWin = { ...win };
    rail.setPointerCapture(e.pointerId);

    const onMove = (ev: PointerEvent) => {
      const at = positionIn(rail, ev.clientX);
      if (mode === 'pan') {
        setWin(panned(originWin, at - origin));
      } else if (mode === 'left') {
        const from = Math.min(at, originWin.to - minWidth);
        setWin({ from: clamp01(from), to: originWin.to });
      } else {
        const to = Math.max(at, originWin.from + minWidth);
        setWin({ from: originWin.from, to: clamp01(to) });
      }
    };
    const onUp = () => {
      rail.releasePointerCapture?.(e.pointerId);
      rail.removeEventListener('pointermove', onMove);
      rail.removeEventListener('pointerup', onUp);
      rail.removeEventListener('pointercancel', onUp);
    };
    rail.addEventListener('pointermove', onMove);
    rail.addEventListener('pointerup', onUp);
    rail.addEventListener('pointercancel', onUp);
  };

  const onStripPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    const strip = e.currentTarget;
    const rect = strip.getBoundingClientRect();
    const startX = e.clientX;
    const toWindowPos = (clientX: number) =>
      win.from + positionIn(strip, clientX) * (win.to - win.from);
    const origin = toWindowPos(startX);
    let dragged = false;
    strip.setPointerCapture(e.pointerId);

    const onMove = (ev: PointerEvent) => {
      if (!dragged && Math.abs(ev.clientX - startX) < DRAG_SLOP_PX) return;
      dragged = true;
      const at = toWindowPos(ev.clientX);
      setBrush({ from: Math.min(origin, at), to: Math.max(origin, at) });
    };
    const onUp = (ev: PointerEvent) => {
      strip.releasePointerCapture?.(e.pointerId);
      strip.removeEventListener('pointermove', onMove);
      strip.removeEventListener('pointerup', onUp);
      strip.removeEventListener('pointercancel', onUp);
      if (dragged) return;
      // A click, not a drag: pick the nearest event, and clear any brush.
      setBrush(null);
      const at = win.from + ((ev.clientX - rect.left) / Math.max(rect.width, 1)) * (win.to - win.from);
      let best: string | null = null;
      let bestDist = Infinity;
      for (const p of visible) {
        const d = Math.abs(p.position - at);
        if (d < bestDist) {
          bestDist = d;
          best = p.event.id;
        }
      }
      if (best) onSelect(best);
    };
    strip.addEventListener('pointermove', onMove);
    strip.addEventListener('pointerup', onUp);
    strip.addEventListener('pointercancel', onUp);
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const width = win.to - win.from;
    let handled = true;
    switch (e.key) {
      case '+':
      case '=':
        zoomBy(ZOOM_STEP);
        break;
      case '-':
      case '_':
        zoomBy(1 / ZOOM_STEP);
        break;
      case '0':
        resetView();
        break;
      case 'ArrowLeft':
        setWin((p) => panned(p, -width * (e.shiftKey ? 1 : 0.1)));
        break;
      case 'ArrowRight':
        setWin((p) => panned(p, width * (e.shiftKey ? 1 : 0.1)));
        break;
      default:
        handled = false;
    }
    if (handled) {
      e.preventDefault();
      // useShellKeys listens on document and only skips text inputs, so arrows
      // would otherwise also move the session list selection.
      e.stopPropagation();
    }
  };

  if (points.length === 0) {
    return (
      <div className="traj-strip traj-strip-empty" ref={containerRef}>
        <span className="traj-empty-text">No events.</span>
      </div>
    );
  }

  const zoomLabel = isFullRange ? 'full' : `${(1 / (win.to - win.from)).toFixed(1)}×`;
  const rangeLabel =
    axis.kind === 'time'
      ? formatRange(axis.timeAt(win.from), axis.timeAt(win.to))
      : `events ${Math.round(win.from * points.length) + 1}–${Math.round(win.to * points.length)}`;

  return (
    <div
      className="traj-scrubber"
      ref={containerRef}
      tabIndex={0}
      onKeyDown={onKeyDown}
      role="group"
      aria-label="Trajectory scrubber"
    >
      {axis.kind === 'sequence' && (
        <div className="traj-axis-note">
          {axis.reason === 'no-span'
            ? 'Every event shares one timestamp — showing sequence order.'
            : 'Time data unavailable for this session — showing sequence order.'}
        </div>
      )}

      <div className="traj-scrub-head">
        <span className="traj-scrub-range">{rangeLabel}</span>
        <span className="traj-scrub-zoom">{zoomLabel}</span>
        <div className="traj-zoom-controls">
          <button
            type="button"
            className="traj-zoom-btn"
            onClick={() => zoomBy(1 / ZOOM_STEP)}
            disabled={isFullRange}
            title="Zoom out (−)"
            aria-label="Zoom out"
          >
            −
          </button>
          <button
            type="button"
            className="traj-zoom-btn"
            onClick={() => zoomBy(ZOOM_STEP)}
            disabled={win.to - win.from <= minWidth * 1.001}
            title="Zoom in (+)"
            aria-label="Zoom in"
          >
            +
          </button>
          <button
            type="button"
            className="traj-zoom-btn"
            onClick={resetView}
            disabled={isFullRange && !brush}
            title="Reset zoom (0)"
            aria-label="Reset zoom"
          >
            0
          </button>
        </div>
        {brush && (
          <button type="button" className="traj-clear-brush" onClick={() => setBrush(null)}>
            Clear range
          </button>
        )}
      </div>

      <div
        className="traj-rail"
        ref={railRef}
        onPointerDown={onRailPointerDown}
        onDoubleClick={resetView}
        title="Drag the window to pan, its edges to zoom"
      >
        {axis.kind === 'time' &&
          axis.gaps.map((g, i) => (
            <span
              key={i}
              className="traj-rail-gap"
              style={{ left: `${g.from * 100}%`, width: `${Math.max(g.to - g.from, 0.002) * 100}%` }}
              title={`idle ${formatDuration(g.durationMs)}`}
            />
          ))}
        {points.map((p, i) =>
          i % Math.ceil(points.length / 600) === 0 ? (
            <span key={p.event.id} className="traj-rail-tick" style={{ left: `${p.position * 100}%` }} />
          ) : null
        )}
        {!isFullRange && (
          <span
            className="traj-rail-window"
            style={{ left: `${win.from * 100}%`, width: `${(win.to - win.from) * 100}%` }}
          />
        )}
      </div>

      <div className="traj-strip" ref={stripRef} onPointerDown={onStripPointerDown}>
        {visible.length === 0 ? (
          <div className="traj-zoom-empty">
            <span>No events in this range.</span>
            <button type="button" className="traj-zoom-reset" onClick={resetView}>
              Reset zoom
            </button>
          </div>
        ) : (
          ROWS.map((row, rowIndex) => {
            const rowBuckets = buckets[row.key] ?? [];
            const rowMax = maxCount[row.key] ?? 0;
            return (
              <div className="traj-strip-row" key={row.key}>
                <span className={`traj-strip-label traj-strip-label-${rowIndex + 1}`}>{row.label}</span>
                <div className="traj-strip-ticks">
                  {rowBuckets.map((bucket, i) =>
                    bucket.count > 0 ? (
                      <span
                        key={i}
                        className={`traj-tick traj-tick-${rowIndex + 1}${bucket.selected ? ' is-selected' : ''}`}
                        style={{ opacity: rowMax > 0 ? 0.28 + (bucket.count / rowMax) * 0.72 : 0.28 }}
                        title={`${bucket.count} event${bucket.count === 1 ? '' : 's'}`}
                      />
                    ) : (
                      <span key={i} className="traj-tick traj-tick-empty" aria-hidden="true" />
                    )
                  )}
                </div>
              </div>
            );
          })
        )}

        {brush && visible.length > 0 && (
          <span
            className="traj-brush"
            style={{
              left: `${((Math.max(brush.from, win.from) - win.from) / (win.to - win.from)) * 100}%`,
              width: `${((Math.min(brush.to, win.to) - Math.max(brush.from, win.from)) / (win.to - win.from)) * 100}%`,
            }}
          />
        )}
      </div>
    </div>
  );
}
