import type { Artifact, Direction, Message, Presence } from '../protocol';

/** How many tool cards render before the count summary caps the run. Stated N for the cap. */
export const MAX_TOOLS_SHOWN = 20;

export type ToolStatus = 'ok' | 'error' | 'running';

export interface ToolEntry {
  id: string;
  title: string;
  ref: string;
  kind: Artifact['kind'];
  status: ToolStatus;
  /** First error-looking line for failures; body excerpt otherwise. Null when nothing loaded yet. */
  detail: string | null;
  /** Full message for the error card. Set only when status is `error`. */
  errorMessage: string | null;
}

// Heuristic, read-only, over data already on the wire: a `command` artifact whose body
// reads like a failure. Documented as a heuristic because the gateway ships no exit code
// on artifacts -- see `apps/cli/src/server/home/protocol.rs`'s `Artifact`.
const FAILURE_RE = /fail|error|timeout|timed out|exit\s+[1-9]/i;

export function isFailedArtifact(a: Artifact): boolean {
  if (a.kind !== 'command' || !a.body) return false;
  return FAILURE_RE.test(a.body);
}

function firstLine(s: string): string {
  return s.split('\n', 1)[0]?.slice(0, 160) ?? '';
}

function firstErrorLine(body: string): string {
  const lines = body.split('\n');
  const hit = lines.find((l) => FAILURE_RE.test(l));
  return (hit ?? lines[0] ?? '').slice(0, 160);
}

/**
 * Read-only derivation: each `work` message's linked artifacts, in stream order,
 * become one card each. Missing artifacts are skipped (backfill may not have
 * loaded them yet); nothing is fetched or written here.
 */
export function deriveToolEntries(
  messages: Message[],
  artifacts: Record<string, Artifact>,
): ToolEntry[] {
  const entries: ToolEntry[] = [];
  for (const m of messages) {
    if (m.kind !== 'work' || !m.artifacts) continue;
    for (const id of m.artifacts) {
      const a = artifacts[id];
      if (!a) continue;
      if (isFailedArtifact(a)) {
        entries.push({
          id: a.id,
          title: a.title,
          ref: a.ref,
          kind: a.kind,
          status: 'error',
          detail: firstErrorLine(a.body ?? ''),
          errorMessage: firstErrorLine(a.body ?? ''),
        });
      } else if (a.kind === 'command' && a.body == null) {
        entries.push({
          id: a.id,
          title: a.title,
          ref: a.ref,
          kind: a.kind,
          status: 'running',
          detail: null,
          errorMessage: null,
        });
      } else {
        entries.push({
          id: a.id,
          title: a.title,
          ref: a.ref,
          kind: a.kind,
          status: 'ok',
          detail: a.body ? firstLine(a.body) : null,
          errorMessage: null,
        });
      }
    }
  }
  return entries;
}

export interface ToolSummary {
  shown: number;
  total: number;
  hidden: number;
  label: string;
}

export function summarizeTools(entries: ToolEntry[], limit: number = MAX_TOOLS_SHOWN): ToolSummary {
  const total = entries.length;
  const shown = Math.min(total, limit);
  const hidden = total - shown;
  return {
    shown,
    total,
    hidden,
    label:
      hidden > 0
        ? `Showing ${shown} of ${total} tool calls — ${hidden} more`
        : `${total} tool call${total === 1 ? '' : 's'}`,
  };
}

export interface RetryAvailability {
  enabled: boolean;
  reason: string;
}

/**
 * Whether the error card's retry button can fire. Retry reuses the existing
 * steer path (`steer` mode `now` via the dock gateway) -- it is only available
 * when a direction is selected, unfinished, and its rider can take a prompt.
 * Otherwise the button renders disabled with the reason visible: never a dead button.
 */
export function retryAvailability(
  direction: Direction | null | undefined,
  riderPresence: Presence | string | undefined,
): RetryAvailability {
  if (!direction) return { enabled: false, reason: 'select a direction to retry' };
  if (direction.state === 'done') return { enabled: false, reason: 'direction is done — nothing to retry' };
  if (riderPresence === 'blocked')
    return { enabled: false, reason: 'rider is blocked — answer the prompt first' };
  return { enabled: true, reason: '' };
}
