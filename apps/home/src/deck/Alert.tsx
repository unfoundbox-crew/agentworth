import { useEffect, useState } from 'react';
import { ThemeToggle } from '@ui/ThemeToggle';
import { useHome, orderDirections, dispatch } from '../model/store';
import { characterFor } from '../model/theme';
import type { Direction, Persona } from '../protocol';

function useElapsedMinutes(since: string): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 15_000);
    return () => clearInterval(id);
  }, []);
  return Math.max(0, Math.round((now - new Date(since).getTime()) / 60_000));
}

const TIME_RE = /\b\d{1,2}:\d{2}\b/;
const DEAD_RE = /exhausted|quota/i;

/** A dead rider line: "<who> · reborn HH:MM" if a time is named, else "<who> · window exhausted". */
function deadLine(who: string, reasonOrNull: string | null): string {
  const time = reasonOrNull?.match(TIME_RE)?.[0];
  return time ? `${who} · reborn ${time}` : `${who} · window exhausted`;
}

export function Alert() {
  const personas = useHome((s) => s.personas);
  const directions = useHome((s) => s.directions);
  const theme = useHome((s) => s.theme);

  const ordered = orderDirections(Object.values(directions));
  const exceptions = ordered.filter((d): d is Direction & { exception: NonNullable<Direction['exception']> } => !!d.exception);
  const current = exceptions[0];
  const elapsed = useElapsedMinutes(current.exception.since);

  const rider = personas[current.riders[0]] as Persona | undefined;
  const who = rider ? characterFor(theme, rider.role).name.toLowerCase() : 'a rider';

  const others = Object.keys(directions).length - exceptions.length;

  const dead = [
    ...Object.values(personas)
      .filter((p) => p.presence === 'unknown')
      .map((p) => deadLine(characterFor(theme, p.role).name.toLowerCase(), null)),
    ...exceptions
      .filter((d) => DEAD_RE.test(d.exception.reason))
      .map((d) => {
        const r = personas[d.riders[0]];
        const label = r ? characterFor(theme, r.role).name.toLowerCase() : d.area;
        return deadLine(label, d.exception.reason);
      }),
  ];

  function openPane() {
    if (!rider) return;
    dispatch({ type: 'local', message: { id: `local-${Date.now()}`, spaceId: current.id, from: 'you', kind: 'system', text: `pane: ${rider.paneId}`, at: new Date().toISOString() } });
  }

  function steer() {
    dispatch({ type: 'open_console', directionId: current.id });
  }

  return (
    <div className="col-start-1 row-span-4 relative">
      <div className="absolute left-6 top-5 font-sans text-sm font-medium text-dim tracking-tight">home</div>
      <div className="absolute right-6 top-4">
        <ThemeToggle />
      </div>

      <div
        className="absolute left-1/2 -translate-x-1/2 top-[35%] w-[560px] rounded-lg bg-panel border border-line p-5 enter"
        style={{ borderLeft: '2px solid var(--mv-warn)' }}
      >
        <div className="text-[13px] text-ink font-medium">{current.goal}</div>
        <div className="mt-2 text-xs text-text">
          {who} is waiting on you &middot; {elapsed} min &middot; {current.exception.reason}
        </div>
        <div className="mt-4 flex items-center gap-2">
          <button
            type="button"
            onClick={openPane}
            className="border border-line rounded-md px-3 py-1.5 text-xs text-text bg-transparent hover:bg-[var(--mv-surface-2)]"
          >
            open the pane
          </button>
          <button
            type="button"
            onClick={steer}
            className="border border-line rounded-md px-3 py-1.5 text-xs text-ink bg-[var(--mv-surface-3)]"
          >
            steer
          </button>
          <div className="flex-1" />
          <div className="text-[10px] text-dim">
            {exceptions.length > 1 ? `1 of ${exceptions.length}` : '1 of 1'}
            {exceptions[1] ? ` · next: ${exceptions[1].goal}` : ''}
          </div>
        </div>
      </div>

      {others > 0 && (
        <div className="absolute left-1/2 -translate-x-1/2 top-[35%] mt-[210px] w-[560px] text-center text-[11px] text-dim">
          {others} others riding fine
        </div>
      )}

      {dead.length > 0 && (
        <div className="absolute left-6 bottom-16 flex flex-col gap-1">
          {dead.map((line) => (
            <div key={line} className="text-[11px] text-dim">{line}</div>
          ))}
        </div>
      )}

      <div className="absolute left-6 right-6 bottom-14 border-t border-line" />
    </div>
  );
}
