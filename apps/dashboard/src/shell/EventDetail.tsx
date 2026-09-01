import { NormalizedEvent } from '../types';
import { formatDate, formatTokens, formatUSD } from '../utils/formatters';

const DASH = '—';

/**
 * The seven payload types the backend actually emits today (verified against
 * live /api/traces/:id responses) plus the four the TS union declares but
 * that have never been observed on disk. Real data can still carry a `type`
 * string outside all eleven — the switch below falls through to a generic
 * "unrecognised" render rather than throwing.
 */
export type EventGroup = 'messages' | 'model' | 'tools';
export type RoleWeight = 'message' | 'action' | 'meta' | 'unknown';
export type RawPayload = { type: string; data?: unknown };

const KNOWN_TYPES = new Set([
  'user_message',
  'assistant_message',
  'model_invocation',
  'tool_call',
  'tool_result',
  'shell_command',
  'file_action',
  'outcome_evidence',
  'error',
  'human_intervention',
  'custom',
]);

const ROLE_LABELS: Record<string, string> = {
  user_message: 'USER',
  assistant_message: 'ASSISTANT',
  model_invocation: 'MODEL',
  tool_call: 'TOOL',
  tool_result: 'RESULT',
  shell_command: 'SHELL',
  file_action: 'FILE',
  outcome_evidence: 'EVIDENCE',
  error: 'ERROR',
  human_intervention: 'HUMAN',
  custom: 'CUSTOM',
};

/** Three strip rows: things said, what the model spent, actions taken. */
export function getEventGroup(type: string): EventGroup {
  if (type === 'model_invocation') return 'model';
  if (type === 'tool_call' || type === 'shell_command' || type === 'file_action' || type === 'tool_result') {
    return 'tools';
  }
  return 'messages';
}

export function getRoleLabel(type: string): string {
  if (ROLE_LABELS[type]) return ROLE_LABELS[type];
  const cleaned = type.replace(/_/g, ' ').trim().toUpperCase();
  return cleaned || 'EVENT';
}

/** Chips differ by weight/case/fill, never by hue — rule 1. */
export function getRoleWeight(type: string): RoleWeight {
  if (!KNOWN_TYPES.has(type)) return 'unknown';
  if (type === 'model_invocation') return 'meta';
  if (type === 'tool_call' || type === 'shell_command' || type === 'file_action' || type === 'tool_result') {
    return 'action';
  }
  return 'message';
}

function singleLine(s: string): string {
  return s.replace(/\s+/g, ' ').trim();
}

function truncate(s: string, max = 140): string {
  return s.length > max ? `${s.slice(0, max - 1)}…` : s;
}

function preview(s: string, max = 100): string {
  return truncate(singleLine(s), max);
}

function d<T = any>(data: unknown): T {
  return (data ?? {}) as T;
}

function summarizeToolArgs(args: unknown): string | null {
  if (args == null) return null;
  if (typeof args === 'string') return args.trim() ? preview(args) : null;
  if (typeof args === 'object') {
    const obj = args as Record<string, unknown>;
    if (typeof obj.command === 'string') return preview(obj.command);
    try {
      return preview(JSON.stringify(obj));
    } catch {
      return null;
    }
  }
  return null;
}

/** The dense single-line description shown in a TrajectoryView row. */
export function getPrimaryText(payload: RawPayload): string {
  const data = d<any>(payload.data);
  switch (payload.type) {
    case 'user_message':
      return data.content ? preview(data.content, 200) : DASH;
    case 'assistant_message':
      if (data.content) return preview(data.content, 200);
      if (data.thinking) return `(thinking) ${preview(data.thinking, 190)}`;
      return DASH;
    case 'model_invocation':
      return data.model ?? DASH;
    case 'tool_call': {
      const name = data.name ?? DASH;
      const argsPreview = summarizeToolArgs(data.arguments);
      return argsPreview ? `${name} ${argsPreview}` : name;
    }
    case 'tool_result':
      return data.name ?? (data.call_id ? `#${data.call_id}` : DASH);
    case 'shell_command':
      return data.command ? preview(data.command, 200) : DASH;
    case 'file_action': {
      const action = data.action ? String(data.action).toUpperCase() : DASH;
      return `${action} ${data.path ?? DASH}`;
    }
    case 'outcome_evidence':
      return data.summary ? preview(data.summary, 200) : data.kind ?? DASH;
    case 'error':
      return data.message ? preview(data.message, 200) : DASH;
    case 'human_intervention':
      return data.action ?? DASH;
    case 'custom':
      return data.kind ?? 'custom';
    default:
      return payload.type || 'unknown';
  }
}

/** The dimmed, arrow-prefixed preview — only returned when one genuinely exists. */
export function getResultPreview(payload: RawPayload): string | null {
  const data = d<any>(payload.data);
  switch (payload.type) {
    case 'shell_command':
      if (typeof data.output === 'string' && data.output.trim()) return preview(data.output);
      if (data.exit_code != null) return `exit ${data.exit_code}`;
      return null;
    case 'file_action':
      if (data.lines_changed != null) return `±${data.lines_changed} lines`;
      if (typeof data.diff === 'string' && data.diff.trim()) return preview(data.diff, 80);
      return null;
    case 'tool_result':
      if (data.is_error) return 'error';
      if (data.output != null) return preview(String(data.output));
      return null;
    case 'model_invocation': {
      const t = data.token_usage;
      if (!t) return null;
      const parts: string[] = [];
      if (t.input_tokens != null) parts.push(`in ${formatTokens(t.input_tokens)}`);
      if (t.output_tokens != null) parts.push(`out ${formatTokens(t.output_tokens)}`);
      const cacheRead = t.cache_read_tokens ?? t.cache_read_input_tokens;
      if (cacheRead) parts.push(`cache ${formatTokens(cacheRead)}`);
      return parts.length ? parts.join(' · ') : null;
    }
    case 'error':
      return data.is_recovered ? 'recovered' : 'unrecovered';
    default:
      return null;
  }
}

function fmtNum(v: unknown): string {
  return typeof v === 'number' ? formatTokens(v) : DASH;
}

function prettyJson(value: unknown): string {
  if (value === undefined || value === null) return DASH;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

interface KvRow {
  label: string;
  value: string;
}
interface TextBlock {
  label: string;
  value: string;
}

/** Type-specific structured fields (kv) and long free text (blocks). Every
 * field falls back to an em-dash rather than being silently omitted, except
 * blocks for genuinely absent optional text (thinking, diff, details) —
 * those are left out rather than rendered as an empty box. */
function buildDetailSections(payload: RawPayload): { kv: KvRow[]; blocks: TextBlock[]; note?: string } {
  const data = d<any>(payload.data);
  switch (payload.type) {
    case 'user_message':
      return { kv: [], blocks: [{ label: 'Content', value: data.content ?? DASH }] };
    case 'assistant_message':
      return {
        kv: [],
        blocks: [
          { label: 'Content', value: data.content || DASH },
          ...(data.thinking ? [{ label: 'Thinking', value: data.thinking as string }] : []),
        ],
      };
    case 'model_invocation': {
      const t = data.token_usage ?? {};
      const cacheRead = t.cache_read_tokens ?? t.cache_read_input_tokens;
      const cacheCreation = t.cache_creation_tokens ?? t.cache_creation_input_tokens;
      return {
        kv: [
          { label: 'Model', value: data.model ?? DASH },
          { label: 'Input tokens', value: fmtNum(t.input_tokens) },
          { label: 'Output tokens', value: fmtNum(t.output_tokens) },
          { label: 'Cache read', value: fmtNum(cacheRead) },
          { label: 'Cache creation', value: fmtNum(cacheCreation) },
          { label: 'Cost', value: typeof data.cost_usd === 'number' ? formatUSD(data.cost_usd) : DASH },
          { label: 'Latency', value: typeof data.latency_ms === 'number' ? `${data.latency_ms} ms` : DASH },
        ],
        blocks: [],
      };
    }
    case 'tool_call':
      return {
        kv: [
          { label: 'Name', value: data.name ?? DASH },
          { label: 'Call ID', value: data.id ?? DASH },
        ],
        blocks: [{ label: 'Arguments', value: prettyJson(data.arguments) }],
      };
    case 'tool_result':
      return {
        kv: [
          { label: 'Name', value: data.name ?? DASH },
          { label: 'Call ID', value: data.call_id ?? DASH },
          { label: 'Error', value: data.is_error ? 'yes' : 'no' },
        ],
        blocks: [{ label: 'Output', value: prettyJson(data.output) }],
      };
    case 'shell_command':
      return {
        kv: [
          { label: 'Cwd', value: data.cwd ?? DASH },
          { label: 'Exit code', value: data.exit_code != null ? String(data.exit_code) : DASH },
        ],
        blocks: [
          { label: 'Command', value: data.command ?? DASH },
          ...(data.output ? [{ label: 'Output', value: data.output as string }] : []),
        ],
      };
    case 'file_action':
      return {
        kv: [
          { label: 'Path', value: data.path ?? DASH },
          { label: 'Action', value: data.action ?? DASH },
          { label: 'Lines changed', value: data.lines_changed != null ? String(data.lines_changed) : DASH },
        ],
        blocks: data.diff ? [{ label: 'Diff', value: data.diff as string }] : [],
      };
    case 'outcome_evidence':
      return {
        kv: [
          { label: 'Kind', value: data.kind ?? DASH },
          {
            label: 'Confidence',
            value: typeof data.confidence === 'number' ? `${Math.round(data.confidence * 100)}%` : DASH,
          },
        ],
        blocks: [{ label: 'Summary', value: data.summary ?? DASH }],
      };
    case 'error':
      return {
        kv: [{ label: 'Recovered', value: data.is_recovered ? 'yes' : 'no' }],
        blocks: [{ label: 'Message', value: data.message ?? DASH }],
      };
    case 'human_intervention':
      return {
        kv: [{ label: 'Action', value: data.action ?? DASH }],
        blocks: data.details ? [{ label: 'Details', value: data.details as string }] : [],
      };
    case 'custom':
      return {
        kv: [{ label: 'Kind', value: data.kind ?? DASH }],
        blocks: [{ label: 'Data', value: prettyJson(data.data) }],
      };
    default:
      return {
        kv: [],
        blocks: [],
        note: `Unrecognised event type "${payload.type || DASH}" — showing the raw payload below.`,
      };
  }
}

export interface EventDetailProps {
  event: NormalizedEvent | null;
}

/** The selected event, rendered as whatever it actually has: a structured
 * summary keyed by payload type, plus the raw payload for forensic access.
 * Sections that would be empty (no thinking, no diff, no result) are left
 * out rather than shown blank. */
export function EventDetail({ event }: EventDetailProps) {
  if (!event) {
    return (
      <div className="traj-detail">
        <div className="traj-detail-empty">Select an event to inspect it.</div>
      </div>
    );
  }

  const payload: RawPayload = event.payload ?? { type: 'unknown' };
  const { kv, blocks, note } = buildDetailSections(payload);
  const rawPayloadJson = prettyJson(payload.data);

  return (
    <div className="traj-detail">
      <div className="traj-detail-header">
        <span className="traj-detail-type">{payload.type || 'unknown'}</span>
        <span className="traj-detail-seq">#{event.sequence}</span>
        <span className="traj-detail-time">{event.timestamp ? formatDate(event.timestamp) : DASH}</span>
      </div>

      {note && (
        <div className="traj-detail-section">
          <p className="traj-note">{note}</p>
        </div>
      )}

      {kv.length > 0 && (
        <div className="traj-detail-section">
          <div className="shell-kv-table">
            {kv.map((row) => (
              <div className="shell-kv-row" key={row.label}>
                <span className="shell-kv-key">{row.label}</span>
                <span className="shell-kv-val">{row.value}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {blocks.map((block) => (
        <div className="traj-detail-section traj-block" key={block.label}>
          <div className="traj-block-label">{block.label}</div>
          <pre className="traj-pre">{block.value}</pre>
        </div>
      ))}

      <div className="traj-detail-section traj-block">
        <div className="traj-block-label">Payload</div>
        <pre className="traj-pre">{rawPayloadJson}</pre>
      </div>
    </div>
  );
}
