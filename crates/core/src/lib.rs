//! Core orchestration engine for discovering, scanning, normalizing, and indexing agent traces.

use std::sync::Arc;

use agentworth_adapter_sdk::{AgentAdapter, ScanOptions, SessionSource};
use agentworth_adapters::{
    AiderAdapter, ClaudeCodeAdapter, ClineAdapter, CodexAdapter, CursorAdapter, DeepSeekAdapter,
    GeminiAdapter, GooseAdapter, GrokAdapter, HerdrAdapter, HermesAdapter, KimiAdapter, ManusAdapter,
    MiniMaxAdapter, OpenClawAdapter, OpenCodeAdapter, PiAdapter, QwenAdapter, WindsurfAdapter,
    ZhipuAdapter,
};
use agentworth_outcomes::{outcome_kind_name, OutcomeHierarchyDetector};
use agentworth_schema::AgentWorthTrace;
use agentworth_scoring::TraceScorer;
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
            Box::new(AiderAdapter::new()),
            Box::new(ClaudeCodeAdapter::new()),
            Box::new(ClineAdapter::new()),
            Box::new(CodexAdapter::new()),
            Box::new(CursorAdapter::new()),
            Box::new(DeepSeekAdapter::new()),
            Box::new(GeminiAdapter::new()),
            Box::new(GooseAdapter::new()),
            Box::new(GrokAdapter::new()),
            Box::new(HerdrAdapter::new()),
            Box::new(HermesAdapter::new()),
            Box::new(KimiAdapter::new()),
            Box::new(ManusAdapter::new()),
            Box::new(MiniMaxAdapter::new()),
            Box::new(OpenClawAdapter::new()),
            Box::new(OpenCodeAdapter::new()),
            Box::new(PiAdapter::new()),
            Box::new(QwenAdapter::new()),
            Box::new(WindsurfAdapter::new()),
            Box::new(ZhipuAdapter::new()),
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

    /// Read-only access to the registered adapters, e.g. for callers that need each
    /// adapter's on-disk detection/session roots without running a full scan.
    pub fn adapters(&self) -> &[Box<dyn AgentAdapter>] {
        &self.adapters
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
        // 1. Enumerate all sources across registered adapters in parallel threads
        let all_sources: Vec<(usize, SessionSource)> = std::thread::scope(|s| {
            let mut handles = Vec::with_capacity(self.adapters.len());
            for (adapter_idx, adapter) in self.adapters.iter().enumerate() {
                handles.push(s.spawn(move || {
                    let res = adapter.enumerate(options);
                    (adapter_idx, res)
                }));
            }

            let mut combined = Vec::new();
            for handle in handles {
                if let Ok((adapter_idx, res)) = handle.join() {
                    match res {
                        Ok(sources) => {
                            for src in sources {
                                combined.push((adapter_idx, src));
                            }
                        }
                        Err(e) => {
                            warn!(
                                "Adapter '{}' failed enumeration: {}",
                                self.adapters[adapter_idx].name(),
                                e
                            );
                        }
                    }
                }
            }
            combined
        });

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
                    let outcome_detector = OutcomeHierarchyDetector::new();
                    let outcomes = outcome_detector.detect_outcomes(&parse_result.trace);
                    let strongest = outcome_detector.strongest_outcome(&outcomes);
                    let primary_outcome_str = strongest.map(|o| outcome_kind_name(o.kind));

                    let scorer = TraceScorer::new();
                    let score = scorer.score(&parse_result.trace);
                    let composite_score = score.composite_score;

                    if let Err(e) = self.storage.upsert_session(
                        &parse_result.trace,
                        primary_outcome_str.as_deref(),
                        Some(composite_score),
                    ) {
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

    /// Regression test for the "blame returns nothing for Claude Code sessions" bug: a scanned
    /// session containing a real Edit tool call must be findable by `find_sessions_for_blame`.
    #[test]
    fn test_scanner_end_to_end_blame_after_claude_code_edit() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner =
            Scanner::with_adapters(vec![Box::new(ClaudeCodeAdapter::new())], storage.clone());

        let mut temp = tempfile::Builder::new()
            .suffix(".jsonl")
            .tempfile()
            .unwrap();
        let sample = r#"
{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"Fix the bug in session.rs"}
{"type":"assistant","timestamp":"2026-08-29T10:00:05Z","model":"claude-3-5-sonnet","usage":{"input_tokens":200,"output_tokens":60,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"Fixing it now."},{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"crates/core/src/session.rs","old_string":"a","new_string":"b"}}]}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: true,
        };

        let summary = scanner.run_scan(&options, |_, _| {}).expect("scan run");
        assert_eq!(summary.scanned_sessions, 1);

        let matches = storage
            .find_sessions_for_blame("session.rs")
            .expect("blame query");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].file_path, "crates/core/src/session.rs");
        assert_eq!(matches[0].action, "edit");
        assert_eq!(matches[0].model.as_deref(), Some("claude-3-5-sonnet"));

        let no_match = storage
            .find_sessions_for_blame("totally_unrelated_file.rs")
            .expect("blame query for absent file");
        assert!(no_match.is_empty());
    }

    #[test]
    fn test_scanner_outcome_detection_and_score_indexing() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner =
            Scanner::with_adapters(vec![Box::new(ClaudeCodeAdapter::new())], storage.clone());

        let mut temp = tempfile::Builder::new()
            .suffix(".jsonl")
            .tempfile()
            .unwrap();
        let sample = r#"
{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"Run tests and commit"}
{"type":"assistant","timestamp":"2026-08-29T10:00:05Z","model":"claude-3-5-sonnet","usage":{"input_tokens":300,"output_tokens":100,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"git commit -m 'feat: complete task'"}}]}
{"type":"tool_result","timestamp":"2026-08-29T10:00:07Z","tool_use_id":"t1","content":"[main 1a2b3c4] feat: complete task\n 2 files changed","is_error":false}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: true,
        };

        let summary = scanner.run_scan(&options, |_, _| {}).expect("scan run");
        assert_eq!(summary.scanned_sessions, 1);
        assert_eq!(summary.aggregate_stats.verified_outcomes_count, 1);

        let session_id = temp
            .path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();

        let session = storage.get_session_by_id(&session_id).unwrap().unwrap();
        // Encoding fix (2026-09-01): primary_outcome now stores OutcomeKind's own serde
        // snake_case form ("commit_observed"), not the old hand-rolled PascalCase
        // ("CommitObserved") — this is an intentional correction, not a weakened assertion.
        assert_eq!(session.primary_outcome.as_deref(), Some("commit_observed"));
        assert!(session.composite_score.is_some());
        assert!(session.composite_score.unwrap() > 0.0);
    }
}
