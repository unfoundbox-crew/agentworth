use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::tokens::TokenUsage;

/// Specific file operation performed by an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileActionType {
    Read,
    Write,
    Edit,
    Delete,
}

/// Confidence-graded outcome classifications as defined in the outcome hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeKind {
    /// Agent claimed completion (weakest).
    DoneClaimed,
    /// Artifact / file changes observed.
    ArtifactChanged,
    /// Build or test suite executed and passed.
    TestOrBuildPassed,
    /// Git commit created or observed.
    CommitObserved,
    /// External CI / PR / Deployment verified (strongest).
    CiOrDeploymentVerified,
}

/// Tool invocation request by the agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: Option<String>,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Result returned from a tool invocation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    pub call_id: Option<String>,
    pub name: Option<String>,
    pub output: serde_json::Value,
    pub is_error: bool,
}

/// Shell command execution details.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShellCommand {
    pub command: String,
    pub cwd: Option<String>,
    pub exit_code: Option<i32>,
    pub output: Option<String>,
}

/// Evidence supporting an outcome inference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutcomeEvidence {
    pub kind: OutcomeKind,
    pub summary: String,
    pub confidence: f32,
}

/// Human intervention or interruption in the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanIntervention {
    pub action: String,
    pub details: Option<String>,
}

/// Model switch or transition occurring in a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSwitch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_model: Option<String>,
    pub to_model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// A context-compaction round: the harness summarized the conversation so far and
/// replaced it with the summary, so most of the original content is no longer in the
/// model's context. Claude Code writes this as a `compact_boundary` system event
/// carrying `compactMetadata`; other harnesses may expose the same concept differently.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactionEvent {
    /// How compaction was triggered. Kept as a free-form string rather than a closed
    /// enum: only `"manual"` has been observed in real Claude Code logs so far, and
    /// there is no confirmed full set of values to enumerate.
    pub trigger: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_tokens: Option<u64>,
    /// Tokens dropped by this specific round, i.e. `pre_tokens - post_tokens`. Deliberately
    /// derived rather than read from the harness's own running counter (Claude Code's
    /// `compactMetadata.cumulativeDroppedTokens`) -- that counter was observed, against real
    /// session logs, to reset mid-session (e.g. across a `/clear`), so reading only its final
    /// value undercounts a session's true total. Summing this field across every compaction
    /// in a session is always correct; summing the harness's raw cumulative field is not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dropped_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// High-level event classification type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    UserMessage,
    AssistantMessage,
    ModelInvocation,
    ModelSwitch,
    ToolCall,
    ToolResult,
    ShellCommand,
    FileAction,
    OutcomeEvidence,
    Error,
    HumanIntervention,
    Compaction,
    Custom,
}

/// Payload representing the specific event occurring in a trace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum EventPayload {
    UserMessage {
        content: String,
    },
    AssistantMessage {
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        thinking: Option<String>,
    },
    ModelInvocation {
        model: String,
        token_usage: TokenUsage,
        #[serde(skip_serializing_if = "Option::is_none")]
        cost_usd: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        latency_ms: Option<u64>,
    },
    ModelSwitch(ModelSwitch),
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    ShellCommand(ShellCommand),
    FileAction {
        path: String,
        action: FileActionType,
        #[serde(skip_serializing_if = "Option::is_none")]
        diff: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        lines_changed: Option<u64>,
    },
    OutcomeEvidence(OutcomeEvidence),
    Error {
        message: String,
        is_recovered: bool,
    },
    HumanIntervention(HumanIntervention),
    Compaction(CompactionEvent),
    Custom {
        kind: String,
        data: serde_json::Value,
    },
}

impl EventPayload {
    /// Returns the high-level EventType for this payload.
    pub fn event_type(&self) -> EventType {
        match self {
            Self::UserMessage { .. } => EventType::UserMessage,
            Self::AssistantMessage { .. } => EventType::AssistantMessage,
            Self::ModelInvocation { .. } => EventType::ModelInvocation,
            Self::ModelSwitch(_) => EventType::ModelSwitch,
            Self::ToolCall(_) => EventType::ToolCall,
            Self::ToolResult(_) => EventType::ToolResult,
            Self::ShellCommand(_) => EventType::ShellCommand,
            Self::FileAction { .. } => EventType::FileAction,
            Self::OutcomeEvidence(_) => EventType::OutcomeEvidence,
            Self::Error { .. } => EventType::Error,
            Self::HumanIntervention(_) => EventType::HumanIntervention,
            Self::Compaction(_) => EventType::Compaction,
            Self::Custom { .. } => EventType::Custom,
        }
    }
}

/// A normalized, time-ordered event in an AgentWorth trace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NormalizedEvent {
    /// Unique identifier for this event.
    pub id: String,
    /// Zero-based or one-based sequence number within the session.
    pub sequence: u64,
    /// Timestamp when the event occurred.
    pub timestamp: DateTime<Utc>,
    /// Semantic payload of the event.
    pub payload: EventPayload,
    /// Optional provenance / lazy reference (e.g. line number or byte offset).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_ref: Option<String>,
}

impl NormalizedEvent {
    pub fn new(sequence: u64, timestamp: DateTime<Utc>, payload: EventPayload) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            sequence,
            timestamp,
            payload,
            raw_ref: None,
        }
    }

    pub fn with_raw_ref(mut self, raw_ref: impl Into<String>) -> Self {
        self.raw_ref = Some(raw_ref.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_serialization_roundtrip() {
        let event = NormalizedEvent::new(
            1,
            Utc::now(),
            EventPayload::ToolCall(ToolCall {
                id: Some("call_123".to_string()),
                name: "bash".to_string(),
                arguments: serde_json::json!({"command": "cargo test"}),
            }),
        );

        let serialized = serde_json::to_string(&event).expect("serialize event");
        let deserialized: NormalizedEvent =
            serde_json::from_str(&serialized).expect("deserialize event");

        assert_eq!(event.id, deserialized.id);
        assert_eq!(event.sequence, deserialized.sequence);
        if let EventPayload::ToolCall(tc) = deserialized.payload {
            assert_eq!(tc.name, "bash");
        } else {
            panic!("unexpected payload variant");
        }
    }

    #[test]
    fn test_model_switch_serialization_and_event_type() {
        let ms = ModelSwitch {
            from_model: Some("claude-3-5-sonnet".to_string()),
            to_model: "claude-3-opus".to_string(),
            reason: Some("user requested deeper reasoning".to_string()),
        };
        let payload = EventPayload::ModelSwitch(ms.clone());
        assert_eq!(payload.event_type(), EventType::ModelSwitch);

        let event = NormalizedEvent::new(2, Utc::now(), payload);
        let serialized = serde_json::to_string(&event).expect("serialize");
        let deserialized: NormalizedEvent = serde_json::from_str(&serialized).expect("deserialize");

        if let EventPayload::ModelSwitch(deser_ms) = deserialized.payload {
            assert_eq!(deser_ms.from_model, Some("claude-3-5-sonnet".to_string()));
            assert_eq!(deser_ms.to_model, "claude-3-opus");
            assert_eq!(
                deser_ms.reason,
                Some("user requested deeper reasoning".to_string())
            );
        } else {
            panic!("expected ModelSwitch payload");
        }
    }

    #[test]
    fn test_compaction_event_serialization_and_event_type() {
        let ce = CompactionEvent {
            trigger: "manual".to_string(),
            pre_tokens: Some(754_356),
            post_tokens: Some(25_834),
            dropped_tokens: Some(728_522),
            duration_ms: Some(213_832),
        };
        let payload = EventPayload::Compaction(ce.clone());
        assert_eq!(payload.event_type(), EventType::Compaction);

        let event = NormalizedEvent::new(3, Utc::now(), payload);
        let serialized = serde_json::to_string(&event).expect("serialize");
        let deserialized: NormalizedEvent = serde_json::from_str(&serialized).expect("deserialize");

        if let EventPayload::Compaction(deser_ce) = deserialized.payload {
            assert_eq!(deser_ce.trigger, "manual");
            assert_eq!(deser_ce.pre_tokens, Some(754_356));
            assert_eq!(deser_ce.post_tokens, Some(25_834));
            assert_eq!(deser_ce.dropped_tokens, Some(728_522));
            assert_eq!(deser_ce.duration_ms, Some(213_832));
        } else {
            panic!("expected Compaction payload");
        }
    }
}
