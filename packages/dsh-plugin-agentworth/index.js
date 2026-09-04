/**
 * AgentWorth for DeepSeek Harness.
 *
 * The bundle's patch (`cordis.patch.yml`) mounts the AgentWorth MCP server
 * through `@deepseek-ai/dsh-mcp-client`; this plugin contributes the
 * `agentworth` skill from the bundled `SKILL.md` as a runtime skill, so the
 * package carries everything and no skill root has to exist on disk.
 * @module dsh-plugin-agentworth
 */
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const name = 'agentworth-skill';
export const inject = ['skills'];

const packageDir = dirname(fileURLToPath(import.meta.url));

/**
 * Split a SKILL.md into its frontmatter fields and markdown body.
 * The frontmatter is the flat `key: value` block every agent skill carries;
 * a YAML parser would be a dependency for two fields.
 * @param {string} text - the file contents.
 * @returns {{ fields: Record<string, string>, body: string }}
 */
export function parseSkill(text) {
  const match = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?/.exec(text);
  if (!match) return { fields: {}, body: text };
  const fields = {};
  for (const line of match[1].split(/\r?\n/)) {
    const colon = line.indexOf(':');
    if (colon <= 0) continue;
    fields[line.slice(0, colon).trim()] = line.slice(colon + 1).trim();
  }
  return { fields, body: text.slice(match[0].length) };
}

/**
 * Build the registration for the bundled skill.
 * @param {string} [skillPath] - override for tests; defaults to the bundled SKILL.md.
 * @returns {import('@deepseek-ai/dsh-skill').SkillRegistration}
 */
export function bundledSkill(skillPath = join(packageDir, 'SKILL.md')) {
  const { fields, body } = parseSkill(readFileSync(skillPath, 'utf8'));
  if (!fields.name || !fields.description) {
    throw new Error(`${skillPath}: SKILL.md frontmatter needs name and description`);
  }
  return {
    name: fields.name,
    description: fields.description,
    content: body,
    source: 'bundled',
    path: skillPath,
    resourceBase: { kind: 'directory', path: dirname(skillPath) },
  };
}

/**
 * Register the bundled skill for the life of the row.
 * @param {import('@deepseek-ai/cordis').Context} ctx - a context with `skills` injected.
 */
export function apply(ctx) {
  ctx.effect(() => ctx.skills.register(bundledSkill()), 'agentworth.skill');
}
