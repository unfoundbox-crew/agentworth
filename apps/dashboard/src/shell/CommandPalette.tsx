import { useEffect, useMemo, useRef, useState } from 'react';
import type { KeyboardEvent } from 'react';
import { useTheme } from '@ui/useTheme';
import type { Theme } from '@ui/useTheme';
import type { RailViewId } from './Rail';
import { Archie } from '@ui/Archie';
import { useArchieSettings } from './archieKit';

interface Command {
  id: string;
  label: string;
  /** 'real' commands do the thing; 'demo' commands toast that they're not wired yet. */
  tag: 'real' | 'demo';
  run: () => void;
}

export interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
  liveTail: boolean;
  onToggleLiveTail: () => void;
  sessionId: string | null;
  showToast: (message: string) => void;
  /** Switches the rail's active view — same state Rail's buttons drive. */
  onNavigateView: (view: RailViewId) => void;
}

const THEME_ORDER: Theme[] = ['light', 'dark', 'system'];

function nextTheme(current: Theme): Theme {
  const idx = THEME_ORDER.indexOf(current);
  return THEME_ORDER[(idx + 1) % THEME_ORDER.length];
}

export function CommandPalette({
  open,
  onClose,
  liveTail,
  onToggleLiveTail,
  sessionId,
  showToast,
  onNavigateView,
}: CommandPaletteProps) {
  const { theme, setTheme } = useTheme();
  const archie = useArchieSettings();
  const [query, setQuery] = useState('');
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const previouslyFocused = useRef<HTMLElement | null>(null);

  const commands = useMemo<Command[]>(
    () => [
      {
        id: 'open',
        label: 'Open session…',
        tag: 'demo',
        run: () => showToast('Use j/k to browse, Enter to inspect.'),
      },
      {
        id: 'livetail',
        label: 'Toggle live tail',
        tag: 'real',
        run: () => {
          onToggleLiveTail();
          showToast(`Live tail ${liveTail ? 'off' : 'on'}`);
        },
      },
      {
        id: 'export',
        label: 'Go to Exports',
        tag: 'real',
        run: () => onNavigateView('exports'),
      },
      {
        id: 'filter-failed',
        label: 'Filter: failed only',
        tag: 'demo',
        run: () => showToast('Filter — not wired yet.'),
      },
      {
        id: 'theme',
        label: 'Toggle theme',
        tag: 'real',
        run: () => {
          const next = nextTheme(theme);
          setTheme(next);
          showToast(`Theme: ${next}`);
        },
      },
      {
        id: 'overview',
        label: 'Go to Overview',
        tag: 'real',
        run: () => onNavigateView('overview'),
      },
      {
        id: 'coverage',
        label: 'Go to Coverage',
        tag: 'real',
        run: () => onNavigateView('coverage'),
      },
      {
        id: 'archaeology',
        label: 'Go to Archaeology',
        tag: 'real',
        run: () => onNavigateView('archaeology'),
      },
      {
        id: 'sessions',
        label: 'Go to Sessions',
        tag: 'real',
        run: () => onNavigateView('sessions'),
      },
      {
        id: 'copyid',
        label: 'Copy session id',
        tag: 'real',
        run: () => {
          if (!sessionId) {
            showToast('No session selected.');
            return;
          }
          if (navigator.clipboard?.writeText) {
            navigator.clipboard.writeText(sessionId).then(
              () => showToast(`Copied ${sessionId}`),
              () => showToast(sessionId!)
            );
          } else {
            showToast(sessionId);
          }
        },
      },
    ],
    [liveTail, onToggleLiveTail, sessionId, showToast, theme, setTheme, onNavigateView]
  );

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return commands;
    return commands.filter((c) => c.label.toLowerCase().includes(q));
  }, [commands, query]);

  useEffect(() => {
    setActiveIndex(0);
  }, [query, open]);

  useEffect(() => {
    if (open) {
      previouslyFocused.current = document.activeElement as HTMLElement | null;
      setQuery('');
      const raf = requestAnimationFrame(() => inputRef.current?.focus());
      return () => cancelAnimationFrame(raf);
    }
    previouslyFocused.current?.focus();
    previouslyFocused.current = null;
    return undefined;
  }, [open]);

  if (!open) return null;

  function runCommand(cmd: Command | undefined) {
    if (!cmd) return;
    onClose();
    cmd.run();
  }

  function handleKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (filtered.length) setActiveIndex((i) => (i + 1) % filtered.length);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (filtered.length) setActiveIndex((i) => (i - 1 + filtered.length) % filtered.length);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      runCommand(filtered[activeIndex]);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      onClose();
    } else if (e.key === 'Tab') {
      // Only one focusable element while open — trap focus on it.
      e.preventDefault();
    }
  }

  return (
    <div
      className="palette-overlay"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="palette" role="dialog" aria-modal="true" aria-label="Command palette">
        <input
          ref={inputRef}
          className="palette-input"
          type="text"
          placeholder="Type a command…"
          autoComplete="off"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={handleKeyDown}
        />
        <ul className="palette-list">
          {filtered.length === 0 && (
            <li className="palette-empty">
              <Archie
                pose="front-sit"
                size={44}
                accessory={archie.accessory}
                colourway="C4"
                label=""
              />
              <span>
                Nothing by that name. He is sitting here waiting; try a shorter word.
              </span>
            </li>
          )}
          {filtered.map((cmd, i) => (
            <li
              key={cmd.id}
              className={`palette-item${i === activeIndex ? ' active' : ''}`}
              onMouseEnter={() => setActiveIndex(i)}
              onClick={() => runCommand(cmd)}
            >
              <span>{cmd.label}</span>
              <span className="palette-item-hint">{cmd.tag}</span>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
