//! Core orchestration engine for discovering, scanning, normalizing, and indexing agent traces.

use std::collections::HashSet;
use std::sync::Arc;

use agentworth_adapter_sdk::{AgentAdapter, ScanOptions, SessionSource};
use agentworth_outcomes::loops::{
    evaluate_trace_for_loops, LoopAlertKind, LoopResolution, DEFAULT_MAX_FILE_REVISIONS,
    DEFAULT_MAX_TOOL_REPEATS,
};
use agentworth_outcomes::{outcome_kind_name, outcome_rank, OutcomeHierarchyDetector, RecoveryDetector};
use agentworth_schema::AgentWorthTrace;
use agentworth_scoring::TraceScorer;
use agentworth_storage::{
    is_near_empty_session, AggregateStats, BackfillReason, DemotedClaim, LoopEvidence, SessionRisk,
    Storage, SESSION_RISK_EVIDENCE_CAP,
};
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
    /// Unchanged-source rows reparsed because the adapter's `parser_version` has moved past
    /// the version stored on the row -- the parse output itself changed, so the indexed
    /// answer is stale even though the file is not. See `AgentAdapter::parser_version`.
    pub reparsed_sessions: usize,
    /// Rows that would otherwise need a backfill or reparse, but whose source lives on
    /// another machine's disk (typically arrived via `agentworth merge`) and so can't be
    /// re-read from here. See `agentworth_storage::BackfillReason::SourceUnavailable`.
    pub sources_unavailable: usize,
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

        let adapter = self
            .adapters
            .iter()
            .find(|a| a.name() == summary.adapter)
            .with_context(|| format!("No adapter registered for '{}'", summary.adapter))?;

        // Rebuild the exact `SessionSource` this session was last scanned with, from the
        // index itself, rather than re-stat'ing `summary.source_path` on disk. A plain
        // `std::fs::metadata` stat (what `SessionSource::from_path` does) only works for a
        // source whose identity string is a real file path -- it always fails for a virtual
        // source (e.g. opencode's `<repo>/.opencode/session-<id>.db::opencode-repo::<db_path>#<id>`
        // identity, which never exists as a literal path even though the session is fully
        // present in its backing SQLite store). `Storage::get_source_metadata` returns the
        // (file_size, mtime, fingerprint) this same source was indexed with, straight from the
        // `sources` table, keyed by the identical identity string -- no filesystem access needed.
        let (file_size_bytes, mtime_epoch_secs, fingerprint) = self
            .storage
            .get_source_metadata(&summary.source_path)?
            .with_context(|| {
                format!(
                    "Session '{}' has no indexed source record for {:?}",
                    session_id, summary.source_path
                )
            })?;
        let source = SessionSource {
            path: std::path::PathBuf::from(&summary.source_path),
            adapter_name: adapter.name().to_string(),
            file_size_bytes,
            mtime_epoch_secs,
            fingerprint,
        };

        if !adapter.source_exists(&source) {
            anyhow::bail!(
                "Source history for session '{}' ({:?}) no longer exists",
                session_id,
                summary.source_path
            );
        }

        let parse_result = adapter.parse(&source)?;
        Ok(parse_result.trace)
    }

    /// Deletes indexed sessions with at most one normalized event (see
    /// `agentworth_storage::Storage::stub_sessions`) whose `(adapter, source_path)` is not in
    /// `valid_sources` -- i.e. sessions no adapter's current detection would produce again.
    /// Broader than the original zero-events-only check (a row with exactly one event is also
    /// this thin, and previously survived this pass), but keeps #68's restriction to
    /// undetectable sources: a source still actively enumerated is presumed a real, if thin,
    /// session and left alone rather than deleted out from under the index. Returns the
    /// number removed; individual delete
    /// failures are logged and skipped rather than aborting the pass.
    fn prune_stub_sessions(&self, valid_sources: &HashSet<(&str, String)>) -> usize {
        let mut removed = 0;
        match self.storage.stub_sessions() {
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
                        "Removed {} stub session(s) whose source no longer passes detection",
                        removed
                    );
                }
            }
            Err(e) => warn!("Failed listing stub sessions for cleanup: {}", e),
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
        let mut reparsed_sessions = 0;
        let mut sources_unavailable = 0;
        let mut errors_encountered = 0;

        for (idx, (adapter_idx, source)) in all_sources.iter().enumerate() {
            let adapter = &self.adapters[*adapter_idx];
            on_progress(idx + 1, total);

            let mut backfill_reason = None;
            if !options.force {
                match self.storage.should_scan_source(source) {
                    Ok(false) => {
                        // Unchanged content, but the indexed row may predate a
                        // derived-field extractor (e.g. prompt_preview), or have been
                        // produced by an older version of this adapter's parser. Reparse
                        // once rather than skipping forever.
                        let path_str = source.path.to_string_lossy();
                        match self.storage.needs_backfill(
                            &path_str,
                            adapter.parser_version(),
                            adapter.source_exists(source),
                        ) {
                            // Reachable here whenever `adapter.source_exists(source)` says no
                            // even though `source` came from this very `enumerate()` pass --
                            // e.g. an opencode session whose backing `opencode.db` row was
                            // deleted between enumeration and this check. Rare, but
                            // `needs_backfill` is a general predicate and a merged row reached
                            // some other way must not be treated as a normal reparse either
                            // (there is nothing local to parse it from).
                            Ok(Some(BackfillReason::SourceUnavailable)) => {
                                sources_unavailable += 1;
                                continue;
                            }
                            Ok(Some(reason)) => {
                                backfill_reason = Some(reason);
                            }
                            Ok(None) => {
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
                    // permanent zero-activity stub, regardless of `include_stubs`: this
                    // isn't a thin-but-real session, it's not a session at all.
                    if parse_result.trace.events.is_empty() {
                        if backfill_reason.is_some() {
                            // The row still has no derivable prompt_preview (no user
                            // message in this source), so it will be retried next scan
                            // too -- see the doc comment on `needs_backfill`.
                            skipped_unchanged += 1;
                        }
                        continue;
                    }

                    // Beyond that parse-time backstop, also skip a session with exactly one
                    // normalized event and nothing more -- still not a real conversation, just
                    // one turn short of the zero-event case above. Deliberately narrower than
                    // `agentworth stats`/`usage`'s own stub definition (`NON_STUB_SQL_PREDICATE`
                    // also requires zero tokens): dropping a row from the index entirely is a
                    // much bigger stakes than excluding it from an aggregate count, and a real,
                    // multi-event session that simply has no captured token telemetry (an
                    // adapter that doesn't report usage, or a model invocation whose response
                    // never included one) must stay indexed -- `agentworth audit`/`autopsy`/
                    // `blind-spots` all depend on exactly that population, querying with
                    // `include_stubs: true` to see it. See `stub_sessions` for the matching
                    // cleanup of rows already indexed under looser past filtering.
                    // `ScanOptions::include_stubs` is the explicit opt-out for callers that
                    // want even one-event rows kept.
                    if !options.include_stubs && is_near_empty_session(parse_result.trace.stats.total_events) {
                        continue;
                    }

                    // Every session that gets stored gets both passes, unconditionally --
                    // there is no path here that indexes a row without running outcome
                    // detection and scoring over it. A NULL `primary_outcome` therefore means
                    // "the detector found no outcome evidence in this trace", never "this
                    // session was never looked at"; a NULL `composite_score` can only be a row
                    // written by something other than a scan, and `needs_backfill` now pulls
                    // any such row back through here on the next scan.
                    let outcome_detector = OutcomeHierarchyDetector::new();
                    // `detect_outcomes` is this same call with the notes thrown away, so
                    // keeping them costs nothing -- the verification pass already ran.
                    let (outcomes, verification_notes) =
                        outcome_detector.detect_outcomes_with_verification(&parse_result.trace);
                    let strongest = outcome_detector.strongest_outcome(&outcomes);
                    let primary_outcome_str = strongest.map(|o| outcome_kind_name(o.kind));

                    let scorer = TraceScorer::new();
                    let score = scorer.score(&parse_result.trace);
                    let composite_score = score.composite_score;

                    let risk = compute_session_risk(&parse_result.trace, &verification_notes);

                    if let Err(e) = self.storage.upsert_session(
                        &parse_result.trace,
                        primary_outcome_str.as_deref(),
                        Some(composite_score),
                        adapter.parser_version(),
                    ) {
                        error!("Failed storing trace for {:?}: {}", source.path, e);
                        errors_encountered += 1;
                    } else {
                        // The session row is stored and useful on its own; a failed risk write
                        // leaves that session reading as "not yet scanned for risk", which is
                        // exactly what it then is. It must not fail the session's own indexing.
                        if let Err(e) = self.storage.upsert_session_risk(&risk) {
                            warn!("Failed storing session risk for {:?}: {}", source.path, e);
                        }
                        match backfill_reason {
                            Some(BackfillReason::StaleParserVersion) => reparsed_sessions += 1,
                            Some(BackfillReason::MissingDerivedField) => backfilled_sessions += 1,
                            // Unreachable: the `SourceUnavailable` arm above always
                            // `continue`s before a parse is even attempted.
                            Some(BackfillReason::SourceUnavailable) => sources_unavailable += 1,
                            None => scanned_sessions += 1,
                        }
                    }
                }
                Err(e) => {
                    error!("Failed parsing {:?}: {}", source.path, e);
                    errors_encountered += 1;
                }
            }
        }

        // Prune stub sessions (config files, telemetry dumps, ... that an adapter's discovery
        // previously accepted, or genuine sessions that never accrued real activity) whose
        // source no longer passes any registered adapter's current detection. Only run on a
        // full, unscoped scan (#68's escape hatch): `all_sources` is the complete set of
        // currently-valid sources in that case, so "not in it" reliably means "detection no
        // longer accepts this file" rather than "outside today's narrower --path scope." Also
        // skipped when `include_stubs` was explicitly requested -- the caller wants the raw
        // rows left alone.
        let stub_sessions_removed = if options.custom_paths.is_empty() && !options.include_stubs {
            let valid_sources: HashSet<(&str, String)> = all_sources
                .iter()
                .map(|(adapter_idx, source)| {
                    (
                        self.adapters[*adapter_idx].name(),
                        source.path.to_string_lossy().to_string(),
                    )
                })
                .collect();
            self.prune_stub_sessions(&valid_sources)
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
            reparsed_sessions,
            sources_unavailable,
            errors_encountered,
            total_indexed_sessions,
            aggregate_stats,
            stub_sessions_removed,
        })
    }
}

/// Collect the risk signals for one parsed trace: claims that verification knocked down, loops
/// the sentinel caught, and recoveries from a broken state.
///
/// Every detector here runs over the already-parsed trace in memory. Nothing re-reads the log.
///
/// A note counts as a *demotion* only when the claim got weaker — a lower rung, or lower
/// confidence at the same rung. `verify_outcomes` also writes a note when reality *confirms* a
/// claim and raises its confidence, and counting those as risk would flag exactly the sessions
/// that proved the most.
pub fn compute_session_risk(
    trace: &AgentWorthTrace,
    verification_notes: &[agentworth_outcomes::VerificationNote],
) -> SessionRisk {
    let demoted: Vec<DemotedClaim> = verification_notes
        .iter()
        .filter(|n| {
            outcome_rank(n.final_kind) < outcome_rank(n.original_kind)
                || n.final_confidence < n.original_confidence
        })
        .map(|n| DemotedClaim {
            event_sequence: n.event_sequence,
            original_kind: outcome_kind_name(n.original_kind),
            final_kind: outcome_kind_name(n.final_kind),
            original_confidence: n.original_confidence,
            final_confidence: n.final_confidence,
            reason: n.reason.clone(),
        })
        .collect();

    let alerts = evaluate_trace_for_loops(
        trace,
        DEFAULT_MAX_TOOL_REPEATS,
        DEFAULT_MAX_FILE_REVISIONS,
    );
    let unresolved_loops = alerts
        .iter()
        .filter(|a| a.resolution != LoopResolution::SelfCorrected)
        .count();
    let loop_evidence: Vec<LoopEvidence> = alerts
        .iter()
        .take(SESSION_RISK_EVIDENCE_CAP)
        .map(|a| LoopEvidence {
            kind: match a.kind {
                LoopAlertKind::IdenticalToolLoop => "identical_tool_loop".to_string(),
                LoopAlertKind::FileOscillation => "file_oscillation".to_string(),
            },
            target: a.offending_target.clone(),
            repeat_count: a.repeat_count,
            resolution: match a.resolution {
                LoopResolution::SelfCorrected => "self_corrected".to_string(),
                LoopResolution::HumanRescued => "human_rescued".to_string(),
                LoopResolution::StillLooping => "still_looping".to_string(),
            },
        })
        .collect();

    let recoveries = RecoveryDetector::new().detect_recoveries(trace).len();

    SessionRisk {
        session_id: trace.session_id.clone(),
        demoted_claims: demoted.len(),
        loop_alerts: alerts.len(),
        unresolved_loops,
        recoveries,
        demoted_evidence: demoted.into_iter().take(SESSION_RISK_EVIDENCE_CAP).collect(),
        loop_evidence,
        computed_at: Some(chrono::Utc::now()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_adapters::ClaudeCodeAdapter;
    use agentworth_schema::Provenance;
    use chrono::Utc;
    use std::io::Write;

    /// A trace with one signal of each kind: a `git commit` that was only ever *requested*
    /// (verification demotes it), an identical tool call repeated past the loop threshold, and
    /// a failing command followed by a passing one (a recovery).
    ///
    /// The demotable claim is a commit, not a test run, for two reasons that both bite. Since
    /// #81 a bare `cargo test` tool call classifies as `done_claimed` outright — there is no
    /// exit code, so it never reaches a rung there is anything to demote *from*. And the
    /// recovery below needs a real `cargo build` that exits 0, which would corroborate a test
    /// claim and *raise* its confidence rather than demote it. A commit claim and a build
    /// success are different rungs, so the two signals stay independent.
    fn trace_with_every_risk_signal() -> AgentWorthTrace {
        use agentworth_schema::{EventPayload, NormalizedEvent, ShellCommand, ToolCall};

        let now = Utc::now();
        let prov = Provenance::new("/tmp/risk.jsonl", "claude_code", 100, 1000, "fp_risk");
        let mut trace = AgentWorthTrace::new("sess_risk", "claude_code", prov, now);

        // Three identical bash tool calls: one demotable claim per call, and a loop. Nothing
        // in this trace ever really commits, so each claim is only ever a request.
        for i in 1..=3 {
            trace.events.push(NormalizedEvent::new(
                i,
                now,
                EventPayload::ToolCall(ToolCall {
                    id: Some(format!("call_{i}")),
                    name: "bash".to_string(),
                    arguments: serde_json::json!({ "command": "git commit -m \"wip\"" }),
                }),
            ));
        }

        // A real failure followed by a real success on the same command: a recovery.
        // `RecoveryDetector` reads the *output*, not just the exit code, so the success needs
        // a line it recognises ("test result: ok.") -- `Finished dev profile` is not one.
        trace.events.push(NormalizedEvent::new(
            4,
            now + chrono::Duration::seconds(10),
            EventPayload::ShellCommand(ShellCommand {
                command: "cargo test --workspace".to_string(),
                exit_code: Some(1),
                output: Some("test result: FAILED. 3 passed; 1 failed".to_string()),
                cwd: None,
            }),
        ));
        trace.events.push(NormalizedEvent::new(
            5,
            now + chrono::Duration::seconds(20),
            EventPayload::ShellCommand(ShellCommand {
                command: "cargo test --workspace".to_string(),
                exit_code: Some(0),
                output: Some("test result: ok. 4 passed; 0 failed".to_string()),
                cwd: None,
            }),
        ));

        trace
    }

    /// Each of the three signals `agentworth suspect` reports has to be produced here, with an
    /// event sequence behind it -- a count with nothing to look at is an assertion, not a
    /// receipt.
    #[test]
    fn test_compute_session_risk_finds_each_signal_with_evidence() {
        let trace = trace_with_every_risk_signal();
        let (_, notes) = OutcomeHierarchyDetector::new().detect_outcomes_with_verification(&trace);
        let risk = compute_session_risk(&trace, &notes);

        assert!(
            risk.demoted_claims > 0,
            "a `git commit` that was only requested must be demoted, got {notes:?}"
        );
        let demoted = &risk.demoted_evidence[0];
        assert_eq!(demoted.original_kind, "commit_observed");
        assert_eq!(demoted.final_kind, "done_claimed");
        assert!(demoted.event_sequence > 0, "the demotion must name its event");

        assert!(risk.loop_alerts > 0, "three identical tool calls is a loop");
        assert_eq!(risk.loop_evidence[0].kind, "identical_tool_loop");
        assert_eq!(risk.loop_evidence[0].target, "bash");

        assert!(risk.recoveries > 0, "a failing build followed by a passing one is a recovery");
        assert_eq!(risk.session_id, "sess_risk");
    }

    /// A clean session must come back with zeroes, not with signals -- the flag has to mean
    /// something.
    #[test]
    fn test_compute_session_risk_is_quiet_on_a_clean_trace() {
        use agentworth_schema::{EventPayload, NormalizedEvent, ShellCommand};

        let now = Utc::now();
        let prov = Provenance::new("/tmp/clean.jsonl", "claude_code", 100, 1000, "fp_clean");
        let mut trace = AgentWorthTrace::new("sess_clean", "claude_code", prov, now);
        trace.events.push(NormalizedEvent::new(
            1,
            now,
            EventPayload::ShellCommand(ShellCommand {
                command: "cargo test --workspace".to_string(),
                exit_code: Some(0),
                output: Some("test result: ok. 42 passed".to_string()),
                cwd: None,
            }),
        ));

        let (_, notes) = OutcomeHierarchyDetector::new().detect_outcomes_with_verification(&trace);
        let risk = compute_session_risk(&trace, &notes);

        assert_eq!(risk.demoted_claims, 0);
        assert_eq!(risk.loop_alerts, 0);
        assert_eq!(risk.recoveries, 0);
    }

    /// The scanner has to actually write the row -- a detector nobody persists is a detector
    /// `agentworth suspect` cannot read.
    #[test]
    fn test_scan_persists_session_risk() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let trace = trace_with_every_risk_signal();
        // The session row first: `session_risk` is a child table and the constraint is
        // enforced, which is the ordering the scanner already uses.
        storage.upsert_trace(&trace).expect("seed session");

        let risk = compute_session_risk(
            &trace,
            &OutcomeHierarchyDetector::new()
                .detect_outcomes_with_verification(&trace)
                .1,
        );
        storage.upsert_session_risk(&risk).expect("write risk");
        storage.upsert_session_risk(&risk).expect("rewrite risk");

        let back = storage
            .get_session_risks(&["sess_risk".to_string()])
            .expect("read risk");
        assert_eq!(back.len(), 1, "a rescan replaces, it does not duplicate");
        let stored = &back["sess_risk"];
        assert_eq!(stored.demoted_claims, risk.demoted_claims);
        assert_eq!(stored.loop_alerts, risk.loop_alerts);
        assert_eq!(stored.demoted_evidence, risk.demoted_evidence);

        // An unscanned session is absent, not zeroed -- the caller must be able to tell
        // "no risk found" from "never looked".
        let missing = storage
            .get_session_risks(&["sess_never_scanned".to_string()])
            .expect("read missing risk");
        assert!(missing.is_empty());
    }

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
            ..Default::default()
        };

        let summary = scanner.run_scan(&options, |_, _| {}).expect("scan run");

        assert_eq!(summary.discovered_sources, 1);
        assert_eq!(summary.scanned_sessions, 0, "a zero-event file must not count as scanned");
        assert_eq!(summary.total_indexed_sessions, 0, "a zero-event file must not be stored");
    }

    /// A session with a single user message and no model response normalizes to one event --
    /// it survives the zero-event backstop above but still fails `NEAR_EMPTY_EVENTS_SQL_PREDICATE`
    /// (`total_events <= 1`). By default the scanner must not store it either, so the raw index
    /// doesn't keep accumulating rows this thin. `include_stubs: true` is the explicit opt-out
    /// that keeps storing it anyway.
    ///
    /// Also proves the boundary that matters most: a session with *more than one* event but
    /// zero recorded tokens (an adapter that never captured usage) is a completely different
    /// case and must still be stored by default -- `agentworth audit`/`autopsy`/`blind-spots`
    /// all depend on exactly this population being indexed, even though it's excluded from
    /// `agentworth stats`/`usage` aggregates by the separate, stricter `NON_STUB_SQL_PREDICATE`.
    /// A regression here (checking `total_tokens` in this store-time gate too, not just
    /// `total_events`) was caught by three existing tests going red: `autopsy`'s and the CLI
    /// safety-audit tests' own real-session fixtures are exactly this shape (multiple events,
    /// zero tokens because the fixture's assistant messages carry no `usage` field).
    #[test]
    fn test_scanner_does_not_store_one_event_stub_by_default_but_keeps_tokenless_multi_event_sessions() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner =
            Scanner::with_adapters(vec![Box::new(ClaudeCodeAdapter::new())], storage.clone());

        let mut one_event_temp = tempfile::Builder::new()
            .suffix(".jsonl")
            .tempfile()
            .unwrap();
        let one_event_sample = r#"
{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"hello"}
"#;
        one_event_temp.write_all(one_event_sample.as_bytes()).unwrap();

        let options = ScanOptions {
            custom_paths: vec![one_event_temp.path().to_path_buf()],
            force: false,
            ..Default::default()
        };
        let summary = scanner.run_scan(&options, |_, _| {}).expect("scan run");
        assert_eq!(
            summary.scanned_sessions, 0,
            "a one-event session must not be stored by default"
        );
        assert_eq!(summary.total_indexed_sessions, 0);

        let include_stubs_options = ScanOptions {
            custom_paths: vec![one_event_temp.path().to_path_buf()],
            force: true,
            include_stubs: true,
        };
        let summary2 = scanner
            .run_scan(&include_stubs_options, |_, _| {})
            .expect("scan run with include_stubs");
        assert_eq!(
            summary2.scanned_sessions, 1,
            "include_stubs: true must store it anyway"
        );
        assert_eq!(summary2.total_indexed_sessions, 1);

        // A real, multi-event exchange whose assistant reply carries no `usage` field at all
        // (no tokens ever recorded) -- must still be stored by default.
        let mut tokenless_temp = tempfile::Builder::new()
            .suffix(".jsonl")
            .tempfile()
            .unwrap();
        let tokenless_sample = r#"
{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"revert that change"}
{"type":"assistant","timestamp":"2026-08-29T10:00:02Z","content":[{"type":"text","text":"Reverted."}]}
"#;
        tokenless_temp.write_all(tokenless_sample.as_bytes()).unwrap();
        let tokenless_options = ScanOptions {
            custom_paths: vec![tokenless_temp.path().to_path_buf()],
            force: false,
            ..Default::default()
        };
        let tokenless_summary =
            scanner.run_scan(&tokenless_options, |_, _| {}).expect("scan run");
        assert_eq!(
            tokenless_summary.scanned_sessions, 1,
            "a multi-event session with zero recorded tokens must still be stored by default"
        );
    }

    /// Item-4 cleanup: a stub row left over from before an adapter's discovery was
    /// tightened (or whose source file was simply deleted) should be pruned once its
    /// `(adapter, source_path)` no longer appears among currently-valid sources. A row
    /// still backed by a currently-valid source must survive. Broader than the original
    /// zero-events-only check: a one-event row is also this thin
    /// (`NEAR_EMPTY_EVENTS_SQL_PREDICATE` is `total_events <= 1`) and must be pruned the
    /// same way once its source is no longer detected -- regardless of token count, unlike
    /// `agentworth stats`/`usage`'s separate, stricter stub definition.
    #[test]
    fn test_prune_stub_sessions_removes_only_undetectable_rows() {
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
        storage.upsert_session(&stale_trace, None, None, 0).expect("seed stale stub");

        // Stale, but one event / zero tokens rather than fully empty -- must still be
        // caught by the broadened predicate, not just the original zero-only check.
        let stale_one_event_provenance = Provenance::new(
            "/tmp/agentworth-test-stub/one-event.jsonl".to_string(),
            "claude_code",
            12,
            0,
            "deadbeef",
        );
        let mut stale_one_event_trace = AgentWorthTrace::new(
            "stale-one-event-stub",
            "claude_code",
            stale_one_event_provenance,
            Utc::now(),
        );
        stale_one_event_trace.stats.total_events = 1;
        storage
            .upsert_session(&stale_one_event_trace, None, None, 0)
            .expect("seed stale one-event stub");

        let live_provenance = Provenance::new(
            "/tmp/agentworth-test-stub/still-valid.jsonl".to_string(),
            "claude_code",
            12,
            0,
            "deadbeef",
        );
        let live_trace = AgentWorthTrace::new("live-stub", "claude_code", live_provenance, Utc::now());
        storage.upsert_session(&live_trace, None, None, 0).expect("seed live stub");

        assert_eq!(storage.get_aggregate_stats(true).unwrap().total_sessions, 3);

        let mut valid_sources = HashSet::new();
        valid_sources.insert(("claude_code", "/tmp/agentworth-test-stub/still-valid.jsonl".to_string()));

        let removed = scanner.prune_stub_sessions(&valid_sources);

        assert_eq!(removed, 2);
        let remaining = storage.get_aggregate_stats(true).unwrap();
        assert_eq!(remaining.total_sessions, 1);
        assert!(storage.get_session_by_id("live-stub").unwrap().is_some());
        assert!(storage.get_session_by_id("stale-stub").unwrap().is_none());
        assert!(storage.get_session_by_id("stale-one-event-stub").unwrap().is_none());
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
            ..Default::default()
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
        storage.upsert_session(&stale_trace, None, None, 0).expect("seed stale row");

        let seeded = storage.get_session_by_id(&session_id).unwrap().unwrap();
        assert!(
            seeded.prompt_preview.as_deref().unwrap_or("").is_empty(),
            "seeded row must start with an empty prompt_preview to reproduce the bug"
        );

        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
            ..Default::default()
        };

        // Backfill scan: source is unchanged, but the row is missing prompt_preview, so it
        // must be reparsed rather than skipped.
        let summary = scanner.run_scan(&options, |_, _| {}).expect("backfill scan");
        assert_eq!(summary.scanned_sessions, 0, "not a fresh/changed source");
        assert_eq!(summary.skipped_unchanged, 0, "must not be silently skipped forever");
        assert_eq!(
            summary.backfilled_sessions + summary.reparsed_sessions,
            1,
            "the row must be reparsed once, for one of the two reasons"
        );

        let backfilled = storage.get_session_by_id(&session_id).unwrap().unwrap();
        assert_eq!(backfilled.prompt_preview.as_deref(), Some("fix the flaky test"));

        // Steady-state scan: fields are now complete and the source is still unchanged, so
        // this must go back to a plain skip, not a repeated backfill.
        let summary2 = scanner.run_scan(&options, |_, _| {}).expect("steady-state scan");
        assert_eq!(summary2.scanned_sessions, 0);
        assert_eq!(summary2.backfilled_sessions, 0, "a complete row must not be rescanned");
        assert_eq!(summary2.reparsed_sessions, 0, "a current-version row must not be rescanned");
        assert_eq!(summary2.skipped_unchanged, 1);
    }

    /// The 8,816-NULL-outcome finding, end to end: a row indexed without a score must be
    /// pulled back through the scanner on the next plain scan -- no `--force` -- and come out
    /// with both a score and a detected outcome. Then it must go quiet.
    #[test]
    fn test_scanner_rescores_an_unscored_row_without_force() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner =
            Scanner::with_adapters(vec![Box::new(ClaudeCodeAdapter::new())], storage.clone());

        let mut temp = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();
        let sample = r#"
{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"Run tests and commit"}
{"type":"assistant","timestamp":"2026-08-29T10:00:05Z","model":"claude-3-5-sonnet","usage":{"input_tokens":300,"output_tokens":100,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"git commit -m 'feat: complete task'"}}]}
{"type":"tool_result","timestamp":"2026-08-29T10:00:07Z","tool_use_id":"t1","content":"[main 1a2b3c4] feat: complete task\n 2 files changed","is_error":false}
"#;
        temp.write_all(sample.as_bytes()).unwrap();
        let session_id = temp.path().file_stem().unwrap().to_string_lossy().to_string();

        // Seed the exact shape found in the real index: provenance matching the file on disk
        // (so `should_scan_source` says "unchanged"), no outcome, no score.
        let source = SessionSource::from_path(temp.path(), "claude_code").expect("source");
        let provenance = Provenance::new(
            source.path.to_string_lossy().to_string(),
            "claude_code",
            source.file_size_bytes,
            source.mtime_epoch_secs,
            &source.fingerprint,
        );
        let mut unscored = AgentWorthTrace::new(&session_id, "claude_code", provenance, Utc::now());
        unscored.stats.total_events = 3;
        unscored.stats.token_usage = agentworth_schema::TokenUsage::new(300, 100, 0, 0);
        unscored.events.push(agentworth_schema::NormalizedEvent::new(
            1,
            Utc::now(),
            agentworth_schema::EventPayload::UserMessage {
                content: "Run tests and commit".to_string(),
            },
        ));
        storage
            .upsert_session(&unscored, None, None, ClaudeCodeAdapter::PARSER_VERSION)
            .expect("seed unscored row");

        let seeded = storage.get_session_by_id(&session_id).unwrap().unwrap();
        assert!(seeded.composite_score.is_none(), "must start unscored");

        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
            ..Default::default()
        };
        let summary = scanner.run_scan(&options, |_, _| {}).expect("rescoring scan");
        assert_eq!(summary.skipped_unchanged, 0, "an unscored row must not be skipped");
        assert_eq!(summary.backfilled_sessions, 1);

        let scored = storage.get_session_by_id(&session_id).unwrap().unwrap();
        assert!(scored.composite_score.is_some(), "the rescan must score it");
        assert_eq!(scored.primary_outcome.as_deref(), Some("commit_observed"));

        let summary2 = scanner.run_scan(&options, |_, _| {}).expect("steady-state scan");
        assert_eq!(summary2.skipped_unchanged, 1, "one pass is enough");
        assert_eq!(summary2.backfilled_sessions, 0);
    }

    /// A parser-version bump reaches sessions an incremental scan would otherwise skip
    /// forever, and reports them separately from a missing-field backfill.
    #[test]
    fn test_scanner_reparses_rows_left_by_an_older_parser_version() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner =
            Scanner::with_adapters(vec![Box::new(ClaudeCodeAdapter::new())], storage.clone());

        let mut temp = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();
        let sample = r#"
{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"fix the flaky test"}
{"type":"assistant","timestamp":"2026-08-29T10:00:05Z","model":"claude-3-5-sonnet","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"Done"}]}
"#;
        temp.write_all(sample.as_bytes()).unwrap();
        let session_id = temp.path().file_stem().unwrap().to_string_lossy().to_string();

        let source = SessionSource::from_path(temp.path(), "claude_code").expect("source");
        let provenance = Provenance::new(
            source.path.to_string_lossy().to_string(),
            "claude_code",
            source.file_size_bytes,
            source.mtime_epoch_secs,
            &source.fingerprint,
        );
        let mut old_row = AgentWorthTrace::new(&session_id, "claude_code", provenance, Utc::now());
        old_row.stats.total_events = 2;
        old_row.stats.token_usage = agentworth_schema::TokenUsage::new(100, 50, 0, 0);
        old_row.events.push(agentworth_schema::NormalizedEvent::new(
            1,
            Utc::now(),
            agentworth_schema::EventPayload::UserMessage {
                content: "fix the flaky test".to_string(),
            },
        ));
        // Complete in every other way -- only the parser version is behind.
        storage
            .upsert_session(
                &old_row,
                Some("done_claimed"),
                Some(0.4),
                ClaudeCodeAdapter::PARSER_VERSION - 1,
            )
            .expect("seed old-parser row");

        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: false,
            ..Default::default()
        };
        let summary = scanner.run_scan(&options, |_, _| {}).expect("reparse scan");
        assert_eq!(summary.skipped_unchanged, 0, "a stale parse must not be skipped");
        assert_eq!(summary.reparsed_sessions, 1);
        assert_eq!(summary.backfilled_sessions, 0, "this is a version bump, not a missing field");

        let summary2 = scanner.run_scan(&options, |_, _| {}).expect("steady-state scan");
        assert_eq!(summary2.reparsed_sessions, 0, "one reparse per bump, not one per scan");
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
            ..Default::default()
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
            ..Default::default()
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

    /// #91 fixed the opencode adapter's own repository resolution but left a sibling bug: any
    /// adapter whose `SessionSource.path` is a virtual identity string (never a literal file on
    /// disk, even though the session is fully present in its backing store) made `load_trace`'s
    /// plain `source_path.exists()` gate bail on every single one of its sessions. This test
    /// proves the general fix -- `AgentAdapter::source_exists` -- with a fake adapter rather
    /// than opencode's SQLite specifics, so it isolates the core-layer behavior from any one
    /// adapter's own locator format.
    #[test]
    fn test_load_trace_parses_a_session_with_a_virtual_nonexistent_source_path() {
        use agentworth_adapter_sdk::{DetectionResult, ParseResult};
        use agentworth_schema::{EventPayload, NormalizedEvent, Provenance};

        struct FakeVirtualAdapter;

        impl AgentAdapter for FakeVirtualAdapter {
            fn name(&self) -> &'static str {
                "fake_virtual"
            }

            // The whole point of this fixture: presence is defined by this override, not by
            // the default `source.path.exists()`, which would always say "gone" for a virtual
            // identity string.
            fn source_exists(&self, _source: &SessionSource) -> bool {
                true
            }

            fn detect(&self, _options: &ScanOptions) -> Result<DetectionResult> {
                Ok(DetectionResult {
                    adapter_name: self.name(),
                    is_present: true,
                    discovered_roots: vec![],
                    confidence: 1.0,
                })
            }

            fn enumerate(&self, _options: &ScanOptions) -> Result<Vec<SessionSource>> {
                Ok(vec![])
            }

            fn parse(&self, source: &SessionSource) -> Result<ParseResult> {
                let provenance = Provenance::new(
                    source.path.to_string_lossy().to_string(),
                    self.name(),
                    source.file_size_bytes,
                    source.mtime_epoch_secs,
                    &source.fingerprint,
                );
                let mut trace =
                    AgentWorthTrace::new("virtual-sess", self.name(), provenance, Utc::now());
                trace.events.push(NormalizedEvent::new(
                    1,
                    Utc::now(),
                    EventPayload::UserMessage { content: "hi from a virtual source".to_string() },
                ));
                trace.events.push(NormalizedEvent::new(
                    2,
                    Utc::now(),
                    EventPayload::AssistantMessage { content: "hello".to_string(), thinking: None },
                ));
                trace.recalculate_stats();
                Ok(ParseResult { trace, malformed_lines: 0, warnings: vec![] })
            }
        }

        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner = Scanner::with_adapters(vec![Box::new(FakeVirtualAdapter)], storage.clone());

        // Modeled on opencode's own wrapped identity: a synthetic string that looks like a
        // path but is never resolvable as one.
        let virtual_path = "/repo/.fake/session-abc.db::fake-repo::/db/path.db#abc";
        assert!(
            !std::path::Path::new(virtual_path).exists(),
            "fixture must actually be non-existent, or this test would pass for the wrong reason"
        );

        let source = SessionSource {
            path: std::path::PathBuf::from(virtual_path),
            adapter_name: "fake_virtual".to_string(),
            file_size_bytes: 4096,
            mtime_epoch_secs: 1_000,
            fingerprint: "deadbeef".to_string(),
        };
        let parse_result = FakeVirtualAdapter.parse(&source).expect("parse fixture");
        storage
            .upsert_session(&parse_result.trace, None, None, 0)
            .expect("seed virtual session");

        let loaded = scanner
            .load_trace("virtual-sess")
            .expect("load_trace must resolve a virtual source via adapter.source_exists, not Path::exists");
        assert_eq!(loaded.events.len(), 2);
    }

    /// The stub-pruning pass (`prune_stub_sessions`) decides survival by checking whether
    /// `(adapter, source_path)` is still among the sources the adapter enumerates right now --
    /// never by stat'ing the path -- so a thin (near-empty) session backed by a virtual,
    /// never-a-real-file identity string must survive exactly like a thin session backed by a
    /// real file would, as long as its adapter still enumerates it. This pins that behavior
    /// down explicitly, since it is the one place a `Path::exists()` habit could most easily
    /// have crept back in alongside the `load_trace` fix above.
    #[test]
    fn test_prune_stub_sessions_does_not_prune_a_virtual_source_still_enumerated() {
        use agentworth_adapter_sdk::{DetectionResult, ParseResult};
        use agentworth_schema::{EventPayload, NormalizedEvent, Provenance};

        struct FakeVirtualStubAdapter;

        impl AgentAdapter for FakeVirtualStubAdapter {
            fn name(&self) -> &'static str {
                "fake_virtual_stub"
            }

            fn source_exists(&self, _source: &SessionSource) -> bool {
                true
            }

            fn detect(&self, _options: &ScanOptions) -> Result<DetectionResult> {
                Ok(DetectionResult {
                    adapter_name: self.name(),
                    is_present: true,
                    discovered_roots: vec![],
                    confidence: 1.0,
                })
            }

            fn enumerate(&self, _options: &ScanOptions) -> Result<Vec<SessionSource>> {
                Ok(vec![SessionSource {
                    path: std::path::PathBuf::from(
                        "/repo/.fake/session-stub.db::fake-repo::/db/path.db#stub",
                    ),
                    adapter_name: self.name().to_string(),
                    file_size_bytes: 4096,
                    mtime_epoch_secs: 1_000,
                    fingerprint: "stub-fp".to_string(),
                }])
            }

            fn parse(&self, source: &SessionSource) -> Result<ParseResult> {
                let provenance = Provenance::new(
                    source.path.to_string_lossy().to_string(),
                    self.name(),
                    source.file_size_bytes,
                    source.mtime_epoch_secs,
                    &source.fingerprint,
                );
                let mut trace = AgentWorthTrace::new(
                    "virtual-stub-sess",
                    self.name(),
                    provenance,
                    Utc::now(),
                );
                // Deliberately one event -- thin enough to qualify as a stub for pruning
                // purposes (`is_near_empty_session`), which is exactly the shape that must
                // still survive as long as the source stays enumerated.
                trace.events.push(NormalizedEvent::new(
                    1,
                    Utc::now(),
                    EventPayload::UserMessage { content: "hi".to_string() },
                ));
                trace.recalculate_stats();
                Ok(ParseResult { trace, malformed_lines: 0, warnings: vec![] })
            }
        }

        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner =
            Scanner::with_adapters(vec![Box::new(FakeVirtualStubAdapter)], storage.clone());

        // include_stubs: true to get this one-event session actually indexed despite the
        // default store-time skip -- pruning has nothing to act on otherwise.
        let seed_options = ScanOptions { include_stubs: true, ..Default::default() };
        let seed_summary = scanner.run_scan(&seed_options, |_, _| {}).expect("seed scan");
        assert_eq!(seed_summary.total_indexed_sessions, 1);

        // A full, unscoped, default (non-include_stubs) scan runs `prune_stub_sessions`. The
        // session is thin, but its virtual source is still enumerated by the adapter -- it
        // must survive, exactly as a thin session backed by a real file would.
        let prune_summary = scanner
            .run_scan(&ScanOptions::default(), |_, _| {})
            .expect("prune scan");
        assert_eq!(
            prune_summary.stub_sessions_removed, 0,
            "a still-enumerated virtual source must not be pruned as missing"
        );
        assert!(storage.get_session_by_id("virtual-stub-sess").unwrap().is_some());
    }

    /// End-to-end regression for the `recovery.rs:571` panic: a real Claude Code transcript
    /// whose Bash tool result carries a failure message packed with Hebrew, Arabic, CJK, an
    /// emoji, and a combining mark. `compute_session_risk` (which runs `RecoveryDetector`
    /// during every scan) used to byte-slice that text at a fixed offset and panic the moment
    /// the cut landed inside one of these multi-byte characters. A full `scan` over this
    /// fixture must complete and index the session instead of aborting mid-scan.
    #[test]
    fn test_scanner_survives_multibyte_failure_text_in_transcript() {
        let storage = Arc::new(Storage::open_in_memory().expect("open storage"));
        let scanner =
            Scanner::with_adapters(vec![Box::new(ClaudeCodeAdapter::new())], storage.clone());

        let mut temp = tempfile::Builder::new().suffix(".jsonl").tempfile().unwrap();
        // "panic: " (7 ASCII bytes) + Hebrew 'ך' (2 bytes/char) puts byte offset 80 one byte
        // into the 37th Hebrew character -- reproducing the crash's exact shape ("bytes
        // 79..81"). Arabic, CJK, an emoji, and a combining mark are appended so every
        // multi-byte width (2, 3, 4 bytes, plus a combining sequence) is represented.
        let failure_text = format!(
            "panic: {}{}{}🎉e\u{0301}",
            "ך".repeat(150),
            "خطأ في تشغيل الاختبار ".repeat(4),
            "测试失败了".repeat(20)
        );
        let sample = format!(
            r#"
{{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"תקן את הבדיקה שנכשלה"}}
{{"type":"assistant","timestamp":"2026-08-29T10:00:02Z","model":"claude-3-5-sonnet-20241022","usage":{{"input_tokens":5000,"output_tokens":800,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{"command":"cargo test"}}}}]}}
{{"type":"tool_result","timestamp":"2026-08-29T10:00:04Z","tool_use_id":"t1","content":"{failure_text}","is_error":false}}
{{"type":"assistant","timestamp":"2026-08-29T10:00:06Z","model":"claude-3-5-sonnet-20241022","usage":{{"input_tokens":200,"output_tokens":30,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"content":[{{"type":"tool_use","id":"t2","name":"Edit","input":{{"file_path":"crates/core/src/lib.rs","old_string":"a","new_string":"b"}}}}]}}
{{"type":"tool_result","timestamp":"2026-08-29T10:00:07Z","tool_use_id":"t2","content":"File modified successfully","is_error":false}}
{{"type":"assistant","timestamp":"2026-08-29T10:00:08Z","model":"claude-3-5-sonnet-20241022","usage":{{"input_tokens":200,"output_tokens":30,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"content":[{{"type":"tool_use","id":"t3","name":"Bash","input":{{"command":"cargo test"}}}}]}}
{{"type":"tool_result","timestamp":"2026-08-29T10:00:10Z","tool_use_id":"t3","content":"test result: ok. 8 passed; 0 failed","is_error":false}}
"#
        );
        temp.write_all(sample.as_bytes()).unwrap();

        let options = ScanOptions {
            custom_paths: vec![temp.path().to_path_buf()],
            force: true,
            ..Default::default()
        };

        // The assertion that matters is simply that this does not panic. Before the fix, the
        // panic happened inside `compute_session_risk` while indexing this session, which
        // `cargo test` reports as the test process aborting rather than a clean failure.
        let summary = scanner
            .run_scan(&options, |_, _| {})
            .expect("scan must survive multibyte failure text without panicking");
        assert_eq!(summary.scanned_sessions, 1);

        let session = storage
            .get_session_by_id(temp.path().file_stem().unwrap().to_string_lossy().as_ref())
            .unwrap()
            .expect("session must be indexed, not lost to a mid-scan panic");
        assert!(
            session.total_events >= 7,
            "expected all 7 transcript lines to normalize into events, got {}",
            session.total_events
        );
    }
}
