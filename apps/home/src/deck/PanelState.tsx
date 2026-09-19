/**
 * Shared designed states for every deck panel: empty / loading / error.
 * One type scale, one spacing rhythm, honest copy -- never a blank hole.
 * Visual-only: no data fetching, no navigation, no behavior change.
 */
export type PanelStateKind = 'empty' | 'loading' | 'error';

export function PanelState({
  kind,
  title,
  hint,
}: {
  kind: PanelStateKind;
  title: string;
  hint?: string;
}) {
  const role = kind === 'error' ? 'alert' : 'status';
  const live = kind === 'loading' ? 'polite' : undefined;
  return (
    <div
      role={role}
      aria-live={live}
      aria-busy={kind === 'loading' ? true : undefined}
      className="deck-panel-state"
    >
      <div className="deck-panel-state-title">{title}</div>
      {hint ? <div className="deck-panel-state-hint">{hint}</div> : null}
    </div>
  );
}
