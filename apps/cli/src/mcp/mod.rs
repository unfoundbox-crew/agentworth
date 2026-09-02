//! `archie mcp`: exposes the local session index as a read-only MCP tool surface over
//! stdio. See `docs/specs/mcp-server.md` for the design.

mod params;
mod server;

pub use server::{run_mcp_server, AgentWorthMcpServer};

#[cfg(test)]
mod tests;
