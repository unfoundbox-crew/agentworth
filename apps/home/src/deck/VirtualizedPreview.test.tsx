import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { VirtualizedPreview } from './VirtualizedPreview';
import { PREVIEW_OVERSCAN, PREVIEW_ROW_HEIGHT, PREVIEW_VIEWPORT_HEIGHT } from './previewWindow';

function bodyOf(n: number): string {
  return Array.from({ length: n }, (_, i) => `row-${String(i).padStart(5, '0')} corpus line ${i}`).join('\n');
}

function countRows(html: string): number {
  return (html.match(/data-preview-row=/g) ?? []).length;
}

describe('VirtualizedPreview (pure view layer)', () => {
  it('renders short bodies as a plain pre without virtualization chrome', () => {
    const html = renderToStaticMarkup(<VirtualizedPreview body={'a\nb\nc'} />);
    expect(html).toContain('<pre');
    expect(html).not.toContain('data-virtualized="true"');
    expect(html).toContain('a\nb\nc');
  });

  it('renders a 10k-row fixture within the stated row budget', () => {
    const html = renderToStaticMarkup(<VirtualizedPreview body={bodyOf(10_000)} scrollTop={0} />);
    expect(html).toContain('data-virtualized="true"');
    const budget = Math.ceil(PREVIEW_VIEWPORT_HEIGHT / PREVIEW_ROW_HEIGHT) + PREVIEW_OVERSCAN * 2;
    expect(countRows(html)).toBeLessThanOrEqual(budget);
    expect(countRows(html)).toBeGreaterThan(0);
  });

  it('spot-checks content: first window holds the head, deep scroll holds the middle', () => {
    const body = bodyOf(10_000);
    const head = renderToStaticMarkup(<VirtualizedPreview body={body} scrollTop={0} />);
    expect(head).toContain('row-00000');
    expect(head).not.toContain('row-09999');

    const deep = renderToStaticMarkup(<VirtualizedPreview body={body} scrollTop={50_000} />);
    // 50_000px / 20px per row = row ~2500 at the top of the viewport
    expect(deep).toContain('row-02500');
    expect(deep).not.toContain('row-00000');
    expect(deep).not.toContain('row-09999');
  });

  it('keeps the scrollbar stable: spacers reconstruct the full height', () => {
    const html = renderToStaticMarkup(<VirtualizedPreview body={bodyOf(10_000)} scrollTop={50_000} />);
    expect(html).toContain('data-preview-spacer="top"');
    expect(html).toContain('data-preview-spacer="bottom"');
  });
});
