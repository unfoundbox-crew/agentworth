//! Canonical schema for AgentWorth traces, events, token accounting, and provenance.

mod event;
mod provenance;
mod tokens;
mod trace;
pub mod vector;

pub use event::{
    CompactionEvent, EventPayload, EventType, FileActionType, HumanIntervention, ModelSwitch,
    NormalizedEvent, OutcomeEvidence, OutcomeKind, ShellCommand, ToolCall, ToolResult,
};
pub use provenance::{extract_repository_or_workspace, Provenance};
pub use tokens::TokenUsage;
pub use trace::{AgentWorthTrace, TraceStats};
pub use vector::{ChunkKind, TrajectoryChunk, VectorSearchResult, VectorStats};
