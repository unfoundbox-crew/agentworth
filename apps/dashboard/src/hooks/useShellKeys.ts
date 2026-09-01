import { useEffect } from 'react';
import type { RefObject } from 'react';

export interface ShellNav {
  next: () => void;
  prev: () => void;
}

export interface UseShellKeysOptions {
  /** Ref populated by the session list's registerNav callback. */
  navRef: RefObject<ShellNav | null>;
  paletteOpen: boolean;
  openPalette: () => void;
  closePalette: () => void;
  /** Move focus into the inspector pane (Enter). */
  focusInspector: () => void;
}

function isTypingTarget(el: EventTarget | null): boolean {
  if (!(el instanceof HTMLElement)) return false;
  if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA') return true;
  return el.isContentEditable;
}

/**
 * Global keyboard handling for the explorer shell, attached once at the
 * shell level. Every binding except Escape and Cmd/Ctrl+K no-ops when the
 * event target is a text input — otherwise typing "j" in a filter box would
 * move the session selection.
 */
export function useShellKeys(opts: UseShellKeysOptions): void {
  const { navRef, paletteOpen, openPalette, closePalette, focusInspector } = opts;

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      const isPaletteToggle = (e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k';
      if (isPaletteToggle) {
        e.preventDefault();
        if (paletteOpen) closePalette();
        else openPalette();
        return;
      }

      if (e.key === 'Escape') {
        if (paletteOpen) {
          e.preventDefault();
          closePalette();
          return;
        }
        const active = document.activeElement;
        if (active instanceof HTMLElement && active !== document.body) {
          active.blur();
        }
        return;
      }

      if (paletteOpen) return;

      const typing = isTypingTarget(e.target);
      if (typing) return;

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
        const el = document.querySelector<HTMLElement>('[data-shell-filter]');
        el?.focus();
        return;
      }
    }

    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [navRef, paletteOpen, openPalette, closePalette, focusInspector]);
}
