import {
  AggregateStats,
  AgentWorthTrace,
  SessionSummary,
  OutcomeKind,
} from '../types';
import {
  mockAggregateStats,
  mockSummaries,
  mockDetailedTraces,
} from './mockData';

const BASE_URL = '/api';

export interface TraceQueryFilters {
  adapter?: string;
  model?: string;
  search?: string;
  outcome?: string;
  orderBy?: string;
  limit?: number;
  offset?: number;
}

export async function fetchAggregateStats(): Promise<AggregateStats> {
  try {
    const res = await fetch(`${BASE_URL}/stats`);
    if (res.ok) {
      const data = await res.json();
      return {
        ...mockAggregateStats,
        ...data,
        archaeology: data.archaeology || mockAggregateStats.archaeology,
      };
    }
  } catch (_err) {
    // Backend API not running; fallback to local mock data
  }
  return mockAggregateStats;
}

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
    // Fallback to local mock filtering
  }

  let filtered = [...mockSummaries];

  if (filters.adapter && filters.adapter !== 'all') {
    filtered = filtered.filter((s) => s.adapter === filters.adapter);
  }

  if (filters.search) {
    const q = filters.search.toLowerCase();
    filtered = filtered.filter(
      (s) =>
        s.session_id.toLowerCase().includes(q) ||
        (s.prompt_preview && s.prompt_preview.toLowerCase().includes(q)) ||
        s.models_used.some((m) => m.toLowerCase().includes(q))
    );
  }

  if (filters.outcome && filters.outcome !== 'all') {
    filtered = filtered.filter((s) => s.primary_outcome === filters.outcome);
  }

  if (filters.orderBy) {
    switch (filters.orderBy) {
      case 'tokens_desc':
        filtered.sort((a, b) => b.total_tokens - a.total_tokens);
        break;
      case 'tokens_asc':
        filtered.sort((a, b) => a.total_tokens - b.total_tokens);
        break;
      case 'events_desc':
        filtered.sort((a, b) => b.total_events - a.total_events);
        break;
      case 'duration_desc':
        filtered.sort((a, b) => (b.duration_seconds || 0) - (a.duration_seconds || 0));
        break;
      case 'score_desc':
        filtered.sort((a, b) => (b.composite_score || 0) - (a.composite_score || 0));
        break;
      case 'started_at_asc':
        filtered.sort(
          (a, b) => new Date(a.started_at).getTime() - new Date(b.started_at).getTime()
        );
        break;
      case 'started_at_desc':
      default:
        filtered.sort(
          (a, b) => new Date(b.started_at).getTime() - new Date(a.started_at).getTime()
        );
        break;
    }
  }

  return { traces: filtered, total: filtered.length };
}

export async function fetchTraceDetail(
  sessionId: string
): Promise<AgentWorthTrace> {
  try {
    const res = await fetch(`${BASE_URL}/traces/${encodeURIComponent(sessionId)}`);
    if (res.ok) {
      const data = await res.json();
      return data;
    }
  } catch (_err) {
    // Fallback to local mock data
  }

  if (mockDetailedTraces[sessionId]) {
    return mockDetailedTraces[sessionId];
  }

  // Generate generic dynamic trace if requested session isn't in detailed cache
  const summary = mockSummaries.find((s) => s.session_id === sessionId) || mockSummaries[0];
  return {
    session_id: summary.session_id,
    adapter: summary.adapter,
    provenance: {
      source_path: summary.source_path,
      adapter: summary.adapter,
      file_size_bytes: 32768,
      modified_timestamp: Math.floor(new Date(summary.started_at).getTime() / 1000),
      fingerprint: 'sha256:7f83b1657ff1fc53b92dc18148a1d65dfc2d4b1fa3d677284addd200126d9069',
    },
    started_at: summary.started_at,
    ended_at: new Date(new Date(summary.started_at).getTime() + (summary.duration_seconds || 120) * 1000).toISOString(),
    stats: {
      total_events: summary.total_events,
      user_messages_count: 1,
      assistant_messages_count: 2,
      tool_calls_count: summary.tool_calls_count,
      token_usage: {
        input_tokens: Math.floor(summary.total_tokens * 0.75),
        output_tokens: Math.floor(summary.total_tokens * 0.25),
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
      },
      models_used: summary.models_used,
      tools_used: { bash: Math.floor(summary.tool_calls_count / 2), replace_file_content: Math.ceil(summary.tool_calls_count / 2) },
      duration_seconds: summary.duration_seconds,
    },
    score: {
      outcome_score: summary.primary_outcome === 'unresolved' ? 0.2 : 0.9,
      verifiability_score: summary.primary_outcome === 'unresolved' ? 0.3 : 0.85,
      complexity_score: 0.75,
      recovery_score: 0.8,
      provenance_score: 1.0,
      composite_score: summary.composite_score || 0.85,
      explanations: [
        `Outcome score reflects detected status: ${summary.primary_outcome}`,
        'Verifiability checked against shell exit codes and repository artifacts',
        'Local provenance checked against disk fingerprint',
      ],
    },
    outcomes: summary.primary_outcome
      ? [
          {
            kind: summary.primary_outcome as OutcomeKind,
            summary: `Automated detection: ${summary.primary_outcome}`,
            confidence: 0.9,
          },
        ]
      : [],
    events: [
      {
        id: 'evt-dyn-1',
        sequence: 1,
        timestamp: summary.started_at,
        payload: {
          type: 'user_message',
          data: {
            content: summary.prompt_preview || 'Investigate codebase',
          },
        },
      },
      {
        id: 'evt-dyn-2',
        sequence: 2,
        timestamp: new Date(new Date(summary.started_at).getTime() + 5000).toISOString(),
        payload: {
          type: 'assistant_message',
          data: {
            thinking: 'Analyzing instructions and checking relevant workspace files...',
            content: `I will address "${summary.prompt_preview || 'the task'}" by reviewing current code structure and applying fixes.`,
          },
        },
      },
      {
        id: 'evt-dyn-3',
        sequence: 3,
        timestamp: new Date(new Date(summary.started_at).getTime() + 15000).toISOString(),
        payload: {
          type: 'shell_command',
          data: {
            command: 'cargo check',
            cwd: '/Users/saurabh/code/unfoundbox/agentworth',
            exit_code: 0,
            output: 'Finished dev [unoptimized + debuginfo] target(s) in 0.82s',
          },
        },
      },
    ],
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
      version: '0.1.0',
      models: trace.stats.models_used,
    },
    environment: {
      started_at: trace.started_at,
      ended_at: trace.ended_at,
      duration_seconds: trace.stats.duration_seconds,
    },
    metrics: {
      total_tokens:
        trace.stats.token_usage.input_tokens +
        trace.stats.token_usage.output_tokens +
        trace.stats.token_usage.cache_read_input_tokens +
        trace.stats.token_usage.cache_creation_input_tokens,
      input_tokens: trace.stats.token_usage.input_tokens,
      output_tokens: trace.stats.token_usage.output_tokens,
      cache_read_tokens: trace.stats.token_usage.cache_read_input_tokens,
      cache_creation_tokens: trace.stats.token_usage.cache_creation_input_tokens,
      events_count: trace.events.length,
      tool_calls_count: trace.stats.tool_calls_count,
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
    steps: trace.events.map((evt) => ({
      sequence: evt.sequence,
      timestamp: evt.timestamp,
      type: evt.payload.type,
      data: evt.payload.data,
    })),
  };
}
