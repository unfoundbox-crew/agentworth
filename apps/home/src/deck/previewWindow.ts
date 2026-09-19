/**
 * Pure window math for virtualized file/handoff previews in the home deck.
 *
 * View layer only: no store reads, no gateway calls, no index writes. Ideas only
 * (windowing + overscan + stable scroll) are borrowed from the session-ui
 * reference; every line here is written against the deck's own row model.
 */

/** Above this many lines a preview windows its rows instead of mounting them all. */
export const PREVIEW_VIRTUALIZE_LINES = 200;
/** Fixed row height in px. Previews are monospace single-line rows, so one height fits all. */
export const PREVIEW_ROW_HEIGHT = 20;
/** Default scroll-viewport height in px for a virtualized preview. */
export const PREVIEW_VIEWPORT_HEIGHT = 320;
/** Rows rendered past each edge of the viewport so fast scrolls never show a gap. */
export const PREVIEW_OVERSCAN = 8;

export function shouldVirtualizePreview(input: { lineCount: number }): boolean {
  return input.lineCount > PREVIEW_VIRTUALIZE_LINES;
}

export interface PreviewWindowInput {
  total: number;
  scrollTop: number;
  viewportHeight: number;
  rowHeight: number;
  overscan: number;
}

export interface PreviewWindow {
  /** First row index rendered (inclusive). */
  start: number;
  /** One past the last row index rendered (exclusive). */
  end: number;
  /** Blank space above the window in px; keeps the scrollbar full-size. */
  topPad: number;
  /** Blank space below the window in px. */
  bottomPad: number;
}

/**
 * Which rows to mount for `scrollTop`. Pure: same input, same window, no DOM reads.
 * `end` is exclusive so `lines.slice(start, end)` is the rows to render, and
 * `topPad + (end - start) * rowHeight + bottomPad === total * rowHeight` always.
 */
export function previewWindow(input: PreviewWindowInput): PreviewWindow {
  const total = Math.max(0, Math.floor(input.total));
  const rowHeight = input.rowHeight > 0 ? input.rowHeight : PREVIEW_ROW_HEIGHT;
  const viewportHeight = Math.max(0, input.viewportHeight);
  const overscan = Math.max(0, Math.floor(input.overscan));
  if (total === 0) return { start: 0, end: 0, topPad: 0, bottomPad: 0 };

  const maxScroll = Math.max(0, total * rowHeight - viewportHeight);
  const scrollTop = Math.min(Math.max(0, input.scrollTop), maxScroll);
  const firstVisible = Math.floor(scrollTop / rowHeight);
  const visibleCount = Math.ceil(viewportHeight / rowHeight);
  const start = Math.max(0, firstVisible - overscan);
  const end = Math.min(total, firstVisible + visibleCount + overscan);
  return {
    start,
    end,
    topPad: start * rowHeight,
    bottomPad: (total - end) * rowHeight,
  };
}

export interface StablePreviewScrollInput {
  prevTotal: number;
  nextTotal: number;
  prevScrollTop: number;
  rowHeight: number;
  viewportHeight: number;
  /** True when the user was pinned to the bottom before the update (follow the tail). */
  atBottom: boolean;
}

/**
 * Scroll offset to keep across a content update. Not pinned: the offset is untouched
 * (clamped into the new range). Pinned: it follows the tail. Pure, no DOM reads.
 */
export function stablePreviewScroll(input: StablePreviewScrollInput): number {
  const rowHeight = input.rowHeight > 0 ? input.rowHeight : PREVIEW_ROW_HEIGHT;
  const viewportHeight = Math.max(0, input.viewportHeight);
  const nextTotal = Math.max(0, Math.floor(input.nextTotal));
  const nextMax = Math.max(0, nextTotal * rowHeight - viewportHeight);
  if (input.atBottom) return nextMax;
  return Math.min(Math.max(0, input.prevScrollTop), nextMax);
}
