import { ThemeToggle } from '@ui/ThemeToggle';
import type { Direction, Persona } from '../protocol';
import type { Theme } from '../model/theme';
import { characterFor } from '../model/theme';
import { Dock, type DockHandle } from './Dock';
import type { RefObject } from 'react';

/**
 * Ten seconds after the first pick: the rider is seated, but has not spoken yet. A calm hold
 * between "nothing exists" and "the normal deck" -- one strip, one seat, a course that says
 * "reading the repo..." instead of nothing. `Deck` swaps this out for the ordinary
 * cruise/alert/consoles machine the moment the rider's first message lands.
 */
export function FirstRide({
  direction,
  persona,
  theme,
  dockRef,
}: {
  direction: Direction;
  persona: Persona | undefined;
  theme: Theme;
  dockRef: RefObject<DockHandle>;
}) {
  const character = persona ? characterFor(theme, persona.role) : undefined;

  return (
    <div className="col-start-1 row-span-4 grid h-full" style={{ gridTemplateRows: '32px 1fr auto' }}>
      <div className="flex items-center gap-2.5 px-5 border-b border-line">
        <span className="font-sans text-xs font-medium text-dim">home</span>
        <div className="flex-1" />
        <ThemeToggle />
      </div>

      <div className="p-5 flex gap-5 overflow-hidden">
        <div className="w-[360px] shrink-0 flex flex-col gap-4">
          <div
            className="rounded-md border border-line bg-panel px-3.5 py-2.5 enter"
            style={{ borderLeft: '4px solid var(--mv-accent)' }}
          >
            <div className="flex items-center justify-between">
              <div className="text-[13px] font-medium text-ink truncate">{direction.goal}</div>
              <div className="text-[11px] text-accent shrink-0">riding</div>
            </div>
            <div className="mt-1.5 text-[10px] text-muted truncate">{direction.area}</div>
          </div>

          <div className="rounded-md border border-line bg-panel p-3 relative enter">
            <div className="text-[13px] font-medium text-ink">
              {character?.name ?? persona?.kind ?? 'seating...'}{' '}
              <span className="text-[10px] text-muted">executor</span>
            </div>
            <div className="mt-1.5 text-[11px] text-muted">reading the repo&hellip;</div>
            <div className="absolute right-2 bottom-2 text-[9px] text-dim border border-dashed border-line rounded px-1.5 py-0.5">
              seat 1
            </div>
          </div>
        </div>

        <div className="flex-1 rounded-md border border-line bg-panel p-3.5 flex items-center justify-center">
          <div className="text-[11px] text-muted presence-working">reading the repo&hellip;</div>
        </div>
      </div>

      <Dock ref={dockRef} direction={direction} personas={persona ? [persona] : []} theme={theme} />
    </div>
  );
}
