import { useMemo } from 'react';
import type { NormalizedEvent } from '../types';
import { analyzeComposition } from '../utils/contextComposition';
import { formatTokens } from '../utils/formatters';

export interface CompactionProps {
  events: NormalizedEvent[] | null | undefined;
  /** False while background pages are still loading — round counts and the
   * "what the model can still see" figure need every event to be right. */
  eventsComplete?: boolean;
}

function survivedPercent(pre: number, post: number): string {
  if (pre <= 0) return '—';
  const pct = (post / pre) * 100;
  return `${pct < 1 ? pct.toFixed(2) : pct.toFixed(1)}%`;
}

/**
 * How many times this session was compacted, what each round kept, and what
 * the model can still see after all of them.
 *
 * A session compacted five times is a different object from one that never
 * was, and right now they look identical in every tool — see
 * docs/specs/compaction.md. Parsing reuses `analyzeComposition`, the same
 * pass ContextComposition (PR #46) already makes over `compactMetadata`,
 * rather than reading that field a second time.
 */
export function Compaction({ events, eventsComplete = true }: CompactionProps) {
  const composition = useMemo(() => analyzeComposition(events ?? []), [events]);
  const rounds = composition.compactions;

  if (!events) return null;

  if (!eventsComplete) {
    return (
      <section className="cmp-comp">
        <div className="cmp-head">
          <span className="cmp-eyebrow">Compaction</span>
          <span className="cmp-rounds">Loading…</span>
        </div>
      </section>
    );
  }

  if (rounds.length === 0) {
    return (
      <section className="cmp-comp">
        <p className="cmp-never">Never compacted.</p>
      </section>
    );
  }

  const last = rounds[rounds.length - 1];

  return (
    <section className="cmp-comp">
      <div className="cmp-head">
        <span className="cmp-eyebrow">Compaction</span>
        <span className="cmp-rounds">
          {rounds.length} round{rounds.length === 1 ? '' : 's'}
        </span>
      </div>

      <div className="cmp-table" role="table" aria-label="Compaction rounds">
        <div className="cmp-row cmp-row-head" role="row">
          <span className="cmp-cell cmp-cell-round" role="columnheader">
            Round
          </span>
          <span className="cmp-cell cmp-cell-num" role="columnheader">
            Context before
          </span>
          <span className="cmp-cell cmp-cell-num" role="columnheader">
            Summary after
          </span>
          <span className="cmp-cell cmp-cell-num" role="columnheader">
            Survived
          </span>
        </div>
        {rounds.map((r, i) => (
          <div className="cmp-row" role="row" key={r.eventId}>
            <span className="cmp-cell cmp-cell-round" role="cell">
              {i + 1}
            </span>
            <span className="cmp-cell cmp-cell-num" role="cell">
              {formatTokens(r.preTokens)}
            </span>
            <span className="cmp-cell cmp-cell-num" role="cell">
              {formatTokens(r.postTokens)}
            </span>
            <span className="cmp-cell cmp-cell-num" role="cell">
              {survivedPercent(r.preTokens, r.postTokens)}
            </span>
          </div>
        ))}
      </div>

      <p className="cmp-cumulative">
        <span className="cmp-cumulative-label">What the model can still see</span>
        <strong>{formatTokens(last.postTokens)}</strong> tokens, after {rounds.length} round
        {rounds.length === 1 ? '' : 's'} of compaction.
      </p>
    </section>
  );
}
