import { useMemo, useState } from 'react';
import type { NormalizedEvent } from '../types';
import { analyzeComposition, formatChars } from '../utils/contextComposition';
import { formatTokens } from '../utils/formatters';

export interface ContextCompositionProps {
  events: NormalizedEvent[] | null | undefined;
  /** False while background pages are still loading — the shares below are
   * computed over everything loaded so far, which would misreport as a real
   * composition on a partial event set, so the headline numbers wait. */
  eventsComplete?: boolean;
}

const TOP_CONTRIBUTORS = 4;

/**
 * What filled this session's transcript, and how much of it was not the work.
 *
 * Nothing here is a verdict — a session that is 70% tool output used a lot of
 * tools, which is not a failure. So the palette is the categorical series and
 * never the state colours, and the accent stays out entirely.
 */
export function ContextComposition({ events, eventsComplete = true }: ContextCompositionProps) {
  const composition = useMemo(() => analyzeComposition(events ?? []), [events]);
  const [open, setOpen] = useState(false);

  if (!eventsComplete) {
    return (
      <section className="ctx-comp">
        <div className="ctx-head">
          <span className="ctx-eyebrow">Context composition</span>
          <span className="ctx-total">Loading…</span>
        </div>
      </section>
    );
  }

  if (composition.totalChars === 0) return null;

  const visible = composition.buckets.filter((b) => b.share > 0);
  const contributors = composition.contributors.slice(0, TOP_CONTRIBUTORS);
  const biggest = composition.contributors[0];

  return (
    <section className="ctx-comp">
      <div className="ctx-head">
        <span className="ctx-eyebrow">Context composition</span>
        <span className="ctx-total">{formatChars(composition.totalChars)} chars</span>
        <span className="ctx-overhead">
          {(composition.overheadShare * 100).toFixed(0)}% not dialogue
        </span>
      </div>

      <div
        className="ctx-bar"
        role="img"
        aria-label={visible
          .map((b) => `${b.label} ${(b.share * 100).toFixed(0)}%`)
          .join(', ')}
      >
        {visible.map((b) => (
          <span
            key={b.key}
            className={`ctx-seg ctx-seg-${b.key}`}
            style={{ width: `${b.share * 100}%` }}
            title={`${b.label} — ${formatChars(b.chars)} chars across ${b.events.toLocaleString()} events`}
          />
        ))}
      </div>

      <ul className="ctx-legend">
        {visible.map((b) => (
          <li key={b.key} className="ctx-legend-item">
            <span className={`ctx-dot ctx-seg-${b.key}`} aria-hidden="true" />
            <span className="ctx-legend-label">{b.label}</span>
            <span className="ctx-legend-value">{(b.share * 100).toFixed(1)}%</span>
          </li>
        ))}
      </ul>

      {biggest && (
        <p className="ctx-biggest">
          Largest injection: <strong>{biggest.label}</strong> — {formatChars(biggest.chars)} chars
          across {biggest.occurrences.toLocaleString()} re-send
          {biggest.occurrences === 1 ? '' : 's'}.
        </p>
      )}

      {contributors.length > 1 && (
        <>
          <button
            type="button"
            className="ctx-toggle"
            aria-expanded={open}
            onClick={() => setOpen((v) => !v)}
          >
            {open ? 'Hide breakdown' : 'What was injected'}
          </button>
          {open && (
            <ul className="ctx-contributors">
              {contributors.map((c) => (
                <li key={c.label} className="ctx-contributor">
                  <span className="ctx-c-label">{c.label}</span>
                  <span className="ctx-c-bar" aria-hidden="true">
                    <span
                      className="ctx-c-fill"
                      style={{ width: `${(c.chars / contributors[0].chars) * 100}%` }}
                    />
                  </span>
                  <span className="ctx-c-value">{formatChars(c.chars)}</span>
                  <span className="ctx-c-count">×{c.occurrences.toLocaleString()}</span>
                </li>
              ))}
            </ul>
          )}
        </>
      )}

      {composition.peakTokens !== null && (
        <p className="ctx-peak">
          <span className="ctx-peak-label">Measured peak</span>
          <strong>{formatTokens(composition.peakTokens)}</strong> tokens of context before
          compaction
          {composition.compactions.length > 1
            ? `, across ${composition.compactions.length} rounds`
            : ''}
          . Exact, from the harness's own accounting — not an estimate.
        </p>
      )}

      {/* Ships with the output rather than as a footnote. Earlier wording here
          claimed the overhead could not be measured at all; that was wrong —
          tool names and exact context size are both recorded, so only the
          schema bodies and system prompt text are missing. */}
      <p className="ctx-caveat">
        Shares are of recorded transcript. Tool schema bodies and the system prompt text are not
        written to the log, so they sit outside these percentages.
      </p>
    </section>
  );
}
