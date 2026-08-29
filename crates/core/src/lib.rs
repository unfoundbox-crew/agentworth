//! Core orchestration engine for discovering, scanning, normalizing, and indexing agent traces.

use std::sync::Arc;

use agentworth_adapter_sdk::{AgentAdapter, ScanOptions, SessionSource};
use agentworth_adapters::{ClaudeCodeAdapter, CodexAdapter, GeminiAdapter, OpenCodeAdapter};
use agentworth_schema::AgentWorthTrace;
use agentworth_storage::{AggregateStats, Storage};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{error, warn};

/// Summary report returned after completing a scan run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanSummary {
    pub discovered_sources: usize,
    pub scanned_sessions: usize,
    pub skipped_unchanged: usize,
    pub errors_encountered: usize,
    pub total_indexed_sessions: usize,
    pub aggregate_stats: AggregateStats,
}

/// Scanner orchestrator that coordinates adapters, parsing, and SQLite storage.
pub struct Scanner {
    adapters: Vec<Box<dyn AgentAdapter>>,
    storage: Arc<Storage>,
}

impl Scanner {
    /// Create a new scanner with default registered adapters.
    pub fn new(storage: Arc<Storage>) -> Self {
        let adapters: Vec<Box<dyn AgentAdapter>> = vec![
            Box::new(ClaudeCodeAdapter::new()),
            Box::new(CodexAdapter::new()),
            Box::new(GeminiAdapter::new()),
            Box::new(OpenCodeAdapter::new()),
        ];
        Self { adapters, storage }
    }

    /// Create a scanner with custom adapter list (useful for testing or selective execution).
    pub fn with_adapters(adapters: Vec<Box<dyn AgentAdapter>>, storage: Arc<Storage>) -> Self {
        Self { adapters, storage }
    }

    /// Reference to underlying storage engine.
    pub fn storage(&self) -> &Arc<Storage> {
        &self.storage
    }

    /// Load full AgentWorthTrace for a given session ID by looking up its indexed source
    /// and lazily parsing the raw session file on disk.
    pub fn load_trace(&self, session_id: &str) -> Result<AgentWorthTrace> {
        let summary = self
            .storage
            .get_session_by_id(session_id)?
            .with_context(|| {
                format!(
                    "Session '{}' not found in SQLite index. Try running 'agentworth scan' first.",
                    session_id
                )
            })?;

        let source_path = std::path::PathBuf::from(&summary.source_path);
        if !source_path.exists() {
            anyhow::bail!(
                "Source history file {:?} no longer exists on disk",
                source_path
            );
        }

        let adapter = self
            .adapters
            .iter()
            .find(|a| a.name() == summary.adapter)
            .with_context(|| format!("No adapter registered for '{}'", summary.adapter))?;

        let source = SessionSource::from_path(&source_path, adapter.name())?;
        let parse_result = adapter.parse(&source)?;
        Ok(parse_result.trace)
    }

    /// Run full scan process according to options.
    pub fn run_scan<F>(&self, options: &ScanOptions, mut on_progress: F) -> Result<ScanSummary>
    where
        F: FnMut(usize, usize),
    {
        let mut all_sources: Vec<(usize, SessionSource)> = Vec::new();

        // 1. Enumerate all sources across registered adapters
        for (adapter_idx, adapter) in self.adapters.iter().enumerate() {
            match adapter.enumerate(options) {
                Ok(sources) => {
                    for src in sources {
                        all_sources.push((adapter_idx, src));
                    }
                }
                Err(e) => {
                    warn!("Adapter '{}' failed enumeration: {}", adapter.name(), e);
                }
            }
        }

        let total = all_sources.len();
        let mut scanned_sessions = 0;
        let mut skipped_unchanged = 0;
        let mut errors_encountered = 0;

        for (idx, (adapter_idx, source)) in all_sources.iter().enumerate() {
            let adapter = &self.adapters[*adapter_idx];
            on_progress(idx + 1, total);

            if !options.force {
                match self.storage.should_scan_source(source) {
                    Ok(false) => {
                        skipped_unchanged += 1;
                        continue;
                    }
                    Err(e) => {
                        warn!("Failed checking cache for {:?}: {}", source.path, e);
                    }
                    _ => {}
                }
            }

            // Parse session
            match adapter.parse(source) {
                Ok(parse_result) => {
                    if let Err(e) = self.storage.upsert_trace(&parse_result.trace) {
                        error!("Failed storing trace for {:?}: {}", source.path, e);
                        errors_encountered += 1;
                    } else {
                        scanned_sessions += 1;
                    }
                }
                Err(e) => {
                    error!("Failed parsing {:?}: {}", source.path, e);
                    errors_encountered += 1;
                }
            }
        }

        let aggregate_stats = self.storage.get_aggregate_stats()?;
        let total_indexed_sessions = aggregate_stats.total_sessions;

        Ok(ScanSummary {
            discovered_sources: total,
            scanned_sessions,
            skipped_unchanged,
            errors_encountered,
            total_indexed_sessions,
            aggregate_stats,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_scanner_end_to_end_with_in_memory_storage() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner =
            Scanner::with_adapters(vec![Box::new(ClaudeCodeAdapter::new())], storage.clone());

        let mut temp = tempfile::Builder::new()
            .suffix(".jsonl")
            .tempfile()
            .unwrap();
        let sample = r#"
{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"Build an app"}
{"type":"assistant","timestamp":"2026-08-29T10:00:05Z","model":"claude-3-5-sonnet","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"Done"}]}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
        };

        // First scan: should scan 1 session
        let summary = scanner.run_scan(&options, |_, _| {}).expect("scan run");

        assert_eq!(summary.discovered_sources, 1);
        assert_eq!(summary.scanned_sessions, 1);
        assert_eq!(summary.skipped_unchanged, 0);
        assert_eq!(summary.total_indexed_sessions, 1);
        assert_eq!(summary.aggregate_stats.token_usage.total(), 150);

        // Test loading the trace lazily
        let session_id = temp
            .path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let loaded = scanner.load_trace(&session_id).expect("load trace");
        assert_eq!(loaded.stats.token_usage.total(), 150);
        assert_eq!(loaded.events.len(), 3);

        // Second scan without force: should skip unchanged
        let summary2 = scanner.run_scan(&options, |_, _| {}).expect("scan run 2");

        assert_eq!(summary2.discovered_sources, 1);
        assert_eq!(summary2.scanned_sessions, 0);
        assert_eq!(summary2.skipped_unchanged, 1);
        assert_eq!(summary2.total_indexed_sessions, 1);
    }
}
