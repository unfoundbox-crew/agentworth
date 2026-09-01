import { useCallback, useEffect, useRef, useState } from 'react';

export interface ResizableWidth {
  width: number;
  /** Props for the drag handle. Pointer-driven, but keyboard-operable too. */
  handleProps: {
    role: 'separator';
    tabIndex: 0;
    'aria-orientation': 'vertical';
    'aria-label': string;
    'aria-valuenow': number;
    'aria-valuemin': number;
    'aria-valuemax': number;
    onPointerDown: (e: React.PointerEvent<HTMLElement>) => void;
    onKeyDown: (e: React.KeyboardEvent<HTMLElement>) => void;
    onDoubleClick: () => void;
  };
  dragging: boolean;
}

const KEY_STEP = 16;

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function readStored(storageKey: string, fallback: number, min: number, max: number): number {
  try {
    const raw = window.localStorage.getItem(storageKey);
    if (raw === null) return fallback;
    const parsed = Number.parseInt(raw, 10);
    if (!Number.isFinite(parsed)) return fallback;
    return clamp(parsed, min, max);
  } catch {
    // Private browsing and blocked site-data both throw on access rather than
    // returning null, so the default has to survive the read itself failing.
    return fallback;
  }
}

export interface UseResizableWidthOptions {
  storageKey: string;
  initial: number;
  min: number;
  max: number;
  label: string;
}

/**
 * A drag-resizable width, persisted across reloads.
 *
 * The width is only ever committed to storage on release, not on every pointer
 * move — a drag across the pane is hundreds of events and localStorage writes
 * are synchronous.
 */
export function useResizableWidth({
  storageKey,
  initial,
  min,
  max,
  label,
}: UseResizableWidthOptions): ResizableWidth {
  const [width, setWidth] = useState(() => readStored(storageKey, initial, min, max));
  const [dragging, setDragging] = useState(false);
  const widthRef = useRef(width);
  widthRef.current = width;

  const persist = useCallback(
    (value: number) => {
      try {
        window.localStorage.setItem(storageKey, String(Math.round(value)));
      } catch {
        // Storage being unavailable must not break resizing for this session.
      }
    },
    [storageKey]
  );

  const onPointerDown = useCallback(
    (e: React.PointerEvent<HTMLElement>) => {
      if (e.button !== 0) return;
      e.preventDefault();
      const handle = e.currentTarget;
      const startX = e.clientX;
      const startWidth = widthRef.current;
      handle.setPointerCapture(e.pointerId);
      setDragging(true);

      const onMove = (ev: PointerEvent) => {
        setWidth(clamp(startWidth + (ev.clientX - startX), min, max));
      };
      const onUp = () => {
        handle.releasePointerCapture?.(e.pointerId);
        handle.removeEventListener('pointermove', onMove);
        handle.removeEventListener('pointerup', onUp);
        handle.removeEventListener('pointercancel', onUp);
        setDragging(false);
        persist(widthRef.current);
      };

      handle.addEventListener('pointermove', onMove);
      handle.addEventListener('pointerup', onUp);
      handle.addEventListener('pointercancel', onUp);
    },
    [min, max, persist]
  );

  const onKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLElement>) => {
      const delta = e.key === 'ArrowLeft' ? -KEY_STEP : e.key === 'ArrowRight' ? KEY_STEP : 0;
      if (delta === 0) return;
      e.preventDefault();
      const next = clamp(widthRef.current + delta, min, max);
      setWidth(next);
      persist(next);
    },
    [min, max, persist]
  );

  const onDoubleClick = useCallback(() => {
    setWidth(initial);
    persist(initial);
  }, [initial, persist]);

  useEffect(() => {
    if (!dragging) return;
    // A drag that crosses text would otherwise select it, and the cursor would
    // flicker back to the default over every element it passes.
    const prevUserSelect = document.body.style.userSelect;
    const prevCursor = document.body.style.cursor;
    document.body.style.userSelect = 'none';
    document.body.style.cursor = 'col-resize';
    return () => {
      document.body.style.userSelect = prevUserSelect;
      document.body.style.cursor = prevCursor;
    };
  }, [dragging]);

  return {
    width,
    dragging,
    handleProps: {
      role: 'separator',
      tabIndex: 0,
      'aria-orientation': 'vertical',
      'aria-label': label,
      'aria-valuenow': Math.round(width),
      'aria-valuemin': min,
      'aria-valuemax': max,
      onPointerDown,
      onKeyDown,
      onDoubleClick,
    },
  };
}
