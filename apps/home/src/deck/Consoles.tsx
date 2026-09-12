import type { RefObject } from 'react';
import { ThemeToggle } from '@ui/ThemeToggle';
import { useHome, orderDirections, dispatch } from '../model/store';
import { Strips } from './Strips';
import { Course } from './Course';
import { Seats } from './Seats';
import { Dock, type DockHandle } from './Dock';

/** Consoles: strips, course, seats, and the dock. Opened by picking a direction; Escape closes it. */
export function Consoles({ dockRef }: { dockRef: RefObject<DockHandle> }) {
  const personas = useHome((s) => s.personas);
  const directions = useHome((s) => s.directions);
  const stops = useHome((s) => s.stops);
  const artifacts = useHome((s) => s.artifacts);
  const theme = useHome((s) => s.theme);
  const selectedDirection = useHome((s) => s.selectedDirection);
  const mute = useHome((s) => s.mute);
  const solo = useHome((s) => s.solo);

  const ordered = orderDirections(Object.values(directions));
  const current = selectedDirection ? directions[selectedDirection] : undefined;

  const needYou = ordered.filter((d) => d.exception).length;
  const riding = ordered.filter((d) => !d.exception && d.state === 'riding').length;
  const reborn = Object.values(personas).filter((p) => p.presence === 'unknown').length;

  return (
    <div className="col-start-1 row-span-4 grid h-full" style={{ gridTemplateRows: '32px 1fr auto' }}>
      <div className="flex items-center gap-2.5 px-5 border-b border-line" aria-label="status">
        <span className="font-sans text-xs font-medium text-dim">home</span>
        <div className="flex-1" />
        {needYou > 0 && <span className="text-[11px] text-warn">{needYou} need you</span>}
        {needYou > 0 && <span className="text-[11px] text-dim">&middot;</span>}
        <span className="text-[11px] text-muted">{riding} riding</span>
        {reborn > 0 && (
          <>
            <span className="text-[11px] text-dim">&middot;</span>
            <span className="text-[11px] text-dim">{reborn} reborn</span>
          </>
        )}
        <ThemeToggle />
      </div>

      <div className="grid overflow-hidden" style={{ gridTemplateColumns: '300px 1fr 260px' }}>
        <div className="border-r border-line overflow-hidden">
          <Strips
            directions={ordered}
            personas={personas}
            theme={theme}
            selectedId={selectedDirection}
            onSelect={(id) => dispatch({ type: 'select_direction', directionId: id })}
          />
        </div>
        <div className="overflow-hidden">
          <Course direction={current} stops={stops} artifacts={artifacts} theme={theme} personas={personas} mute={mute} solo={solo} />
        </div>
        <div className="border-l border-line overflow-hidden">
          <Seats personas={Object.values(personas)} theme={theme} mute={mute} solo={solo} />
        </div>
      </div>

      <Dock ref={dockRef} direction={current} personas={Object.values(personas)} theme={theme} />
    </div>
  );
}
