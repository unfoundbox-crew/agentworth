import { useEffect, useMemo, useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { Artifact, Direction, Message, Persona, Stop } from '../protocol';
import type { Theme } from '../model/theme';
import { characterFor } from '../model/theme';
import { useHome } from '../model/store';
import { gateway } from '../ws/client';

const RUNG_LABEL: Record<string, string> = {
  said: 'said',
  artifact: 'diff',
  test: 'test ok',
  commit: 'commit',
  ci: 'ci',
};

function MessageLine({ m, theme, personas }: { m: Message; theme: Theme; personas: Record<string, Persona> }) {
  if (m.kind === 'system') {
    return <div className="italic text-[11px] text-dim">{m.text}</div>;
  }
  if (m.kind === 'work') {
    return <div className="text-[11px] text-dim">{m.text}</div>;
  }
  const p = personas[m.from];
  const name = m.from === 'you' ? 'you' : p ? characterFor(theme, p.role).name : m.from;
  const color = m.from === 'you' ? undefined : p ? characterFor(theme, p.role).badge : undefined;
  return (
    <div className="text-[11px] text-text">
      <span style={{ color }} className="font-medium">{name}</span>: {m.text}
    </div>
  );
}

export function Course({
  direction,
  stops,
  artifacts,
  theme,
  personas,
  mute,
  solo,
}: {
  direction: Direction | undefined;
  stops: Record<string, Stop[]>;
  artifacts: Record<string, Artifact>;
  theme: Theme;
  personas: Record<string, Persona>;
  mute: Set<string>;
  solo: Set<string>;
}) {
  const officeId = direction ? `office-${direction.riders[0]}` : null;
  const messagesBySpace = useHome((s) => s.messages);
  const messages = officeId ? messagesBySpace[officeId] ?? [] : [];

  const rider = direction ? personas[direction.riders[0]] : undefined;
  const riderWorking = rider?.presence === 'working';
  const riderName = rider ? characterFor(theme, rider.role).name.toLowerCase() : direction?.riders[0];

  useEffect(() => {
    if (officeId) gateway.open(officeId);
  }, [officeId]);

  const list = direction ? stops[direction.id] ?? [] : [];
  const ordered = useMemo(() => [...list].sort((a, b) => a.at.localeCompare(b.at)), [list]);
  const latest = ordered[ordered.length - 1];
  const latestArtifact = latest?.artifactId ? artifacts[latest.artifactId] : undefined;

  useEffect(() => {
    if (latest?.artifactId && !artifacts[latest.artifactId]) {
      gateway.send({ t: 'fetch', artifactId: latest.artifactId });
    }
  }, [latest?.artifactId, artifacts]);

  const activePersonaIds = solo.size > 0 ? solo : null;
  const visible = messages.filter((m) => {
    if (mute.has(m.from)) return false;
    if (activePersonaIds && m.from !== 'you' && !activePersonaIds.has(m.from)) return false;
    return true;
  });

  const parentRef = useRef<HTMLDivElement>(null);
  const virtualize = visible.length > 200;
  const virtualizer = useVirtualizer({
    count: visible.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 20,
    enabled: virtualize,
  });

  if (!direction) {
    return <div className="h-full flex items-center justify-center text-dim text-xs">select a direction</div>;
  }

  const min = ordered[0] ? new Date(ordered[0].at).getTime() : 0;
  const max = ordered[ordered.length - 1] ? new Date(ordered[ordered.length - 1].at).getTime() : min + 1;
  const span = Math.max(1, max - min);

  return (
    <div className="h-full overflow-y-auto p-4" aria-label="course">
      <div className="text-[10px] text-muted">course &middot; {direction.goal}</div>

      <div className="relative mt-4 h-16 rounded-lg border border-line bg-[var(--mv-ground)]">
        {ordered.map((s) => (
          <div
            key={s.id}
            className="absolute top-1/2 -translate-y-1/2 w-2 h-2 rounded-full border border-ink bg-[var(--mv-ground)]"
            style={{ left: `${8 + ((new Date(s.at).getTime() - min) / span) * 84}%` }}
            title={`${RUNG_LABEL[s.rung]} · ${s.from} · ${s.at}`}
          />
        ))}
        <div className="absolute right-2 top-1 text-[9px] text-accent">now</div>
        {ordered.length === 0 && (
          <div className="absolute inset-0 flex items-center justify-center text-[10px] text-dim">
            no evidence yet
          </div>
        )}
      </div>

      <div className="mt-4 rounded-lg border border-line bg-panel p-3">
        <div className="text-[10px] text-muted mb-1.5">evidence &middot; last stop</div>
        {latestArtifact?.body ? (
          <pre className="bg-[var(--mv-ground)] border border-line rounded p-2.5 text-[11px] leading-relaxed text-muted whitespace-pre-wrap overflow-x-auto">
            {latestArtifact.body}
          </pre>
        ) : (
          <span className="inline-block text-[10px] text-dim border border-dashed border-line rounded-full px-2.5 py-0.5">
            no evidence yet
          </span>
        )}
        {latest && (
          <div className="mt-2 text-[11px] text-success">
            {RUNG_LABEL[latest.rung]} &middot; {latest.from}
          </div>
        )}
      </div>

      {(visible.length > 0 || riderWorking) && (
        <div className="mt-4 rounded-lg border border-line bg-panel p-3" ref={parentRef} style={{ maxHeight: 220, overflowY: 'auto' }}>
          <div className="text-[10px] text-muted mb-1.5">stream</div>
          {virtualize ? (
            <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
              {virtualizer.getVirtualItems().map((v) => (
                <div key={v.key} style={{ position: 'absolute', top: v.start, left: 0, right: 0 }}>
                  <MessageLine m={visible[v.index]} theme={theme} personas={personas} />
                </div>
              ))}
            </div>
          ) : (
            <div className="flex flex-col gap-1">
              {visible.map((m) => (
                <MessageLine key={m.id} m={m} theme={theme} personas={personas} />
              ))}
            </div>
          )}
          {riderWorking && (
            <div className="presence-working text-[11px] italic text-dim">{riderName} is working&hellip;</div>
          )}
        </div>
      )}
    </div>
  );
}
