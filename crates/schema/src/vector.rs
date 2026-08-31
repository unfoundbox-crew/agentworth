//! Vector schema definitions for semantic latent embeddings, trajectory chunking, and search.

use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

/// Semantic category of a trajectory chunk extracted from an agent session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChunkKind {
    /// Initial user objective + final outcome claim + model stats.
    SessionSummary,
    /// Tool failure output + assistant's immediate next corrective action/thought turn.
    ErrorRecovery,
    /// Destructive or critical tool calls (e.g. `rm -rf`, `git reset`, DB migrations).
    ToolInvocation,
    /// Assistant turns with retreat/panic signatures ("my mistake", "I apologize", "lost the repo").
    ApologyPanic,
    /// File / code changes and diff lineage.
    CodeLineage,
}

impl ChunkKind {
    /// Returns the canonical snake_case string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionSummary => "session_summary",
            Self::ErrorRecovery => "error_recovery",
            Self::ToolInvocation => "tool_invocation",
            Self::ApologyPanic => "apology_panic",
            Self::CodeLineage => "code_lineage",
        }
    }
}

impl std::fmt::Display for ChunkKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for ChunkKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().replace('-', "_").as_str() {
            "session_summary" | "sessionsummary" | "summary" => Ok(Self::SessionSummary),
            "error_recovery" | "errorrecovery" | "error" => Ok(Self::ErrorRecovery),
            "tool_invocation" | "toolinvocation" | "tool" => Ok(Self::ToolInvocation),
            "apology_panic" | "apologypanic" | "panic" | "apology" => Ok(Self::ApologyPanic),
            "code_lineage" | "codelineage" | "lineage" => Ok(Self::CodeLineage),
            other => Err(format!("Unknown chunk kind: {}", other)),
        }
    }
}

/// A discrete, semantically rich slice of an agent session trajectory ready for local ONNX embedding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrajectoryChunk {
    /// Unique identifier for this chunk (e.g. `<session_id>-<kind>-<turn_index>`).
    pub chunk_id: String,
    /// Originating session ID.
    pub session_id: String,
    /// Originating agent adapter (e.g. `claude_code`, `antigravity`).
    pub adapter: String,
    /// Category / semantic classification of this chunk.
    pub kind: ChunkKind,
    /// Sequence or turn index within the trace.
    pub turn_index: usize,
    /// RFC 3339 timestamp when the turn occurred.
    pub timestamp: String,
    /// Plaintext content to be embedded and searched.
    pub text_content: String,
    /// JSON metadata payload (parameters, error messages, flags, etc.).
    pub metadata_json: String,
}

impl TrajectoryChunk {
    /// Create a new trajectory chunk with auto-generated chunk ID.
    pub fn new(
        session_id: impl Into<String>,
        adapter: impl Into<String>,
        kind: ChunkKind,
        turn_index: usize,
        timestamp: impl Into<String>,
        text_content: impl Into<String>,
        metadata_json: impl Into<String>,
    ) -> Self {
        let session_id = session_id.into();
        let chunk_id = format!("{}-{}-{}", session_id, kind.as_str(), turn_index);
        Self {
            chunk_id,
            session_id,
            adapter: adapter.into(),
            kind,
            turn_index,
            timestamp: timestamp.into(),
            text_content: text_content.into(),
            metadata_json: metadata_json.into(),
        }
    }

    /// Override the chunk ID.
    pub fn with_chunk_id(mut self, chunk_id: impl Into<String>) -> Self {
        self.chunk_id = chunk_id.into();
        self
    }
}

/// A scored vector similarity search result returned by the latent vector search engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorSearchResult {
    /// Unique identifier of the matching trajectory chunk.
    pub chunk_id: String,
    /// Originating session ID.
    pub session_id: String,
    /// Originating agent adapter (e.g. `claude_code`).
    pub adapter: String,
    /// Chunk type classification.
    pub kind: ChunkKind,
    /// Turn index within the session.
    #[serde(default)]
    /// Cosine similarity score (0.0 to 1.0).
    pub score: f32,
    /// Embedded text snippet.
    pub text_content: String,
    /// Turn index within the trajectory.
    #[serde(default)]
    pub turn_index: usize,
    /// Metadata payload in JSON string format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata_json: Option<String>,
    /// Timestamp when the session started (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    /// Total tokens consumed by the session.
    #[serde(default)]
    pub total_tokens: u64,
    /// Primary model used in the session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl VectorSearchResult {
    pub fn new(
        chunk_id: impl Into<String>,
        session_id: impl Into<String>,
        adapter: impl Into<String>,
        kind: ChunkKind,
        score: f32,
        text_content: impl Into<String>,
        started_at: Option<String>,
        total_tokens: u64,
        model: Option<String>,
    ) -> Self {
        Self {
            chunk_id: chunk_id.into(),
            session_id: session_id.into(),
            adapter: adapter.into(),
            kind,
            turn_index: 0,
            score,
            text_content: text_content.into(),
            metadata_json: None,
            started_at,
            total_tokens,
            model,
        }
    }

    pub fn with_turn_and_metadata(
        mut self,
        turn_index: usize,
        metadata_json: Option<String>,
    ) -> Self {
        self.turn_index = turn_index;
        self.metadata_json = metadata_json;
        self
    }
}

/// Statistical summary of the vector embeddings index.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VectorStats {
    pub total_chunks: usize,
    pub total_sessions: usize,
    pub dimensions: usize,
    #[serde(default)]
    pub index_backend: String,
    #[serde(default)]
    pub chunks_by_kind: BTreeMap<String, usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_kind_serialization_and_display() {
        assert_eq!(ChunkKind::SessionSummary.to_string(), "session_summary");
        assert_eq!(ChunkKind::ErrorRecovery.to_string(), "error_recovery");
        assert_eq!(ChunkKind::ToolInvocation.to_string(), "tool_invocation");
        assert_eq!(ChunkKind::ApologyPanic.to_string(), "apology_panic");
        assert_eq!(ChunkKind::CodeLineage.to_string(), "code_lineage");

        let serialized = serde_json::to_string(&ChunkKind::ApologyPanic).unwrap();
        assert_eq!(serialized, "\"apology_panic\"");

        let deserialized: ChunkKind = serde_json::from_str("\"apology_panic\"").unwrap();
        assert_eq!(deserialized, ChunkKind::ApologyPanic);

        assert_eq!(
            "panic".parse::<ChunkKind>().unwrap(),
            ChunkKind::ApologyPanic
        );
        assert_eq!(
            "summary".parse::<ChunkKind>().unwrap(),
            ChunkKind::SessionSummary
        );
        assert_eq!(
            "error_recovery".parse::<ChunkKind>().unwrap(),
            ChunkKind::ErrorRecovery
        );
    }

    #[test]
    fn test_trajectory_chunk_creation() {
        let chunk = TrajectoryChunk::new(
            "sess-99",
            "claude_code",
            ChunkKind::ToolInvocation,
            42,
            "2026-08-31T10:00:00Z",
            "rm -rf /tmp/build",
            r#"{"command":"rm -rf /tmp/build"}"#,
        );

        assert_eq!(chunk.chunk_id, "sess-99-tool_invocation-42");
        assert_eq!(chunk.session_id, "sess-99");
        assert_eq!(chunk.adapter, "claude_code");
        assert_eq!(chunk.kind, ChunkKind::ToolInvocation);
        assert_eq!(chunk.turn_index, 42);
        assert_eq!(chunk.text_content, "rm -rf /tmp/build");
    }

    #[test]
    fn test_trajectory_chunk_with_override_id() {
        let chunk = TrajectoryChunk::new(
            "sess-99",
            "claude_code",
            ChunkKind::ToolInvocation,
            42,
            "2026-08-31T10:00:00Z",
            "rm -rf /tmp/build",
            "{}",
        ).with_chunk_id("custom_id");

        assert_eq!(chunk.chunk_id, "custom_id");
    }

    #[test]
    fn test_vector_search_result_serde() {
        let res = VectorSearchResult::new(
            "c-1",
            "s-1",
            "claude_code",
            ChunkKind::ApologyPanic,
            0.948,
            "my mistake, deleted directory",
            Some("2026-08-22T18:53:45Z".to_string()),
            187_000_000,
            Some("claude-opus-5".to_string()),
        );

        let json = serde_json::to_string(&res).unwrap();
        let parsed: VectorSearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.chunk_id, "c-1");
        assert_eq!(parsed.score, 0.948);
        assert_eq!(parsed.model.as_deref(), Some("claude-opus-5"));
    }

    #[test]
    fn test_vector_stats_defaults() {
        let stats = VectorStats::default();
        assert_eq!(stats.total_chunks, 0);
        assert_eq!(stats.dimensions, 0);
    }
}
