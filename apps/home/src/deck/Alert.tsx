import { useEffect, useState } from 'react';
import { ThemeToggle } from '@ui/ThemeToggle';
import { useHome, orderDirections, dispatch } from '../model/store';
import { characterFor } from '../model/theme';
import { gateway } from '../ws/client';
import type { AnswerKey, Direction, Persona } from '../protocol';

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
const DONT_ASK_AGAIN_RE = /don.t ask again/i;
const YES_NO_RE = /\(y\/n\)|\[y\/n\]|y\/n\?/i;

/**
 * Buttons the alert plate offers for a blocked pane's prompt: 1/3/esc always (yes / no / cancel,
 * whatever the pane calls them), "2 -- don't ask again" only when the prompt text actually names
 * that option (never pre-selected -- it's a standing-permission change), y/n only for a plain
 * yes/no prompt.
 */
function detectAnswerOptions(reason: string): { key: AnswerKey; label: string }[] {
  const options: { key: AnswerKey; label: string }[] = [{ key: '1', label: '1' }];
  if (DONT_ASK_AGAIN_RE.test(reason)) options.push({ key: '2', label: "2 · don't ask again" });
  options.push({ key: '3', label: '3' }, { key: 'esc', label: 'esc' });
  if (YES_NO_RE.test(reason)) options.push({ key: 'y', label: 'y' }, { key: 'n', label: 'n' });
  return options;
}

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

  function answer(key: AnswerKey) {
    if (!rider) return;
    gateway.send({ t: 'answer', directionId: current.id, personaId: rider.id, key });
  }

  // A blocked pane's prompt is what `fetch_blocked_prompt` in the gateway reads off the pane --
  // multi-line, verbatim. Other exceptions (halted, over budget) stay a single short line, so
  // this only offers answer buttons when there's an actual prompt to answer.
  const isPrompt = current.state === 'waiting' && current.exception.reason.includes('\n');

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
          {who} is waiting on you &middot; {elapsed} min{!isPrompt ? ` · ${current.exception.reason}` : ''}
        </div>

        {isPrompt && (
          <pre className="mt-2 max-h-[168px] overflow-auto rounded border border-line bg-[var(--mv-ground)] p-2 text-[10px] leading-snug text-muted whitespace-pre-wrap">
            {current.exception.reason}
          </pre>
        )}

        <div className="mt-4 flex items-center gap-2 flex-wrap">
          {isPrompt &&
            rider &&
            detectAnswerOptions(current.exception.reason).map((o) => (
              <button
                key={o.key}
                type="button"
                onClick={() => answer(o.key)}
                className="border border-line rounded-md px-2.5 py-1.5 text-xs text-text bg-transparent hover:bg-[var(--mv-surface-2)]"
              >
                {o.label}
              </button>
            ))}
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
