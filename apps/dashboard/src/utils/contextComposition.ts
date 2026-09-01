import type { NormalizedEvent } from '../types';

/**
 * What filled the transcript, by share.
 *
 * Measured in characters of recorded content, not tokens, and the distinction
 * is deliberate. `usage` reports token totals per request but never divides
 * them between the system prompt, the tool schemas and the conversation — that
 * breakdown is not written to the log by anyone, so it cannot be recovered.
 * Characters of transcript are what the file actually contains, so that is what
 * this counts. See docs/specs/context-composition.md.
 */
export type Bucket = 'dialogue' | 'tools' | 'injected' | 'bookkeeping';

export interface BucketShare {
  key: Bucket;
  label: string;
  /** Characters of recorded content. */
  chars: number;
  /** Share of the transcript, 0..1. */
  share: number;
  events: number;
}

export interface Contributor {
  label: string;
  chars: number;
  share: number;
  /** How many times this kind of thing was recorded. */
  occurrences: number;
}

/**
 * A compaction boundary, with the harness's own exact token accounting.
 *
 * These are not estimates: `preTokens` is what the context actually measured
 * when it overflowed. As of the `EventPayload::Compaction` schema variant,
 * `droppedCumulative` actually holds that round's own derived delta
 * (`pre_tokens - post_tokens`), not the harness's raw cumulative counter --
 * see `CompactionEvent::dropped_tokens` in the Rust schema for why summing
 * per-round deltas is the only value that's correct across multiple rounds.
 */
export interface Compaction {
  preTokens: number;
  postTokens: number;
  droppedCumulative: number;
  /** Tools discovered at that point. Not carried by the new event; 0 there. */
  toolCount: number;
  /** The event this round was recorded on — lets a marker layer place it. */
  eventId: string;
  /** For ordering rounds chronologically regardless of input event order. */
  sequence: number;
}

export interface ContextComposition {
  totalChars: number;
  buckets: BucketShare[];
  /** Injected-context kinds, largest first — the actionable detail. */
  contributors: Contributor[];
  /** Share that is not the conversation itself. */
  overheadShare: number;
  /** Exact context measurements, when the session compacted. */
  compactions: Compaction[];
  /** Largest context this session is known to have reached, exactly. */
  peakTokens: number | null;
}

const LABELS: Record<Bucket, string> = {
  dialogue: 'Dialogue',
  tools: 'Tool traffic',
  injected: 'Injected context',
  bookkeeping: 'Harness bookkeeping',
};

/** Order is the stacking order, and it runs conversation-first on purpose. */
const ORDER: Bucket[] = ['dialogue', 'tools', 'injected', 'bookkeeping'];

function sizeOf(value: unknown): number {
  if (value === null || value === undefined) return 0;
  try {
    return JSON.stringify(value).length;
  } catch {
    // Cyclic or otherwise unserialisable payloads are rare but must not throw
    // in the middle of rendering a session.
    return 0;
  }
}

/** Turns `deferred_tools_delta` into `deferred tools delta`. */
function humanise(kind: string): string {
  return kind.replace(/[_-]+/g, ' ').trim();
}

/**
 * Splits a session's transcript into what was conversation and what was
 * everything else.
 *
 * `model_invocation` events are skipped: they carry token accounting rather
 * than content, so counting them would double-count the very thing being
 * measured.
 */
export function analyzeComposition(events: NormalizedEvent[]): ContextComposition {
  const chars: Record<Bucket, number> = { dialogue: 0, tools: 0, injected: 0, bookkeeping: 0 };
  const counts: Record<Bucket, number> = { dialogue: 0, tools: 0, injected: 0, bookkeeping: 0 };
  const byKind = new Map<string, { chars: number; occurrences: number }>();
  const compactions: Compaction[] = [];

  for (const event of events) {
    const payload = event.payload as
      | { type?: string; data?: Record<string, unknown>; kind?: string }
      | undefined;
    const type = payload?.type;
    if (!type || type === 'model_invocation') continue;

    const data = payload?.data ?? {};
    const size = sizeOf(data);
    let bucket: Bucket;

    if (type === 'user_message' || type === 'assistant_message') {
      bucket = 'dialogue';
    } else if (type === 'tool_call' || type === 'shell_command' || type === 'file_action') {
      bucket = 'tools';
    } else if (type === 'compaction') {
      // A first-class event as of the compaction-tracking schema change. Older parses
      // (before that change) routed this through the 'custom' branch below instead —
      // that fallback stays in place for anything still shaped that way.
      bucket = 'bookkeeping';
      const meta = data as unknown as {
        pre_tokens?: number;
        post_tokens?: number;
        dropped_tokens?: number;
      };
      if (typeof meta.pre_tokens === 'number') {
        compactions.push({
          preTokens: meta.pre_tokens,
          postTokens: meta.post_tokens ?? 0,
          droppedCumulative: meta.dropped_tokens ?? 0,
          toolCount: 0,
        });
      }
    } else if (type === 'custom') {
      const kind = (data.kind as string | undefined) ?? payload?.kind;
      if (kind === 'attachment') {
        bucket = 'injected';
        const attachment = ((data.data as Record<string, unknown> | undefined)?.attachment ??
          {}) as Record<string, unknown>;
        const label = humanise((attachment.type as string | undefined) ?? 'unlabelled');
        const prev = byKind.get(label) ?? { chars: 0, occurrences: 0 };
        byKind.set(label, { chars: prev.chars + size, occurrences: prev.occurrences + 1 });
      } else {
        bucket = 'bookkeeping';
        const inner = (data.data ?? {}) as Record<string, unknown>;
        const meta = inner.compactMetadata as Record<string, unknown> | undefined;
        if (meta && typeof meta.preTokens === 'number') {
          compactions.push({
            preTokens: meta.preTokens as number,
            postTokens: Number(meta.postTokens ?? 0) || 0,
            droppedCumulative: Number(meta.cumulativeDroppedTokens ?? 0) || 0,
            toolCount: Array.isArray(meta.preCompactDiscoveredTools)
              ? (meta.preCompactDiscoveredTools as unknown[]).length
              : 0,
            eventId: event.id,
            sequence: event.sequence,
          });
        }
      }
    } else {
      bucket = 'bookkeeping';
    }

    chars[bucket] += size;
    counts[bucket] += 1;
  }

  const totalChars = ORDER.reduce((sum, key) => sum + chars[key], 0);
  const buckets: BucketShare[] = ORDER.map((key) => ({
    key,
    label: LABELS[key],
    chars: chars[key],
    share: totalChars > 0 ? chars[key] / totalChars : 0,
    events: counts[key],
  }));

  const contributors: Contributor[] = [...byKind.entries()]
    .map(([label, v]) => ({
      label,
      chars: v.chars,
      occurrences: v.occurrences,
      share: totalChars > 0 ? v.chars / totalChars : 0,
    }))
    .sort((a, b) => b.chars - a.chars);

  const overheadShare = totalChars > 0 ? 1 - chars.dialogue / totalChars : 0;

  // Input order is trusted elsewhere in this file, but a compaction round
  // list is read as a timeline (round 1, round 2, ...), so it gets an
  // explicit sort rather than inheriting whatever order events arrived in.
  compactions.sort((a, b) => a.sequence - b.sequence);

  const peakTokens = compactions.length
    ? compactions.reduce((max, c) => Math.max(max, c.preTokens), 0)
    : null;

  return { totalChars, buckets, contributors, overheadShare, compactions, peakTokens };
}

/** Compact character counts — "1.2M", "38k". */
export function formatChars(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${Math.round(n / 1_000)}k`;
  return `${n}`;
}
