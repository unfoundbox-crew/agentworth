//! Agent history adapters for AgentWorth.

mod claude;
mod codex;
mod cursor;
mod gemini;
mod goose;
mod grok;
mod herdr;
mod hermes;
mod openclaw;
mod opencode;
mod pi;

pub use claude::ClaudeCodeAdapter;
pub use codex::CodexAdapter;
pub use cursor::CursorAdapter;
pub use gemini::{GeminiAdapter, GeminiAdapter as AntigravityAdapter};
pub use goose::GooseAdapter;
pub use grok::GrokAdapter;
pub use herdr::HerdrAdapter;
pub use hermes::HermesAdapter;
pub use openclaw::OpenClawAdapter;
pub use opencode::OpenCodeAdapter;
pub use pi::PiAdapter;
