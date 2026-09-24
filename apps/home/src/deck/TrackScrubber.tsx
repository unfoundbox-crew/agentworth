import { memo, useRef } from 'react';
import type { Stop } from '../protocol';
import {
  forkState,
  fracToIndex,
  positionsByTime,
  stepIndex,
  TRACK_POS_MAX,
  TRACK_POS_MIN,
  type TrackEvent,
} from '../model/trackTimeline';

/**
 * The course track strip with the scrubber on it. Evidence stops sit where they always
 * did (time left to right); the session's distilled moments are the scrub surface under
 * them; the two shaped marks DESIGN.md names — the handoff arrow and the fork on retry —
 * render only where the feed reports a handoff or an error, and nowhere else. A playhead
 * snaps to moments on drag and on arrow keys; the track below reads the timeline state at
 * the playhead. The scrub is UI-side state: nothing is stored, nothing re-fetched, and
 * reaching the last moment returns the track to live.
 *
 * Degrades honestly: no timeline moments (a paused or not-yet-indexed rider) leaves the
 * plain stop strip and says "no session timeline yet"; a session with no handoffs or
 * errors shows no marks.
 */

export interface TrackScrubberProps {
  events: TrackEvent[];
  stops: Stop[];
  /** The playhead's snapped event index, or null for live (following the end). */
  scrub: number | null;
  truncated: boolean;
  onScrub: (index: number) => void;
  onLive: () => void;
}

/** Past this many moments, per-event ticks stay unrendered; snapping stays exact. */
const MAX_RENDERED_TICKS = 200;

function fmtClock(atMs: number): string {
  const d = new Date(atMs);
  const two = (n: number) => String(n).padStart(2, '0');
  return `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}`;
}

/**
 * Percent along the strip for a time inside [min, max] — the same placement math the
 * stop strip used before the scrubber, so the dots do not move when a timeline loads.
 */
function percentIn(ms: number, min: number, span: number): number {
  return TRACK_POS_MIN + (span === 0 ? 0 : (ms - min) / span) * (TRACK_POS_MAX - TRACK_POS_MIN);
}

function StopDot({ left, label, dimmed, rung }: { left: number; label: string; dimmed: boolean; rung: string }) {
  return (
    <div
      role="img"
      aria-label={label}
      tabIndex={0}
      data-rung={rung}
      className="track-stop absolute top-1/2 -translate-y-1/2 w-2 h-2 rounded-full border border-ink bg-[var(--mv-ground)]"
      style={{ left: `${left}%`, opacity: dimmed ? 0.35 : 1 }}
      title={label}
    />
  );
}

function HandoffMark({ left, label }: { left: number; label: string }) {
  return (
    <div
      data-marker="handoff"
      className="absolute top-[38%] -translate-x-1/2 text-accent"
      style={{ left: `${left}%` }}
      title={`handoff · ${label}`}
      aria-label={`handoff at this point: ${label}`}
    >
      <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
        <path d="M1 8V7c0-1.6 1-2.6 2.6-2.6H7" stroke="currentColor" strokeWidth="1.2" />
        <path d="M5.4 2.4 7.6 4.4 5.4 6.4" stroke="currentColor" strokeWidth="1.2" fill="none" />
      </svg>
    </div>
  );
}

function ForkMark({ left, retried, label }: { left: number; retried: boolean; label: string }) {
  return (
    <div
      data-marker="fork"
      data-state={retried ? 'retried' : 'open'}
      className="absolute top-[38%] -translate-x-1/2 text-warn"
      style={{ left: `${left}%` }}
      title={`fork on ${retried ? 'retry' : 'unretried error'} · ${label}`}
      aria-label={`fork on retry at this point: ${label} (${retried ? 'retried' : 'retry pending'})`}
    >
      <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
        <path d="M5 9V5.5M5 5.5 2 2.5M5 5.5l3-3" stroke="currentColor" strokeWidth="1.2" />
      </svg>
    </div>
  );
}

function TrackSurface({
  events,
  stops,
  scrub,
  onScrub,
  onLive,
}: {
  events: TrackEvent[];
  stops: Stop[];
  scrub: number | null;
  onScrub: (index: number) => void;
  onLive: () => void;
}) {
  const trackRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);
  const min = events[0].atMs;
  const max = events[events.length - 1].atMs;
  const span = Math.max(1, max - min);
  const live = scrub === null;
  const active = scrub ?? events.length - 1;
  // Live playhead rides the right edge; scrubbed, it sits on the snapped moment.
  const playheadAtMs = live ? max : events[active].atMs;

  const fracFromPointer = (clientX: number): number => {
    const rect = trackRef.current?.getBoundingClientRect();
    if (!rect || rect.width <= 0) return 0;
    return Math.min(Math.max((clientX - rect.left) / rect.width, 0), 1);
  };
  const scrubTo = (frac: number) => onScrub(fracToIndex(events, frac));

  return (
    <div
      ref={trackRef}
      role="slider"
      tabIndex={0}
      aria-label="session timeline scrubber"
      aria-valuemin={0}
      aria-valuemax={events.length - 1}
      aria-valuenow={active}
      aria-valuetext={live ? 'live' : `at ${events[active].at}`}
      className="absolute inset-0 cursor-crosshair touch-none select-none"
      data-track="surface"
      onPointerDown={(e) => {
        dragging.current = true;
        e.currentTarget.setPointerCapture(e.pointerId);
        scrubTo(fracFromPointer(e.clientX));
      }}
      onPointerMove={(e) => {
        if (dragging.current) scrubTo(fracFromPointer(e.clientX));
      }}
      onPointerUp={(e) => {
        dragging.current = false;
        e.currentTarget.releasePointerCapture(e.pointerId);
      }}
      onKeyDown={(e) => {
        if (e.key === 'ArrowRight') {
          e.preventDefault();
          onScrub(stepIndex(events, active, 1));
        } else if (e.key === 'ArrowLeft') {
          e.preventDefault();
          onScrub(stepIndex(events, active, -1));
        } else if (e.key === 'Home') {
          e.preventDefault();
          onScrub(0);
        } else if (e.key === 'End') {
          e.preventDefault();
          onLive();
        }
      }}
    >
      {events.length <= MAX_RENDERED_TICKS &&
        positionsByTime(events).map((p, i) => (
          <div
            key={events[i].key}
            role="presentation"
            className="absolute top-1/2 w-px h-2 -translate-y-1/2 bg-[var(--mv-border)]"
            style={{ left: `${p}%` }}
          />
        ))}

      {stops.map((s) => (
        <StopDot
          key={s.id}
          left={percentIn(Date.parse(s.at), min, span)}
          label={`${s.rung} · ${s.from} · ${s.at}`}
          dimmed={Date.parse(s.at) > playheadAtMs}
          rung={s.rung}
        />
      ))}

      {events.map((e, i) => {
        if (e.kind === 'handoff') {
          return <HandoffMark key={e.key} left={percentIn(e.atMs, min, span)} label={e.text} />;
        }
        const fork = forkState(events, i);
        if (fork) {
          return (
            <ForkMark
              key={e.key}
              left={percentIn(e.atMs, min, span)}
              retried={fork === 'retried'}
              label={e.text}
            />
          );
        }
        return null;
      })}

      {!live && scrub !== null && (
        <div
          data-track="playhead"
          className="absolute inset-y-0 w-px bg-[var(--mv-ink)] opacity-60"
          style={{ left: `${percentIn(events[scrub].atMs, min, span)}%` }}
        >
          <div className="absolute top-0 left-1/2 -translate-x-1/2 w-1.5 h-1.5 bg-[var(--mv-ink)]" />
        </div>
      )}
    </div>
  );
}

/** The stops-only strip: the stops are the span, exactly like the pre-scrubber strip. */
function StopsOnly({ stops }: { stops: Stop[] }) {
  if (stops.length === 0) {
    return (
      <div className="absolute inset-0 flex items-center justify-center text-[10px] text-dim">
        no evidence yet
      </div>
    );
  }
  const times = stops.map((s) => Date.parse(s.at));
  const min = Math.min(...times);
  const max = Math.max(...times);
  const span = Math.max(1, max - min);
  return (
    <>
      {stops.map((s) => (
        <StopDot
          key={s.id}
          left={percentIn(Date.parse(s.at), min, span)}
          label={`${s.rung} · ${s.from} · ${s.at}`}
          dimmed={false}
          rung={s.rung}
        />
      ))}
      <div className="absolute left-2 bottom-1 text-[9px] text-dim" data-track="no-timeline">
        no session timeline yet
      </div>
    </>
  );
}

function TrackScrubberInner({ events, stops, scrub, truncated, onScrub, onLive }: TrackScrubberProps) {
  const live = scrub === null;

  return (
    <div className="mt-4 rounded-lg border border-line bg-[var(--mv-ground)]" data-track="scrubber">
      <div className="relative" style={{ height: 64 }}>
        <div className="absolute right-2 top-1 z-10 flex items-center gap-2">
          {live ? (
            <span className="text-[9px] text-accent" data-track="live-label">now</span>
          ) : (
            <>
              <span className="text-[9px] text-muted tabular-nums" data-track="scrub-clock">
                {fmtClock(events[scrub]?.atMs ?? 0)}
              </span>
              <button
                type="button"
                onClick={onLive}
                className="text-[9px] text-accent hover:text-ink"
                data-track="live-button"
              >
                live
              </button>
            </>
          )}
        </div>

        {events.length === 0 ? (
          <StopsOnly stops={stops} />
        ) : (
          <TrackSurface events={events} stops={stops} scrub={scrub} onScrub={onScrub} onLive={onLive} />
        )}
      </div>
      {truncated && (
        <div className="px-2 pb-1 text-[9px] text-dim" data-track="truncated-note">
          showing the most recent of a long session; earlier moments are not on the strip
        </div>
      )}
    </div>
  );
}

export const TrackScrubber = memo(TrackScrubberInner);
