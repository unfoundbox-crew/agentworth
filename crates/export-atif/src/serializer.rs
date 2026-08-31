use crate::error::Result;
use crate::models::{
    AtifAgent, AtifEnvironment, AtifError, AtifFileAction, AtifHumanIntervention, AtifMetrics,
    AtifModelInvocation, AtifOutcomeEvidence, AtifShellCommand, AtifStep, AtifStepSource,
    AtifTokens, AtifToolCall, AtifToolInfo, AtifToolResult, AtifTrajectory,
};
use agentworth_redaction::Redactor;
use agentworth_schema::{AgentWorthTrace, EventPayload};

pub const ATIF_SCHEMA_VERSION: &str = "atif-v1.0";

impl AtifTrajectory {
    /// Constructs an ATIF representation from a canonical AgentWorth trace.
    pub fn from_trace(trace: &AgentWorthTrace) -> Self {
        let agent = AtifAgent {
            name: trace.adapter.clone(),
            adapter: trace.adapter.clone(),
            models: trace.stats.models_used.clone(),
            version: None,
        };

        let environment = AtifEnvironment {
            source_path: trace.provenance.source_path.clone(),
            adapter: trace.adapter.clone(),
            started_at: trace.started_at,
            ended_at: trace.ended_at,
            duration_seconds: trace.stats.duration_seconds,
        };

        let steps: Vec<AtifStep> = trace
            .events
            .iter()
            .map(|event| {
                let mut content = None;
                let mut thinking = None;
                let mut tool_calls = None;
                let mut tool_results = None;
                let mut shell_command = None;
                let mut file_action = None;
                let mut model_invocation = None;
                let mut outcome_evidence = None;
                let mut error = None;
                let mut human_intervention = None;

                let (source, step_type) = match &event.payload {
                    EventPayload::UserMessage { content: c } => {
                        content = Some(c.clone());
                        (AtifStepSource::User, "user_message".to_string())
                    }
                    EventPayload::AssistantMessage {
                        content: c,
                        thinking: t,
                    } => {
                        content = Some(c.clone());
                        thinking = t.clone();
                        (AtifStepSource::Agent, "assistant_message".to_string())
                    }
                    EventPayload::ModelInvocation {
                        model,
                        token_usage,
                        cost_usd,
                        latency_ms,
                    } => {
                        model_invocation = Some(AtifModelInvocation {
                            model: model.clone(),
                            input_tokens: token_usage.input_tokens,
                            output_tokens: token_usage.output_tokens,
                            cache_read_tokens: token_usage.cache_read_tokens,
                            cache_creation_tokens: token_usage.cache_creation_tokens,
                            cost_usd: *cost_usd,
                            latency_ms: *latency_ms,
                        });
                        (AtifStepSource::Agent, "model_invocation".to_string())
                    }
                    EventPayload::ToolCall(tc) => {
                        tool_calls = Some(vec![AtifToolCall {
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                        }]);
                        (AtifStepSource::Agent, "tool_call".to_string())
                    }
                    EventPayload::ToolResult(tr) => {
                        tool_results = Some(vec![AtifToolResult {
                            call_id: tr.call_id.clone(),
                            name: tr.name.clone(),
                            output: tr.output.clone(),
                            is_error: tr.is_error,
                        }]);
                        (AtifStepSource::Tool, "tool_result".to_string())
                    }
                    EventPayload::ShellCommand(sc) => {
                        shell_command = Some(AtifShellCommand {
                            command: sc.command.clone(),
                            cwd: sc.cwd.clone(),
                            exit_code: sc.exit_code,
                            output: sc.output.clone(),
                        });
                        (AtifStepSource::Environment, "shell_command".to_string())
                    }
                    EventPayload::FileAction {
                        path,
                        action,
                        diff,
                        lines_changed,
                    } => {
                        file_action = Some(AtifFileAction {
                            path: path.clone(),
                            action: *action,
                            diff: diff.clone(),
                            lines_changed: *lines_changed,
                        });
                        (AtifStepSource::Agent, "file_action".to_string())
                    }
                    EventPayload::OutcomeEvidence(oe) => {
                        outcome_evidence = Some(AtifOutcomeEvidence {
                            kind: format!("{:?}", oe.kind),
                            summary: oe.summary.clone(),
                            confidence: oe.confidence,
                        });
                        (AtifStepSource::Environment, "outcome_evidence".to_string())
                    }
                    EventPayload::Error {
                        message,
                        is_recovered,
                    } => {
                        error = Some(AtifError {
                            message: message.clone(),
                            is_recovered: *is_recovered,
                        });
                        (AtifStepSource::System, "error".to_string())
                    }
                    EventPayload::ModelSwitch(ms) => {
                        content = Some(format!("Switched model to {}", ms.to_model));
                        (AtifStepSource::User, "model_switch".to_string())
                    }
                    EventPayload::HumanIntervention(hi) => {
                        human_intervention = Some(AtifHumanIntervention {
                            action: hi.action.clone(),
                            details: hi.details.clone(),
                        });
                        (AtifStepSource::User, "human_intervention".to_string())
                    }
                    EventPayload::Custom { kind, data } => {
                        content = Some(data.to_string());
                        (AtifStepSource::System, format!("custom_{}", kind))
                    }
                };

                AtifStep {
                    step_id: event.id.clone(),
                    sequence: event.sequence,
                    timestamp: event.timestamp,
                    source,
                    step_type,
                    content,
                    thinking,
                    tool_calls,
                    tool_results,
                    shell_command,
                    file_action,
                    model_invocation,
                    outcome_evidence,
                    error,
                    human_intervention,
                    raw_ref: event.raw_ref.clone(),
                }
            })
            .collect();

        let tools: Vec<AtifToolInfo> = trace
            .stats
            .tools_used
            .iter()
            .map(|(name, count)| AtifToolInfo {
                name: name.clone(),
                call_count: *count,
            })
            .collect();

        let metrics = AtifMetrics {
            total_events: trace.stats.total_events,
            user_messages_count: trace.stats.user_messages_count,
            assistant_messages_count: trace.stats.assistant_messages_count,
            tool_calls_count: trace.stats.tool_calls_count,
            duration_seconds: trace.stats.duration_seconds,
        };

        let tokens = AtifTokens {
            input_tokens: trace.stats.token_usage.input_tokens,
            output_tokens: trace.stats.token_usage.output_tokens,
            cache_read_tokens: trace.stats.token_usage.cache_read_tokens,
            cache_creation_tokens: trace.stats.token_usage.cache_creation_tokens,
            total_tokens: trace.stats.token_usage.total(),
        };

        Self {
            schema_version: ATIF_SCHEMA_VERSION.to_string(),
            session_id: trace.session_id.clone(),
            agent,
            environment,
            steps,
            tools,
            metrics,
            tokens,
            metadata: trace.metadata.clone(),
        }
    }
}

/// Serializes an AgentWorthTrace into standard ATIF JSON format.
pub fn export_to_atif(trace: &AgentWorthTrace, pretty: bool) -> Result<String> {
    let trajectory = AtifTrajectory::from_trace(trace);
    if pretty {
        Ok(serde_json::to_string_pretty(&trajectory)?)
    } else {
        Ok(serde_json::to_string(&trajectory)?)
    }
}

/// Applies redaction rules to a trace before exporting to ATIF.
pub fn export_redacted_atif(
    trace: &AgentWorthTrace,
    redactor: &Redactor,
    pretty: bool,
) -> Result<String> {
    let sanitized_trace = redactor.redact_trace(trace);
    export_to_atif(&sanitized_trace, pretty)
}

/// Exporter builder for customized ATIF generation.
#[derive(Debug, Clone, Default)]
pub struct AtifExporter {
    pretty: bool,
    redactor: Option<Redactor>,
}

impl AtifExporter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pretty(mut self, pretty: bool) -> Self {
        self.pretty = pretty;
        self
    }

    pub fn with_redactor(mut self, redactor: Redactor) -> Self {
        self.redactor = Some(redactor);
        self
    }

    pub fn export(&self, trace: &AgentWorthTrace) -> Result<String> {
        if let Some(ref redactor) = self.redactor {
            export_redacted_atif(trace, redactor, self.pretty)
        } else {
            export_to_atif(trace, self.pretty)
        }
    }
}
