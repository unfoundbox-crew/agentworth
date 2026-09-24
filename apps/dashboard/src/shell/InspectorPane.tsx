import { useMemo } from 'react';
import { AgentWorthTrace } from '../types';
import { formatDate, formatDuration } from '../utils/formatters';
import { useSessions } from '../hooks/useSessions';
import { useTraceDetail } from '../hooks/useTraceDetail';
import { OutcomeLadder, captionsFromOutcomes, determineReachedLevel } from './OutcomeLadder';
import { TrajectoryView } from './TrajectoryView';
import { OverviewPane } from './OverviewPane';
import { ScoreBreakdown } from './ScoreBreakdown';
import { TokenEconomics } from './TokenEconomics';
import { CacheWarmth } from './CacheWarmth';
import { LooseEnds } from './LooseEnds';
import { ContextComposition } from './ContextComposition';
import { Compaction } from './Compaction';
import { ProvenanceBlock } from './ProvenanceBlock';
import { analyzeComposition } from '../utils/contextComposition';

export interface InspectorPaneProps {
  trajectoryFocused?: boolean;
  onToggleTrajectoryFocus?: () => void;
  sessionId: string | null;
  liveTail: boolean;
}

const DASH = '—';

function collectChangedFiles(trace: AgentWorthTrace): string[] {
  const seen = new Set<string>();
  for (const evt of trace.events ?? []) {
    if (evt.payload?.type !== 'file_action') continue;
    const { path, action } = evt.payload.data;
    if (!path) continue;
    if (action === 'write' || action === 'edit' || action === 'delete') {
      seen.add(path);
    }
  }
  return Array.from(seen);
}

export function InspectorPane({ sessionId, liveTail, trajectoryFocused, onToggleTrajectoryFocus }: InspectorPaneProps) {
  const { sessions } = useSessions();
  // Fetches the first 500 events for a fast paint, then pages the rest in
  // from /events in the background (#72) — see the hook for the two-phase
  // load and how it cancels on a session change.
  const { trace, loading, error, eventsTotal, eventsComplete } = useTraceDetail(sessionId);

  const summary = useMemo(
    () => sessions.find((s) => s.session_id === sessionId) ?? null,
    [sessions, sessionId]
  );

  // Computed once here (rather than only inside Compaction/TrajectoryView)
  // so the scrubber's marker layer and the compaction pane agree on exactly
  // which events are compaction boundaries.
  const compactionEventIds = useMemo(
    () => (trace?.events ? analyzeComposition(trace.events).compactions.map((c) => c.eventId) : []),
    [trace?.events]
  );

  if (!sessionId) {
    // Nothing selected is the state you land in, so it should answer "how am I
    // doing" rather than "pick something". The overview was a separate rail
    // item nobody would think to click first.
    return (
      <section className="shell-inspector-pane" tabIndex={-1}>
        <OverviewPane />
        <p className="shell-inspector-empty">
          Select a session on the left, or press <kbd>j</kbd> to start.
        </p>
      </section>
    );
  }

  const changedFiles = trace ? collectChangedFiles(trace) : [];
  const captions = captionsFromOutcomes(trace?.outcomes);
  const reachedLevel = determineReachedLevel(
    summary?.primary_outcome,
    trace?.outcomes && trace.outcomes.length > 0 ? trace.outcomes : undefined
  );

  const durationSeconds = trace?.stats?.duration_seconds ?? summary?.duration_seconds;
  const adapter = trace?.adapter ?? summary?.adapter;
  const modelsUsed = trace?.stats?.models_used?.length
    ? trace.stats.models_used
    : summary?.models_used ?? [];
  const compositeScore = trace?.score?.composite_score;

  const startedLabel = trace?.started_at ? formatDate(trace.started_at) : null;
  const durationLabel = durationSeconds != null ? formatDuration(durationSeconds) : null;

  return (
    <section className="shell-inspector-pane" tabIndex={-1}>
      <div className="shell-insp-header">
        <div className="shell-insp-heading">
          <span className="shell-insp-id">{sessionId}</span>
          <span className="shell-insp-meta">
            <b>{adapter ?? DASH}</b>
            {modelsUsed.length > 0 ? <> &middot; {modelsUsed.join(', ')}</> : null}
          </span>
          {startedLabel && (
            <span className="shell-insp-started">
              Started {startedLabel}
              {durationLabel ? ` · ${durationLabel}` : ''}
            </span>
          )}
        </div>
        <div className="shell-insp-score-wrap">
          <div className="shell-insp-score">
            {compositeScore != null ? Math.round(compositeScore * 100) : DASH}
          </div>
          <div className="shell-insp-score-label">score</div>
        </div>
      </div>

      {liveTail && (
        <div className="shell-livetail-banner">
          <span className="shell-livetail-dot" />
          Live tail — awaiting stream, not yet wired.
        </div>
      )}

      {error && (
        <div className="shell-inspector-error">
          <p>{error}</p>
        </div>
      )}

      {loading && !trace && <div className="shell-inspector-loading">Loading session…</div>}

      {!error && (
        <>
          <div className="shell-ladder-block">
            <div className="shell-section-title">Outcome ladder</div>
            <OutcomeLadder reachedLevel={reachedLevel} captions={captions} />
          </div>

          {trace && (
            <TrajectoryView
              events={trace.events}
              focused={trajectoryFocused}
              onToggleFocus={onToggleTrajectoryFocus}
              compactionEventIds={compactionEventIds}
              eventsTotal={eventsTotal}
              eventsComplete={eventsComplete}
            />
          )}

          {trace?.score && (
            <div className="shell-score-section">
              <ScoreBreakdown score={trace.score} />
            </div>
          )}

          {trace?.stats?.token_usage && (
            <div className="shell-tokens-section">
              <TokenEconomics tokenUsage={trace.stats.token_usage} />
              <CacheWarmth events={trace.events} eventsComplete={eventsComplete} />
            </div>
          )}

          {trace?.events && (
            <ContextComposition events={trace.events} eventsComplete={eventsComplete} />
          )}

          {trace?.events && (
            <Compaction events={trace.events} eventsComplete={eventsComplete} />
          )}

          {trace?.events && (
            <LooseEnds
              events={trace.events}
              sessionId={trace.session_id}
              eventsComplete={eventsComplete}
            />
          )}

          {trace?.recoveries && trace.recoveries.length > 0 && (
            <div className="shell-recovery-block">
              <div className="shell-section-title">
                Recovery ({trace.recoveries.length} {trace.recoveries.length === 1 ? 'cycle' : 'cycles'})
              </div>
              <div className="shell-recovery-list">
                {trace.recoveries.map((rec, i) => (
                  <div className="shell-recovery-item" key={i}>
                    <div className="shell-recovery-row">
                      <span className="shell-recovery-tag is-fail">FAIL #{rec.failure_sequence}</span>
                      <span className="shell-recovery-text">{rec.failure_summary}</span>
                    </div>
                    <div className="shell-recovery-row">
                      <span className="shell-recovery-tag is-fix">FIX #{rec.recovery_sequence}</span>
                      <span className="shell-recovery-text">{rec.recovery_summary}</span>
                    </div>
                    <div className="shell-recovery-meta">
                      {rec.steps_to_recover} step{rec.steps_to_recover === 1 ? '' : 's'}
                      {' · '}
                      {rec.corrective_actions_count} corrective action
                      {rec.corrective_actions_count === 1 ? '' : 's'}
                      {rec.duration_seconds != null ? ` · ${formatDuration(rec.duration_seconds)}` : ''}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}

          {trace?.provenance && (
            <div className="shell-prov-section">
              <ProvenanceBlock provenance={trace.provenance} />
            </div>
          )}

          {changedFiles.length > 0 && (
            <div className="shell-support-block">
              <div className="shell-section-title">Support set ({changedFiles.length})</div>
              <ul className="shell-support-list">
                {changedFiles.slice(0, 12).map((p) => (
                  <li key={p}>{p}</li>
                ))}
              </ul>
            </div>
          )}
        </>
      )}
    </section>
  );
}
