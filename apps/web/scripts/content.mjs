// Build-time content: the root CHANGELOG.md and the markdown in content/blog.
// Nothing here runs in a browser. The output is plain data that the page
// components render, so every route can be written to disk as real HTML.
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { Marked } from 'marked';

const here = path.dirname(fileURLToPath(import.meta.url));
export const webRoot = path.resolve(here, '..');
export const repoRoot = path.resolve(webRoot, '../..');

export const SITE = 'https://agentworth.dev';
export const REPO = 'https://github.com/unfoundbox-crew/agentworth';
export const INSTALL_CMD = 'npx -y agentworth scan';

const marked = new Marked({ gfm: true, breaks: false });

/**
 * `(#77)` and bare `#77` become links to the PR.
 *
 * Runs on rendered HTML, so it has to step over two things that look like a
 * PR reference and are not: an anchor that already links the number, and a
 * numeric character reference — `&#39;` for an apostrophe ends in `#39`, and
 * an earlier version of this happily linked every apostrophe on the site to
 * pull request 39.
 */
function linkPrNumbers(html) {
  return html.replace(
    /(<a\b[^>]*>.*?<\/a>)|(&#?[\w]+;)|#(\d{1,5})\b/gs,
    (m, anchor, entity, num) =>
      anchor || entity
        ? m
        : `<a class="pr" href="${REPO}/pull/${num}">#${num}</a>`
  );
}

// ---------------------------------------------------------------- changelog

/**
 * Keep a Changelog: `## [version] - date`, then `### Added` / `### Fixed`
 * sections of bullets. Anything before the first version heading is the
 * file's preamble and is dropped — the page writes its own.
 */
export function parseChangelog() {
  const raw = readFileSync(path.join(repoRoot, 'CHANGELOG.md'), 'utf8');
  const lines = raw.split('\n');

  const releases = [];
  let release = null;
  let section = null;

  const flushSection = () => {
    if (release && section && section.body.trim()) {
      section.html = linkPrNumbers(marked.parse(section.body));
      release.sections.push(section);
    }
    section = null;
  };

  for (const line of lines) {
    const version = line.match(/^##\s+\[([^\]]+)\]\s*-\s*(\S+)\s*$/);
    if (version) {
      flushSection();
      release = {
        version: version[1],
        date: version[2],
        id: `v${version[1]}`,
        sections: [],
      };
      releases.push(release);
      continue;
    }
    const head = line.match(/^###\s+(.+?)\s*$/);
    if (head && release) {
      flushSection();
      section = { title: head[1], body: '', html: '' };
      continue;
    }
    if (section) section.body += line + '\n';
  }
  flushSection();

  if (!releases.length) throw new Error('CHANGELOG.md: no version headings matched');

  for (const r of releases) {
    r.changeCount = r.sections.reduce(
      (n, s) => n + (s.body.match(/^\s*-\s+/gm)?.length ?? 0),
      0
    );
  }
  return releases;
}

// ------------------------------------------------------------------ reference

/**
 * `docs/reference.json` and `docs/REFERENCE.md` are both written by `agentworth docs
 * --write` (see `apps/cli/src/commands/docs.rs`) and committed like `CHANGELOG.md` is --
 * this just reads the committed files, it never invokes cargo, so the site build has no
 * Rust toolchain dependency.
 */
export function parseReference() {
  const jsonPath = path.join(repoRoot, 'docs/reference.json');
  if (!existsSync(jsonPath)) {
    throw new Error(
      'docs/reference.json missing -- run `cargo run -p agentworth-cli -- docs --write` from the repo root and commit the result'
    );
  }
  const reference = JSON.parse(readFileSync(jsonPath, 'utf8'));
  reference.markdown = readFileSync(path.join(repoRoot, 'docs/REFERENCE.md'), 'utf8');
  return reference;
}

// --------------------------------------------------------------------- blog

/** Front matter: `key: value` lines between `---` fences. `tags` takes a
 *  bracketed list. Deliberately not YAML — the posts are written here. */
function parseFrontMatter(raw, file) {
  const m = raw.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n([\s\S]*)$/);
  if (!m) throw new Error(`${file}: missing front matter`);
  const meta = {};
  for (const line of m[1].split('\n')) {
    if (!line.trim()) continue;
    const kv = line.match(/^([A-Za-z_][\w-]*)\s*:\s*(.*)$/);
    if (!kv) throw new Error(`${file}: bad front-matter line: ${line}`);
    // A quoted value may escape its own quote character; unescape it after
    // unwrapping, or the backslash reaches the page title.
    let value = kv[2].trim();
    const quoted = value.match(/^(["'])([\s\S]*)\1$/);
    if (quoted) value = quoted[2].replace(new RegExp(`\\\\${quoted[1]}`, 'g'), quoted[1]);
    meta[kv[1]] =
      kv[1] === 'tags'
        ? value
            .replace(/^\[|\]$/g, '')
            .split(',')
            .map((t) => t.trim().replace(/^["']|["']$/g, ''))
            .filter(Boolean)
        : value;
  }
  return { meta, body: m[2] };
}

/** ~230 wpm, the usual figure for technical prose. Rounded up, never 0. */
const readingMinutes = (text) =>
  Math.max(1, Math.round(text.trim().split(/\s+/).length / 230));

export function parsePosts() {
  const dir = path.join(webRoot, 'content/blog');
  if (!existsSync(dir)) return [];

  const posts = readdirSync(dir)
    .filter((f) => f.endsWith('.md'))
    .map((file) => {
      const raw = readFileSync(path.join(dir, file), 'utf8');
      const { meta, body } = parseFrontMatter(raw, file);
      for (const key of ['title', 'date', 'description']) {
        if (!meta[key]) throw new Error(`${file}: front matter needs "${key}"`);
      }
      const slug = meta.slug || file.replace(/^\d+-/, '').replace(/\.md$/, '');
      return {
        file,
        slug,
        title: meta.title,
        date: meta.date,
        description: meta.description,
        tags: meta.tags ?? [],
        author: meta.author || 'AgentWorth',
        canonical: meta.canonical || `${SITE}/blog/${slug}/`,
        readingMinutes: readingMinutes(body),
        words: body.trim().split(/\s+/).length,
        html: linkPrNumbers(marked.parse(body)),
      };
    })
    // Newest first. Posts published the same day tie-break on the numeric
    // filename prefix, so the order on the page is authored, not incidental.
    .sort((a, b) =>
      a.date === b.date ? a.file.localeCompare(b.file) : a.date < b.date ? 1 : -1
    );

  // Newest first, so prev is the newer neighbour and next is the older one.
  posts.forEach((p, i) => {
    p.newer = i > 0 ? { slug: posts[i - 1].slug, title: posts[i - 1].title } : null;
    p.older =
      i < posts.length - 1
        ? { slug: posts[i + 1].slug, title: posts[i + 1].title }
        : null;
  });

  const seen = new Set();
  for (const p of posts) {
    if (seen.has(p.slug)) throw new Error(`duplicate blog slug: ${p.slug}`);
    seen.add(p.slug);
  }
  return posts;
}

export const isoDate = (d) => new Date(`${d}T00:00:00Z`).toISOString();

export const rfc822 = (d) =>
  new Date(`${d}T00:00:00Z`).toUTCString().replace('GMT', '+0000');

export const humanDate = (d) =>
  new Date(`${d}T00:00:00Z`).toLocaleDateString('en-US', {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
    timeZone: 'UTC',
  });

export const escapeXml = (s) =>
  String(s).replace(
    /[<>&'"]/g,
    (c) => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;', "'": '&apos;', '"': '&quot;' })[c]
  );
