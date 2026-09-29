import { useEffect, useRef, useState } from 'react';
import { useHome, orderDirections, dispatch } from '../model/store';
import {
  parseHomeLocation,
  syncArchivePath,
  syncInsightsPath,
  useRoute,
} from '../model/route';
import { gateway } from '../ws/client';
import type { Direction, Harness } from '../protocol';
import { Cruise } from './Cruise';
import { Alert } from './Alert';
import { Consoles } from './Consoles';
import { Insights } from './Insights';
import { Archive } from '../archive/Archive';
import { Ambient } from './Ambient';
import { FirstRun } from './FirstRun';
import { FirstDirection } from './FirstDirection';
import { FirstRide } from './FirstRide';
import { useDeckKeys } from './keys';
import type { DockHandle } from './Dock';
import { selectionToArgs, type ModelSelection } from '../model/modelSelection';
import { resolveCatalog } from '../model/catalog';

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
 *
 * App-merge overlays: `i` / `/insights` opens Insights; `a` / `/s/<id>` opens Archive (P2).
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
  const route = useRoute();

  const [insightsOpen, setInsightsOpen] = useState(() =>
    typeof window === 'undefined'
      ? false
      : parseHomeLocation(window.location.pathname, window.location.hash).insights
  );
  // Archive opens from `a`, or whenever the URL carries `/s/<id>`.
  const [archiveOpen, setArchiveOpen] = useState(() =>
    typeof window === 'undefined'
      ? false
      : !!parseHomeLocation(window.location.pathname, window.location.hash).sessionId
  );

  // URL → phase: session deep link wins; insights path/hash next.
  useEffect(() => {
    if (typeof window === 'undefined') return;
    const onChange = () => {
      const parsed = parseHomeLocation(window.location.pathname, window.location.hash);
      if (parsed.sessionId) {
        setArchiveOpen(true);
        setInsightsOpen(false);
        return;
      }
      if (parsed.insights) {
        setInsightsOpen(true);
        setArchiveOpen(false);
        return;
      }
      // Root (or other) path via back/forward: drop archive only when we left a
      // session URL; `a` can keep archive open at `/` without a session id.
      if (parsed.path === '/' || parsed.path === '') {
        setInsightsOpen(false);
      }
    };
    window.addEventListener('popstate', onChange);
    window.addEventListener('hashchange', onChange);
    window.addEventListener('agentworth-home-route-change', onChange);
    return () => {
      window.removeEventListener('popstate', onChange);
      window.removeEventListener('hashchange', onChange);
      window.removeEventListener('agentworth-home-route-change', onChange);
    };
  }, []);

  // Phase → URL. Archive owns `/s/<id>`; insights owns `/insights`.
  useEffect(() => {
    if (archiveOpen) {
      syncArchivePath(true, route.sessionId);
      return;
    }
    syncInsightsPath(insightsOpen);
  }, [archiveOpen, insightsOpen, route.sessionId]);

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
  const phase = archiveOpen
    ? 'archive'
    : insightsOpen
      ? 'insights'
      : showFirstRun
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

  function closeArchive() {
    setArchiveOpen(false);
    syncArchivePath(false, null);
  }

  function closeInsights() {
    setInsightsOpen(false);
  }

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
      // While archive is open, `/` focuses the session filter (archive keys,
      // capture phase). While insights are open, `/` leaves them.
      if (archiveOpen) return;
      if (insightsOpen) {
        closeInsights();
        return;
      }
      if (consoleOpen) {
        dockRef.current?.focus();
        return;
      }
      setFlow({ phase: 'firstrun' });
    },
    onEscape: () => {
      if (archiveOpen) {
        closeArchive();
        return;
      }
      if (insightsOpen) {
        closeInsights();
        return;
      }
      dispatch({ type: 'close_console' });
      if (flow.phase === 'firstrun' || flow.phase === 'firstdirection') setFlow({ phase: 'none' });
    },
    onToggleInsights: () => {
      if (insightsOpen) {
        closeInsights();
        return;
      }
      if (archiveOpen) closeArchive();
      setInsightsOpen(true);
    },
    onToggleArchive: () => {
      if (archiveOpen) {
        closeArchive();
        return;
      }
      setInsightsOpen(false);
      setArchiveOpen(true);
    },
  });

  function beginDirection(goal: string) {
    setFlow({ phase: 'firstdirection', directionId: newLocalId(), goal });
  }

  function pickRider(harness: Harness, area: string, selection?: ModelSelection) {
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
    const args = selection ? selectionToArgs(selection, resolveCatalog()) : undefined;
    gateway.send({
      t: 'start_rider',
      directionId,
      harness: harness.id,
      ...(args ? { args } : {}),
      clientId: `c-${newLocalId()}`,
    });
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
      {phase === 'insights' && <Insights open />}
      {phase === 'archive' && (
        <Archive sessionId={route.sessionId} onNavigate={route.navigate} />
      )}
      <Ambient />
    </div>
  );
}
