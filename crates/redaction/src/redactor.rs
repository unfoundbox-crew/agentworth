use crate::report::RedactionReport;
use crate::rules::{default_rules, RedactionRule};
use agentworth_outcomes::RecoverySignal;
use agentworth_schema::{
    AgentWorthTrace, EventPayload, NormalizedEvent, OutcomeEvidence, ShellCommand, ToolCall,
    ToolResult,
};
use serde_json::Value;
use std::borrow::Cow;

/// Privacy and redaction engine for sanitizing agent session histories.
#[derive(Debug, Clone)]
pub struct Redactor {
    rules: Vec<RedactionRule>,
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new()
    }
}

impl Redactor {
    /// Creates a new Redactor equipped with default security and privacy rules.
    pub fn new() -> Self {
        Self {
            rules: default_rules(),
        }
    }

    /// Creates a Redactor with custom rules.
    pub fn with_rules(rules: Vec<RedactionRule>) -> Self {
        Self { rules }
    }

    /// Appends an extra rule to the active redactor.
    pub fn add_rule(&mut self, rule: RedactionRule) {
        self.rules.push(rule);
    }

    /// Redacts all sensitive information from a text string.
    pub fn redact_text(&self, text: &str) -> String {
        let mut report = None;
        self.redact_text_internal(text, &mut report)
    }

    /// Redacts text and records counts into the provided report.
    pub fn redact_text_with_counts(&self, text: &str, report: &mut RedactionReport) -> String {
        let mut rep = Some(report);
        self.redact_text_internal(text, &mut rep)
    }

    fn redact_text_internal(
        &self,
        text: &str,
        report: &mut Option<&mut RedactionReport>,
    ) -> String {
        if text.is_empty() {
            return String::new();
        }

        let mut current: Cow<str> = Cow::Borrowed(text);
        for rule in &self.rules {
            let next = rule.apply(&current, report);
            current = Cow::Owned(next.into_owned());
        }
        current.into_owned()
    }

    /// Recursively scrubs all sensitive strings within a JSON value.
    pub fn redact_json(&self, val: &Value) -> Value {
        let mut report = None;
        self.redact_json_internal(val, &mut report)
    }

    /// Recursively scrubs JSON and records counts into the provided report.
    pub fn redact_json_with_counts(&self, val: &Value, report: &mut RedactionReport) -> Value {
        let mut rep = Some(report);
        self.redact_json_internal(val, &mut rep)
    }

    fn redact_json_internal(
        &self,
        val: &Value,
        report: &mut Option<&mut RedactionReport>,
    ) -> Value {
        match val {
            Value::String(s) => Value::String(self.redact_text_internal(s, report)),
            Value::Array(arr) => {
                let redacted_arr = arr
                    .iter()
                    .map(|item| self.redact_json_internal(item, report))
                    .collect();
                Value::Array(redacted_arr)
            }
            Value::Object(map) => {
                let mut redacted_map = serde_json::Map::with_capacity(map.len());
                for (k, v) in map {
                    let redacted_key = self.redact_text_internal(k, report);
                    let redacted_val = self.redact_json_internal(v, report);
                    redacted_map.insert(redacted_key, redacted_val);
                }
                Value::Object(redacted_map)
            }
            other => other.clone(),
        }
    }

    /// Redacts a single NormalizedEvent, returning the sanitized copy.
    pub fn redact_event(&self, event: &NormalizedEvent) -> NormalizedEvent {
        let mut report = None;
        self.redact_event_internal(event, &mut report)
    }

    /// Redacts an event and accumulates findings into the provided report.
    pub fn redact_event_with_counts(
        &self,
        event: &NormalizedEvent,
        report: &mut RedactionReport,
    ) -> NormalizedEvent {
        let mut rep = Some(report);
        self.redact_event_internal(event, &mut rep)
    }

    fn redact_event_internal(
        &self,
        event: &NormalizedEvent,
        report: &mut Option<&mut RedactionReport>,
    ) -> NormalizedEvent {
        let payload = match &event.payload {
            EventPayload::UserMessage { content } => EventPayload::UserMessage {
                content: self.redact_text_internal(content, report),
            },
            EventPayload::AssistantMessage { content, thinking } => {
                EventPayload::AssistantMessage {
                    content: self.redact_text_internal(content, report),
                    thinking: thinking
                        .as_ref()
                        .map(|t| self.redact_text_internal(t, report)),
                }
            }
            EventPayload::ModelInvocation {
                model,
                token_usage,
                cost_usd,
                latency_ms,
            } => EventPayload::ModelInvocation {
                model: self.redact_text_internal(model, report),
                token_usage: *token_usage,
                cost_usd: *cost_usd,
                latency_ms: *latency_ms,
            },
            EventPayload::ToolCall(tc) => EventPayload::ToolCall(ToolCall {
                id: tc.id.clone(),
                name: self.redact_text_internal(&tc.name, report),
                arguments: self.redact_json_internal(&tc.arguments, report),
            }),
            EventPayload::ToolResult(tr) => EventPayload::ToolResult(ToolResult {
                call_id: tr.call_id.clone(),
                name: tr
                    .name
                    .as_ref()
                    .map(|n| self.redact_text_internal(n, report)),
                output: self.redact_json_internal(&tr.output, report),
                is_error: tr.is_error,
            }),
            EventPayload::ShellCommand(sc) => EventPayload::ShellCommand(ShellCommand {
                command: self.redact_text_internal(&sc.command, report),
                cwd: sc
                    .cwd
                    .as_ref()
                    .map(|c| self.redact_text_internal(c, report)),
                exit_code: sc.exit_code,
                output: sc
                    .output
                    .as_ref()
                    .map(|o| self.redact_text_internal(o, report)),
            }),
            EventPayload::FileAction {
                path,
                action,
                diff,
                lines_changed,
            } => EventPayload::FileAction {
                path: self.redact_text_internal(path, report),
                action: *action,
                diff: diff.as_ref().map(|d| self.redact_text_internal(d, report)),
                lines_changed: *lines_changed,
            },
            EventPayload::OutcomeEvidence(oe) => EventPayload::OutcomeEvidence(OutcomeEvidence {
                kind: oe.kind,
                summary: self.redact_text_internal(&oe.summary, report),
                confidence: oe.confidence,
            }),
            EventPayload::Error {
                message,
                is_recovered,
            } => EventPayload::Error {
                message: self.redact_text_internal(message, report),
                is_recovered: *is_recovered,
            },
            EventPayload::ModelSwitch(ms) => {
                EventPayload::ModelSwitch(agentworth_schema::ModelSwitch {
                    from_model: ms
                        .from_model
                        .as_ref()
                        .map(|m| self.redact_text_internal(m, report)),
                    to_model: self.redact_text_internal(&ms.to_model, report),
                    reason: ms
                        .reason
                        .as_ref()
                        .map(|r| self.redact_text_internal(r, report)),
                })
            }
            EventPayload::HumanIntervention(hi) => {
                EventPayload::HumanIntervention(agentworth_schema::HumanIntervention {
                    action: self.redact_text_internal(&hi.action, report),
                    details: hi
                        .details
                        .as_ref()
                        .map(|d| self.redact_text_internal(d, report)),
                })
            }
            EventPayload::Compaction(c) => {
                EventPayload::Compaction(agentworth_schema::CompactionEvent {
                    trigger: self.redact_text_internal(&c.trigger, report),
                    pre_tokens: c.pre_tokens,
                    post_tokens: c.post_tokens,
                    dropped_tokens: c.dropped_tokens,
                    duration_ms: c.duration_ms,
                })
            }
            EventPayload::Custom { kind, data } => EventPayload::Custom {
                kind: self.redact_text_internal(kind, report),
                data: self.redact_json_internal(data, report),
            },
        };

        let raw_ref = event
            .raw_ref
            .as_ref()
            .map(|r| self.redact_text_internal(r, report));

        NormalizedEvent {
            id: event.id.clone(),
            sequence: event.sequence,
            timestamp: event.timestamp,
            payload,
            raw_ref,
        }
    }

    /// Redacts an entire AgentWorthTrace, returning a sanitized copy.
    pub fn redact_trace(&self, trace: &AgentWorthTrace) -> AgentWorthTrace {
        let mut report = None;
        self.redact_trace_internal(trace, &mut report)
    }

    /// Scans a trace for sensitive elements without modifying it, returning a comprehensive preview report.
    pub fn preview_redactions(&self, trace: &AgentWorthTrace) -> RedactionReport {
        let mut report = RedactionReport::new();
        let mut rep = Some(&mut report);
        let _ = self.redact_trace_internal(trace, &mut rep);
        report
    }

    fn redact_trace_internal(
        &self,
        trace: &AgentWorthTrace,
        report: &mut Option<&mut RedactionReport>,
    ) -> AgentWorthTrace {
        let augmented = self.with_repository_identity_rule(&trace.provenance.source_path);

        let mut sanitized_provenance = trace.provenance.clone();
        sanitized_provenance.source_path =
            augmented.redact_text_internal(&trace.provenance.source_path, report);

        let sanitized_metadata = augmented.redact_json_internal(&trace.metadata, report);

        let sanitized_events = trace
            .events
            .iter()
            .map(|e| augmented.redact_event_internal(e, report))
            .collect();

        AgentWorthTrace {
            session_id: trace.session_id.clone(),
            adapter: trace.adapter.clone(),
            provenance: sanitized_provenance,
            started_at: trace.started_at,
            ended_at: trace.ended_at,
            stats: trace.stats.clone(),
            events: sanitized_events,
            metadata: sanitized_metadata,
        }
    }

    /// Returns a copy of this redactor with an extra rule added that masks this session's own
    /// repository/workspace identity, if one can be derived from `source_path`. A plain clone
    /// (no-op addition) when it can't -- see [`crate::rules::repository_identity_rule`].
    fn with_repository_identity_rule(&self, source_path: &str) -> Self {
        let mut augmented = self.clone();
        let identity = agentworth_schema::extract_repository_or_workspace(source_path);
        if let Some(rule) = crate::rules::repository_identity_rule(&identity) {
            augmented.add_rule(rule);
        }
        augmented
    }

    /// Returns a copy of this redactor augmented with `trace`'s own repository/workspace
    /// identity rule -- the same augmentation [`Redactor::redact_trace`] applies internally,
    /// exposed so callers can redact auxiliary data derived from a trace but not part of the
    /// `AgentWorthTrace` struct itself (outcome evidence, recovery signals, and similar), which
    /// [`Redactor::redact_trace`] alone doesn't touch. Call this once, then use the *same*
    /// returned instance for [`Redactor::redact_trace`] and for
    /// [`Redactor::redact_outcome_evidence`] / [`Redactor::redact_recovery_signal`] on that
    /// trace's own outcomes/recoveries, so repository identity gets masked consistently across
    /// all three rather than only on the trace object.
    pub fn for_trace(&self, trace: &AgentWorthTrace) -> Self {
        self.with_repository_identity_rule(&trace.provenance.source_path)
    }

    /// Redacts free-text fields on outcome evidence.
    ///
    /// `OutcomeEvidence.summary` is derived from event content and can carry the same secrets
    /// an event can, but nothing redacts it today -- [`Redactor::redact_trace`] only ever walks
    /// `trace.events`, `trace.provenance`, and `trace.metadata`. Resolves the gap
    /// `docs/specs/mcp-server.md`'s `session_get` design flagged as new, unbuilt work. Call this
    /// on the same (optionally [`Redactor::for_trace`]-augmented) instance used to redact the
    /// trace itself, so repository/workspace identity gets masked here too.
    pub fn redact_outcome_evidence(&self, evidence: &[OutcomeEvidence]) -> Vec<OutcomeEvidence> {
        evidence
            .iter()
            .map(|e| OutcomeEvidence {
                kind: e.kind,
                summary: self.redact_text(&e.summary),
                confidence: e.confidence,
            })
            .collect()
    }

    /// Redacts free-text fields on recovery signals -- `failure_summary`/`recovery_summary` are
    /// derived from event content, and `correlated_files` carries real file paths. Same gap and
    /// same usage note as [`Redactor::redact_outcome_evidence`] above.
    pub fn redact_recovery_signal(&self, signals: &[RecoverySignal]) -> Vec<RecoverySignal> {
        signals
            .iter()
            .map(|s| RecoverySignal {
                failure_sequence: s.failure_sequence,
                failure_summary: self.redact_text(&s.failure_summary),
                recovery_sequence: s.recovery_sequence,
                recovery_summary: self.redact_text(&s.recovery_summary),
                steps_to_recover: s.steps_to_recover,
                duration_seconds: s.duration_seconds,
                corrective_actions_count: s.corrective_actions_count,
                correlated_files: s
                    .correlated_files
                    .iter()
                    .map(|f| self.redact_text(f))
                    .collect(),
            })
            .collect()
    }
}
