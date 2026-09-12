import { forwardRef, useImperativeHandle, useRef, useState } from 'react';
import clsx from 'clsx';
import type { Direction, Persona, SteerMode } from '../protocol';
import type { Theme } from '../model/theme';
import { characterFor } from '../model/theme';
import { dispatch } from '../model/store';
import { gateway } from '../ws/client';

export interface DockHandle {
  focus(): void;
}

/**
 * The one input. A steer names its direction and its timing explicitly --
 * "now" or "after this step" -- never an ambiguous default. `@` completes a
 * rider name from the current theme; the app never guesses who was meant.
 */
export const Dock = forwardRef<DockHandle, { direction: Direction | undefined; personas: Persona[]; theme: Theme }>(
  function Dock({ direction, personas, theme }, ref) {
    const [text, setText] = useState('');
    const [mode, setMode] = useState<SteerMode>('now');
    const [showMentions, setShowMentions] = useState(false);
    const [justSent, setJustSent] = useState(false);
    const inputRef = useRef<HTMLInputElement>(null);

    useImperativeHandle(ref, () => ({
      focus: () => inputRef.current?.focus(),
    }));

    function riderName(id: string): string {
      const p = personas.find((x) => x.id === id);
      return p ? characterFor(theme, p.role).name.toLowerCase().replace(/\s+/g, '_') : id;
    }

    function mentionsIn(value: string): string[] {
      const found: string[] = [];
      for (const p of personas) {
        if (value.includes(`@${riderName(p.id)}`)) found.push(p.id);
      }
      return found;
    }

    function send() {
      if (!direction || !text.trim()) return;
      const mentions = mentionsIn(text);
      // `clientId` lets the gateway's own echo of this steer replace this local one in place
      // (see `store.ts`'s `message` case) instead of showing up twice on the course.
      const clientId = `c-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
      gateway.send({ t: 'steer', directionId: direction.id, text, mode, mentions, clientId });
      dispatch({
        type: 'local',
        message: {
          id: `local-${Date.now()}`,
          spaceId: `office-${direction.riders[0]}`,
          from: 'you',
          kind: 'speech',
          text,
          at: new Date().toISOString(),
          mentions,
          clientId,
        },
      });
      setText('');
      setShowMentions(false);
      setJustSent(true);
      setTimeout(() => setJustSent(false), 1000);
    }

    return (
      <div className="relative col-start-1 border-t border-line bg-panel px-3 py-2.5 flex items-center gap-2.5" aria-label="dock">
        {showMentions && (
          <div className="absolute bottom-full left-3 mb-1 rounded-md border border-line bg-panel py-1 shadow-lg">
            {personas.map((p) => (
              <button
                key={p.id}
                type="button"
                className="block w-full text-left px-3 py-1 text-[11px] text-text hover:bg-[var(--mv-surface-2)]"
                onClick={() => {
                  setText((t) => `${t}@${riderName(p.id)} `);
                  setShowMentions(false);
                  inputRef.current?.focus();
                }}
              >
                @{riderName(p.id)}
              </button>
            ))}
          </div>
        )}
        <div className="text-[10px] text-muted shrink-0">
          {direction ? `steering: ${direction.goal}` : 'no direction selected'}
        </div>
        <input
          ref={inputRef}
          value={text}
          readOnly={justSent}
          onChange={(e) => {
            setText(e.target.value);
            setShowMentions(e.target.value.endsWith('@'));
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter') send();
          }}
          placeholder={justSent ? 'sent' : 'steer the selected direction, or @ a rider'}
          className={clsx(
            'flex-1 bg-transparent outline-none text-[13px] text-text placeholder:text-dim',
            justSent && 'opacity-60',
          )}
        />
        <div className="flex border border-line rounded-md overflow-hidden shrink-0">
          <button
            type="button"
            onClick={() => setMode('now')}
            className={clsx('px-3 py-1.5 text-[13px]', mode === 'now' ? 'bg-accent text-[var(--mv-accent-contrast)]' : 'text-muted')}
          >
            now
          </button>
          <button
            type="button"
            onClick={() => setMode('after')}
            className={clsx('px-3 py-1.5 text-[13px]', mode === 'after' ? 'bg-accent text-[var(--mv-accent-contrast)]' : 'text-muted')}
          >
            after this step
          </button>
        </div>
      </div>
    );
  },
);
