import { useEffect } from 'react';
import type { RefObject } from 'react';
import type { ShellNav } from './shell/SessionList';

export interface UseArchiveKeysOptions {
  exitTrajectoryFocus?: () => boolean;
  navRef: RefObject<ShellNav | null>;
  focusInspector: () => void;
  paletteOpen?: boolean;
  openPalette?: () => void;
  closePalette?: () => void;
}

function isTypingTarget(el: EventTarget | null): boolean {
  if (!(el instanceof HTMLElement)) return false;
  if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT') return true;
  return el.isContentEditable;
}

/**
 * j/k/Enter/`/` for the archive phase, plus ⌘/Ctrl+K for the command palette.
 * Escape closes the palette first; otherwise Deck owns leaving the phase.
 */
export function useArchiveKeys(opts: UseArchiveKeysOptions): void {
  const {
    navRef,
    focusInspector,
    exitTrajectoryFocus,
    paletteOpen = false,
    openPalette,
    closePalette,
  } = opts;

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      const isPaletteToggle = (e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k';
      if (isPaletteToggle && openPalette && closePalette) {
        e.preventDefault();
        e.stopPropagation();
        if (paletteOpen) closePalette();
        else openPalette();
        return;
      }

      if (e.key === 'Escape') {
        if (paletteOpen && closePalette) {
          e.preventDefault();
          e.stopPropagation();
          closePalette();
          return;
        }
        if (exitTrajectoryFocus && exitTrajectoryFocus()) {
          e.preventDefault();
          e.stopPropagation();
        }
        return;
      }

      if (paletteOpen) return;
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

    // Capture so `/` and ⌘K reach us before Deck's handlers.
    document.addEventListener('keydown', onKeyDown, true);
    return () => document.removeEventListener('keydown', onKeyDown, true);
  }, [
    navRef,
    focusInspector,
    exitTrajectoryFocus,
    paletteOpen,
    openPalette,
    closePalette,
  ]);
}
