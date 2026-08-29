//! Agent history adapters for AgentWorth.

mod claude;
mod codex;
mod gemini;
mod opencode;

pub use claude::ClaudeCodeAdapter;
pub use codex::CodexAdapter;
pub use gemini::{GeminiAdapter, GeminiAdapter as AntigravityAdapter};
pub use opencode::OpenCodeAdapter;
