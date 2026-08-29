//! Explainable scoring engine for AgentWorth traces.
//!
//! Produces transparent, multidimensional score breakdowns across:
//! - Outcome quality & hierarchy
//! - Objective verifiability (tests/commits vs self-claims)
//! - Trajectory complexity & richness
//! - Autonomous error recovery resilience
//! - Cryptographic provenance integrity

use agentworth_outcomes::{OutcomeDetector, RecoveryDetector, RecoverySignal};
use agentworth_schema::{AgentWorthTrace, EventPayload, OutcomeEvidence, OutcomeKind};
use serde::{Deserialize, Serialize};

/// Configurable weights for composite score computation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoringWeights {
    pub outcome_weight: f64,
    pub verifiability_weight: f64,
    pub complexity_weight: f64,
    pub recovery_weight: f64,
    pub provenance_weight: f64,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self {
            outcome_weight: 0.30,
            verifiability_weight: 0.25,
            complexity_weight: 0.20,
            recovery_weight: 0.15,
            provenance_weight: 0.10,
        }
    }
}

/// Explainable multidimensional score result for an agent trace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceScore {
    /// Score based on highest verified outcome hierarchy (0.0 to 1.0).
    pub outcome_score: f64,
    /// Score based on test/commit/CI evidence vs self-claimed done (0.0 to 1.0).
    pub verifiability_score: f64,
    /// Score based on multi-turn depth, tool breadth, tokens, and files changed (0.0 to 1.0).
    pub complexity_score: f64,
    /// Score based on resilience bonus when failure -> recovery occurs (0.0 to 1.0).
    pub recovery_score: f64,
    /// Score based on metadata completeness, fingerprint validity, and timing (0.0 to 1.0).
    pub provenance_score: f64,
    /// Weighted explainable composite score (0.0 to 1.0).
    pub composite_score: f64,
    /// Human-readable explanations for each scoring dimension.
    pub explanations: Vec<String>,
}

/// Scorer for computing transparent quality, verifiability, and resilience metrics.
#[derive(Debug, Clone, Default)]
pub struct TraceScorer {
    pub weights: ScoringWeights,
}

impl TraceScorer {
    pub fn new() -> Self {
        Self {
            weights: ScoringWeights::default(),
        }
    }

    pub fn with_weights(weights: ScoringWeights) -> Self {
        Self { weights }
    }

    /// Complete end-to-end trace scoring: detects outcomes and recoveries automatically.
    pub fn score(&self, trace: &AgentWorthTrace) -> TraceScore {
        let outcome_detector = OutcomeDetector::new();
        let recovery_detector = RecoveryDetector::new();

        let outcomes = outcome_detector.detect_outcomes(trace);
        let recoveries = recovery_detector.detect_recoveries(trace);

        self.score_trace(trace, &outcomes, &recoveries)
    }

    /// Computes explainable trace score from pre-extracted outcomes and recovery signals.
    pub fn score_trace(
        &self,
        trace: &AgentWorthTrace,
        outcomes: &[OutcomeEvidence],
        recoveries: &[RecoverySignal],
    ) -> TraceScore {
        let mut explanations = Vec::new();

        // 1. Outcome score
        let (outcome_score, outcome_expl) = self.compute_outcome_score(outcomes);
        explanations.push(outcome_expl);

        // 2. Verifiability score
        let (verifiability_score, verifiability_expl) =
            self.compute_verifiability_score(outcomes, trace);
        explanations.push(verifiability_expl);

        // 3. Complexity score
        let (complexity_score, complexity_expl) = self.compute_complexity_score(trace);
        explanations.push(complexity_expl);

        // 4. Recovery score
        let (recovery_score, recovery_expl) = self.compute_recovery_score(trace, recoveries);
        explanations.push(recovery_expl);

        // 5. Provenance score
        let (provenance_score, provenance_expl) = self.compute_provenance_score(trace);
        explanations.push(provenance_expl);

        // 6. Composite score
        let total_weight = self.weights.outcome_weight
            + self.weights.verifiability_weight
            + self.weights.complexity_weight
            + self.weights.recovery_weight
            + self.weights.provenance_weight;

        let composite_raw = if total_weight > 0.0 {
            (outcome_score * self.weights.outcome_weight
                + verifiability_score * self.weights.verifiability_weight
                + complexity_score * self.weights.complexity_weight
                + recovery_score * self.weights.recovery_weight
                + provenance_score * self.weights.provenance_weight)
                / total_weight
        } else {
            0.0
        };

        let composite_score = clamp01(composite_raw);

        TraceScore {
            outcome_score,
            verifiability_score,
            complexity_score,
            recovery_score,
            provenance_score,
            composite_score,
            explanations,
        }
    }

    fn compute_outcome_score(&self, outcomes: &[OutcomeEvidence]) -> (f64, String) {
        if outcomes.is_empty() {
            return (
                0.0,
                "No clear outcome evidence detected in trace".to_string(),
            );
        }

        let mut max_score = 0.0f64;
        let mut best_kind = OutcomeKind::DoneClaimed;
        let mut best_conf = 0.0f32;

        for ev in outcomes {
            let base = match ev.kind {
                OutcomeKind::CiOrDeploymentVerified => 1.0,
                OutcomeKind::CommitObserved => 0.85,
                OutcomeKind::TestOrBuildPassed => 0.70,
                OutcomeKind::ArtifactChanged => 0.40,
                OutcomeKind::DoneClaimed => 0.20,
            };
            let val = base * (ev.confidence as f64);
            if val > max_score {
                max_score = val;
                best_kind = ev.kind;
                best_conf = ev.confidence;
            }
        }

        let score = clamp01(max_score);
        let explanation = format!(
            "Outcome score {:.2}: Peak evidence '{:?}' with {:.0}% confidence",
            score,
            best_kind,
            best_conf * 100.0
        );

        (score, explanation)
    }

    fn compute_verifiability_score(
        &self,
        outcomes: &[OutcomeEvidence],
        trace: &AgentWorthTrace,
    ) -> (f64, String) {
        let has_ci = outcomes
            .iter()
            .any(|o| o.kind == OutcomeKind::CiOrDeploymentVerified);
        let has_commit = outcomes
            .iter()
            .any(|o| o.kind == OutcomeKind::CommitObserved);
        let has_tests = outcomes
            .iter()
            .any(|o| o.kind == OutcomeKind::TestOrBuildPassed);
        let has_artifacts = outcomes
            .iter()
            .any(|o| o.kind == OutcomeKind::ArtifactChanged);
        let has_done_only = outcomes.iter().all(|o| o.kind == OutcomeKind::DoneClaimed);

        let score: f64;
        let explanation: String;

        if has_ci {
            score = 1.0;
            explanation =
                "Verifiability score 1.00: Externally verified via CI / deployment".to_string();
        } else if has_commit {
            score = 0.90;
            explanation =
                "Verifiability score 0.90: Backed by persistent git commit history".to_string();
        } else if has_tests {
            score = 0.80;
            explanation = "Verifiability score 0.80: Backed by passing test or build verification"
                .to_string();
        } else if has_artifacts {
            score = 0.50;
            explanation =
                "Verifiability score 0.50: Observable file artifact changes produced".to_string();
        } else if has_done_only && !outcomes.is_empty() {
            score = 0.15;
            explanation = "Verifiability score 0.15: Unverified self-claim ('done') without test or commit evidence".to_string();
        } else if trace.events.is_empty() && trace.stats.total_events > 0 {
            score = 0.30;
            explanation = "Verifiability score 0.30: Indexed trace statistics present without detailed events".to_string();
        } else {
            score = 0.0;
            explanation = "Verifiability score 0.00: No verifiable execution evidence".to_string();
        }

        (clamp01(score), explanation)
    }

    fn compute_complexity_score(&self, trace: &AgentWorthTrace) -> (f64, String) {
        let total_events = if trace.stats.total_events > 0 {
            trace.stats.total_events
        } else {
            trace.events.len()
        };

        let tools_count = trace.stats.tools_used.len();
        let total_tokens = trace.stats.token_usage.total();

        let mut file_actions = 0;
        for ev in &trace.events {
            if matches!(ev.payload, EventPayload::FileAction { .. }) {
                file_actions += 1;
            }
        }

        // Multi-turn depth factor: saturation curve
        let s_events = 1.0 - (-(total_events as f64) / 15.0).exp();

        // Tool breadth factor: 1 tool = 0.33, 2 = 0.66, 3+ = 1.0
        let s_tools = (tools_count as f64 / 3.0).min(1.0);

        // Token scale factor: logarithmic
        let s_tokens = if total_tokens > 0 {
            ((total_tokens as f64).log10() / 5.0).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // File modifications factor
        let s_files = (file_actions as f64 / 4.0).min(1.0);

        let complexity = 0.35 * s_events + 0.25 * s_tools + 0.20 * s_tokens + 0.20 * s_files;
        let score = clamp01(complexity);

        let explanation = format!(
            "Complexity score {:.2}: {} events, {} distinct tools, {} tokens, {} file actions",
            score, total_events, tools_count, total_tokens, file_actions
        );

        (score, explanation)
    }

    fn compute_recovery_score(
        &self,
        trace: &AgentWorthTrace,
        recoveries: &[RecoverySignal],
    ) -> (f64, String) {
        if !recoveries.is_empty() {
            let count = recoveries.len();
            let score = clamp01(0.70 + (count as f64 * 0.15).min(0.30));
            let explanation = format!(
                "Recovery score {:.2}: Successfully detected {} autonomous failure-to-recovery loop(s)",
                score, count
            );
            return (score, explanation);
        }

        // Check if there were unresolved errors
        let has_unresolved_errors = trace.events.iter().any(|ev| match &ev.payload {
            EventPayload::Error { is_recovered, .. } => !*is_recovered,
            EventPayload::ToolResult(r) => r.is_error,
            EventPayload::ShellCommand(c) => c.exit_code.is_some_and(|code| code != 0),
            _ => false,
        });

        if has_unresolved_errors {
            (
                0.0,
                "Recovery score 0.00: Errors encountered without observed successful recovery"
                    .to_string(),
            )
        } else {
            (
                0.50,
                "Recovery score 0.50: Clean execution with zero runtime or command errors"
                    .to_string(),
            )
        }
    }

    fn compute_provenance_score(&self, trace: &AgentWorthTrace) -> (f64, String) {
        let mut score = 0.0;
        let mut items = Vec::new();

        if !trace.provenance.source_path.is_empty() {
            score += 0.25;
            items.push("valid source path");
        }
        if !trace.adapter.is_empty() {
            score += 0.20;
            items.push("identified adapter");
        }
        if !trace.provenance.content_fingerprint.is_empty() {
            score += 0.25;
            items.push("cryptographic fingerprint");
        }
        if trace.provenance.file_size_bytes > 0 {
            score += 0.15;
            items.push("file metadata");
        }
        if trace.stats.token_usage.total() > 0 {
            score += 0.15;
            items.push("token accounting");
        }

        let clamped = clamp01(score);
        let explanation = format!(
            "Provenance score {:.2}: Contains {}",
            clamped,
            items.join(", ")
        );

        (clamped, explanation)
    }
}

fn clamp01(val: f64) -> f64 {
    if val.is_nan() {
        0.0
    } else {
        val.clamp(0.0, 1.0)
    }
}
