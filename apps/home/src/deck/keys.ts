import { useEffect } from 'react';

export interface DeckKeyHandlers {
  /** 1-9: open the console for the Nth strip (1-indexed, in strip order). */
  onSelectIndex(index: number): void;
  /** 0: select every rider (steer-to-all). */
  onSelectAll(): void;
  /** "/": focus the dock. */
  onFocusDock(): void;
  /** Escape: close consoles back to alert/cruise. */
  onEscape(): void;
}

const EDITABLE = new Set(['INPUT', 'TEXTAREA']);

function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return !!el && (EDITABLE.has(el.tagName) || el.isContentEditable);
}

/** Global keyboard shortcuts for the deck. Disabled while a text field has focus, except Escape. */
export function useDeckKeys(handlers: DeckKeyHandlers) {
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        handlers.onEscape();
        return;
      }
      if (isTyping(e.target)) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === '/') {
        e.preventDefault();
        handlers.onFocusDock();
        return;
      }
      if (e.key === '0') {
        handlers.onSelectAll();
        return;
      }
      if (/^[1-9]$/.test(e.key)) {
        handlers.onSelectIndex(Number(e.key));
      }
    }
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [handlers]);
}
