import { useState } from 'react';
import { selectReviewPath, type SessionReviewData } from '../model/sessionReview';

const STATUS_LETTER = { added: 'A', modified: 'M', deleted: 'D' } as const;

/**
 * Session review: the files a session changed, file by file, each row
 * carrying the session's proof (outcome rung + test citation). Read-only --
 * it renders store data the gateway already delivered and fetches nothing
 * itself. The reference for the layout is opencode's session-review v2, which
 * shows diffs without any proof; the proof badge per file is this view's
 * whole point.
 */
export function SessionReview({
  review,
  selectedPath,
  onSelect,
}: {
  review: SessionReviewData;
  selectedPath?: string | null;
  onSelect?: (path: string) => void;
}) {
  const [internal, setInternal] = useState<string | null>(null);
  const controlled = selectedPath !== undefined ? selectedPath : internal;
  const pick = onSelect ?? setInternal;
  const selected = selectReviewPath(review, controlled);
  const current = review.files.find((f) => f.path === selected) ?? null;

  if (review.files.length === 0) {
    return (
      <div aria-label="session review">
        <span className="inline-block text-[10px] text-dim border border-dashed border-line rounded-full px-2.5 py-0.5">
          no files changed in this session
        </span>
        <ProofLine proof={review.proof} />
      </div>
    );
  }

  return (
    <div aria-label="session review">
      <ul className="flex flex-col gap-1 max-h-40 overflow-y-auto">
        {review.files.map((f) => {
          const active = f.path === selected;
          return (
            <li key={f.path}>
              <button
                type="button"
                onClick={() => pick(f.path)}
                aria-pressed={active}
                className={`w-full flex items-center gap-2 rounded px-2 py-1 text-left text-[11px] ${
                  active ? 'bg-[var(--mv-ground)] text-text' : 'text-muted'
                }`}
              >
                <span className="text-[10px] text-dim w-3 shrink-0" title={f.status}>
                  {STATUS_LETTER[f.status]}
                </span>
                <span className="flex-1 truncate font-mono">{f.path}</span>
                {review.proof && (
                  <span className="shrink-0 rounded-full border border-line px-2 py-px text-[10px] text-success">
                    {review.proof.label}
                  </span>
                )}
              </button>
            </li>
          );
        })}
      </ul>
      <div className="mt-2">
        <ProofLine proof={review.proof} detail />
        {current?.body ? (
          <pre className="mt-1.5 bg-[var(--mv-ground)] border border-line rounded p-2.5 text-[11px] leading-relaxed text-muted whitespace-pre-wrap overflow-x-auto">
            {current.body}
          </pre>
        ) : (
          <span className="mt-1.5 inline-block text-[10px] text-dim border border-dashed border-line rounded-full px-2.5 py-0.5">
            diff not loaded yet
          </span>
        )}
      </div>
    </div>
  );
}

function ProofLine({ proof, detail }: { proof: SessionReviewData['proof']; detail?: boolean }) {
  if (!proof) {
    return <div className="mt-1.5 text-[10px] text-dim">no proof yet — unflown</div>;
  }
  return (
    <div className="mt-1.5 text-[11px] text-success">
      session proof · {proof.label}
      {detail && proof.citation && <span className="text-muted"> · {proof.citation}</span>}
      {detail && !proof.citation && <span className="text-dim"> · no test cited yet</span>}
    </div>
  );
}
