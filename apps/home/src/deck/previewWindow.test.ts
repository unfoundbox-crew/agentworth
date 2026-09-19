import { describe, expect, it } from 'vitest';
import {
  PREVIEW_OVERSCAN,
  PREVIEW_ROW_HEIGHT,
  PREVIEW_VIEWPORT_HEIGHT,
  PREVIEW_VIRTUALIZE_LINES,
  previewWindow,
  shouldVirtualizePreview,
  stablePreviewScroll,
} from './previewWindow';

function fixtureLines(n: number): string[] {
  return Array.from({ length: n }, (_, i) => `line ${String(i).padStart(5, '0')} :: corpus row ${i}`);
}

describe('previewWindow (pure view math, no index writes)', () => {
  it('does not virtualize short previews', () => {
    expect(shouldVirtualizePreview({ lineCount: 10 })).toBe(false);
    expect(shouldVirtualizePreview({ lineCount: PREVIEW_VIRTUALIZE_LINES })).toBe(false);
  });

  it('virtualizes corpus-scale previews', () => {
    expect(shouldVirtualizePreview({ lineCount: PREVIEW_VIRTUALIZE_LINES + 1 })).toBe(true);
    expect(shouldVirtualizePreview({ lineCount: 10_000 })).toBe(true);
  });

  it('renders a 10k-row fixture within the stated row budget', () => {
    const total = 10_000;
    const w = previewWindow({
      total,
      scrollTop: 0,
      viewportHeight: PREVIEW_VIEWPORT_HEIGHT,
      rowHeight: PREVIEW_ROW_HEIGHT,
      overscan: PREVIEW_OVERSCAN,
    });
    const rendered = w.end - w.start;
    const budget = Math.ceil(PREVIEW_VIEWPORT_HEIGHT / PREVIEW_ROW_HEIGHT) + PREVIEW_OVERSCAN * 2;
    expect(rendered).toBeGreaterThan(0);
    expect(rendered).toBeLessThanOrEqual(budget);
    expect(w.topPad).toBe(0);
    expect(w.topPad + rendered * PREVIEW_ROW_HEIGHT + w.bottomPad).toBe(total * PREVIEW_ROW_HEIGHT);
  });

  it('windows the middle of a 10k fixture without blowing the budget', () => {
    const total = 10_000;
    const scrollTop = 50_000; // deep into the corpus
    const w = previewWindow({
      total,
      scrollTop,
      viewportHeight: PREVIEW_VIEWPORT_HEIGHT,
      rowHeight: PREVIEW_ROW_HEIGHT,
      overscan: PREVIEW_OVERSCAN,
    });
    const budget = Math.ceil(PREVIEW_VIEWPORT_HEIGHT / PREVIEW_ROW_HEIGHT) + PREVIEW_OVERSCAN * 2;
    expect(w.end - w.start).toBeLessThanOrEqual(budget);
    expect(w.start).toBeGreaterThan(0);
    expect(w.end).toBeLessThan(total);
    // pads reconstruct the full height so the scrollbar stays stable
    expect(w.topPad + (w.end - w.start) * PREVIEW_ROW_HEIGHT + w.bottomPad).toBe(total * PREVIEW_ROW_HEIGHT);
  });

  it('spot-checks content: the window slice matches the source lines', () => {
    const lines = fixtureLines(10_000);
    for (const scrollTop of [0, 50_000, 199_000]) {
      const w = previewWindow({
        total: lines.length,
        scrollTop,
        viewportHeight: PREVIEW_VIEWPORT_HEIGHT,
        rowHeight: PREVIEW_ROW_HEIGHT,
        overscan: PREVIEW_OVERSCAN,
      });
      const slice = lines.slice(w.start, w.end);
      expect(slice.length).toBe(w.end - w.start);
      expect(slice[0]).toBe(lines[w.start]);
      expect(slice[slice.length - 1]).toBe(lines[w.end - 1]);
    }
  });

  it('keeps scroll position stable when rows append above the fold', () => {
    const rowHeight = PREVIEW_ROW_HEIGHT;
    const viewportHeight = PREVIEW_VIEWPORT_HEIGHT;
    const kept = stablePreviewScroll({
      prevTotal: 1_000,
      nextTotal: 1_200,
      prevScrollTop: 400,
      rowHeight,
      viewportHeight,
      atBottom: false,
    });
    expect(kept).toBe(400);
  });

  it('follows the tail only when pinned to the bottom', () => {
    const rowHeight = PREVIEW_ROW_HEIGHT;
    const viewportHeight = PREVIEW_VIEWPORT_HEIGHT;
    const followed = stablePreviewScroll({
      prevTotal: 1_000,
      nextTotal: 1_200,
      prevScrollTop: 1_000 * rowHeight - viewportHeight,
      rowHeight,
      viewportHeight,
      atBottom: true,
    });
    expect(followed).toBe(1_200 * rowHeight - viewportHeight);
  });

  it('clamps scroll into the new range when the preview shrinks', () => {
    const clamped = stablePreviewScroll({
      prevTotal: 1_000,
      nextTotal: 100,
      prevScrollTop: 19_000,
      rowHeight: PREVIEW_ROW_HEIGHT,
      viewportHeight: PREVIEW_VIEWPORT_HEIGHT,
      atBottom: false,
    });
    expect(clamped).toBe(100 * PREVIEW_ROW_HEIGHT - PREVIEW_VIEWPORT_HEIGHT);
  });
});
