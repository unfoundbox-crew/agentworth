//! ATIF (Agent Trajectory Interchange Format) exporter for AgentWorth traces.

mod error;
mod models;
mod serializer;

pub use error::{AtifExportError, Result};
pub use models::{
    AtifAgent, AtifEnvironment, AtifError, AtifFileAction, AtifHumanIntervention, AtifMetrics,
    AtifModelInvocation, AtifOutcomeEvidence, AtifShellCommand, AtifStep, AtifStepSource,
    AtifTokens, AtifToolCall, AtifToolInfo, AtifToolResult, AtifTrajectory,
};
pub use serializer::{export_redacted_atif, export_to_atif, AtifExporter, ATIF_SCHEMA_VERSION};
