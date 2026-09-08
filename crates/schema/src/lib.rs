//! Canonical schema for AgentWorth traces, events, token accounting, and provenance.

mod compaction;
mod event;
mod human;
mod machine;
mod provenance;
pub mod text;
mod tokens;
mod trace;
pub mod vector;

pub use compaction::{
    compact_summary_text, compaction_rounds, CompactionRound, COMPACT_SUMMARY_KIND,
};
pub use human::{human_prompt_text, is_human_prompt};
pub use event::{
    CompactionEvent, EventPayload, EventType, FileActionType, HumanIntervention, ModelSwitch,
    NormalizedEvent, OutcomeEvidence, OutcomeKind, ShellCommand, ToolCall, ToolResult,
};
pub use machine::{
    host_fingerprint, host_fingerprint_for, MachineInfo, FINGERPRINT_SALT_DEFAULT,
    FINGERPRINT_SALT_ENV, FINGERPRINT_SALT_ENV_LEGACY,
};
pub use provenance::{extract_repository_or_workspace, is_subagent_transcript, Provenance};
pub use text::{preview, tail_chars, truncate_chars};
pub use tokens::TokenUsage;
pub use trace::{AgentWorthTrace, IdentitySighting, TraceKind, TraceStats};
pub use vector::{ChunkKind, TrajectoryChunk, VectorSearchResult, VectorStats};
