import clsx from 'clsx';
import type { Persona } from '../protocol';
import type { Theme } from '../model/theme';
import { characterFor } from '../model/theme';
import { dispatch } from '../model/store';

/** Presence as DESIGN.md defines it: idle hollow, working pulses, blocked filled amber, done filled in badge hue, unknown dashed. */
function PresenceDot({ presence, badge }: { presence: Persona['presence']; badge: string }) {
  const size = 6;
  if (presence === 'unknown') {
    return (
      <span
        aria-hidden
        style={{ width: size, height: size, borderRadius: '50%', border: `1px dashed var(--mv-faint)` }}
      />
    );
  }
  if (presence === 'idle') {
    return (
      <span
        aria-hidden
        style={{ width: size, height: size, borderRadius: '50%', border: `1px solid var(--mv-muted)` }}
      />
    );
  }
  if (presence === 'blocked') {
    return <span aria-hidden style={{ width: size, height: size, borderRadius: '50%', background: 'var(--mv-warn)' }} />;
  }
  const color = presence === 'done' ? badge : 'var(--mv-success)';
  return (
    <span
      aria-hidden
      className={presence === 'working' ? 'presence-working' : undefined}
      style={{ width: size, height: size, borderRadius: '50%', background: color }}
    />
  );
}

export function Seats({
  personas,
  theme,
  mute,
  solo,
}: {
  personas: Persona[];
  theme: Theme;
  mute: Set<string>;
  solo: Set<string>;
}) {
  return (
    <div className="h-full overflow-y-auto p-3" aria-label="seats">
      <div className="text-[10px] text-muted mb-2">seats</div>
      {personas.map((p) => {
        const c = characterFor(theme, p.role);
        const muted = mute.has(p.id);
        const soloed = solo.has(p.id);
        return (
          <div key={p.id} className="flex items-center gap-2 py-1.5 border-b border-[var(--mv-border-soft)] last:border-0">
            <div
              className="w-[18px] h-[18px] rounded flex items-center justify-center text-[10px] font-semibold shrink-0"
              style={{ background: c.badge, color: 'var(--mv-accent-contrast)' }}
            >
              {c.name.charAt(0).toUpperCase()}
            </div>
            <PresenceDot presence={p.presence} badge={c.badge} />
            <div className={clsx('flex-1 truncate text-[11px]', muted ? 'text-dim' : 'text-text')}>{p.title}</div>
            <div className="flex gap-1 shrink-0 text-[9px] text-dim">
              <button
                type="button"
                onClick={() => dispatch({ type: 'toggle_solo', personaId: p.id })}
                className={clsx('px-1 rounded hover:text-ink', soloed && 'text-ink')}
              >
                solo
              </button>
              <button
                type="button"
                onClick={() => dispatch({ type: 'toggle_mute', personaId: p.id })}
                className={clsx('px-1 rounded hover:text-ink', muted && 'text-ink')}
              >
                mute
              </button>
            </div>
          </div>
        );
      })}
    </div>
  );
}
