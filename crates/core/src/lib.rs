//! Core orchestration engine for discovering, scanning, normalizing, and indexing agent traces.

use std::collections::HashSet;
use std::sync::Arc;

use agentworth_adapter_sdk::{AgentAdapter, ScanOptions, SessionSource};
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
    /// Unchanged-source rows reparsed anyway because they were missing a field derived
    /// from the source content (e.g. `prompt_preview`, added in v0.1.10 after these rows
    /// were first indexed). See `Storage::needs_backfill`.
    pub backfilled_sessions: usize,
    pub errors_encountered: usize,
    pub total_indexed_sessions: usize,
    pub aggregate_stats: AggregateStats,
    /// Previously-indexed sessions with zero events and zero tokens whose source no
    /// longer passes any registered adapter's current detection, removed during this
    /// scan. Only computed on a full (unscoped) scan -- see `run_scan`.
    pub stub_sessions_removed: usize,
}

/// Scanner orchestrator that coordinates adapters, parsing, and SQLite storage.
pub struct Scanner {
    adapters: Vec<Box<dyn AgentAdapter>>,
    storage: Arc<Storage>,
}

impl Scanner {
    /// Create a new scanner with every adapter in `agentworth_adapters::all_adapters()` --
    /// the canonical registry, so a newly-added adapter is picked up here automatically.
    pub fn new(storage: Arc<Storage>) -> Self {
        Self { adapters: agentworth_adapters::all_adapters(), storage }
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

    /// Deletes indexed sessions with zero events and zero tokens whose `(adapter,
    /// source_path)` is not in `valid_sources` -- i.e. sessions no adapter's current
    /// detection would produce again. Returns the number removed; individual delete
    /// failures are logged and skipped rather than aborting the pass.
    fn prune_zero_activity_stubs(&self, valid_sources: &HashSet<(&str, String)>) -> usize {
        let mut removed = 0;
        match self.storage.zero_activity_sessions() {
            Ok(stubs) => {
                for (session_id, adapter_name, source_path) in stubs {
                    let key = (adapter_name.as_str(), source_path.clone());
                    if valid_sources.contains(&key) {
                        continue;
                    }
                    match self.storage.delete_session(&session_id, &source_path) {
                        Ok(()) => removed += 1,
                        Err(e) => warn!("Failed deleting stub session {}: {}", session_id, e),
                    }
                }
                if removed > 0 {
                    tracing::info!(
                        "Removed {} zero-activity stub session(s) whose source no longer passes detection",
                        removed
                    );
                }
            }
            Err(e) => warn!("Failed listing zero-activity sessions for cleanup: {}", e),
        }
        removed
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
        let mut backfilled_sessions = 0;
        let mut errors_encountered = 0;

        for (idx, (adapter_idx, source)) in all_sources.iter().enumerate() {
            let adapter = &self.adapters[*adapter_idx];
            on_progress(idx + 1, total);

            let mut is_backfill = false;
            if !options.force {
                match self.storage.should_scan_source(source) {
                    Ok(false) => {
                        // Unchanged content, but the indexed row may predate a
                        // derived-field extractor (e.g. prompt_preview). Reparse once
                        // to backfill rather than skipping forever.
                        let path_str = source.path.to_string_lossy();
                        match self.storage.needs_backfill(&path_str) {
                            Ok(true) => {
                                is_backfill = true;
                            }
                            Ok(false) => {
                                skipped_unchanged += 1;
                                continue;
                            }
                            Err(e) => {
                                warn!(
                                    "Failed checking backfill status for {:?}: {}",
                                    source.path, e
                                );
                                skipped_unchanged += 1;
                                continue;
                            }
                        }
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
                    // A file that normalizes to zero events is not a session -- most
                    // often a non-session file an adapter's discovery still let through
                    // (config, cache, telemetry). Skip storing it rather than indexing a
                    // permanent zero-activity stub; see `zero_activity_sessions` for the
                    // matching cleanup of rows already stored under looser past detection.
                    if parse_result.trace.events.is_empty() {
                        if is_backfill {
                            // The row still has no derivable prompt_preview (no user
                            // message in this source), so it will be retried next scan
                            // too -- see the doc comment on `needs_backfill`.
                            skipped_unchanged += 1;
                        }
                        continue;
                    }

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
                    } else if is_backfill {
                        backfilled_sessions += 1;
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

        // Prune zero-activity stub sessions (config files, telemetry dumps, ... that an
        // adapter's discovery previously accepted) whose source no longer passes any
        // registered adapter's current detection. Only run on a full, unscoped scan:
        // `all_sources` is the complete set of currently-valid sources in that case, so
        // "not in it" reliably means "detection no longer accepts this file" rather than
        // "outside today's narrower --path scope."
        let stub_sessions_removed = if options.custom_paths.is_empty() {
            let valid_sources: HashSet<(&str, String)> = all_sources
                .iter()
                .map(|(adapter_idx, source)| {
                    (
                        self.adapters[*adapter_idx].name(),
                        source.path.to_string_lossy().to_string(),
                    )
                })
                .collect();
            self.prune_zero_activity_stubs(&valid_sources)
        } else {
            0
        };

        // `true`: this summary's own "Total Indexed" / "N total in index" labels (main.rs,
        // static_files.rs) promise a raw count of everything in the SQLite index, stubs
        // included -- not a "real activity" count. See get_aggregate_stats's doc comment.
        let aggregate_stats = self.storage.get_aggregate_stats(true)?;
        let total_indexed_sessions = aggregate_stats.total_sessions;

        Ok(ScanSummary {
            discovered_sources: total,
            scanned_sessions,
            skipped_unchanged,
            backfilled_sessions,
            errors_encountered,
            total_indexed_sessions,
            aggregate_stats,
            stub_sessions_removed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_adapters::ClaudeCodeAdapter;
    use agentworth_schema::Provenance;
    use chrono::Utc;
    use std::io::Write;

    /// A file that discovery still lets through but which normalizes to zero events
    /// (here: empty content) must not be stored as a session at all -- the scanner should
    /// skip it rather than index a permanent zero-activity stub. This is the parse-time
    /// backstop behind the "reject non-session rows" fix; adapter-level discovery
    /// tightening is the primary fix, this is defense in depth for whatever still slips
    /// through.
    #[test]
    fn test_scanner_does_not_store_zero_event_session() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner =
            Scanner::with_adapters(vec![Box::new(ClaudeCodeAdapter::new())], storage.clone());

        let temp = tempfile::Builder::new()
            .suffix(".jsonl")
            .tempfile()
            .unwrap();
        // Deliberately empty: zero bytes -> zero normalized events.

        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
        };

        let summary = scanner.run_scan(&options, |_, _| {}).expect("scan run");

        assert_eq!(summary.discovered_sources, 1);
        assert_eq!(summary.scanned_sessions, 0, "a zero-event file must not count as scanned");
        assert_eq!(summary.total_indexed_sessions, 0, "a zero-event file must not be stored");
    }

    /// Item-4 cleanup: a zero-activity row left over from before an adapter's discovery
    /// was tightened (or whose source file was simply deleted) should be pruned once its
    /// `(adapter, source_path)` no longer appears among currently-valid sources. A row
    /// still backed by a currently-valid source must survive.
    #[test]
    fn test_prune_zero_activity_stubs_removes_only_undetectable_rows() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner =
            Scanner::with_adapters(vec![Box::new(ClaudeCodeAdapter::new())], storage.clone());

        let stale_provenance = Provenance::new(
            "/tmp/agentworth-test-stub/config.json".to_string(),
            "claude_code",
            12,
            0,
            "deadbeef",
        );
        let stale_trace = AgentWorthTrace::new("stale-stub", "claude_code", stale_provenance, Utc::now());
        storage.upsert_session(&stale_trace, None, None).expect("seed stale stub");

        let live_provenance = Provenance::new(
            "/tmp/agentworth-test-stub/still-valid.jsonl".to_string(),
            "claude_code",
            12,
            0,
            "deadbeef",
        );
        let live_trace = AgentWorthTrace::new("live-stub", "claude_code", live_provenance, Utc::now());
        storage.upsert_session(&live_trace, None, None).expect("seed live stub");

        assert_eq!(storage.get_aggregate_stats(true).unwrap().total_sessions, 2);

        let mut valid_sources = HashSet::new();
        valid_sources.insert(("claude_code", "/tmp/agentworth-test-stub/still-valid.jsonl".to_string()));

        let removed = scanner.prune_zero_activity_stubs(&valid_sources);

        assert_eq!(removed, 1);
        let remaining = storage.get_aggregate_stats(true).unwrap();
        assert_eq!(remaining.total_sessions, 1);
        assert!(storage.get_session_by_id("live-stub").unwrap().is_some());
        assert!(storage.get_session_by_id("stale-stub").unwrap().is_none());
    }

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

    /// Regression test for the real-index bug (handoff.md, #71): `prompt_preview` was empty
    /// in all 2,960 non-stub rows because `should_scan_source` treats an unchanged source as
    /// permanently skippable, so a session indexed before v0.1.10 added prompt_preview
    /// extraction never got rescanned to pick it up. The fix backfills such rows once, on
    /// their next scan, even though their source is unchanged.
    #[test]
    fn test_scanner_backfills_missing_prompt_preview_on_unchanged_source() {
        use agentworth_schema::TokenUsage;

        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner =
            Scanner::with_adapters(vec![Box::new(ClaudeCodeAdapter::new())], storage.clone());

        let mut temp = tempfile::Builder::new()
            .suffix(".jsonl")
            .tempfile()
            .unwrap();
        let sample = r#"
{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"fix the flaky test"}
{"type":"assistant","timestamp":"2026-08-29T10:00:05Z","model":"claude-3-5-sonnet","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"Done"}]}
"#;
        temp.write_all(sample.as_bytes()).unwrap();

        let session_id = temp
            .path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();

        // Seed a row with the exact provenance (size/mtime/fingerprint) the real file on disk
        // already has, but with no prompt_preview -- exactly the shape left behind by a scan
        // that ran before v0.1.10 added the extractor. `should_scan_source` alone would treat
        // this source as unchanged forever.
        let source = SessionSource::from_path(temp.path(), "claude_code").expect("source");
        let provenance = Provenance::new(
            source.path.to_string_lossy().to_string(),
            "claude_code",
            source.file_size_bytes,
            source.mtime_epoch_secs,
            &source.fingerprint,
        );
        let mut stale_trace = AgentWorthTrace::new(&session_id, "claude_code", provenance, Utc::now());
        stale_trace.stats.total_events = 2;
        stale_trace.stats.token_usage = TokenUsage::new(100, 50, 0, 0);
        storage.upsert_session(&stale_trace, None, None).expect("seed stale row");

        let seeded = storage.get_session_by_id(&session_id).unwrap().unwrap();
        assert!(
            seeded.prompt_preview.as_deref().unwrap_or("").is_empty(),
            "seeded row must start with an empty prompt_preview to reproduce the bug"
        );

        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
        };

        // Backfill scan: source is unchanged, but the row is missing prompt_preview, so it
        // must be reparsed rather than skipped.
        let summary = scanner.run_scan(&options, |_, _| {}).expect("backfill scan");
        assert_eq!(summary.scanned_sessions, 0, "not a fresh/changed source");
        assert_eq!(summary.skipped_unchanged, 0, "must not be silently skipped forever");
        assert_eq!(summary.backfilled_sessions, 1);

        let backfilled = storage.get_session_by_id(&session_id).unwrap().unwrap();
        assert_eq!(backfilled.prompt_preview.as_deref(), Some("fix the flaky test"));

        // Steady-state scan: fields are now complete and the source is still unchanged, so
        // this must go back to a plain skip, not a repeated backfill.
        let summary2 = scanner.run_scan(&options, |_, _| {}).expect("steady-state scan");
        assert_eq!(summary2.scanned_sessions, 0);
        assert_eq!(summary2.backfilled_sessions, 0, "a complete row must not be rescanned");
        assert_eq!(summary2.skipped_unchanged, 1);
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
