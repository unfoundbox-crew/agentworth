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
  // The wire names. These were cache_*_input_tokens here and never matched what
  // the server sends, so the cache split read as zero however real the data was.
  cache_read_tokens: number;
  cache_creation_tokens: number;
  /** @deprecated aliases some older payloads still carry */
  cache_read_input_tokens?: number;
  cache_creation_input_tokens?: number;
}

export interface Provenance {
  source_path: string;
  file_size_bytes: number;
  // Wire names. Previously adapter / modified_timestamp / fingerprint, none of
  // which the server sends — provenance rendered as em-dashes on real data.
  adapter_name: string;
  mtime_epoch_secs: number;
  content_fingerprint: string;
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
  /** Times this session's context was compacted. 0 (not absent) when never compacted. */
  compaction_count: number;
  /** Sum of each compaction round's own pre/post token delta. */
  compaction_tokens_dropped: number;
}

export interface CompactionEvent {
  trigger: string;
  pre_tokens?: number;
  post_tokens?: number;
  dropped_tokens?: number;
  duration_ms?: number;
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
  | { type: 'compaction'; data: CompactionEvent }
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

/**
 * The dashboard's flattened view of `GET /api/traces/:id`'s response (server
 * type `TraceDetailResponse` in apps/cli/src/server/routes.rs): score,
 * outcomes and recoveries merged onto the trace object, plus the two
 * pagination fields the server started returning in #72.
 *
 * `events_total`/`events_offset` are absent on a server that predates #72 —
 * such a response already carries every event in `trace.events`, so absence
 * means "complete", not "unknown total".
 */
export interface TraceDetailResponse extends AgentWorthTrace {
  events_total?: number;
  events_offset?: number;
}

/**
 * The real wire shape of `GET /api/traces/:id`, mirroring `TraceDetailResponse` in
 * `apps/cli/src/server/routes.rs` field-for-field: a nested `trace`, not the flattened
 * shape above. `fetchTraceDetail` in `../services/api.ts` does the flattening itself after
 * fetching — this type exists so the contract test (`contract.test.ts`) can check the
 * actual bytes the server sends, independent of that client-side reshaping.
 */
export interface TraceDetailWireResponse {
  trace: AgentWorthTrace;
  score: TraceScore;
  outcomes: OutcomeEvidence[];
  recoveries: RecoverySignal[];
  events_total: number;
  events_offset: number;
}

/** `GET /api/traces/:id/events` — one page of a trace's events, for lazy loading. */
export interface EventsPageResponse {
  events: NormalizedEvent[];
  events_total: number;
  events_offset: number;
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
  /** Source file mtime. Absent on builds that do not join `sources.mtime` yet. */
  source_mtime_epoch_secs?: number;
  /** Times this session's context was compacted. 0 (not absent) when never compacted. */
  compaction_count?: number;
  /** Sum of each compaction round's own pre/post token delta. */
  compaction_tokens_dropped?: number;
}

// These mirror apps/cli/src/server/archaeology.rs's ArchaeologyHighlights and its nested
// structs field-for-field (serde's default snake_case) — the response shape of GET
// /api/archaeology, this dashboard's only source of archaeology data.

export interface UnsolvedTaskHighlight {
  session_id: string;
  adapter: string;
  prompt: string;
  total_tokens: number;
  duration_seconds?: number | null;
  models_used: string[];
  outcome_summary: string;
  error_count: number;
}

export interface RecoveryLoopHighlight {
  session_id: string;
  adapter: string;
  failure_sequence: number;
  recovery_sequence: number;
  steps_to_recover: number;
  corrective_actions_count: number;
  duration_seconds?: number | null;
  failure_summary: string;
  recovery_summary: string;
}

export interface ModelSwitchesHighlight {
  session_id: string;
  adapter: string;
  switch_count: number;
  unique_models: string[];
  models_sequence: string[];
  total_tokens: number;
}

export interface CarbonDatingEra {
  period: string;
  tokens: number;
  sessions_count: number;
}

export interface TokenCarbonDating {
  earliest_session_at?: string | null;
  latest_session_at?: string | null;
  total_days_active: number;
  total_tokens: number;
  average_tokens_per_session: number;
  timeline: CarbonDatingEra[];
  adapter_tokens: Record<string, number>;
}

export interface ArchaeologyData {
  most_expensive_unsolved: UnsolvedTaskHighlight | null;
  longest_recovery_loop: RecoveryLoopHighlight | null;
  most_frequent_model_switches: ModelSwitchesHighlight | null;
  token_carbon_dating: TokenCarbonDating;
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

/**
 * One row of `GET /api/usage`'s `daily`/`weekly`/`monthly` arrays, mirroring
 * `UsagePeriodSummary` in `crates/storage/src/lib.rs` field-for-field. `period` is a real
 * date-shaped string, not the literal `"day"|"week"|"month"` — `"2026-09-07"` for daily rows,
 * `"2026-W36"` for weekly, `"2026-09"` for monthly (see `v_daily_usage`/`v_weekly_usage`/
 * `v_monthly_usage` in `crates/storage/src/lib.rs`). Each period can carry more than one row —
 * one per `adapter` — since the view groups by `(period, adapter)`.
 */
export interface UsagePeriodSummary {
  period: string;
  adapter: string;
  session_count: number;
  total_events: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  total_tokens: number;
  total_duration_seconds: number;
  estimated_cost_usd: number;
  cache_hit_ratio: number;
}

/**
 * `GET /api/usage`'s real response shape, mirroring `UsageResponse` in
 * `apps/cli/src/server/routes.rs`. The route ignores any `?period=` query string — it always
 * returns all three rollups at once, most-recent period first in each array (`ORDER BY period
 * DESC`). `cost_basis`/`subscription_tier` label every `estimated_cost_usd` as an API
 * list-price equivalent, not what the account actually paid.
 */
export interface UsageResponse {
  daily: UsagePeriodSummary[];
  weekly: UsagePeriodSummary[];
  monthly: UsagePeriodSummary[];
  cost_basis: string;
  subscription_tier?: string;
}

/**
 * `GET /api/pacing`'s real response shape, mirroring `PacingSummary` in
 * `crates/storage/src/lib.rs` field-for-field. The previous version of this type used
 * different field names entirely (`tokens_in_window`, `cache_hit_percent`,
 * `estimated_cost_in_window_usd`, `active_sessions_count`) that the server never sent.
 */
export interface PacingResponse {
  window_hours: number;
  started_at: string;
  ended_at: string;
  session_count: number;
  total_events: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  total_tokens: number;
  burn_rate_tokens_per_hour: number;
  estimated_cost_usd: number;
  cache_hit_ratio: number;
  active_adapters: string[];
  active_models: string[];
}

/**
 * One entry of `GET /api/blame`'s response, mirroring `BlameMatch` in
 * `crates/storage/src/lib.rs` field-for-field. The route itself returns a bare JSON array
 * of these -- there is no wrapper object with `file_path`/`total_edits`/`edits` on the wire.
 */
export interface BlameMatch {
  session_id: string;
  adapter: string;
  source_path: string;
  started_at: string;
  models_used: string[];
  total_tokens: number;
  tool_calls_count: number;
  file_path: string;
  action: FileActionType;
  modified_at: string;
  model?: string;
}

/** `GET /api/blame`'s real response shape: a bare array, not an object. */
export type BlameResponse = BlameMatch[];

/**
 * One row of `GET /api/matrix`'s `adapters` array, mirroring `AdapterMatrixItem` in
 * `apps/cli/src/server/routes.rs` field-for-field. Every capability column is a plain
 * boolean measured against the fixture suite — there is no `'partial'` rung on the wire.
 */
export interface AdapterCapability {
  adapter: string;
  name: string;
  detected: boolean;
  sessions_count: number;
  identities: string[];
  formats: string[];
  token_accounting: boolean;
  cache_breakdown: boolean;
  tool_calls: boolean;
  file_actions: boolean;
  shell_commands: boolean;
  model_switches: boolean;
  thinking_blocks: boolean;
  error_recovery: boolean;
}

/**
 * `GET /api/matrix`'s real response shape, mirroring `AdapterMatrixResponse` in
 * `apps/cli/src/server/routes.rs`. There is no `generated_at` field on the wire.
 */
export interface CoverageMatrixResponse {
  total_adapters: number;
  detected_adapters: number;
  adapters: AdapterCapability[];
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
}


export interface ScanSummary {
  discovered_sources: number;
  scanned_sessions: number;
  skipped_unchanged: number;
  errors_encountered: number;
  total_indexed_sessions: number;
}
