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
    pub tools_used: BTreeMap<String, usize>,
    pub duration_seconds: Option<f64>,
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
        self.stats = stats;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(trace.stats.duration_seconds.unwrap() >= 4.0);
    }
}
