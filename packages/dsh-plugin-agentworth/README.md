# dsh-plugin-agentworth

AgentWorth for [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness).
Your agents left receipts; this bundle lets a dsh agent read them.

```bash
dsh plugin --profile <name> add dsh-plugin-agentworth
```

One package, two rows over `dsh-base`:

| row | what the agent gets |
| :--- | :--- |
| `mcp-agentworth` | the 13 read-only AgentWorth MCP tools as `mcp__agentworth__<tool>`: `session_list`, `session_show`, `session_handoff`, `session_carry_forward`, `session_forgotten`, `session_asks`, `repo_blame`, `repo_suspect`, `stats_usage`, `stats_outcomes`, `stats_ladder`, `window_show`, `agent_list` |
| `agentworth-skill` | the `agentworth` skill, so the agent knows when to reach for those tools |

The server runs as `npx -y agentworth@<version> mcp`, the same pinned command
the Claude Code plugin uses: the native binary comes down on first use and
nothing has to be on PATH. Every tool reads the local SQLite index; none scans,
writes, or talks to a network. Run `archie scan` first if the index looks stale.

## Override

Patches replace a row's whole `config`. To point at a binary already on PATH,
restate the row in your profile's `cordis.patch.yml`:

```yaml
- id: mcp-agentworth
  name: '@deepseek-ai/dsh-mcp-client'
  config:
    serverName: agentworth
    transport: stdio
    command: archie
    args: ['mcp']
```

## Develop

```bash
npm test                                        # node --test
dsh plugin --profile dev add ./packages/dsh-plugin-agentworth
dsh --profile dev --dump-config | grep -A8 agentworth
```

`SKILL.md` here is a copy of `skills/agentworth/SKILL.md` at the repo root;
the test suite fails when the two differ.
