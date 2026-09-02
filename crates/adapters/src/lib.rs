//! Agent history adapters for AgentWorth.

mod aider;
mod claude;
mod cline;
mod codex;
mod cursor;
mod deepseek;
mod exit_status;
mod gemini;
mod goose;
mod grok;
mod herdr;
mod hermes;
mod kimi;
mod manus;
mod mcp;
mod minimax;
mod openclaw;
mod opencode;
mod pi;
mod qwen;
mod windsurf;
mod zhipu;

pub use aider::AiderAdapter;
pub use claude::ClaudeCodeAdapter;
pub use cline::ClineAdapter;
pub use codex::CodexAdapter;
pub use cursor::CursorAdapter;
pub use deepseek::DeepSeekAdapter;
pub use exit_status::{backfill_shell_exit_codes, exit_code_from_result, parse_exit_code_phrase};
pub use gemini::{GeminiAdapter, GeminiAdapter as AntigravityAdapter};
pub use goose::GooseAdapter;
pub use grok::GrokAdapter;
pub use herdr::HerdrAdapter;
pub use hermes::HermesAdapter;
pub use kimi::KimiAdapter;
pub use manus::ManusAdapter;
pub use mcp::normalize_mcp_tool_name;
pub use minimax::MiniMaxAdapter;
pub use openclaw::OpenClawAdapter;
pub use opencode::OpenCodeAdapter;
pub use pi::PiAdapter;
pub use qwen::QwenAdapter;
pub use windsurf::WindsurfAdapter;
pub use zhipu::ZhipuAdapter;

use agentworth_adapter_sdk::AgentAdapter;

/// The single canonical list of every registered adapter. `crates/core::Scanner`, the
/// `agentworth matrix` CLI command, and the `/api/matrix` coverage endpoint each used to
/// keep their own hand-typed copy of this list; a new adapter added here without also being
/// added to those copies silently never showed up in scans or in the coverage matrix (see
/// the `docs/capability-matrix.md` writeup on the "antigravity" adapter identity, which is
/// a related but distinct join bug -- see `AgentAdapter::identity_names`). Callers that need
/// "every registered adapter" should build their list from this function instead of
/// hand-copying it again.
pub fn all_adapters() -> Vec<Box<dyn AgentAdapter>> {
    vec![
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
    ]
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    /// Every adapter's `name()` must be unique, and `identity_names()` must include it --
    /// otherwise a join keyed by name (the coverage matrix, `Scanner::load_trace`) can't
    /// find sessions this adapter itself produced.
    #[test]
    fn test_all_adapters_names_are_unique_and_self_consistent() {
        let adapters = all_adapters();
        let mut seen = std::collections::HashSet::new();
        for adapter in &adapters {
            assert!(
                seen.insert(adapter.name()),
                "duplicate adapter name in registry: {}",
                adapter.name()
            );
            assert!(
                adapter.identity_names().contains(&adapter.name()),
                "{}'s identity_names() must include its own name()",
                adapter.name()
            );
        }
        assert_eq!(adapters.len(), 20, "update this count when adding/removing an adapter");
    }
}
