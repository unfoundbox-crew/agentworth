//! Privacy and redaction engine for scrubbing API keys, credentials, PII, and sensitive paths.

mod entropy;
mod redactor;
mod report;
mod rules;

pub use entropy::EntropyConfig;
pub use redactor::Redactor;
pub use report::{RedactionCategory, RedactionReport};
pub use rules::{default_rules, repository_identity_rule, RedactionRule};

/// Convenience function to redact an entire trace using default security rules.
pub fn redact_trace(
    trace: &agentworth_schema::AgentWorthTrace,
) -> agentworth_schema::AgentWorthTrace {
    Redactor::new().redact_trace(trace)
}
