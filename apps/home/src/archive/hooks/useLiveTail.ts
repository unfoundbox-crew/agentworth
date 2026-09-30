import { useEffect, useRef, useState } from 'react';

/** Matches `LiveTailChangeKind` in apps/cli/src/server/live_tail.rs. */
export type LiveTailChangeKind = 'created' | 'modified' | 'removed' | 'other';

/** Matches `LiveTailEvent` on the wire from GET /api/live-tail. */
export interface LiveTailEvent {
  path: string;
  kind: LiveTailChangeKind;
  adapter: string | null;
  timestamp: string;
}

/**
 * Connection state for the live-tail SSE stream.
 *
 * `idle` means the toggle is off (no socket). `connecting` covers the gap
 * between open and the first `onopen`. `open` is a live EventSource.
 * `error` is a broken stream that the browser is still trying to reconnect
 * for — EventSource reconnects on its own, so we do not tear it down.
 */
export type LiveTailStatus = 'idle' | 'connecting' | 'open' | 'error';

export interface LiveTailState {
  status: LiveTailStatus;
  /** Most recent `change` event, if any have arrived this connection. */
  lastEvent: LiveTailEvent | null;
  /** How many `change` events have arrived this connection. */
  eventCount: number;
  /** Most recent `lagged` skip count from the server, if any. */
  laggedSkipped: number | null;
  /**
   * Monotonic counter bumped (debounced) when a filesystem change arrives.
   * Pass it as a reload/refresh signal so the session list and fleet strip
   * re-read without waiting for their own poll intervals.
   */
  changeEpoch: number;
}

const INITIAL: LiveTailState = {
  status: 'idle',
  lastEvent: null,
  eventCount: 0,
  laggedSkipped: null,
  changeEpoch: 0,
};

/** Collapse a burst of FS events into one list/fleet refresh. */
const CHANGE_EPOCH_DEBOUNCE_MS = 800;

function parseChange(data: string): LiveTailEvent | null {
  try {
    const raw = JSON.parse(data) as Partial<LiveTailEvent>;
    if (typeof raw.path !== 'string' || typeof raw.kind !== 'string') return null;
    return {
      path: raw.path,
      kind: raw.kind as LiveTailChangeKind,
      adapter: typeof raw.adapter === 'string' ? raw.adapter : null,
      timestamp: typeof raw.timestamp === 'string' ? raw.timestamp : new Date().toISOString(),
    };
  } catch {
    return null;
  }
}

/**
 * Opens `GET /api/live-tail` (SSE) while `enabled` is true and closes it when
 * the toggle turns off or the component unmounts.
 *
 * The server already exists (`apps/cli/src/server/live_tail.rs` + the
 * `get_live_tail_handler` route). This hook is the missing client side of
 * the Live Tail toggle that previously rendered "awaiting stream, not yet
 * wired" in the inspector banner.
 */
export function useLiveTail(enabled: boolean): LiveTailState {
  const [state, setState] = useState<LiveTailState>(INITIAL);
  const epochTimer = useRef<number | null>(null);

  useEffect(() => {
    if (!enabled) {
      setState(INITIAL);
      return;
    }

    if (typeof EventSource === 'undefined') {
      setState({
        ...INITIAL,
        status: 'error',
      });
      return;
    }

    setState({
      status: 'connecting',
      lastEvent: null,
      eventCount: 0,
      laggedSkipped: null,
      changeEpoch: 0,
    });

    const source = new EventSource('/api/live-tail');

    const bumpEpoch = () => {
      if (epochTimer.current !== null) window.clearTimeout(epochTimer.current);
      epochTimer.current = window.setTimeout(() => {
        epochTimer.current = null;
        setState((prev) => ({ ...prev, changeEpoch: prev.changeEpoch + 1 }));
      }, CHANGE_EPOCH_DEBOUNCE_MS);
    };

    source.onopen = () => {
      setState((prev) => ({ ...prev, status: 'open' }));
    };

    source.addEventListener('change', (evt) => {
      const message = evt as MessageEvent<string>;
      const parsed = parseChange(message.data);
      if (!parsed) return;
      setState((prev) => ({
        ...prev,
        status: 'open',
        lastEvent: parsed,
        eventCount: prev.eventCount + 1,
      }));
      bumpEpoch();
    });

    source.addEventListener('lagged', (evt) => {
      const message = evt as MessageEvent<string>;
      const skipped = Number.parseInt(message.data, 10);
      setState((prev) => ({
        ...prev,
        status: 'open',
        laggedSkipped: Number.isFinite(skipped) ? skipped : prev.laggedSkipped,
      }));
      bumpEpoch();
    });

    source.onerror = () => {
      // EventSource reconnects automatically; surface the broken moment so
      // the banner does not keep claiming the stream is live.
      setState((prev) => ({
        ...prev,
        status: source.readyState === EventSource.CLOSED ? 'error' : 'error',
      }));
    };

    return () => {
      if (epochTimer.current !== null) {
        window.clearTimeout(epochTimer.current);
        epochTimer.current = null;
      }
      source.close();
    };
  }, [enabled]);

  return state;
}

/** Basename for the banner — full paths are noise in an 11px mono line. */
export function liveTailPathLabel(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1]! : path;
}

/** One-line banner copy for the inspector livetail strip. */
export function liveTailBannerText(state: LiveTailState): string {
  if (state.status === 'connecting') return 'Live tail — connecting…';
  if (state.status === 'error') return 'Live tail — stream interrupted, retrying…';
  if (state.status === 'idle') return 'Live tail — off';

  if (state.laggedSkipped != null && state.lastEvent == null) {
    return `Live tail — listening (lagged ${state.laggedSkipped})`;
  }

  if (state.lastEvent) {
    const who = state.lastEvent.adapter ?? 'session';
    const file = liveTailPathLabel(state.lastEvent.path);
    const lag =
      state.laggedSkipped != null ? ` · lagged ${state.laggedSkipped}` : '';
    return `Live tail — ${who} ${state.lastEvent.kind} ${file}${lag}`;
  }

  return 'Live tail — listening';
}
