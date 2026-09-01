import { OutcomeEvidence, OutcomeKind } from '../types';
import { IconVerified, IconUnflown } from './dsIcons';

export interface LadderRungDef {
  level: 1 | 2 | 3 | 4 | 5;
  kind: OutcomeKind;
  name: string;
}

/**
 * The five rungs, highest confidence first. Levels 5..2 are machine-checked
 * evidence; level 1 is only the agent's own word — that boundary is drawn in
 * the CSS spine (solid success above it, dashed danger below), not just in
 * this ordering.
 */
export const LADDER_RUNGS: LadderRungDef[] = [
  { level: 5, kind: 'ci_or_deployment_verified', name: 'CI or deployment green' },
  { level: 4, kind: 'commit_observed', name: 'Commit observed in git log' },
  { level: 3, kind: 'test_or_build_passed', name: 'Test or build passed' },
  { level: 2, kind: 'artifact_changed', name: 'Artifact changed on disk' },
  { level: 1, kind: 'done_claimed', name: 'Done claimed by the agent' },
];

const KIND_TO_LEVEL: Record<OutcomeKind, number> = {
  ci_or_deployment_verified: 5,
  commit_observed: 4,
  test_or_build_passed: 3,
  artifact_changed: 2,
  done_claimed: 1,
  unresolved: 0,
};

/**
 * Highest rung reached, from whichever data is available: the full trace's
 * `outcomes` evidence list when present, otherwise the summary's single
 * `primary_outcome`. Returns 0 (nothing reached) when both are missing or
 * `unresolved` — that is a normal, non-error state.
 */
export function determineReachedLevel(
  primaryOutcome?: OutcomeKind | null,
  outcomes?: OutcomeEvidence[] | null
): number {
  if (outcomes && outcomes.length > 0) {
    return outcomes.reduce((max, o) => Math.max(max, KIND_TO_LEVEL[o.kind] ?? 0), 0);
  }
  if (primaryOutcome) {
    return KIND_TO_LEVEL[primaryOutcome] ?? 0;
  }
  return 0;
}

/** Per-rung evidence captions keyed by outcome kind, e.g. from `trace.outcomes[].summary`. */
export type LadderCaptions = Partial<Record<OutcomeKind, string>>;

/** Builds a caption lookup from a trace's outcome evidence list. */
export function captionsFromOutcomes(outcomes?: OutcomeEvidence[] | null): LadderCaptions {
  const out: LadderCaptions = {};
  if (!outcomes) return out;
  for (const o of outcomes) {
    if (o.summary) out[o.kind] = o.summary;
  }
  return out;
}

const NODE_SIZE: Record<number, number> = { 5: 20, 4: 17, 3: 14, 2: 11, 1: 9 };

export interface OutcomeLadderProps {
  /** Highest rung reached: 0 (none) through 5. */
  reachedLevel: number;
  /** Optional evidence caption per outcome kind; missing entries render as an em-dash. */
  captions?: LadderCaptions;
}

export function OutcomeLadder({ reachedLevel, captions = {} }: OutcomeLadderProps) {
  return (
    <div className="ol-ladder" role="list" aria-label="Outcome ladder">
      {LADDER_RUNGS.map((rung, i) => {
        const reached = rung.level <= reachedLevel;
        const caption = captions[rung.kind];
        const size = NODE_SIZE[rung.level];
        const isLast = i === LADDER_RUNGS.length - 1;
        const nextRung = !isLast ? LADDER_RUNGS[i + 1] : null;
        const segIsCliff = nextRung ? nextRung.level === 1 : false;
        const segReached = nextRung ? nextRung.level <= reachedLevel : false;

        return (
          <div key={rung.kind}>
            <div className={`ol-rung${reached ? '' : ' ol-rung-dim'}`} role="listitem">
              <div className="ol-node-col">
                {rung.level === 1 ? (
                  <IconUnflown size={size} className="ol-node" />
                ) : (
                  <IconVerified size={size} className="ol-node" />
                )}
              </div>
              <div className="ol-rung-text">
                <span className="ol-rung-name">
                  {rung.level}&nbsp;&middot;&nbsp;{rung.name}
                </span>
                <span className="ol-rung-caption">{caption ?? '—'}</span>
              </div>
            </div>
            {nextRung && (
              <div className="ol-spine-row">
                <div className="ol-spine-col">
                  <div
                    className={`ol-spine-seg${segIsCliff ? ' ol-spine-cliff' : ''}${
                      segReached ? '' : ' ol-spine-dim'
                    }`}
                  />
                </div>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
