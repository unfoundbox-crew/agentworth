import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { apply, bundledSkill, parseSkill } from '../index.js';

const here = dirname(fileURLToPath(import.meta.url));
const packageDir = join(here, '..');
const repoRoot = join(packageDir, '..', '..');
const read = (p) => readFileSync(p, 'utf8');

test('the bundled SKILL.md is the repo skill, byte for byte', () => {
  assert.equal(read(join(packageDir, 'SKILL.md')), read(join(repoRoot, 'skills', 'agentworth', 'SKILL.md')));
});

test('parseSkill splits frontmatter from the body', () => {
  const { fields, body } = parseSkill('---\nname: x\ndescription: a: b\n---\n# Body\n');
  assert.deepEqual(fields, { name: 'x', description: 'a: b' });
  assert.equal(body, '# Body\n');
  assert.deepEqual(parseSkill('no frontmatter'), { fields: {}, body: 'no frontmatter' });
});

test('bundledSkill is a complete runtime registration', () => {
  const skill = bundledSkill();
  assert.equal(skill.name, 'agentworth');
  assert.match(skill.description, /local-first/i);
  assert.equal(skill.source, 'bundled');
  assert.ok(!skill.content.startsWith('---'), 'frontmatter must be stripped from the body');
  assert.match(skill.content, /# AgentWorth/);
  assert.deepEqual(skill.resourceBase, { kind: 'directory', path: packageDir });
});

test('apply registers the skill inside an effect and disposes with it', () => {
  const calls = [];
  let disposed = 0;
  const ctx = {
    skills: { register(skill) { calls.push(skill); return () => { disposed += 1; }; } },
    effect(fn, label) { assert.equal(label, 'agentworth.skill'); const dispose = fn(); dispose(); },
  };
  apply(ctx);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].name, 'agentworth');
  assert.equal(disposed, 1);
});

test('the patch pins the npx package to this package version', () => {
  const patch = read(join(packageDir, 'cordis.patch.yml'));
  const { version } = JSON.parse(read(join(packageDir, 'package.json')));
  assert.match(patch, new RegExp(`'agentworth@${version.replace(/\\./g, '\\\\.')}'`));
  assert.match(patch, /serverName: agentworth\b/);
  assert.match(patch, /name: dsh-plugin-agentworth\b/);
  assert.match(patch, /name: '@deepseek-ai\/dsh-mcp-client'/);
});

test('package.json declares the bundle patch and ships every file the plugin reads', () => {
  const pkg = JSON.parse(read(join(packageDir, 'package.json')));
  assert.equal(pkg.dsh.bundle.patch, './cordis.patch.yml');
  for (const f of ['index.js', 'cordis.patch.yml', 'SKILL.md']) assert.ok(pkg.files.includes(f), f);
});
