import React from "react";

/**
 * Client-side search over the index scripts/prerender.mjs writes to
 * /docs/search-index.json. One static file, fetched once on first open — after
 * that every keystroke is a substring scan in memory. No library, no service.
 *
 * The server renders the button and nothing else; the palette mounts only once
 * a browser opens it, so there is no markup for hydration to disagree about.
 */

export interface SearchEntry {
  /** title */
  t: string;
  /** section label */
  s: string;
  /** url */
  u: string;
  /** excerpt */
  x: string;
}

interface Hit {
  entry: SearchEntry;
  score: number;
}

/** Title matches beat body matches, and a match at the start beats one in the
 *  middle. Enough ordering for a few hundred entries; no ranking library. */
function search(entries: SearchEntry[], q: string): SearchEntry[] {
  const needle = q.trim().toLowerCase();
  if (!needle) return [];
  const terms = needle.split(/\s+/);
  const hits: Hit[] = [];

  for (const entry of entries) {
    const title = entry.t.toLowerCase();
    const body = `${entry.s} ${entry.x}`.toLowerCase();
    let score = 0;
    let matchedAll = true;
    for (const term of terms) {
      const inTitle = title.indexOf(term);
      const inBody = body.indexOf(term);
      if (inTitle === 0) score += 100;
      else if (inTitle > 0) score += 60;
      else if (inBody >= 0) score += 15;
      else {
        matchedAll = false;
        break;
      }
    }
    if (matchedAll) hits.push({ entry, score: score - entry.t.length * 0.05 });
  }

  hits.sort((a, b) => b.score - a.score);
  return hits.slice(0, 24).map((h) => h.entry);
}

export const DocsSearch: React.FC = () => {
  const [open, setOpen] = React.useState(false);
  const [entries, setEntries] = React.useState<SearchEntry[] | null>(null);
  const [query, setQuery] = React.useState("");
  const [active, setActive] = React.useState(0);
  const inputRef = React.useRef<HTMLInputElement>(null);
  const listRef = React.useRef<HTMLUListElement>(null);

  React.useEffect(() => {
    if (!open || entries) return;
    let live = true;
    fetch("/docs/search-index.json")
      .then((r) => (r.ok ? r.json() : []))
      .then((data: SearchEntry[]) => live && setEntries(data))
      .catch(() => live && setEntries([]));
    return () => {
      live = false;
    };
  }, [open, entries]);

  React.useEffect(() => {
    if (open) inputRef.current?.focus();
    else {
      setQuery("");
      setActive(0);
    }
  }, [open]);

  const results = React.useMemo(
    () => (entries ? search(entries, query) : []),
    [entries, query]
  );

  React.useEffect(() => setActive(0), [query]);

  React.useEffect(() => {
    listRef.current
      ?.querySelector<HTMLElement>('[data-active="true"]')
      ?.scrollIntoView({ block: "nearest" });
  }, [active, results]);

  // One listener on the window, not on the input: the shortcut has to work from
  // anywhere on the page, and once the palette is open the arrow keys have to
  // keep working even if focus has drifted off the field.
  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() === "k" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        setOpen((o) => !o);
        return;
      }
      if (!open) return;
      if (e.key === "Escape") {
        e.preventDefault();
        setOpen(false);
      } else if (e.key === "ArrowDown") {
        e.preventDefault();
        setActive((i) => (results.length ? (i + 1) % results.length : 0));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setActive((i) => (results.length ? (i - 1 + results.length) % results.length : 0));
      } else if (e.key === "Enter") {
        e.preventDefault();
        const entry = results[active];
        if (entry) window.location.href = entry.u;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, results, active]);

  return (
    <>
      <button type="button" className="search-bar" onClick={() => setOpen(true)}>
        <svg width="15" height="15" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true">
          <circle cx="9" cy="9" r="6.2" />
          <path d="M17 17l-3.5-3.5" strokeLinecap="round" />
        </svg>
        <span className="search-bar-label">Search the docs</span>
        <kbd>&#8984;K</kbd>
      </button>

      {open && (
        <div
          className="palette-scrim"
          onMouseDown={(e) => e.target === e.currentTarget && setOpen(false)}
        >
          <div className="palette" role="dialog" aria-modal="true" aria-label="Search the docs">
            <div className="palette-input">
              <svg width="15" height="15" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true">
                <circle cx="9" cy="9" r="6.2" />
                <path d="M17 17l-3.5-3.5" strokeLinecap="round" />
              </svg>
              <input
                ref={inputRef}
                type="text"
                value={query}
                placeholder="Search docs, specs, reference"
                aria-label="Search the docs"
                autoComplete="off"
                spellCheck={false}
                onChange={(e) => setQuery(e.target.value)}
              />
              <kbd>Esc</kbd>
            </div>

            {query.trim() !== "" && (
              <ul className="palette-results" ref={listRef}>
                {results.map((entry, i) => (
                  <li key={entry.u + entry.t}>
                    <a
                      href={entry.u}
                      data-active={i === active ? "true" : undefined}
                      onMouseEnter={() => setActive(i)}
                    >
                      <span className="pr-top">
                        <b>{entry.t}</b>
                        <span className="pr-section">{entry.s}</span>
                      </span>
                      {entry.x && <span className="pr-excerpt">{entry.x}</span>}
                    </a>
                  </li>
                ))}
                {results.length === 0 && (
                  <li className="palette-empty">
                    {entries === null ? "Loading the index" : "Nothing matches that."}
                  </li>
                )}
              </ul>
            )}

            <p className="palette-foot">
              <span>Arrow keys to move</span>
              <span>Enter to open</span>
              <span>Esc to close</span>
            </p>
          </div>
        </div>
      )}
    </>
  );
};
