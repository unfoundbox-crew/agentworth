import { useEffect, useMemo, useState } from 'react';
import { AgentWorthTrace } from '../types';
import { formatDuration, formatTokens } from '../utils/formatters';
import { useSessions } from '../hooks/useSessions';
import { fetchTraceDetail } from '../services/api';
import { OutcomeLadder, captionsFromOutcomes, determineReachedLevel } from './OutcomeLadder';

export interface InspectorPaneProps {
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

export function InspectorPane({ sessionId, liveTail }: InspectorPaneProps) {
  const { sessions } = useSessions();
  const [trace, setTrace] = useState<AgentWorthTrace | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!sessionId) {
      setTrace(null);
      setError(null);
      setLoading(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError(null);

    // fetchTraceDetail also normalizes the backend's short-form token_usage
    // field names (cache_read_tokens/cache_creation_tokens) to the long
    // names this component reads — don't re-fetch/re-normalize by hand here,
    // that's exactly the duplication that let this drift out of sync before.
    fetchTraceDetail(sessionId).then((data) => {
      if (cancelled) return;
      if (!data) {
        setError('Could not load this session.');
      } else {
        setTrace(data);
      }
      setLoading(false);
    });

    return () => {
      cancelled = true;
    };
  }, [sessionId]);

  const summary = useMemo(
    () => sessions.find((s) => s.session_id === sessionId) ?? null,
    [sessions, sessionId]
  );

  if (!sessionId) {
    return (
      <section className="shell-inspector-pane" tabIndex={-1}>
        <div className="shell-inspector-empty">Select a session to inspect it.</div>
      </section>
    );
  }

  const changedFiles = trace ? collectChangedFiles(trace) : [];
  const captions = captionsFromOutcomes(trace?.outcomes);
  const reachedLevel = determineReachedLevel(
    summary?.primary_outcome,
    trace?.outcomes && trace.outcomes.length > 0 ? trace.outcomes : undefined
  );

  const tokenUsage = trace?.stats?.token_usage;
  const totalTokens = tokenUsage
    ? (tokenUsage.input_tokens ?? 0) +
      (tokenUsage.output_tokens ?? 0) +
      (tokenUsage.cache_read_input_tokens ?? 0) +
      (tokenUsage.cache_creation_input_tokens ?? 0)
    : null;

  const durationSeconds = trace?.stats?.duration_seconds ?? summary?.duration_seconds;
  const model = trace?.stats?.models_used?.[0] ?? summary?.models_used?.[0];
  const adapter = trace?.adapter ?? summary?.adapter;
  const compositeScore = trace?.score?.composite_score;

  const kv: [string, string][] = [
    ['Duration', durationSeconds != null ? formatDuration(durationSeconds) : DASH],
    ['Tokens', totalTokens != null ? formatTokens(totalTokens) : DASH],
    ['Files changed', trace ? String(changedFiles.length) : DASH],
    ['Tests', captions.test_or_build_passed ?? DASH],
    ['Commit', captions.commit_observed ?? DASH],
  ];

  return (
    <section className="shell-inspector-pane" tabIndex={-1}>
      <div className="shell-insp-header">
        <span className="shell-insp-id">{sessionId}</span>
        <span className="shell-insp-meta">
          <b>{adapter ?? DASH}</b> &middot; {model ?? DASH}
        </span>
        <div className="shell-insp-score-wrap">
          <div className="shell-insp-score">
            {compositeScore != null ? (compositeScore * 100).toFixed(0) : DASH}
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

          <div className="shell-kv-block">
            <div className="shell-section-title">Evidence</div>
            <div className="shell-kv-table">
              {kv.map(([k, v]) => (
                <div className="shell-kv-row" key={k}>
                  <span className="shell-kv-key">{k}</span>
                  <span className="shell-kv-val">{v}</span>
                </div>
              ))}
            </div>
          </div>

          {changedFiles.length > 0 && (
            <div className="shell-support-block">
              <div className="shell-section-title">Support set</div>
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
