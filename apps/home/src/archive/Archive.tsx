import { useCallback, useRef, useState } from 'react';
import { SessionList, type ShellNav } from './shell/SessionList';
import { InspectorPane } from './shell/InspectorPane';
import { useArchiveKeys } from './useArchiveKeys';
import './archive-shell.css';
import './panes.css';
import './list.css';
import './inspector.css';
import './trajectory.css';

export interface ArchiveProps {
  /** Session id from `/home/s/<id>` (P1 route). Null = list only. */
  sessionId: string | null;
  /** Navigate to an app-relative path (`/s/<id>` or `/`). */
  onNavigate: (appPath: string) => void;
}

/**
 * Archive phase — session list + inspector inside the deck shell.
 *
 * Port of the dashboard explorer's sessions view (SessionList + InspectorPane),
 * wired to P1's `/s/<id>` path. Rail / overview / coverage / archaeology /
 * exports and the command palette stay on the dashboard until P3.
 */
export function Archive({ sessionId, onNavigate }: ArchiveProps) {
  const [liveTail, setLiveTail] = useState(false);
  const [trajectoryFocused, setTrajectoryFocused] = useState(false);
  const navRef = useRef<ShellNav | null>(null);
  const inspectorRegionRef = useRef<HTMLDivElement>(null);

  const exitTrajectoryFocus = useCallback(() => {
    if (!trajectoryFocused) return false;
    setTrajectoryFocused(false);
    return true;
  }, [trajectoryFocused]);

  const focusInspector = useCallback(() => {
    inspectorRegionRef.current?.focus();
  }, []);

  useArchiveKeys({
    exitTrajectoryFocus,
    navRef,
    focusInspector,
  });

  return (
    <div className="shell-root archive-phase enter" data-testid="archive-phase">
      <div className="archive-topbar">
        <span className="archive-topbar-title">Archive</span>
        <div className="archive-topbar-spacer" />
        <button
          type="button"
          className="livetail-btn"
          aria-pressed={liveTail}
          onClick={() => setLiveTail((v) => !v)}
          title="Live tail (stream not yet wired)"
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 6,
            appearance: 'none',
            border: '0.5px solid var(--mv-border)',
            borderRadius: 6,
            background: liveTail ? 'var(--mv-accent-soft)' : 'transparent',
            color: 'var(--mv-muted)',
            fontSize: 10,
            padding: '4px 8px',
            cursor: 'pointer',
          }}
        >
          <span
            aria-hidden
            style={{
              width: 6,
              height: 6,
              borderRadius: '50%',
              background: liveTail ? 'var(--mv-success)' : 'var(--mv-faint)',
            }}
          />
          Live Tail
        </button>
        <span className="archive-topbar-hint">
          <kbd>j</kbd>/<kbd>k</kbd> move · <kbd>/</kbd> filter · <kbd>esc</kbd> leave
        </span>
      </div>

      <div className="shell-body">
        {!trajectoryFocused && (
          <div className="list-region">
            <SessionList
              selectedId={sessionId}
              onSelect={(id) => onNavigate(`/s/${encodeURIComponent(id)}`)}
              registerNav={(nav) => {
                navRef.current = nav;
              }}
              liveTail={liveTail}
            />
          </div>
        )}

        <div className="inspector-region" ref={inspectorRegionRef} tabIndex={-1}>
          <InspectorPane
            sessionId={sessionId}
            liveTail={liveTail}
            trajectoryFocused={trajectoryFocused}
            onToggleTrajectoryFocus={() => setTrajectoryFocused((v) => !v)}
          />
        </div>
      </div>
    </div>
  );
}
