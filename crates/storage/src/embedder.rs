//! Local 384-dimensional vector embedding inference engine.
//!
//! Powered by FastEmbed (`BAAI/bge-small-en-v1.5` / `sentence-transformers/all-MiniLM-L6-v2`)
//! with a deterministic zero-network offline fallback engine.

use anyhow::Result;

#[cfg(feature = "fastembed")]
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
#[cfg(feature = "fastembed")]
use tracing::{info, warn};

/// The default embedding dimension for BGE-Small and all-MiniLM models.
pub const EMBEDDING_DIMENSION: usize = 384;

/// Backend engine used for generating vector embeddings.
enum EmbedderBackend {
    #[cfg(feature = "fastembed")]
    Onnx(Box<TextEmbedding>),
    DeterministicFallback,
}

/// Local embedding generator for semantic trajectory chunks.
pub struct LocalEmbedder {
    backend: EmbedderBackend,
    dimension: usize,
}

impl LocalEmbedder {
    /// Initialize local embedder with ONNX acceleration (BGE-Small-EN-v1.5),
    /// gracefully falling back to deterministic local vectors if ONNX models cannot be loaded offline.
    pub fn new() -> Self {
        #[cfg(feature = "fastembed")]
        {
            // `fastembed` only reaches the network the first time a given model is
            // missing from its local cache; every run after that loads from disk. This
            // crate has no visibility into that cache from here, so the line below says
            // "may fetch" rather than "will fetch" -- still strictly more honest than
            // the previous silence. `with_show_download_progress(false)` stays off on
            // purpose: fastembed's own indicatif bar would race the CLI's own status
            // spinner (`apps/cli/src/commands/search.rs`, `with_status`) for the same
            // terminal line. One clear line, printed once before the attempt, replaces it.
            warn!("{}", model_fetch_announcement());

            // Attempt 1: BGE-Small-EN-v1.5 (highest accuracy 384-dim)
            let bge_opts = InitOptions::new(EmbeddingModel::BGESmallENV15)
                .with_show_download_progress(false);

            match TextEmbedding::try_new(bge_opts) {
                Ok(model) => {
                    info!("Initialized local ONNX BGESmallENV15 embedder (384-dim)");
                    return Self {
                        backend: EmbedderBackend::Onnx(Box::new(model)),
                        dimension: EMBEDDING_DIMENSION,
                    };
                }
                Err(err1) => {
                    warn!("BGESmallENV15 initialization skipped ({}). Trying AllMiniLML6V2...", err1);
                    // Attempt 2: AllMiniLML6V2
                    let minilm_opts = InitOptions::new(EmbeddingModel::AllMiniLML6V2)
                        .with_show_download_progress(false);
                    match TextEmbedding::try_new(minilm_opts) {
                        Ok(model) => {
                            info!("Initialized local ONNX AllMiniLML6V2 embedder (384-dim)");
                            return Self {
                                backend: EmbedderBackend::Onnx(Box::new(model)),
                                dimension: EMBEDDING_DIMENSION,
                            };
                        }
                        Err(err2) => {
                            warn!("{}", model_unavailable_message(&err2.to_string()));
                        }
                    }
                }
            }
        }

        Self::new_deterministic()
    }

    /// Construct a deterministic offline embedder with zero network/file dependencies.
    pub fn new_deterministic() -> Self {
        Self {
            backend: EmbedderBackend::DeterministicFallback,
            dimension: EMBEDDING_DIMENSION,
        }
    }

    /// Returns true if running via real ONNX runtime.
    pub fn is_onnx(&self) -> bool {
        match &self.backend {
            #[cfg(feature = "fastembed")]
            EmbedderBackend::Onnx(_) => true,
            _ => false,
        }
    }

    /// Embedding dimension size (384).
    pub fn dimension(&self) -> usize {
        self.dimension
    }

    /// Generate embeddings for a batch of text strings.
    pub fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        match &self.backend {
            #[cfg(feature = "fastembed")]
            EmbedderBackend::Onnx(model) => {
                let texts_vec: Vec<String> = texts.to_vec();
                let embeddings = model.embed(texts_vec, None)?;
                Ok(embeddings)
            }
            EmbedderBackend::DeterministicFallback => {
                let mut embeddings = Vec::with_capacity(texts.len());
                for t in texts {
                    embeddings.push(deterministic_hash_embedding(t, self.dimension));
                }
                Ok(embeddings)
            }
        }
    }

    /// Generate an embedding vector for a single text string.
    pub fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        let batch = vec![text.to_string()];
        let results = self.embed_batch(&batch)?;
        Ok(results.into_iter().next().unwrap_or_else(|| vec![0.0; self.dimension]))
    }
}

impl Default for LocalEmbedder {
    fn default() -> Self {
        Self::new()
    }
}

/// Deterministic, normalized 384-dimensional feature embedding for offline environments.
///
/// Uses word n-grams and hashing to project text into a normalized hypersphere.
pub fn deterministic_hash_embedding(text: &str, dim: usize) -> Vec<f32> {
    let mut vec = vec![0.0f32; dim];
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();

    if words.is_empty() {
        // Return unit vector along first dimension
        vec[0] = 1.0;
        return vec;
    }

    // 1. Unigram feature hashing
    for word in &words {
        let hash = sha256_hash_str(word);
        let idx = (hash as usize) % dim;
        vec[idx] += 1.0 + (word.len() as f32).min(8.0) * 0.2;
    }

    // 2. Bigram feature hashing
    for window in words.windows(2) {
        let bigram = format!("{}_{}", window[0], window[1]);
        let hash = sha256_hash_str(&bigram);
        let idx = (hash as usize) % dim;
        vec[idx] += 2.0;
    }

    // 3. Character trigrams for typo-resilient semantic overlap
    let chars: Vec<char> = lower.chars().collect();
    for window in chars.windows(3) {
        let trigram: String = window.iter().collect();
        let hash = sha256_hash_str(&trigram);
        let idx = (hash as usize) % dim;
        vec[idx] += 0.3;
    }

    // L2 Normalize
    let mut norm = 0.0f32;
    for v in &vec {
        norm += v * v;
    }
    if norm > 0.0 {
        let sqrt_norm = norm.sqrt();
        for v in &mut vec {
            *v /= sqrt_norm;
        }
    } else {
        vec[0] = 1.0;
    }

    vec
}

fn sha256_hash_str(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// One-line, one-time announcement printed (via `tracing::warn!`, visible at the CLI's
/// default log level) before `LocalEmbedder::new()` attempts to initialize the ONNX
/// backend. Only reachable when this crate is built with the `fastembed` feature --
/// off by default in every shipped `agwt`/`archie`/`agentworth` binary today (checked:
/// `crates/storage/Cargo.toml` has `default = []`, and `apps/cli/Cargo.toml` depends on
/// `agentworth-storage` with `default-features = false`), so this only fires for a
/// build someone has explicitly opted into with `--features fastembed`.
///
/// Names the model, where it comes from, roughly how big it is, that it's a one-time
/// dependency fetch (not telemetry), and that no local data is sent -- the four things
/// AGENTS.md item 8 says a silent download must announce.
#[allow(dead_code)] // only called from the fastembed-feature-gated path in `new()`; kept
                     // testable unconditionally (see the tests module) since the feature
                     // is off in every shipped binary today
fn model_fetch_announcement() -> String {
    "Preparing local semantic search: may fetch BAAI/bge-small-en-v1.5 (~133MB) from \
     Hugging Face if it isn't already cached at ~/.cache/fastembed. This is a one-time \
     dependency fetch, the same kind of download as `npm install` -- no session data, \
     prompts, or code leave this machine."
        .to_string()
}

/// Wraps a `fastembed` initialization error with the same four facts as
/// [`model_fetch_announcement`], plus what happens next: silent fallback to the
/// deterministic offline embedder. Called only after both the BGE-Small and
/// AllMiniLM attempts have failed.
#[allow(dead_code)] // see model_fetch_announcement
fn model_unavailable_message(cause: &str) -> String {
    format!(
        "Could not load BAAI/bge-small-en-v1.5 or sentence-transformers/all-MiniLM-L6-v2 \
         from Hugging Face ({cause}). No local data was sent in that attempt. Falling back \
         to AgentWorth's offline deterministic embedder (weaker semantic matching, zero \
         network, works air-gapped)."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector::cosine_similarity;

    #[test]
    fn test_deterministic_embedding_dimension_and_norm() {
        let embedder = LocalEmbedder::new_deterministic();
        assert_eq!(embedder.dimension(), 384);

        let emb = embedder.embed_text("Claude panicked and executed rm -rf on repo").expect("embed");
        assert_eq!(emb.len(), 384);

        // Verify L2 norm is ~1.0
        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_semantic_similarity_relative_ordering() {
        let embedder = LocalEmbedder::new_deterministic();

        let target = embedder.embed_text("rm -rf repository deletion error").expect("embed");
        let similar = embedder.embed_text("rm -rf repository deleted by accident").expect("embed");
        let dissimilar = embedder.embed_text("react tailwind button component UI color").expect("embed");

        let score_similar = cosine_similarity(&target, &similar);
        let score_dissimilar = cosine_similarity(&target, &dissimilar);

        assert!(
            score_similar > score_dissimilar,
            "Similar score ({}) should be higher than dissimilar score ({})",
            score_similar,
            score_dissimilar
        );
    }

    // These two tests exercise the user-facing message text directly rather than the
    // network path itself. The download can't be red/green tested offline -- there is
    // no way to force `fastembed::TextEmbedding::try_new` to attempt (and fail to reach)
    // Hugging Face without an actual network condition to assert against, and this repo
    // has no fixture for that. What *is* testable without a network, and is exactly what
    // AGENTS.md item 8 asks the copy to say: the announcement and the fallback message
    // both name the model, its source, its rough size (announcement only -- the failure
    // message doesn't re-assert size), that it's a one-time dependency fetch, and that
    // no local data is sent.

    #[test]
    fn model_fetch_announcement_names_model_source_size_and_privacy() {
        let msg = model_fetch_announcement();
        assert!(msg.contains("bge-small-en-v1.5"), "names the model: {msg}");
        assert!(msg.contains("Hugging Face"), "names the source: {msg}");
        assert!(msg.contains("133MB"), "names the rough size: {msg}");
        assert!(
            msg.contains("one-time"),
            "says it's a one-time dependency fetch, not telemetry: {msg}"
        );
        assert!(
            msg.contains("no session data") || msg.contains("no local data") || msg.to_lowercase().contains("no session data, prompts, or code leave"),
            "says nothing about the user is sent: {msg}"
        );
    }

    #[test]
    fn model_unavailable_message_names_models_cause_and_fallback() {
        let msg = model_unavailable_message("dns resolution failed");
        assert!(msg.contains("bge-small-en-v1.5"), "names the primary model tried: {msg}");
        assert!(msg.contains("all-MiniLM-L6-v2"), "names the second model tried: {msg}");
        assert!(msg.contains("Hugging Face"), "names the source: {msg}");
        assert!(msg.contains("dns resolution failed"), "carries the underlying cause: {msg}");
        assert!(
            msg.contains("No local data was sent"),
            "says nothing about the user was sent even on failure: {msg}"
        );
        assert!(
            msg.contains("deterministic") && msg.contains("offline"),
            "says what happens next -- offline deterministic fallback: {msg}"
        );
    }
}
