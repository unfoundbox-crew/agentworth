import { useEffect } from 'react';
import type { RefObject } from 'react';
import type { ShellNav } from './shell/SessionList';

export interface UseArchiveKeysOptions {
  exitTrajectoryFocus?: () => boolean;
  navRef: RefObject<ShellNav | null>;
  focusInspector: () => void;
}

function isTypingTarget(el: EventTarget | null): boolean {
  if (!(el instanceof HTMLElement)) return false;
  if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT') return true;
  return el.isContentEditable;
}

/**
 * j/k/Enter/`/` for the archive phase. Escape is owned by Deck (closes the
 * phase). Palette (⌘K) stays out of scope for P2.
 */
export function useArchiveKeys(opts: UseArchiveKeysOptions): void {
  const { navRef, focusInspector, exitTrajectoryFocus } = opts;

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        if (exitTrajectoryFocus && exitTrajectoryFocus()) {
          e.preventDefault();
          e.stopPropagation();
        }
        return;
      }

      if (isTypingTarget(e.target)) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;

      if (e.key === 'j' || e.key === 'ArrowDown') {
        e.preventDefault();
        navRef.current?.next();
        return;
      }
      if (e.key === 'k' || e.key === 'ArrowUp') {
        e.preventDefault();
        navRef.current?.prev();
        return;
      }
      if (e.key === 'Enter') {
        e.preventDefault();
        focusInspector();
        return;
      }
      if (e.key === '/') {
        e.preventDefault();
        e.stopPropagation();
        const el = document.querySelector<HTMLElement>('[data-shell-filter]');
        el?.focus();
      }
    }

    // Capture so `/` reaches us before Deck's focus-dock handler.
    document.addEventListener('keydown', onKeyDown, true);
    return () => document.removeEventListener('keydown', onKeyDown, true);
  }, [navRef, focusInspector, exitTrajectoryFocus]);
}
