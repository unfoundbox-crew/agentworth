/**
 * Minimal wire types shared by TrajectoryScrubber + OutcomeLadder.
 * Keep in sync with each app's OutcomeKind / OutcomeEvidence / NormalizedEvent.
 * Full app types stay in each app until a broader shared-types cut.
 */

export type OutcomeKind =
  | 'done_claimed'
  | 'artifact_changed'
  | 'test_or_build_passed'
  | 'commit_observed'
  | 'ci_or_deployment_verified'
  | 'unresolved';

export interface OutcomeEvidence {
  kind: OutcomeKind;
  summary: string;
  confidence: number;
}

export interface TokenUsage {
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  cache_read_input_tokens?: number;
  cache_creation_input_tokens?: number;
}

export type FileActionType = 'read' | 'write' | 'edit' | 'delete';

export interface ToolCall {
  id?: string;
  name: string;
  arguments: Record<string, unknown> | string;
}

export interface ToolResult {
  call_id?: string;
  name?: string;
  output: unknown;
  is_error: boolean;
}

export interface ShellCommand {
  command: string;
  cwd?: string;
  exit_code?: number;
  output?: string;
}

export interface CompactionEvent {
  trigger: string;
  pre_tokens?: number;
  post_tokens?: number;
  dropped_tokens?: number;
  duration_ms?: number;
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
  | { type: 'custom'; data: { kind: string; data: unknown } };

export interface NormalizedEvent {
  id: string;
  sequence: number;
  timestamp: string;
  payload: EventPayload;
  raw_ref?: string;
}

/** Minimal aggregate shape VerdictBoard reads (apps may pass a wider AggregateStats). */
export interface OutcomeDistribution {
  ci_or_deployment_verified: number;
  commit_observed: number;
  test_or_build_passed: number;
  artifact_changed: number;
  done_claimed: number;
  unresolved: number;
}

export interface AggregateStats {
  total_sessions: number;
  verified_outcomes_count: number;
  outcome_distribution?: OutcomeDistribution;
}

/** Minimal session row FleetStrip / useFleet need from GET /api/traces. */
export interface FleetSessionSummary {
  session_id: string;
  adapter: string;
  source_mtime_epoch_secs?: number;
}

/** Minimal usage row for today's spend line on FleetStrip. */
export interface FleetUsagePeriodSummary {
  period: string;
  estimated_cost_usd: number;
  total_tokens: number;
  cost_weighted_tokens?: number;
}

export interface FleetUsageResponse {
  daily: FleetUsagePeriodSummary[];
}
