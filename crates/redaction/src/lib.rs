//! Privacy and redaction engine for scrubbing API keys, credentials, PII, and sensitive paths.

mod redactor;
mod report;
mod rules;

pub use redactor::Redactor;
pub use report::{RedactionCategory, RedactionReport};
pub use rules::{default_rules, RedactionRule};

/// Convenience function to redact an entire trace using default security rules.
pub fn redact_trace(
    trace: &agentworth_schema::AgentWorthTrace,
) -> agentworth_schema::AgentWorthTrace {
    Redactor::new().redact_trace(trace)
}
