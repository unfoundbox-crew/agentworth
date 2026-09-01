export type AdapterType = 'claude_code' | 'codex' | 'gemini' | 'opencode';

export type OutcomeKind =
  | 'done_claimed'
  | 'artifact_changed'
  | 'test_or_build_passed'
  | 'commit_observed'
  | 'ci_or_deployment_verified'
  | 'unresolved';

export interface TokenUsage {
  input_tokens: number;
  output_tokens: number;
  cache_read_input_tokens: number;
  cache_creation_input_tokens: number;
}

export interface Provenance {
  source_path: string;
  adapter: string;
  file_size_bytes: number;
  modified_timestamp: number;
  fingerprint: string;
}

export interface TraceStats {
  total_events: number;
  user_messages_count: number;
  assistant_messages_count: number;
  tool_calls_count: number;
  token_usage: TokenUsage;
  models_used: string[];
  tools_used: Record<string, number>;
  duration_seconds?: number;
}

export type FileActionType = 'read' | 'write' | 'edit' | 'delete';

export interface ToolCall {
  id?: string;
  name: string;
  arguments: Record<string, any> | string;
}

export interface ToolResult {
  call_id?: string;
  name?: string;
  output: any;
  is_error: boolean;
}

export interface ShellCommand {
  command: string;
  cwd?: string;
  exit_code?: number;
  output?: string;
}

export interface OutcomeEvidence {
  kind: OutcomeKind;
  summary: string;
  confidence: number;
}

export interface HumanIntervention {
  action: string;
  details?: string;
}

export type EventPayload =
  | { type: 'user_message'; data: { content: string } }
  | { type: 'assistant_message'; data: { content: string; thinking?: string } }
  | {
      type: 'model_invocation';
      data: {
        model: string;
        token_usage: TokenUsage;
        cost_usd?: number;
        latency_ms?: number;
      };
    }
  | { type: 'tool_call'; data: ToolCall }
  | { type: 'tool_result'; data: ToolResult }
  | { type: 'shell_command'; data: ShellCommand }
  | {
      type: 'file_action';
      data: {
        path: string;
        action: FileActionType;
        diff?: string;
        lines_changed?: number;
      };
    }
  | { type: 'outcome_evidence'; data: OutcomeEvidence }
  | { type: 'error'; data: { message: string; is_recovered: boolean } }
  | { type: 'human_intervention'; data: HumanIntervention }
  | { type: 'custom'; data: { kind: string; data: any } };

export interface NormalizedEvent {
  id: string;
  sequence: number;
  timestamp: string;
  payload: EventPayload;
  raw_ref?: string;
}

export interface TraceScore {
  outcome_score: number;
  verifiability_score: number;
  complexity_score: number;
  recovery_score: number;
  provenance_score: number;
  composite_score: number;
  explanations: string[];
}

export interface RecoverySignal {
  failure_sequence: number;
  failure_summary: string;
  recovery_sequence: number;
  recovery_summary: string;
  steps_to_recover: number;
  duration_seconds?: number;
  corrective_actions_count: number;
}

export interface AgentWorthTrace {
  session_id: string;
  adapter: string;
  provenance: Provenance;
  started_at: string;
  ended_at?: string;
  stats: TraceStats;
  events: NormalizedEvent[];
  metadata?: Record<string, any>;
  score?: TraceScore;
  outcomes?: OutcomeEvidence[];
  recoveries?: RecoverySignal[];
}

export interface SessionSummary {
  session_id: string;
  adapter: string;
  source_path: string;
  started_at: string;
  duration_seconds?: number;
  total_tokens: number;
  total_events: number;
  tool_calls_count: number;
  models_used: string[];
  prompt_preview?: string;
  primary_outcome?: OutcomeKind;
  composite_score?: number;
}

export interface ArchaeologyTask {
  title: string;
  prompt: string;
  tokens: string;
  models_count: number;
  models_list: string[];
  duration: string;
  outcome: string;
  notes: string;
}

export interface RecoveryArchaeology {
  title: string;
  attempts_count: number;
  initial_error: string;
  corrective_action: string;
  final_resolution: string;
  tokens_burned: string;
  tool_calls: number;
}

export interface ModelHoppingArchaeology {
  title: string;
  sequence: string[];
  reason: string;
  total_cost: string;
}

export interface WeirdDiscovery {
  id: string;
  title: string;
  description: string;
  severity: 'hilarious' | 'bizarre' | 'costly';
  stat: string;
}

export interface ArchaeologyData {
  most_expensive_task: ArchaeologyTask;
  longest_recovery_loop: RecoveryArchaeology;
  model_hopping: ModelHoppingArchaeology;
  weird_discoveries: WeirdDiscovery[];
}

export interface OutcomeDistribution {
  ci_or_deployment_verified: number;
  commit_observed: number;
  test_or_build_passed: number;
  artifact_changed: number;
  done_claimed: number;
  unresolved: number;
}

export interface DailyUsageEntry {
  date: string;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  total_tokens: number;
  estimated_cost_usd: number;
  sessions_count: number;
}

export interface UsageRollupResponse {
  period: 'day' | 'week' | 'month' | string;
  entries: DailyUsageEntry[];
  total_cost_usd: number;
  total_tokens: number;
}

export interface PacingResponse {
  window_hours: number;
  tokens_in_window: number;
  burn_rate_tokens_per_hour: number;
  cache_hit_percent: number;
  estimated_cost_in_window_usd: number;
  active_sessions_count: number;
  recent_switches_cost_usd?: number;
}

export interface BlameEditEntry {
  session_id: string;
  adapter: string;
  model: string;
  timestamp: string;
  prompt_preview?: string;
  diff_snippet?: string;
  lines_added?: number;
  lines_deleted?: number;
  outcome?: OutcomeKind;
}

export interface BlameResponse {
  file_path: string;
  total_edits: number;
  edits: BlameEditEntry[];
}

export interface AdapterCapability {
  id: string;
  name: string;
  sessions: 'yes' | 'no' | 'partial';
  tokens: 'yes' | 'no' | 'partial';
  cache_split: 'yes' | 'no' | 'partial';
  models: 'yes' | 'no' | 'partial';
  file_edits: 'yes' | 'no' | 'partial';
  shell_exit: 'yes' | 'no' | 'partial';
  outcomes: string;
  notes?: string;
  sessions_count?: number;
  tokens_count?: number;
}

export interface CoverageMatrixResponse {
  adapters: AdapterCapability[];
  generated_at: string;
}

export interface AggregateStats {
  total_sessions: number;
  total_events: number;
  token_usage: TokenUsage;
  sessions_by_adapter: Record<string, number>;
  models_usage_count: Record<string, number>;
  tools_usage_count: Record<string, number>;
  verified_outcomes_count: number;
  outcome_distribution?: OutcomeDistribution;
  first_session_at?: string;
  last_session_at?: string;
  archaeology?: ArchaeologyData;
}


export interface ScanSummary {
  discovered_sources: number;
  scanned_sessions: number;
  skipped_unchanged: number;
  errors_encountered: number;
  total_indexed_sessions: number;
}
