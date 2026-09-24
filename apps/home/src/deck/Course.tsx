import { useEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import type { Artifact, Direction, Message, Persona, Stop } from '../protocol';
import type { Theme } from '../model/theme';
import { characterFor } from '../model/theme';
import { useHome } from '../model/store';
import { VirtualizedPreview } from './VirtualizedPreview';
import { gateway } from '../ws/client';
import { buildSessionReview, selectReviewPath } from '../model/sessionReview';
import { SessionReview } from './SessionReview';
import { ToolCards } from './ToolCards';
import { PanelState } from './PanelState';
import { TrackScrubber } from './TrackScrubber';
import {
  buildTrackEvents,
  latestStopBefore,
  rewoundEvents,
  type TrackEvent,
} from '../model/trackTimeline';

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

/** One rewound timeline line under the playhead. Speech stays the rider's voice;
 *  handoff/error render as the shaped marks DESIGN.md names, deck-native. */
function TrackLine({ e }: { e: TrackEvent }) {
  if (e.kind === 'handoff') {
    return (
      <div className="text-[11px] text-accent" title="handoff">
        <span aria-hidden="true">→</span> {e.text}
      </div>
    );
  }
  if (e.kind === 'error') {
    return (
      <div className="text-[11px] text-warn" title="fork on retry">
        <span aria-hidden="true">⑂</span> {e.text}
      </div>
    );
  }
  if (e.kind === 'work') {
    return <div className="text-[11px] text-dim">{e.text}</div>;
  }
  return <div className="text-[11px] text-text">{e.text}</div>;
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
  const timelines = useHome((s) => s.timelines);
  const connection = useHome((s) => s.connection);
  const messages = officeId ? messagesBySpace[officeId] ?? [] : [];

  const rider = direction ? personas[direction.riders[0]] : undefined;
  const riderWorking = rider?.presence === 'working';
  const riderName = rider ? characterFor(theme, rider.role).name.toLowerCase() : direction?.riders[0];

  useEffect(() => {
    if (officeId) gateway.open(officeId);
  }, [officeId]);

  // The scrubber's timeline: one request per office, answered by `timeline` frames into
  // the store. A room-less or fresh rider simply stays timeline-less; the strip degrades.
  useEffect(() => {
    if (officeId) gateway.send({ t: 'timeline', spaceId: officeId });
  }, [officeId]);

  const list = direction ? stops[direction.id] ?? [] : [];
  const ordered = useMemo(() => [...list].sort((a, b) => a.at.localeCompare(b.at)), [list]);
  const latest = ordered[ordered.length - 1];

  useEffect(() => {
    if (latest?.artifactId && !artifacts[latest.artifactId]) {
      gateway.send({ t: 'fetch', artifactId: latest.artifactId });
    }
  }, [latest?.artifactId, artifacts]);

  const directionStops = direction ? (stops[direction.id] ?? []) : [];
  const review = useMemo(
    () => buildSessionReview(direction, directionStops, artifacts),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [direction, directionStops, artifacts],
  );
  const [reviewPath, setReviewPath] = useState<string | null>(null);
  const reviewSelected = selectReviewPath(review, reviewPath);
  const reviewArtifactId = Object.keys(artifacts).find((id) => {
    const a = artifacts[id];
    return (a.kind === 'diff' || a.kind === 'file') && (a.ref || a.title) === reviewSelected;
  });

  useEffect(() => {
    if (reviewArtifactId && !artifacts[reviewArtifactId]?.body) {
      gateway.send({ t: 'fetch', artifactId: reviewArtifactId });
    }
  }, [reviewArtifactId, artifacts]);

  const activePersonaIds = solo.size > 0 ? solo : null;
  const visible = messages.filter((m) => {
    if (mute.has(m.from)) return false;
    if (activePersonaIds && m.from !== 'you' && !activePersonaIds.has(m.from)) return false;
    return true;
  });

  // The scrubber: the distilled session timeline plus this client's live tail, and a
  // playhead. Live (scrub = null) the track follows the end; scrubbed, everything below
  // reads the state at the playhead. Reaching the last moment is live again.
  const timeline = officeId ? timelines[officeId] : undefined;
  const trackEvents = useMemo(
    () => buildTrackEvents(timeline?.moments ?? [], messages),
    [timeline?.moments, messages],
  );
  const [scrub, setScrubRaw] = useState<number | null>(null);
  useEffect(() => {
    setScrubRaw(null);
  }, [officeId]);
  const scrubbable = trackEvents.length > 1;
  const setScrub = (index: number) => {
    if (!scrubbable) return;
    setScrubRaw(index >= trackEvents.length - 1 ? null : index);
  };
  const scrubbed = scrub !== null;
  const playheadAtMs = scrubbed && scrub !== null ? trackEvents[scrub].atMs : null;
  const evidenceStop = playheadAtMs !== null ? latestStopBefore(ordered, playheadAtMs) : latest;
  const rewindShown = scrubbed ? rewoundEvents(trackEvents, scrub ?? 0) : [];
  const latestArtifact = evidenceStop?.artifactId ? artifacts[evidenceStop.artifactId] : undefined;

  // A rewound evidence stop can be an older artifact this client never fetched.
  useEffect(() => {
    const id = evidenceStop?.artifactId;
    if (id && !artifacts[id]) gateway.send({ t: 'fetch', artifactId: id });
  }, [evidenceStop, artifacts]);

  const parentRef = useRef<HTMLDivElement>(null);
  const virtualize = visible.length > 200;
  const virtualizer = useVirtualizer({
    count: visible.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 20,
    enabled: virtualize,
  });

  if (!direction) {
    return (
      <div className="h-full flex items-center justify-center p-4" aria-label="course">
        <PanelState kind="empty" title="select a direction" hint="pick a strip to see its course" />
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto p-4" aria-label="course">
      <div className="text-[10px] text-muted">course &middot; {direction.goal}</div>

      <TrackScrubber
        events={trackEvents}
        stops={ordered}
        scrub={scrub}
        truncated={timeline?.truncated ?? false}
        onScrub={setScrub}
        onLive={() => setScrubRaw(null)}
      />

      <div className="mt-4 rounded-lg border border-line bg-panel p-3">
        <div className="text-[10px] text-muted mb-1.5">review &middot; files changed</div>
        <SessionReview review={review} selectedPath={reviewPath} onSelect={setReviewPath} />
      </div>

      <div className="mt-4 rounded-lg border border-line bg-panel p-3">
        <div className="text-[10px] text-muted mb-1.5">
          evidence &middot; {scrubbed ? 'the rung so far' : 'last stop'}
          {playheadAtMs !== null && (
            <span className="ml-1" aria-hidden="true">
              · rewound to {new Date(playheadAtMs).toLocaleTimeString()}
            </span>
          )}
        </div>
        {latestArtifact?.body ? (
          <VirtualizedPreview body={latestArtifact.body} />
        ) : evidenceStop?.artifactId ? (
          <PanelState kind="loading" title="fetching the evidence" />
        ) : (
          <PanelState kind="empty" title="no evidence yet" hint="stops land here when a rider earns a rung" />
        )}
        {evidenceStop && (
          <div className="mt-2 text-[11px] text-success">
            {RUNG_LABEL[evidenceStop.rung]} &middot; {evidenceStop.from}
          </div>
        )}
      </div>

      <div className="mt-4 rounded-lg border border-line bg-panel p-3">
        <div className="text-[10px] text-muted mb-1.5">tools</div>
        <ToolCards
          messages={visible.filter((m) => m.kind === 'work')}
          artifacts={artifacts}
          direction={direction}
          riderPresence={rider?.presence}
          onRetry={(entry) => {
            if (!direction) return;
            gateway.send({
              t: 'steer',
              directionId: direction.id,
              text: `retry: ${entry.ref}`,
              mode: 'now',
              mentions: [],
            });
          }}
        />
      </div>

      {scrubbed ? (
        <div className="mt-4 rounded-lg border border-line bg-panel p-3" style={{ maxHeight: 320, overflowY: 'auto' }}>
          <div className="text-[10px] text-muted mb-1.5">stream &middot; rewound</div>
          {rewindShown.length === 0 ? (
            <PanelState kind="empty" title="nothing yet at this moment" hint="drag the playhead forward to see more" />
          ) : (
            <div className="flex flex-col gap-1">
              {rewindShown.map((e) => (
                <TrackLine key={e.key} e={e} />
              ))}
            </div>
          )}
        </div>
      ) : (visible.length > 0 || riderWorking) ? (
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
      ) : connection === 'connecting' ? (
        <div className="mt-4 rounded-lg border border-line bg-panel p-3">
          <div className="text-[10px] text-muted mb-1.5">stream</div>
          <PanelState kind="loading" title="loading the stream" />
        </div>
      ) : (
        <div className="mt-4 rounded-lg border border-line bg-panel p-3">
          <div className="text-[10px] text-muted mb-1.5">stream</div>
          <PanelState kind="empty" title="quiet so far" hint="the stream stays quiet until a rider speaks" />
        </div>
      )}
    </div>
  );
}
