import { useCallback, useEffect, useState } from 'react';

type Palette = 'colour' | 'mono';
const KEY = 'agentworth_palette';

/**
 * Colour vs monochrome for categorical series.
 *
 * Categorical colour is correct by default — a stacked bar of eight segments
 * cannot be read in one hue plus greys. But some people find a coloured chart
 * noisy, and a monochrome index is genuinely easier to scan when you only care
 * about magnitude. This flips --mv-cat-1..8 to a zinc ramp; it does not touch
 * the accent or the semantic state colours, which mean something either way.
 */
export function PaletteToggle() {
  const [palette, setPalette] = useState<Palette>(() => {
    try {
      return localStorage.getItem(KEY) === 'mono' ? 'mono' : 'colour';
    } catch {
      return 'colour';
    }
  });

  useEffect(() => {
    const root = document.documentElement;
    if (palette === 'mono') root.setAttribute('data-palette', 'mono');
    else root.removeAttribute('data-palette');
    try {
      localStorage.setItem(KEY, palette);
    } catch {
      /* private window, or site data blocked — the toggle still works, it just
         does not persist. */
    }
  }, [palette]);

  const toggle = useCallback(() => {
    setPalette((p) => (p === 'mono' ? 'colour' : 'mono'));
  }, []);

  const isMono = palette === 'mono';

  return (
    <button
      type="button"
      className="palette-toggle"
      onClick={toggle}
      aria-pressed={!isMono}
      title={isMono ? 'Colour charts' : 'Monochrome charts'}
    >
      <svg
        width="14"
        height="14"
        viewBox="0 0 20 20"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        {isMono ? (
          <>
            <circle cx="10" cy="10" r="6.5" />
            <path d="M10 3.5v13" />
          </>
        ) : (
          <>
            <circle cx="7.4" cy="8" r="4" />
            <circle cx="12.6" cy="8" r="4" />
            <circle cx="10" cy="12.6" r="4" />
          </>
        )}
      </svg>
      <span className="sr-only">{isMono ? 'Monochrome charts' : 'Colour charts'}</span>
    </button>
  );
}
