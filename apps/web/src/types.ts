/**
 * Trimmed type set for apps/web. The landing page embeds a handful of live
 * demo widgets (VerdictBoard, CacheCliffWidget, VerdictStamp, HeroReceipt)
 * that are also used, with live data, in apps/dashboard — this file carries
 * just the type-only shapes those components' props reference. The full
 * type surface (trace events, session details, coverage matrix, ...) lives
 * in apps/dashboard/src/types/index.ts; it is not needed here because the
 * marketing site makes no API calls.
 */

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

export interface OutcomeDistribution {
  ci_or_deployment_verified: number;
  commit_observed: number;
  test_or_build_passed: number;
  artifact_changed: number;
  done_claimed: number;
  unresolved: number;
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
