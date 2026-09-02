//! Vector storage traits, math utilities, and backend implementations.

pub mod sqlite_store;

use std::collections::HashSet;

use agentworth_schema::vector::{ChunkKind, TrajectoryChunk, VectorSearchResult, VectorStats};
use anyhow::Result;

pub use sqlite_store::SqliteVectorStore;

/// Pluggable Vector Store trait for indexing and semantic similarity search across trajectory chunks.
pub trait VectorStore: Send + Sync {
    /// Initialize tables, virtual tables, or collections.
    fn initialize(&self) -> Result<()>;

    /// Upsert chunk embeddings into the index.
    fn insert_embeddings(
        &self,
        chunks: &[TrajectoryChunk],
        embeddings: &[Vec<f32>],
    ) -> Result<()>;

    /// Query similar trajectory chunks using cosine similarity.
    fn search(
        &self,
        query_vector: &[f32],
        limit: usize,
        min_score: f32,
    ) -> Result<Vec<VectorSearchResult>> {
        self.search_filtered(query_vector, limit, min_score, None, None)
    }

    /// Query similar trajectory chunks with optional adapter and kind filtering.
    fn search_filtered(
        &self,
        query_vector: &[f32],
        limit: usize,
        min_score: f32,
        adapter_filter: Option<&str>,
        kind_filter: Option<ChunkKind>,
    ) -> Result<Vec<VectorSearchResult>>;

    /// Delete all vectors associated with a session (for rescans/updates).
    fn delete_session(&self, session_id: &str) -> Result<()>;

    /// Session IDs that already have at least one embedded chunk stored.
    ///
    /// Lets a caller do incremental / resumable indexing: diff this against the full
    /// session list and embed only what's missing, instead of a one-shot bootstrap
    /// that never revisits sessions added (or left uncapped) after the first run.
    fn indexed_session_ids(&self) -> Result<HashSet<String>>;

    /// Return total vector count and index statistics.
    fn stats(&self) -> Result<VectorStats>;
}

/// Compute cosine similarity between two float vectors.
///
/// Cosine similarity: (A · B) / (||A|| * ||B||)
/// Returns a value between -1.0 and 1.0 (or 0.0 if either norm is zero).
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;

    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    if norm_a <= 0.0 || norm_b <= 0.0 {
        return 0.0;
    }

    let sim = dot / (norm_a.sqrt() * norm_b.sqrt());
    // Clamp to valid range [-1.0, 1.0] to guard against floating-point inaccuracy
    sim.clamp(-1.0, 1.0)
}

/// Serialize a slice of f32 to raw little-endian bytes for SQLite BLOB storage.
pub fn f32_slice_to_bytes(vec: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vec.len() * 4);
    for val in vec {
        bytes.extend_from_slice(&val.to_le_bytes());
    }
    bytes
}

/// Deserialize raw little-endian bytes from SQLite BLOB storage into a Vec<f32>.
pub fn bytes_to_f32_vec(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let v1 = vec![1.0, 2.0, 3.0, 4.0];
        let v2 = vec![1.0, 2.0, 3.0, 4.0];
        let sim = cosine_similarity(&v1, &v2);
        assert!((sim - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let v1 = vec![1.0, 0.0, 0.0];
        let v2 = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&v1, &v2);
        assert!(sim.abs() < 1e-5);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let v1 = vec![1.0, 2.0, 3.0];
        let v2 = vec![-1.0, -2.0, -3.0];
        let sim = cosine_similarity(&v1, &v2);
        assert!((sim - (-1.0)).abs() < 1e-5);
    }

    #[test]
    fn test_f32_byte_roundtrip() {
        let original = vec![0.1234f32, -56.78f32, 999.0f32, 0.0f32];
        let bytes = f32_slice_to_bytes(&original);
        let restored = bytes_to_f32_vec(&bytes);
        assert_eq!(original, restored);
    }
}
