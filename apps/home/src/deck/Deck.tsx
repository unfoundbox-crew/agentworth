import { useRef } from 'react';
import { useHome, orderDirections, dispatch } from '../model/store';
import { Cruise } from './Cruise';
import { Alert } from './Alert';
import { Consoles } from './Consoles';
import { Ambient } from './Ambient';
import { useDeckKeys } from './keys';
import type { DockHandle } from './Dock';

/**
 * The elastic deck's state machine: cruise (no exceptions) -> alert (>=1
 * exception) -> consoles (the human opened one). Cruise and alert never
 * disappear underneath consoles -- opening one is a deliberate act, and
 * Escape always returns to whichever of the first two the data implies.
 */
export function Deck() {
  const directions = useHome((s) => s.directions);
  const consoleOpen = useHome((s) => s.consoleOpen);
  const dockRef = useRef<DockHandle>(null);

  const ordered = orderDirections(Object.values(directions));
  const hasException = ordered.some((d) => d.exception);
  const phase = consoleOpen ? 'consoles' : hasException ? 'alert' : 'cruise';

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
      if (!consoleOpen && ordered[0]) dispatch({ type: 'open_console', directionId: ordered[0].id });
      dockRef.current?.focus();
    },
    onEscape: () => dispatch({ type: 'close_console' }),
  });

  return (
    <div className="h-full grid" style={{ gridTemplateRows: '1fr', gridTemplateColumns: '1fr 28px' }}>
      {phase === 'cruise' && <Cruise />}
      {phase === 'alert' && <Alert />}
      {phase === 'consoles' && <Consoles dockRef={dockRef} />}
      <Ambient />
    </div>
  );
}
