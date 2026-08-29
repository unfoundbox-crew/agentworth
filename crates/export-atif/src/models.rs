use agentworth_schema::FileActionType;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Source originating an execution step in the trajectory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtifStepSource {
    User,
    Agent,
    Tool,
    Environment,
    System,
}

/// Agent descriptor in ATIF.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtifAgent {
    pub name: String,
    pub adapter: String,
    pub models: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Execution environment and provenance details.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtifEnvironment {
    pub source_path: String,
    pub adapter: String,
    pub started_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
}

/// Tool invocation record within an ATIF step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtifToolCall {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Tool execution result within an ATIF step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtifToolResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub output: serde_json::Value,
    pub is_error: bool,
}

/// Shell command execution details.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtifShellCommand {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

/// File modification record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtifFileAction {
    pub path: String,
    pub action: FileActionType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines_changed: Option<u64>,
}

/// Model invocation metrics in ATIF.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtifModelInvocation {
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
}

/// Outcome evidence in ATIF.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtifOutcomeEvidence {
    pub kind: String,
    pub summary: String,
    pub confidence: f32,
}

/// Error details in ATIF.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtifError {
    pub message: String,
    pub is_recovered: bool,
}

/// Human intervention event in ATIF.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtifHumanIntervention {
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

/// Single trajectory step in ATIF.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtifStep {
    pub step_id: String,
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    pub source: AtifStepSource,
    pub step_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<AtifToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_results: Option<Vec<AtifToolResult>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_command: Option<AtifShellCommand>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_action: Option<AtifFileAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_invocation: Option<AtifModelInvocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome_evidence: Option<AtifOutcomeEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AtifError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub human_intervention: Option<AtifHumanIntervention>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_ref: Option<String>,
}

/// Tool usage aggregate info.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtifToolInfo {
    pub name: String,
    pub call_count: usize,
}

/// Trajectory metrics summary.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AtifMetrics {
    pub total_events: usize,
    pub user_messages_count: usize,
    pub assistant_messages_count: usize,
    pub tool_calls_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
}

/// Token usage summary in ATIF.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtifTokens {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_tokens: u64,
}

/// The root ATIF (Agent Trajectory Interchange Format) document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtifTrajectory {
    pub schema_version: String,
    pub session_id: String,
    pub agent: AtifAgent,
    pub environment: AtifEnvironment,
    pub steps: Vec<AtifStep>,
    pub tools: Vec<AtifToolInfo>,
    pub metrics: AtifMetrics,
    pub tokens: AtifTokens,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub metadata: serde_json::Value,
}
