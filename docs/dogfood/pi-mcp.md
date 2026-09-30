# Dogfood: register `archie mcp` in Pi

Pi reads MCP servers from `~/.pi/agent/mcp.json` (user) and `.pi/mcp.json`
(project, only after trust). The AgentWorth MCP surface is the same stdio
server every other client uses — `archie mcp` — documented in
`docs/specs/mcp-server.md`. This note is the Pi-shaped registration receipt
only; it does not add a Pi adapter beyond what already exists in
`crates/adapters/src/pi.rs`.

## One-liner

```bash
pi mcp add agentworth -- archie mcp
```

That writes a user-level entry (default). Prefer user scope for the same
reason Claude's `--scope user` matters: ask about *any* repo's history from
*any* other checkout. Use `pi mcp add -l agentworth -- archie mcp` only when
the project itself should own the registration, and only in a trusted tree.

If `pi mcp` is unavailable in your Pi build, edit the file directly (below).

## Hand-written `mcp.json`

User file: `~/.pi/agent/mcp.json`

```json
{
  "mcpServers": {
    "agentworth": {
      "command": "archie",
      "args": ["mcp"],
      "description": "Local AgentWorth session index — read-only receipts for what agents already did on this machine"
    }
  }
}
```

`archie` must be on `PATH` (same binary as `agentworth` / `agwt`). A full path
works if your install is elsewhere; keep credentials out of this file — the
server needs none.

Project file (trusted projects only): `.pi/mcp.json` with the same
`mcpServers.agentworth` object. A project entry replaces a user entry with the
same name.

## Codemode auto-load

Pi's default MCP `exposure` is `codemode`. When a server with that exposure
connects, Pi activates the built-in `codemode` tool automatically — you do not
need a separate `"defaultTools": ["+codemode"]` for AgentWorth alone.

Tools appear as `mcp__agentworth__<tool>` inside codemode scripts (for example
`mcp__agentworth__session_wake`). They are not declared as direct model tools
unless you set `"exposure": "direct"` (usually unnecessary for a large
read-only index).

After editing outside a session, run `/reload` or restart Pi, then `/mcp` (or
`pi mcp list`) to confirm `agentworth` is connected.

## Quick check

```bash
pi mcp list
# expect agentworth connected, tools present

# or, without Pi, smoke the same stdio server:
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"dogfood","version":"0"}}}\n' | archie mcp
```

Run `archie scan` first if the index looks empty — the MCP server is
read-only and never scans on its own.
