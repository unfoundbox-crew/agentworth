import { useState } from 'react';
import { TraceScore } from '../types';

type ComponentKey =
  | 'outcome_score'
  | 'verifiability_score'
  | 'complexity_score'
  | 'recovery_score'
  | 'provenance_score';

const COMPONENTS: { key: ComponentKey; label: string }[] = [
  { key: 'outcome_score', label: 'Outcome' },
  { key: 'verifiability_score', label: 'Verifiability' },
  { key: 'complexity_score', label: 'Complexity' },
  { key: 'recovery_score', label: 'Recovery' },
  { key: 'provenance_score', label: 'Provenance' },
];

const EXPLANATIONS_COLLAPSED = 4;

export interface ScoreBreakdownProps {
  score: TraceScore;
}

/** The five weighted components behind a session's composite score, plus the
 * audit explanations that say why each one landed where it did. */
export function ScoreBreakdown({ score }: ScoreBreakdownProps) {
  const [expanded, setExpanded] = useState(false);
  const explanations = score.explanations ?? [];
  const overflow = explanations.length - EXPLANATIONS_COLLAPSED;
  const visible = expanded ? explanations : explanations.slice(0, EXPLANATIONS_COLLAPSED);

  return (
    <div className="shell-score-block">
      <div className="shell-section-title-row">
        <span className="shell-section-title">Score breakdown</span>
        <span className="shell-score-composite">
          <span className="shell-score-composite-val">{Math.round(score.composite_score * 100)}</span>
          <span className="shell-score-composite-max">/100</span>
        </span>
      </div>

      <div className="shell-score-bars">
        {COMPONENTS.map(({ key, label }) => {
          const raw = score[key];
          const pct = Math.round(raw * 100);
          const width = Math.max(0, Math.min(100, pct));
          return (
            <div className="shell-score-bar-row" key={key}>
              <span className="shell-score-bar-label">{label}</span>
              <div className="shell-score-bar-track">
                <div className="shell-score-bar-fill" style={{ width: `${width}%` }} />
              </div>
              <span className="shell-score-bar-val">{pct}</span>
            </div>
          );
        })}
      </div>

      {explanations.length > 0 && (
        <div className="shell-score-explanations">
          {visible.map((exp, i) => (
            <p className="shell-score-explanation" key={i}>
              {exp}
            </p>
          ))}
          {overflow > 0 && (
            <button
              type="button"
              className="shell-score-explanations-toggle"
              onClick={() => setExpanded((v) => !v)}
            >
              {expanded ? 'Show fewer' : `+${overflow} more`}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
