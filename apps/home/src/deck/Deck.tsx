import { useRef, useState } from 'react';
import { useHome, orderDirections, dispatch } from '../model/store';
import { gateway } from '../ws/client';
import type { Direction, Harness } from '../protocol';
import { Cruise } from './Cruise';
import { Alert } from './Alert';
import { Consoles } from './Consoles';
import { Ambient } from './Ambient';
import { FirstRun } from './FirstRun';
import { FirstDirection } from './FirstDirection';
import { FirstRide } from './FirstRide';
import { useDeckKeys } from './keys';
import type { DockHandle } from './Dock';

type Flow =
  | { phase: 'none' }
  | { phase: 'firstrun' }
  | { phase: 'firstdirection'; directionId: string; goal: string }
  | { phase: 'firstride'; directionId: string; personaId: string };

function newLocalId(): string {
  return `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;
}

/**
 * What `start_rider` will name the persona it seats -- `<harness>-<short direction id>`,
 * mirrored from `dispatch_start_rider` in `gateway.rs`. `newLocalId` only ever produces
 * lowercase letters, digits and no punctuation, so this is already a valid persona id with
 * no separate slugify step needed on this side.
 */
function expectedPersonaId(directionId: string, harnessId: string): string {
  const shortId = directionId.replace(/-/g, '').slice(0, 8);
  return `${harnessId}-${shortId}`;
}

/**
 * The elastic deck's state machine: firstrun/firstdirection/firstride (setting up the very
 * first ride, or a fresh one summoned with `/`) -> cruise (no exceptions) -> alert (>=1
 * exception) -> consoles (the human opened one). Cruise and alert never show while a first
 * ride is starting (docs/specs/home.md's "nothing opens on an end state" -- an empty deck is
 * itself an end state). Escape always returns to whichever of cruise/alert the data implies.
 */
export function Deck() {
  const directions = useHome((s) => s.directions);
  const personas = useHome((s) => s.personas);
  const messages = useHome((s) => s.messages);
  const env = useHome((s) => s.env);
  const theme = useHome((s) => s.theme);
  const consoleOpen = useHome((s) => s.consoleOpen);
  const dockRef = useRef<DockHandle>(null);
  const [flow, setFlow] = useState<Flow>({ phase: 'none' });

  const ordered = orderDirections(Object.values(directions));
  const active = ordered.filter((d) => d.state !== 'done');
  const hasException = ordered.some((d) => d.exception);

  // Once the seated rider's first message lands, this flow is done -- the ordinary state
  // machine takes over from here on (apps/home/DESIGN.md's "the normal deck state machine").
  if (flow.phase === 'firstride') {
    const office = messages[`office-${flow.personaId}`] ?? [];
    if (office.length > 0) setFlow({ phase: 'none' });
  }

  const showFirstRun = flow.phase === 'firstrun' || (flow.phase === 'none' && active.length === 0);
  const phase = showFirstRun
    ? 'firstrun'
    : flow.phase === 'firstdirection'
      ? 'firstdirection'
      : flow.phase === 'firstride'
        ? 'firstride'
        : consoleOpen
          ? 'consoles'
          : hasException
            ? 'alert'
            : 'cruise';

  useDeckKeys({
    onSelectIndex: (n) => {
      const d = ordered[n - 1];
      if (!d) return;
      dispatch({ type: 'open_console', directionId: d.id });
    },
    onSelectAll: () => {
      if (!consoleOpen && ordered[0]) dispatch({ type: 'open_console', directionId: ordered[0].id });
      dockRef.current?.focus();
    },
    onFocusDock: () => {
      // `/` is the new-direction hotkey (docs/specs/home.md decision 5) when nothing is
      // already open to steer; otherwise it keeps its old job of focusing the dock.
      if (consoleOpen) {
        dockRef.current?.focus();
        return;
      }
      setFlow({ phase: 'firstrun' });
    },
    onEscape: () => {
      dispatch({ type: 'close_console' });
      if (flow.phase === 'firstrun' || flow.phase === 'firstdirection') setFlow({ phase: 'none' });
    },
  });

  function beginDirection(goal: string) {
    setFlow({ phase: 'firstdirection', directionId: newLocalId(), goal });
  }

  function pickRider(harness: Harness, area: string) {
    if (flow.phase !== 'firstdirection') return;
    const directionId = flow.directionId;
    const direction: Omit<Direction, 'reached' | 'spentTokens' | 'state' | 'exception' | 'createdAt' | 'updatedAt'> = {
      id: directionId,
      goal: flow.goal,
      area,
      done: 'test',
      budgetTokens: env.budgetDefaultTokens,
      riders: [],
    };
    gateway.send({ t: 'set_direction', direction });
    gateway.send({ t: 'start_rider', directionId, harness: harness.id, clientId: `c-${newLocalId()}` });
    setFlow({ phase: 'firstride', directionId, personaId: expectedPersonaId(directionId, harness.id) });
  }

  // `set_direction`'s broadcast lands within a tick, but the very first render of `firstride`
  // can beat it here -- a synthesized placeholder avoids a blank frame in that gap rather than
  // waiting on the real one.
  const firstRideDirection =
    flow.phase === 'firstride'
      ? (directions[flow.directionId] ?? {
          id: flow.directionId,
          goal: '',
          area: env.repo ?? env.cwd,
          done: 'test' as const,
          reached: null,
          budgetTokens: env.budgetDefaultTokens,
          spentTokens: 0,
          riders: [],
          state: 'idle' as const,
          exception: null,
          createdAt: new Date().toISOString(),
          updatedAt: new Date().toISOString(),
        })
      : undefined;

  return (
    <div className="h-full grid" style={{ gridTemplateRows: '1fr', gridTemplateColumns: '1fr 28px' }}>
      {phase === 'firstrun' && <FirstRun env={env} onSubmit={beginDirection} />}
      {phase === 'firstdirection' && flow.phase === 'firstdirection' && (
        <FirstDirection goal={flow.goal} env={env} onPick={pickRider} />
      )}
      {phase === 'firstride' && flow.phase === 'firstride' && firstRideDirection && (
        <FirstRide
          direction={firstRideDirection}
          persona={personas[flow.personaId]}
          theme={theme}
          dockRef={dockRef}
        />
      )}
      {phase === 'cruise' && <Cruise />}
      {phase === 'alert' && <Alert />}
      {phase === 'consoles' && <Consoles dockRef={dockRef} />}
      <Ambient />
    </div>
  );
}
