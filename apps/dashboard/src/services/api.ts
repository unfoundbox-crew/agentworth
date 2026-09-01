import {
  AggregateStats,
  AgentWorthTrace,
  SessionSummary,
  UsageRollupResponse,
  PacingResponse,
  BlameResponse,
  CoverageMatrixResponse,
  AdapterCapability,
  ScanSummary,
} from '../types';

const BASE_URL = '/api';

export const EMPTY_AGGREGATE_STATS: AggregateStats = {
  total_sessions: 0,
  total_events: 0,
  token_usage: {
    input_tokens: 0,
    output_tokens: 0,
    cache_read_tokens: 0,
    cache_creation_tokens: 0,
  },
  sessions_by_adapter: {},
  models_usage_count: {},
  tools_usage_count: {},
  verified_outcomes_count: 0,
  outcome_distribution: {
    ci_or_deployment_verified: 0,
    commit_observed: 0,
    test_or_build_passed: 0,
    artifact_changed: 0,
    done_claimed: 0,
    unresolved: 0,
  },
  archaeology: undefined,
};

export const GROUNDED_CAPABILITY_MATRIX: AdapterCapability[] = [
  {
    id: 'claude_code',
    name: 'Claude Code',
    sessions: 'yes',
    tokens: 'yes',
    cache_split: 'yes',
    models: 'yes',
    file_edits: 'yes',
    shell_exit: 'yes',
    outcomes: 'rung 2',
    notes: 'Input, output, cache-read (0.1x), cache-write (1.25x) fully split',
  },
  {
    id: 'codex',
    name: 'OpenAI Codex',
    sessions: 'yes',
    tokens: 'partial',
    cache_split: 'no',
    models: 'partial',
    file_edits: 'yes',
    shell_exit: 'partial',
    outcomes: 'no',
    notes: 'Session JSON parsed; token counts partial',
  },
  {
    id: 'cursor',
    name: 'Cursor Composer',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'partial',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'workspaceStorage sqlite parsed; tokens not extracted',
  },
  {
    id: 'antigravity',
    name: 'Google Antigravity',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'partial',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'Brain trajectory JSONL detected; token breakdown pending',
  },
  {
    id: 'gemini',
    name: 'Gemini CLI',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'partial',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'Dotfile detection active; full event stream partial',
  },
  {
    id: 'hermes',
    name: 'Nous Hermes',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'no',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'Sessions detected; event normalization in progress',
  },
  {
    id: 'goose',
    name: 'Block Goose',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'no',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'Session files enumerated; token parsing pending',
  },
  {
    id: 'pi',
    name: 'Pi Task Agent',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'no',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'Tasks directory detected',
  },
  {
    id: 'grok',
    name: 'xAI Grok',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'no',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'Dotfiles enumerated; zero-token stubs detected',
  },
  {
    id: 'openclaw',
    name: 'OpenClaw',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'no',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'Log stream format parsing pending',
  },
  {
    id: 'herdr',
    name: 'Herdr Orchestrator',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'no',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'Orchestrator configs indexed',
  },
  {
    id: 'opencode',
    name: 'OpenCode',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'no',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'History detection active',
  },
  {
    id: 'deepseek',
    name: 'DeepSeek Code',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'no',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'Dotfile directory detected',
  },
  {
    id: 'kimi',
    name: 'Kimi Code',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'no',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'Dotfile directory detected',
  },
  {
    id: 'minimax',
    name: 'MiniMax',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'no',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'Dotfile directory detected',
  },
  {
    id: 'qwen',
    name: 'Qwen Code',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'no',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'Dotfile directory detected',
  },
  {
    id: 'zhipu',
    name: 'Zhipu CodeGeeX',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'no',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'Dotfile directory detected',
  },
  {
    id: 'aider',
    name: 'Aider',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'no',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'Chat history markdown detected',
  },
  {
    id: 'cline',
    name: 'Cline / Roo-Code',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'no',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'VS Code globalStorage tasks detected',
  },
  {
    id: 'windsurf',
    name: 'Windsurf / Cascade',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'no',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'Cascade session logs detected',
  },
  {
    id: 'manus',
    name: 'Manus',
    sessions: 'yes',
    tokens: 'no',
    cache_split: 'no',
    models: 'no',
    file_edits: 'no',
    shell_exit: 'no',
    outcomes: 'no',
    notes: 'Dotfile directory detected',
  },
];

export interface TraceQueryFilters {
  adapter?: string;
  model?: string;
  search?: string;
  outcome?: string;
  orderBy?: string;
  limit?: number;
  offset?: number;
}

/**
 * Fetches machine-wide aggregate stats from /api/stats.
 * ZERO MOCK FALLBACK: Missing data returns empty/unmeasured state.
 */
export async function fetchAggregateStats(): Promise<AggregateStats> {
  try {
    const res = await fetch(`${BASE_URL}/stats`);
    if (res.ok) {
      const data = await res.json();

      const input = data.token_usage?.input_tokens ?? 0;
      const output = data.token_usage?.output_tokens ?? 0;
      const cacheRead =
        data.token_usage?.cache_read_tokens ??
        data.token_usage?.cache_read_input_tokens ??
        0;
      const cacheCreation =
        data.token_usage?.cache_creation_tokens ??
        data.token_usage?.cache_creation_input_tokens ??
        0;

      return {
        total_sessions: data.total_sessions ?? 0,
        total_events: data.total_events ?? 0,
        token_usage: {
          input_tokens: input,
          output_tokens: output,
          cache_read_tokens: cacheRead,
          cache_creation_tokens: cacheCreation,
        },
        sessions_by_adapter: data.sessions_by_adapter ?? {},
        models_usage_count: data.models_usage_count ?? {},
        tools_usage_count: data.tools_usage_count ?? {},
        verified_outcomes_count: data.verified_outcomes_count ?? 0,
        outcome_distribution: data.outcome_distribution,
        first_session_at: data.date_range?.first_session_at ?? data.first_session_at,
        last_session_at: data.date_range?.last_session_at ?? data.last_session_at,
        archaeology: data.archaeology,
      };
    }
  } catch (_err) {
    // Backend API not running or returned error; report genuine empty/unmeasured state
  }
  return EMPTY_AGGREGATE_STATS;
}

/**
 * Fetches filtered session traces from /api/traces.
 */
export async function fetchTraces(
  filters: TraceQueryFilters = {}
): Promise<{ traces: SessionSummary[]; total: number }> {
  try {
    const params = new URLSearchParams();
    if (filters.adapter && filters.adapter !== 'all') params.set('adapter', filters.adapter);
    if (filters.model) params.set('model', filters.model);
    if (filters.search) params.set('search', filters.search);
    if (filters.limit) params.set('limit', filters.limit.toString());
    if (filters.offset) params.set('offset', filters.offset.toString());
    if (filters.orderBy) params.set('order_by', filters.orderBy);

    const res = await fetch(`${BASE_URL}/traces?${params.toString()}`);
    if (res.ok) {
      const data = await res.json();
      if (Array.isArray(data)) {
        return { traces: data, total: data.length };
      }
      if (data.traces) {
        return { traces: data.traces, total: data.total ?? data.traces.length };
      }
    }
  } catch (_err) {
    // Fetch failed
  }
  return { traces: [], total: 0 };
}

/**
 * Fetches trace detail from /api/traces/:id.
 */
export async function fetchTraceDetail(
  sessionId: string
): Promise<AgentWorthTrace | null> {
  try {
    const res = await fetch(`${BASE_URL}/traces/${encodeURIComponent(sessionId)}`);
    if (res.ok) {
      const data = await res.json();
      if (data.trace) {
        return {
          ...data.trace,
          score: data.score || data.trace.score,
          outcomes: data.outcomes || data.trace.outcomes,
          recoveries: data.recoveries || data.trace.recoveries,
        };
      }
      return data;
    }
  } catch (_err) {
    // Trace not found
  }
  return null;
}

/**
 * GET /api/usage -> Rollup of daily/weekly token burn & cost
 */
export async function fetchUsageRollups(
  period: 'day' | 'week' | 'month' = 'day'
): Promise<UsageRollupResponse> {
  try {
    const res = await fetch(`${BASE_URL}/usage?period=${period}`);
    if (res.ok) {
      return await res.json();
    }
  } catch (_err) {
    // Usage endpoint offline
  }
  return {
    period,
    entries: [],
    total_cost_usd: 0,
    total_tokens: 0,
  };
}

/**
 * GET /api/pacing -> 5-hour pacing window & burn velocity
 */
export async function fetchPacing(): Promise<PacingResponse> {
  try {
    const res = await fetch(`${BASE_URL}/pacing`);
    if (res.ok) {
      return await res.json();
    }
  } catch (_err) {
    // Pacing endpoint offline
  }
  return {
    window_hours: 5,
    tokens_in_window: 0,
    burn_rate_tokens_per_hour: 0,
    cache_hit_percent: 0,
    estimated_cost_in_window_usd: 0,
    active_sessions_count: 0,
    recent_switches_cost_usd: 0,
  };
}

/**
 * GET /api/blame -> line-by-line file edit lineage to sessions and prompts
 */
export async function fetchBlame(filePath: string): Promise<BlameResponse> {
  try {
    const res = await fetch(`${BASE_URL}/blame?path=${encodeURIComponent(filePath)}`);
    if (res.ok) {
      return await res.json();
    }
  } catch (_err) {
    // Blame endpoint offline
  }
  return {
    file_path: filePath,
    total_edits: 0,
    edits: [],
  };
}

/**
 * GET /api/matrix -> grounded capability coverage matrix across all 20+ adapters
 */
export async function fetchCoverageMatrix(): Promise<CoverageMatrixResponse> {
  try {
    const res = await fetch(`${BASE_URL}/matrix`);
    if (res.ok) {
      return await res.json();
    }
  } catch (_err) {
    // Fallback to grounded canonical matrix
  }
  return {
    generated_at: new Date().toISOString(),
    adapters: GROUNDED_CAPABILITY_MATRIX,
  };
}

export function performClientSideRedaction(
  trace: AgentWorthTrace
): { redactedTrace: AgentWorthTrace; redactedCount: number; categories: Record<string, number> } {
  let count = 0;
  const categories: Record<string, number> = {
    api_keys: 0,
    file_paths: 0,
    emails: 0,
    credentials: 0,
  };

  const redactString = (str: string): string => {
    let result = str;

    // Anthropic / OpenAI / GitHub keys
    const apiKeyPattern = /\b(sk-ant-[a-zA-Z0-9_-]{20,}|sk-[a-zA-Z0-9]{32,}|ghp_[a-zA-Z0-9]{36}|github_pat_[a-zA-Z0-9_]{30,})\b/g;
    result = result.replace(apiKeyPattern, (match) => {
      count++;
      categories.api_keys++;
      return `[REDACTED_API_KEY_${match.slice(0, 6)}***]`;
    });

    // Absolute home paths
    const homePathPattern = /\/Users\/[a-zA-Z0-9._-]+\//g;
    result = result.replace(homePathPattern, () => {
      count++;
      categories.file_paths++;
      return '~/';
    });

    // Emails
    const emailPattern = /\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,7}\b/g;
    result = result.replace(emailPattern, () => {
      count++;
      categories.emails++;
      return '[REDACTED_EMAIL]';
    });

    return result;
  };

  const jsonCopy = JSON.parse(JSON.stringify(trace));

  // Redact provenance source path
  if (jsonCopy.provenance?.source_path) {
    jsonCopy.provenance.source_path = redactString(jsonCopy.provenance.source_path);
  }

  // Redact events
  if (Array.isArray(jsonCopy.events)) {
    for (const evt of jsonCopy.events) {
      if (evt.payload) {
        if (evt.payload.type === 'user_message' || evt.payload.type === 'assistant_message') {
          if (evt.payload.data.content) {
            evt.payload.data.content = redactString(evt.payload.data.content);
          }
          if (evt.payload.data.thinking) {
            evt.payload.data.thinking = redactString(evt.payload.data.thinking);
          }
        } else if (evt.payload.type === 'shell_command') {
          if (evt.payload.data.command) {
            evt.payload.data.command = redactString(evt.payload.data.command);
          }
          if (evt.payload.data.output) {
            evt.payload.data.output = redactString(evt.payload.data.output);
          }
          if (evt.payload.data.cwd) {
            evt.payload.data.cwd = redactString(evt.payload.data.cwd);
          }
        } else if (evt.payload.type === 'file_action') {
          if (evt.payload.data.path) {
            evt.payload.data.path = redactString(evt.payload.data.path);
          }
          if (evt.payload.data.diff) {
            evt.payload.data.diff = redactString(evt.payload.data.diff);
          }
        }
      }
    }
  }

  return {
    redactedTrace: jsonCopy,
    redactedCount: count,
    categories,
  };
}

export function convertToAtif(trace: AgentWorthTrace): any {
  return {
    schema_version: '1.1.0',
    session_id: trace.session_id,
    agent: {
      name: trace.adapter,
      version: '0.1.2',
      models: trace.stats?.models_used || [],
    },
    environment: {
      started_at: trace.started_at,
      ended_at: trace.ended_at,
      duration_seconds: trace.stats?.duration_seconds,
    },
    metrics: {
      total_tokens:
        (trace.stats?.token_usage?.input_tokens || 0) +
        (trace.stats?.token_usage?.output_tokens || 0) +
        (trace.stats?.token_usage?.cache_read_input_tokens || 0) +
        (trace.stats?.token_usage?.cache_creation_input_tokens || 0),
      input_tokens: trace.stats?.token_usage?.input_tokens || 0,
      output_tokens: trace.stats?.token_usage?.output_tokens || 0,
      cache_read_tokens: trace.stats?.token_usage?.cache_read_input_tokens || 0,
      cache_creation_tokens: trace.stats?.token_usage?.cache_creation_input_tokens || 0,
      events_count: trace.events?.length || 0,
      tool_calls_count: trace.stats?.tool_calls_count || 0,
    },
    scores: trace.score
      ? {
          composite: trace.score.composite_score,
          outcome: trace.score.outcome_score,
          verifiability: trace.score.verifiability_score,
          complexity: trace.score.complexity_score,
          recovery: trace.score.recovery_score,
          provenance: trace.score.provenance_score,
        }
      : null,
    steps: (trace.events || []).map((evt) => ({
      sequence: evt.sequence,
      timestamp: evt.timestamp,
      type: evt.payload?.type,
      data: evt.payload?.data,
    })),
  };
}


/** Re-reads the session logs on disk. The one mutation the dashboard owns —
 *  everything else you do from the CLI. */
export async function runScan(): Promise<ScanSummary> {
  const res = await fetch('/api/scan', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: '{}',
  });
  if (!res.ok) {
    const detail = await res.json().catch(() => null);
    throw new Error(detail?.error ?? `Scan failed (${res.status})`);
  }
  return res.json();
}
