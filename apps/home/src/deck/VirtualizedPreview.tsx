import { useMemo, useState } from 'react';
import {
  PREVIEW_OVERSCAN,
  PREVIEW_ROW_HEIGHT,
  PREVIEW_VIEWPORT_HEIGHT,
  previewWindow,
  shouldVirtualizePreview,
} from './previewWindow';

/**
 * Virtualized file/handoff preview: windowed rows over long bodies, plain <pre>
 * under the threshold. Pure view layer -- reads `body`, writes nothing, keeps no
 * index handle. Native scroll only (never JS-driven); spacers reconstruct the full
 * height so the scrollbar stays stable while only the window mounts.
 */
export function VirtualizedPreview({
  body,
  scrollTop: controlledScrollTop = 0,
  viewportHeight = PREVIEW_VIEWPORT_HEIGHT,
  rowHeight = PREVIEW_ROW_HEIGHT,
  overscan = PREVIEW_OVERSCAN,
}: {
  body: string;
  /** Initial scroll offset; updates live from native scroll afterwards. */
  scrollTop?: number;
  viewportHeight?: number;
  rowHeight?: number;
  overscan?: number;
}) {
  const lines = useMemo(() => body.split('\n'), [body]);
  const [liveTop, setLiveTop] = useState(controlledScrollTop);
  const scrollTop = liveTop;

  if (!shouldVirtualizePreview({ lineCount: lines.length })) {
    return (
      <pre className="bg-[var(--mv-ground)] border border-line rounded p-2.5 text-[11px] leading-relaxed text-muted whitespace-pre-wrap overflow-x-auto">
        {body}
      </pre>
    );
  }

  const w = previewWindow({ total: lines.length, scrollTop, viewportHeight, rowHeight, overscan });
  const rows = lines.slice(w.start, w.end);

  return (
    <div
      data-virtualized="true"
      aria-label="file preview"
      className="bg-[var(--mv-ground)] border border-line rounded overflow-y-auto"
      style={{ maxHeight: viewportHeight }}
      onScroll={(e) => setLiveTop(e.currentTarget.scrollTop)}
    >
      <div style={{ height: lines.length * rowHeight, position: 'relative' }}>
        <div data-preview-spacer="top" style={{ height: w.topPad }} />
        {rows.map((line, i) => {
          const index = w.start + i;
          return (
            <div
              key={index}
              data-preview-row={index}
              className="px-2.5 text-[11px] leading-relaxed text-muted overflow-hidden whitespace-pre"
              style={{ height: rowHeight }}
              title={`line ${index + 1}`}
            >
              {line === '' ? ' ' : line}
            </div>
          );
        })}
        <div data-preview-spacer="bottom" style={{ height: w.bottomPad }} />
      </div>
    </div>
  );
}
