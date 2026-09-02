import { useCallback, useEffect, useRef, useState } from 'react';
import {
  Archie,
  ARCHIE_ACCESSORIES,
  ARCHIE_COLOURWAYS,
  type ArchieAccessory,
  type ArchieColourway,
} from '@ui/Archie';
import { IconSettings } from './dsIcons';
import { loadArchieSettings, setArchieSetting, useArchieSettings } from './archieKit';

const COLOURWAY_NAMES: Record<ArchieColourway, string> = {
  C1: 'Mono',
  C2: 'Dark',
  C3: 'AgentWorth',
  C4: 'Quiet',
};

/** Archie's kit and colourway, from the topbar. Reads the persisted config when it
 *  opens and writes it back on every change — the same file `agentworth config set`
 *  writes, so the dashboard and the CLI never disagree about how he is drawn. */
export function ArchieSettings() {
  const [open, setOpen] = useState(false);
  const settings = useArchieSettings();
  const panel = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (open) void loadArchieSettings();
  }, [open]);

  useEffect(() => {
    if (!open) return;
    function onDown(e: MouseEvent) {
      const target = e.target as Node;
      if (panel.current?.contains(target) || trigger.current?.contains(target)) return;
      setOpen(false);
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        setOpen(false);
        trigger.current?.focus();
      }
    }
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  const pickAccessory = useCallback(
    (accessory: ArchieAccessory) => void setArchieSetting({ accessory }),
    []
  );
  const pickColourway = useCallback(
    (colourway: ArchieColourway) => void setArchieSetting({ colourway }),
    []
  );

  return (
    <div className="archie-settings">
      <button
        ref={trigger}
        type="button"
        className="palette-toggle"
        aria-expanded={open}
        aria-haspopup="dialog"
        onClick={() => setOpen((o) => !o)}
        title="Archie's kit and colourway"
      >
        <IconSettings size={14} />
        <span className="sr-only">Settings</span>
      </button>

      {open && (
        <div className="archie-panel" ref={panel} role="dialog" aria-label="Archie">
          <div className="archie-panel-head">
            <span className="archie-panel-stage">
              <Archie
                pose="front-sit"
                size={64}
                accessory={settings.accessory}
                colourway={settings.colourway}
                label=""
              />
            </span>
            <div>
              <p className="archie-panel-title">Archie</p>
              <p className="archie-panel-sub">
                {settings.colourway} {COLOURWAY_NAMES[settings.colourway]} &middot;{' '}
                {settings.accessory}
              </p>
            </div>
          </div>

          <div className="archie-row" role="group" aria-label="Accessory">
            <span className="archie-row-label">Kit</span>
            <div className="archie-row-options">
              {ARCHIE_ACCESSORIES.map((value) => (
                <button
                  key={value}
                  type="button"
                  aria-pressed={settings.accessory === value}
                  onClick={() => pickAccessory(value)}
                >
                  {value}
                </button>
              ))}
            </div>
          </div>

          <div className="archie-row" role="group" aria-label="Colourway">
            <span className="archie-row-label">Colour</span>
            <div className="archie-row-options">
              {ARCHIE_COLOURWAYS.map((value) => (
                <button
                  key={value}
                  type="button"
                  aria-pressed={settings.colourway === value}
                  onClick={() => pickColourway(value)}
                  title={COLOURWAY_NAMES[value]}
                >
                  {value}
                </button>
              ))}
            </div>
          </div>

          {settings.source === 'local' && (
            <p className="archie-panel-note">
              No local API here, so this is remembered in this browser only.
            </p>
          )}
        </div>
      )}
    </div>
  );
}
