use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::event::{EventPayload, NormalizedEvent};
use crate::provenance::Provenance;
use crate::tokens::TokenUsage;

/// Aggregate statistical summary of a trace without needing to retain full events in memory.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TraceStats {
    pub total_events: usize,
    pub user_messages_count: usize,
    pub assistant_messages_count: usize,
    pub tool_calls_count: usize,
    pub token_usage: TokenUsage,
    pub models_used: Vec<String>,
    /// Token usage broken out per model (sessions can invoke more than one, e.g.
    /// via subagent delegation). Values sum to `token_usage`.
    #[serde(default)]
    pub per_model_token_usage: BTreeMap<String, TokenUsage>,
    pub tools_used: BTreeMap<String, usize>,
    pub duration_seconds: Option<f64>,
    /// Number of times this session's context was compacted (summarized and replaced).
    /// 0 for the common case of a session that was never compacted.
    #[serde(default)]
    pub compaction_count: usize,
    /// Total tokens dropped across every compaction round in this session -- the sum of
    /// each round's own `pre_tokens - post_tokens`, not the harness's raw cumulative
    /// counter (see `CompactionEvent::dropped_tokens` for why that distinction matters).
    #[serde(default)]
    pub compaction_tokens_dropped: u64,
}

/// The canonical top-level representation of an AI agent session trace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentWorthTrace {
    pub session_id: String,
    pub adapter: String,
    pub provenance: Provenance,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub stats: TraceStats,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<NormalizedEvent>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub metadata: serde_json::Value,
}

impl AgentWorthTrace {
    pub fn new(
        session_id: impl Into<String>,
        adapter: impl Into<String>,
        provenance: Provenance,
        started_at: DateTime<Utc>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            adapter: adapter.into(),
            provenance,
            started_at,
            ended_at: None,
            stats: TraceStats::default(),
            events: Vec::new(),
            metadata: serde_json::Value::Null,
        }
    }

    /// Compute and refresh statistics based on the events contained in the trace.
    pub fn recalculate_stats(&mut self) {
        let mut stats = TraceStats::default();
        let mut models = Vec::new();
        let mut tools = BTreeMap::new();
        let mut per_model_usage: BTreeMap<String, TokenUsage> = BTreeMap::new();
        let mut latest_ts = self.started_at;

        for event in &self.events {
            stats.total_events += 1;
            if event.timestamp > latest_ts {
                latest_ts = event.timestamp;
            }

            match &event.payload {
                EventPayload::UserMessage { .. } => {
                    stats.user_messages_count += 1;
                }
                EventPayload::AssistantMessage { .. } => {
                    stats.assistant_messages_count += 1;
                }
                EventPayload::ToolCall(tool_call) => {
                    stats.tool_calls_count += 1;
                    *tools.entry(tool_call.name.clone()).or_insert(0) += 1;
                }
                EventPayload::ModelInvocation {
                    model, token_usage, ..
                } => {
                    if !models.contains(model) {
                        models.push(model.clone());
                    }
                    stats.token_usage += *token_usage;
                    *per_model_usage.entry(model.clone()).or_default() += *token_usage;
                }
                EventPayload::ModelSwitch(ms) => {
                    if !models.contains(&ms.to_model) {
                        models.push(ms.to_model.clone());
                    }
                    if let Some(ref from) = ms.from_model {
                        if !models.contains(from) {
                            models.push(from.clone());
                        }
                    }
                }
                EventPayload::Compaction(c) => {
                    stats.compaction_count += 1;
                    stats.compaction_tokens_dropped += c.dropped_tokens.unwrap_or(0);
                }
                _ => {}
            }
        }

        if self.ended_at.is_none() && stats.total_events > 0 {
            self.ended_at = Some(latest_ts);
        }

        if let Some(ended) = self.ended_at {
            let duration = (ended - self.started_at).num_milliseconds() as f64 / 1000.0;
            stats.duration_seconds = Some(duration.max(0.0));
        }

        stats.models_used = models;
        stats.tools_used = tools;
        stats.per_model_token_usage = per_model_usage;
        self.stats = stats;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::CompactionEvent;
    use chrono::Duration;

    #[test]
    fn test_trace_stats_recalculation() {
        let start = Utc::now();
        let prov = Provenance::new("/test/path.jsonl", "claude_code", 100, 12345, "fp123");
        let mut trace = AgentWorthTrace::new("sess-1", "claude_code", prov, start);

        trace.events.push(NormalizedEvent::new(
            1,
            start + Duration::seconds(1),
            EventPayload::UserMessage {
                content: "Hello".to_string(),
            },
        ));

        trace.events.push(NormalizedEvent::new(
            2,
            start + Duration::seconds(3),
            EventPayload::ModelInvocation {
                model: "claude-3-5-sonnet".to_string(),
                token_usage: TokenUsage::new(100, 50, 10, 5),
                cost_usd: None,
                latency_ms: Some(1200),
            },
        ));

        trace.events.push(NormalizedEvent::new(
            3,
            start + Duration::seconds(4),
            EventPayload::ToolCall(crate::event::ToolCall {
                id: Some("t1".to_string()),
                name: "Bash".to_string(),
                arguments: serde_json::json!({"command": "ls"}),
            }),
        ));

        trace.recalculate_stats();

        assert_eq!(trace.stats.total_events, 3);
        assert_eq!(trace.stats.user_messages_count, 1);
        assert_eq!(trace.stats.tool_calls_count, 1);
        assert_eq!(
            trace.stats.models_used,
            vec!["claude-3-5-sonnet".to_string()]
        );
        assert_eq!(trace.stats.tools_used.get("Bash"), Some(&1));
        assert_eq!(trace.stats.token_usage.total(), 165);
        assert_eq!(
            trace.stats.per_model_token_usage.get("claude-3-5-sonnet"),
            Some(&TokenUsage::new(100, 50, 10, 5))
        );
        assert!(trace.stats.duration_seconds.unwrap() >= 4.0);
    }

    #[test]
    fn test_trace_stats_per_model_token_usage_with_multiple_models() {
        let start = Utc::now();
        let prov = Provenance::new("/test/multi.jsonl", "claude_code", 100, 12345, "fp456");
        let mut trace = AgentWorthTrace::new("sess-multi", "claude_code", prov, start);

        trace.events.push(NormalizedEvent::new(
            1,
            start + Duration::seconds(1),
            EventPayload::ModelInvocation {
                model: "claude-sonnet-5".to_string(),
                token_usage: TokenUsage::new(500, 100, 200, 50),
                cost_usd: None,
                latency_ms: None,
            },
        ));

        trace.events.push(NormalizedEvent::new(
            2,
            start + Duration::seconds(2),
            EventPayload::ModelInvocation {
                model: "claude-fable-5".to_string(),
                token_usage: TokenUsage::new(300, 60, 0, 0),
                cost_usd: None,
                latency_ms: None,
            },
        ));

        // A second invocation of a model already seen must accumulate, not overwrite.
        trace.events.push(NormalizedEvent::new(
            3,
            start + Duration::seconds(3),
            EventPayload::ModelInvocation {
                model: "claude-sonnet-5".to_string(),
                token_usage: TokenUsage::new(50, 10, 0, 0),
                cost_usd: None,
                latency_ms: None,
            },
        ));

        trace.recalculate_stats();

        assert_eq!(
            trace.stats.models_used,
            vec!["claude-sonnet-5".to_string(), "claude-fable-5".to_string()]
        );

        assert_eq!(trace.stats.per_model_token_usage.len(), 2);
        assert_eq!(
            trace.stats.per_model_token_usage.get("claude-sonnet-5"),
            Some(&TokenUsage::new(550, 110, 200, 50))
        );
        assert_eq!(
            trace.stats.per_model_token_usage.get("claude-fable-5"),
            Some(&TokenUsage::new(300, 60, 0, 0))
        );

        // The flat aggregate must still equal the sum across every model (backward compat).
        let summed: TokenUsage = trace
            .stats
            .per_model_token_usage
            .values()
            .fold(TokenUsage::default(), |acc, u| acc + *u);
        assert_eq!(summed, trace.stats.token_usage);
        assert_eq!(trace.stats.token_usage.total(), 1270);
    }

    #[test]
    fn test_trace_stats_compaction_count_and_dropped_tokens_sum_across_rounds() {
        let start = Utc::now();
        let prov = Provenance::new("/test/compacted.jsonl", "claude_code", 100, 12345, "fp789");
        let mut trace = AgentWorthTrace::new("sess-compacted", "claude_code", prov, start);

        // A session never compacted must read as 0, not absent/None -- 0 is the common
        // and meaningful case, matching how `tool_calls_count` etc. already behave.
        trace.recalculate_stats();
        assert_eq!(trace.stats.compaction_count, 0);
        assert_eq!(trace.stats.compaction_tokens_dropped, 0);

        // Real numbers from an actual compacted Claude Code session (4 rounds). The
        // harness's own `cumulativeDroppedTokens` counter reset between round 1 and
        // round 2 in the real log this is drawn from, so summing each round's own
        // pre-post delta (728522 + 832179 + 836526 + 929259 = 3326486) is the only
        // reliable way to get the session's true total; naively trusting the last
        // round's cumulative counter would silently undercount by round 1's contribution.
        let rounds = [
            (754_356u64, 25_834u64, 213_832u64),
            (851_578, 19_399, 249_964),
            (854_648, 18_122, 108_979),
            (950_460, 21_201, 127_503),
        ];
        for (i, (pre, post, duration)) in rounds.iter().enumerate() {
            trace.events.push(NormalizedEvent::new(
                i as u64 + 1,
                start + chrono::Duration::seconds(i as i64),
                EventPayload::Compaction(CompactionEvent {
                    trigger: "manual".to_string(),
                    pre_tokens: Some(*pre),
                    post_tokens: Some(*post),
                    dropped_tokens: Some(pre - post),
                    duration_ms: Some(*duration),
                }),
            ));
        }

        trace.recalculate_stats();
        assert_eq!(trace.stats.compaction_count, 4);
        assert_eq!(trace.stats.compaction_tokens_dropped, 3_326_486);
        assert_eq!(trace.stats.total_events, 4);
    }
}
