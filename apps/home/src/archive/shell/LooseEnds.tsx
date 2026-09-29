import { useMemo, useState } from 'react';
import type { NormalizedEvent } from '../types';
import { findLooseEnds, looseEndsPrompt } from '../utils/looseEnds';

export interface LooseEndsProps {
  events: NormalizedEvent[] | null | undefined;
  sessionId: string;
  /** False while background pages are still loading — the count below is
   * only a lower bound until every event has been scanned. */
  eventsComplete?: boolean;
}

/** How many to show before collapsing the rest behind a toggle. */
const PREVIEW = 5;

/**
 * Things this session said it would do, with no evidence of doing.
 *
 * The framing is load-bearing. These are "loose ends", not misses: roughly half
 * of what any such detector finds is work the user cancelled rather than work
 * the agent forgot, and a surface that says "missed" about cancelled work gets
 * argued with instead of used.
 *
 * The output is a prompt, never a patch. Writing the fix would mean being right
 * about the fix from something that has read a transcript and never opened the
 * codebase; being right about what is missing is the answerable half, and
 * whatever already has the repo open can do the rest.
 */
export function LooseEnds({ events, sessionId, eventsComplete = true }: LooseEndsProps) {
  const ends = useMemo(() => findLooseEnds(events ?? []), [events]);
  const [expanded, setExpanded] = useState(false);
  const [copied, setCopied] = useState(false);

  if (!eventsComplete) {
    return (
      <section className="loose-ends">
        <div className="loose-head">
          <span className="loose-eyebrow">Loose ends</span>
          <span className="loose-count">Loading…</span>
        </div>
      </section>
    );
  }

  if (ends.length === 0) return null;

  const shown = expanded ? ends : ends.slice(0, PREVIEW);

  async function copyPrompt() {
    try {
      await navigator.clipboard.writeText(looseEndsPrompt(ends, sessionId));
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch {
      // Clipboard is permission-gated and refuses outright in some contexts;
      // the list is still readable, so this fails quietly rather than alarming.
    }
  }

  return (
    <section className="loose-ends">
      <div className="loose-head">
        <span className="loose-eyebrow">Loose ends</span>
        <span className="loose-count">{ends.length}</span>
        <button type="button" className="loose-copy" onClick={copyPrompt}>
          {copied ? 'Copied' : 'Copy as prompt'}
        </button>
      </div>

      <p className="loose-intro">
        Stated in this session, with no tool call before the turn ended. Some will be work you
        cancelled.
      </p>

      <ul className="loose-list">
        {shown.map((end) => (
          <li key={`${end.eventId}:${end.sequence}:${end.text.slice(0, 24)}`} className="loose-item">
            <span className="loose-quote">{end.text}</span>
            <span className="loose-meta">
              {end.model && <span className="loose-model">{end.model}</span>}
              <span className="loose-seq">#{end.sequence}</span>
            </span>
          </li>
        ))}
      </ul>

      {ends.length > PREVIEW && (
        <button type="button" className="loose-more" onClick={() => setExpanded((v) => !v)}>
          {expanded ? 'Show fewer' : `Show all ${ends.length}`}
        </button>
      )}
    </section>
  );
}
