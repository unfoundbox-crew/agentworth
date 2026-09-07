import clsx from 'clsx';
import type { Direction, Persona } from '../protocol';
import type { Theme } from '../model/theme';
import { characterFor } from '../model/theme';
import { rungIndex } from '../model/store';

const RUNG_COUNT = 5;

function rungDots(reached: Direction['reached']) {
  const filled = rungIndex(reached) + 1;
  return '●'.repeat(Math.max(0, filled)) + '○'.repeat(RUNG_COUNT - Math.max(0, filled));
}

export function Strips({
  directions,
  personas,
  theme,
  selectedId,
  onSelect,
}: {
  directions: Direction[];
  personas: Record<string, Persona>;
  theme: Theme;
  selectedId: string | null;
  onSelect(directionId: string): void;
}) {
  return (
    <div className="h-full overflow-y-auto p-3 flex flex-col gap-2" aria-label="strips">
      {directions.map((d, i) => {
        const exception = !!d.exception;
        const riders = d.riders.map((id) => personas[id]).filter((p): p is Persona => !!p);
        return (
          <button
            key={d.id}
            type="button"
            onClick={() => onSelect(d.id)}
            className={clsx(
              'text-left rounded-md border bg-panel px-3 py-2 transition-colors',
              exception ? 'border-line' : 'border-[var(--mv-border-soft)] opacity-70',
              selectedId === d.id && 'ring-1 ring-[var(--mv-accent)]',
            )}
            style={exception ? { borderLeft: '2px solid var(--mv-warn)' } : undefined}
          >
            <div className="flex items-center justify-between gap-2">
              <div className={clsx('truncate', exception ? 'text-ink font-medium' : 'text-muted')}>
                {i < 9 ? <span className="text-dim mr-1">{i + 1}</span> : null}
                {exception ? d.exception!.reason : d.state}
              </div>
              {exception && <span className="text-[10px] text-warn shrink-0">{d.state}</span>}
            </div>
            <div className={clsx('mt-1 truncate', exception ? 'text-xs text-text' : 'text-[11px] text-dim')}>{d.goal}</div>
            <div className="mt-1 flex items-center justify-between text-[10px] text-dim">
              <span className="truncate">
                {d.area} &middot; rung {rungDots(d.reached)}
              </span>
              <span className="flex gap-1 shrink-0">
                {riders.map((r) => (
                  <span key={r.id} style={{ color: characterFor(theme, r.role).badge }}>
                    {characterFor(theme, r.role).name.slice(0, 3).toLowerCase()}
                  </span>
                ))}
              </span>
            </div>
          </button>
        );
      })}
    </div>
  );
}
