// Turns the SPA build into real HTML: one index.html per route, each with its
// own title, description, canonical, Open Graph, Twitter card and JSON-LD,
// plus the feeds, sitemap, robots.txt and llms.txt the site advertises.
//
// Runs after `vite build` and `vite build --ssr`. React 18 has no
// react-dom/static, so this uses renderToString from react-dom/server —
// prerenderToNodeStream, which React now recommends for exactly this job,
// lands with React 19.
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import path from 'node:path';
import {
  webRoot,
  SITE,
  REPO,
  isoDate,
  rfc822,
  humanDate,
  escapeXml,
  docPath,
} from './content.mjs';
import { readLockup, postCardSvg, renderSvgToPng } from './og-template.mjs';

const dist = path.join(webRoot, 'dist');
const { render } = await import(path.join(webRoot, 'dist-ssr/entry-server.js'));
const { releases, posts, reference, docs, downloads } = JSON.parse(
  readFileSync(path.join(webRoot, 'src/content.generated.json'), 'utf8')
);
const allDocs = [...docs.learn, ...docs.specs, ...docs.research];

const template = readFileSync(path.join(dist, 'index.html'), 'utf8');
const OG_DEFAULT = `${SITE}/og.png`;
const TWITTER = '@unfoundbox';

const write = (rel, body) => {
  const file = path.join(dist, rel);
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, body);
  return file;
};

const esc = (s) =>
  String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

const ld = (obj) =>
  `<script type="application/ld+json">\n${JSON.stringify(obj, null, 2)}\n</script>`;

const crumbs = (items) => ({
  '@type': 'BreadcrumbList',
  itemListElement: items.map((it, i) => ({
    '@type': 'ListItem',
    position: i + 1,
    name: it.name,
    item: it.url,
  })),
});

// ------------------------------------------------------------------- heads

/**
 * One page's head. `type` is "website" or "article"; article pages add the
 * article:* properties and swap og:image for their own card.
 */
function head({
  title,
  description,
  canonical,
  type = 'website',
  image = OG_DEFAULT,
  imageAlt,
  published,
  modified,
  tags = [],
  jsonLd = [],
  feeds = [],
}) {
  const lines = [
    `<title>${esc(title)}</title>`,
    `<meta name="description" content="${esc(description)}" />`,
    `<meta name="author" content="AgentWorth" />`,
    `<meta name="robots" content="index, follow, max-image-preview:large, max-snippet:-1, max-video-preview:-1" />`,
    `<link rel="canonical" href="${canonical}" />`,
    ``,
    `<meta property="og:type" content="${type}" />`,
    `<meta property="og:url" content="${canonical}" />`,
    `<meta property="og:title" content="${esc(title)}" />`,
    `<meta property="og:description" content="${esc(description)}" />`,
    `<meta property="og:image" content="${image}" />`,
    `<meta property="og:image:width" content="1200" />`,
    `<meta property="og:image:height" content="630" />`,
    `<meta property="og:image:alt" content="${esc(imageAlt ?? title)}" />`,
    `<meta property="og:site_name" content="AgentWorth" />`,
    `<meta property="og:locale" content="en_US" />`,
  ];

  if (type === 'article') {
    lines.push(
      `<meta property="article:published_time" content="${published}" />`,
      `<meta property="article:modified_time" content="${modified ?? published}" />`,
      `<meta property="article:author" content="AgentWorth" />`,
      `<meta property="article:section" content="Engineering" />`,
      ...tags.map((t) => `<meta property="article:tag" content="${esc(t)}" />`)
    );
  }

  lines.push(
    ``,
    `<meta name="twitter:card" content="summary_large_image" />`,
    `<meta name="twitter:site" content="${TWITTER}" />`,
    `<meta name="twitter:creator" content="${TWITTER}" />`,
    `<meta name="twitter:title" content="${esc(title)}" />`,
    `<meta name="twitter:description" content="${esc(description)}" />`,
    `<meta name="twitter:image" content="${image}" />`,
    ``
  );

  for (const f of feeds) {
    lines.push(
      `<link rel="alternate" type="application/rss+xml" title="${esc(f.title)}" href="${f.href}" />`
    );
  }
  // llms.txt spec v2 advertises itself with rel="describedby".
  lines.push(
    `<link rel="describedby" type="text/plain" href="/llms.txt" title="LLM knowledge manifest" />`,
    ``,
    ...jsonLd.map(ld)
  );

  return lines.map((l) => (l ? `    ${l}` : '')).join('\n');
}

function page(route, headHtml) {
  const app = render(route);
  return template
    .replace(/\s*<!--@dev-head-->[\s\S]*?<!--\/@dev-head-->/, '')
    .replace('    <!--@head-->', headHtml)
    .replace('<!--@app-->', app);
}

// ------------------------------------------------------------------ routes

const ORG = {
  '@type': 'Organization',
  '@id': `${SITE}/#org`,
  name: 'Unfoundbox Crew',
  url: 'https://github.com/unfoundbox-crew',
};

const WEBSITE = {
  '@type': 'WebSite',
  '@id': `${SITE}/#website`,
  url: `${SITE}/`,
  name: 'AgentWorth',
  publisher: { '@id': `${SITE}/#org` },
};

const SOFTWARE = {
  '@type': 'SoftwareApplication',
  '@id': `${SITE}/#software`,
  name: 'AgentWorth',
  operatingSystem: 'macOS, Linux',
  applicationCategory: 'DeveloperApplication',
  description:
    'A local-first native Rust tool that reads the session logs AI coding agents leave on disk and grades each claimed outcome against files changed, tests run, commits made and CI results.',
  url: `${SITE}/`,
  softwareVersion: releases[0].version,
  downloadUrl: `${REPO}/releases`,
  license: `${REPO}/blob/main/LICENSE`,
  offers: { '@type': 'Offer', price: '0', priceCurrency: 'USD' },
  author: { '@id': `${SITE}/#org` },
};

const FEEDS = [
  { title: 'AgentWorth blog', href: '/blog/rss.xml' },
  { title: 'AgentWorth releases', href: '/changelog/rss.xml' },
];

const FAQ = {
  '@type': 'FAQPage',
  '@id': `${SITE}/#faq`,
  mainEntity: [
    [
      'Does AgentWorth upload my code or transcripts to the cloud?',
      'No. AgentWorth is 100% local-first and offline by design. It scans dotfiles on your machine and stores an index in a local SQLite database. Nothing ever leaves your computer.',
    ],
    [
      'Which AI coding agents are supported?',
      'AgentWorth supports 21 native adapters: Claude Code, Google Antigravity (agy), Cursor Composer, OpenAI Codex, Block Goose, Pi, Herdr, Nous Hermes, OpenClaw, xAI Grok, OpenCode, DeepSeek Code, Kimi Code, MiniMax, Qwen Code, Zhipu/CodeGeeX, Aider, Cline & Roo-Code, Windsurf/Cascade, and Manus.',
    ],
    [
      'How do I run AgentWorth?',
      "Run 'npx -y agentworth scan' to index the agent logs already on your machine, or install the native binary with 'curl -fsSL https://agentworth.dev/install.sh | sh'.",
    ],
  ].map(([name, text]) => ({
    '@type': 'Question',
    name,
    acceptedAnswer: { '@type': 'Answer', text },
  })),
};

const pages = [];

// /
pages.push({
  route: '/',
  file: 'index.html',
  head: head({
    title: "AgentWorth — every agent says it's done. Check the git log.",
    description:
      'AgentWorth reads the session logs your AI coding agents already left on disk and checks what they claimed against files changed, tests run, commits made and CI. 21 harnesses, 100% local, nothing uploaded.',
    canonical: `${SITE}/`,
    imageAlt: 'AgentWorth — AI coding agent receipts and archaeology',
    feeds: FEEDS,
    jsonLd: [{ '@context': 'https://schema.org', '@graph': [ORG, WEBSITE, SOFTWARE, FAQ] }],
  }),
});

// /changelog/
pages.push({
  route: '/changelog/',
  file: 'changelog/index.html',
  head: head({
    title: `Changelog — AgentWorth ${releases[0].version} and every release before it`,
    description: `What changed in each AgentWorth release, with the pull request behind every line. Latest: ${releases[0].version}, ${humanDate(releases[0].date)}, ${releases[0].changeCount} changes.`,
    canonical: `${SITE}/changelog/`,
    feeds: FEEDS,
    jsonLd: [
      {
        '@context': 'https://schema.org',
        '@graph': [
          ORG,
          {
            ...SOFTWARE,
            '@id': `${SITE}/changelog/#software`,
            // schema.org defines releaseNotes on SoftwareApplication. Google
            // does not document it for rich results; it is here because it is
            // valid and machine-readable, not because it earns a snippet.
            releaseNotes: `${SITE}/changelog/`,
          },
          crumbs([
            { name: 'Home', url: `${SITE}/` },
            { name: 'Changelog', url: `${SITE}/changelog/` },
          ]),
        ],
      },
    ],
  }),
});

// /docs/
pages.push({
  route: '/docs/',
  file: 'docs/index.html',
  head: head({
    title: 'Docs — AgentWorth',
    description: `Guides, the generated CLI/API/MCP reference, the specs behind every feature, and the research that fed them. ${docs.learn.length} guides, ${docs.specs.length} specs, ${docs.research.length} research memos, and v${reference.version} of the reference.`,
    canonical: `${SITE}/docs/`,
    feeds: FEEDS,
    jsonLd: [
      {
        '@context': 'https://schema.org',
        '@graph': [
          ORG,
          {
            '@type': 'CollectionPage',
            '@id': `${SITE}/docs/#page`,
            url: `${SITE}/docs/`,
            name: 'AgentWorth documentation',
            about: { '@id': `${SITE}/#software` },
            publisher: { '@id': `${SITE}/#org` },
          },
          crumbs([
            { name: 'Home', url: `${SITE}/` },
            { name: 'Docs', url: `${SITE}/docs/` },
          ]),
        ],
      },
    ],
  }),
});

// /docs/specs/ and /docs/research/
const SECTION_INDEXES = [
  {
    section: 'specs',
    name: 'Specs',
    title: 'Specs — the design doc behind every AgentWorth feature',
    description: `Every feature here was specified and measured before it was built. ${docs.specs.length} design documents, read straight out of docs/specs in the repository: the problem, the measurement, what shipped, and what was deliberately left out.`,
  },
  {
    section: 'research',
    name: 'Research',
    title: 'Research — memos that fed a spec',
    description: `${docs.research.length} research memos behind the AgentWorth specs. Every claim carries its source; unverified means no primary source was found, not false. Context only, never a decision.`,
  },
];

for (const idx of SECTION_INDEXES) {
  pages.push({
    route: `/docs/${idx.section}/`,
    file: `docs/${idx.section}/index.html`,
    head: head({
      title: idx.title,
      description: idx.description,
      canonical: `${SITE}/docs/${idx.section}/`,
      feeds: FEEDS,
      jsonLd: [
        {
          '@context': 'https://schema.org',
          '@graph': [
            ORG,
            {
              '@type': 'CollectionPage',
              '@id': `${SITE}/docs/${idx.section}/#page`,
              url: `${SITE}/docs/${idx.section}/`,
              name: `AgentWorth ${idx.name.toLowerCase()}`,
              publisher: { '@id': `${SITE}/#org` },
            },
            crumbs([
              { name: 'Home', url: `${SITE}/` },
              { name: 'Docs', url: `${SITE}/docs/` },
              { name: idx.name, url: `${SITE}/docs/${idx.section}/` },
            ]),
          ],
        },
      ],
    }),
  });
}

// /docs/learn/<slug>/, /docs/specs/<slug>/, /docs/research/<slug>/
const SECTION_NAME = { learn: 'Learn', specs: 'Specs', research: 'Research' };
const SECTION_URL = {
  learn: `${SITE}/docs/`,
  specs: `${SITE}/docs/specs/`,
  research: `${SITE}/docs/research/`,
};

for (const doc of allDocs) {
  const url = `${SITE}${docPath(doc.section, doc.slug)}`;
  pages.push({
    route: docPath(doc.section, doc.slug),
    file: `docs/${doc.section}/${doc.slug}/index.html`,
    head: head({
      title: `${doc.title} — AgentWorth docs`,
      description: doc.description || `${doc.title}, from the AgentWorth documentation.`,
      canonical: url,
      feeds: FEEDS,
      jsonLd: [
        {
          '@context': 'https://schema.org',
          '@graph': [
            ORG,
            {
              '@type': 'TechArticle',
              '@id': `${url}#article`,
              headline: doc.title,
              description: doc.description || undefined,
              url,
              mainEntityOfPage: { '@type': 'WebPage', '@id': url },
              about: { '@id': `${SITE}/#software` },
              publisher: { '@id': `${SITE}/#org` },
            },
            crumbs([
              { name: 'Home', url: `${SITE}/` },
              { name: 'Docs', url: `${SITE}/docs/` },
              { name: SECTION_NAME[doc.section], url: SECTION_URL[doc.section] },
              { name: doc.title, url },
            ]),
          ],
        },
      ],
    }),
  });
}

// /docs/reference/
pages.push({
  route: '/docs/reference/',
  file: 'docs/reference/index.html',
  head: head({
    title: `Reference — CLI, API, and MCP tools (AgentWorth ${reference.version})`,
    description: `Every agentworth subcommand and flag, every local API route, and every MCP tool with its schema -- generated from the code, not hand-written. v${reference.version}, ${reference.cli.length} commands, ${reference.api.length} routes, ${reference.mcp.length} MCP tools.`,
    canonical: `${SITE}/docs/reference/`,
    feeds: FEEDS,
    jsonLd: [
      {
        '@context': 'https://schema.org',
        '@graph': [
          ORG,
          {
            '@type': 'TechArticle',
            '@id': `${SITE}/docs/reference/#article`,
            headline: 'AgentWorth reference: CLI, API, and MCP tools',
            url: `${SITE}/docs/reference/`,
            dateModified: reference.generatedDate,
            about: { '@id': `${SITE}/#software` },
            publisher: { '@id': `${SITE}/#org` },
          },
          crumbs([
            { name: 'Home', url: `${SITE}/` },
            { name: 'Reference', url: `${SITE}/docs/reference/` },
          ]),
        ],
      },
    ],
  }),
});

// /archie/
pages.push({
  route: '/archie/',
  file: 'archie/index.html',
  head: head({
    title: 'Archie — the AgentWorth mascot, and what he digs up',
    description:
      "Archie is the hound with the headlamp who reads the session logs your AI coding agents already left on disk. Try his kit and his four colourways, and see the three things he fetches: forgotten decisions, the last handoff, and the questions still open.",
    canonical: `${SITE}/archie/`,
    imageAlt: 'Archie, the AgentWorth mascot',
    feeds: FEEDS,
    jsonLd: [
      {
        '@context': 'https://schema.org',
        '@graph': [
          ORG,
          {
            '@type': 'AboutPage',
            '@id': `${SITE}/archie/#page`,
            url: `${SITE}/archie/`,
            name: 'Archie',
            about: { '@id': `${SITE}/#software` },
            publisher: { '@id': `${SITE}/#org` },
          },
          crumbs([
            { name: 'Home', url: `${SITE}/` },
            { name: 'Archie', url: `${SITE}/archie/` },
          ]),
        ],
      },
    ],
  }),
});

// The 404. Written to dist/404.html at the output root, which is where a static host
// looks for the page it serves when nothing else matches. noindex, and out of the
// sitemap: it is a real 404, not a page anyone should be sent to.
pages.push({
  route: '/404/',
  file: '404.html',
  head: head({
    title: 'Not found — AgentWorth',
    description: 'No page at that address. The reference lists every command, route and MCP tool.',
    canonical: `${SITE}/404/`,
  }).replace(
    '<meta name="robots" content="index, follow, max-image-preview:large, max-snippet:-1, max-video-preview:-1" />',
    '<meta name="robots" content="noindex, follow" />'
  ),
});

// /blog/
pages.push({
  route: '/blog/',
  file: 'blog/index.html',
  head: head({
    title: 'Blog — AgentWorth',
    description:
      'Notes from measuring our own coding agents. Every number has a spec or a pull request behind it, and where a measurement is narrow the post says how narrow.',
    canonical: `${SITE}/blog/`,
    feeds: FEEDS,
    jsonLd: [
      {
        '@context': 'https://schema.org',
        '@graph': [
          ORG,
          {
            '@type': 'Blog',
            '@id': `${SITE}/blog/#blog`,
            url: `${SITE}/blog/`,
            name: 'AgentWorth blog',
            description: 'Notes from measuring our own coding agents.',
            publisher: { '@id': `${SITE}/#org` },
            blogPost: posts.map((p) => ({
              '@type': 'BlogPosting',
              '@id': `${SITE}/blog/${p.slug}/#post`,
              headline: p.title,
              url: `${SITE}/blog/${p.slug}/`,
              datePublished: isoDate(p.date),
            })),
          },
          crumbs([
            { name: 'Home', url: `${SITE}/` },
            { name: 'Blog', url: `${SITE}/blog/` },
          ]),
        ],
      },
    ],
  }),
});

// /blog/<slug>/
for (const p of posts) {
  const url = `${SITE}/blog/${p.slug}/`;
  const image = `${SITE}/og/blog/${p.slug}.png`;
  pages.push({
    route: `/blog/${p.slug}/`,
    file: `blog/${p.slug}/index.html`,
    head: head({
      title: `${p.title} — AgentWorth`,
      description: p.description,
      canonical: p.canonical || url,
      type: 'article',
      image,
      imageAlt: p.title,
      published: isoDate(p.date),
      tags: p.tags,
      feeds: FEEDS,
      jsonLd: [
        {
          '@context': 'https://schema.org',
          '@graph': [
            ORG,
            {
              '@type': 'BlogPosting',
              '@id': `${url}#post`,
              headline: p.title,
              description: p.description,
              url,
              mainEntityOfPage: { '@type': 'WebPage', '@id': url },
              datePublished: isoDate(p.date),
              dateModified: isoDate(p.date),
              image,
              keywords: p.tags.join(', '),
              wordCount: p.words,
              inLanguage: 'en',
              author: { '@type': 'Organization', name: p.author, url: `${SITE}/` },
              publisher: { '@id': `${SITE}/#org` },
              isPartOf: { '@id': `${SITE}/blog/#blog` },
              about: { '@id': `${SITE}/#software` },
            },
            crumbs([
              { name: 'Home', url: `${SITE}/` },
              { name: 'Blog', url: `${SITE}/blog/` },
              { name: p.title, url },
            ]),
          ],
        },
      ],
    }),
  });
}

for (const p of pages) {
  write(p.file, page(p.route, p.head));
}
console.log(`prerendered ${pages.length} routes`);

// ------------------------------------------------------------------- feeds

/** RSS 2.0. Dates are RFC 822 (not ISO 8601), and the feed names its own URL
 *  with atom:link rel="self", which the W3C validator warns about otherwise. */
function rss({ title, link, description, self, items }) {
  const body = items
    .map(
      (it) => `    <item>
      <title>${escapeXml(it.title)}</title>
      <link>${it.link}</link>
      <guid isPermaLink="true">${it.link}</guid>
      <pubDate>${rfc822(it.date)}</pubDate>
      ${(it.categories ?? []).map((c) => `<category>${escapeXml(c)}</category>`).join('\n      ')}
      <description><![CDATA[${it.description}]]></description>
    </item>`
    )
    .join('\n');

  return `<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom">
  <channel>
    <title>${escapeXml(title)}</title>
    <link>${link}</link>
    <description>${escapeXml(description)}</description>
    <language>en-us</language>
    <lastBuildDate>${new Date().toUTCString().replace('GMT', '+0000')}</lastBuildDate>
    <atom:link href="${self}" rel="self" type="application/rss+xml" />
${body}
  </channel>
</rss>
`;
}

write(
  'blog/rss.xml',
  rss({
    title: 'AgentWorth blog',
    link: `${SITE}/blog/`,
    description: 'Notes from measuring our own coding agents.',
    self: `${SITE}/blog/rss.xml`,
    items: posts.map((p) => ({
      title: p.title,
      link: `${SITE}/blog/${p.slug}/`,
      date: p.date,
      categories: p.tags,
      description: `<p>${esc(p.description)}</p>\n${p.html}`,
    })),
  })
);

write(
  'changelog/rss.xml',
  rss({
    title: 'AgentWorth releases',
    link: `${SITE}/changelog/`,
    description: 'Every AgentWorth release, with the pull request behind each line.',
    self: `${SITE}/changelog/rss.xml`,
    items: releases.map((r) => ({
      title: `AgentWorth ${r.version}`,
      link: `${SITE}/changelog/#${r.id}`,
      date: r.date,
      description: r.sections.map((s) => `<h3>${s.title}</h3>\n${s.html}`).join('\n'),
    })),
  })
);
console.log('wrote blog/rss.xml and changelog/rss.xml');

// ----------------------------------------------------------------- sitemap

// No <priority> or <changefreq>: Google ignores both. <lastmod> comes from the
// content's own date, never the build clock — a lastmod that changes on every
// deploy is one Google learns to distrust.
const newest = (a, b) => (a > b ? a : b);
const latestPost = posts.reduce((d, p) => newest(d, p.date), '0000-00-00');
// The docs pages carry the reference's generated date rather than a file mtime:
// a fresh CI checkout stamps every file with the checkout time, which is exactly
// the deploy-clock lastmod the rule above rejects. reference.generatedDate is
// committed and only moves when the docs bundle is regenerated.
const docsDate = reference.generatedDate;
const sitemapUrls = [
  { loc: `${SITE}/`, lastmod: newest(releases[0].date, latestPost) },
  { loc: `${SITE}/changelog/`, lastmod: releases[0].date },
  { loc: `${SITE}/docs/`, lastmod: docsDate },
  { loc: `${SITE}/docs/reference/`, lastmod: reference.generatedDate },
  { loc: `${SITE}/docs/specs/`, lastmod: docsDate },
  { loc: `${SITE}/docs/research/`, lastmod: docsDate },
  ...allDocs.map((d) => ({ loc: `${SITE}${docPath(d.section, d.slug)}`, lastmod: docsDate })),
  { loc: `${SITE}/archie/`, lastmod: releases[0].date },
  { loc: `${SITE}/blog/`, lastmod: latestPost },
  ...posts.map((p) => ({ loc: `${SITE}/blog/${p.slug}/`, lastmod: p.date })),
];

write(
  'sitemap.xml',
  `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
${sitemapUrls
  .map((u) => `  <url>\n    <loc>${u.loc}</loc>\n    <lastmod>${u.lastmod}</lastmod>\n  </url>`)
  .join('\n')}
</urlset>
`
);

// The search-facing crawlers are the ones that gate being cited in an answer,
// so they are named explicitly rather than left to the wildcard.
write(
  'robots.txt',
  `# https://www.robotstxt.org/robotstxt.html
User-agent: *
Allow: /

# Answer-engine crawlers. Blocking these costs citations in ChatGPT search,
# Claude search and Perplexity; there is nothing here we do not want quoted.
User-agent: OAI-SearchBot
Allow: /

User-agent: Claude-SearchBot
Allow: /

User-agent: PerplexityBot
Allow: /

User-agent: GPTBot
Allow: /

User-agent: ClaudeBot
Allow: /

User-agent: Google-Extended
Allow: /

Sitemap: ${SITE}/sitemap.xml
`
);
console.log('wrote sitemap.xml and robots.txt');

// ---------------------------------------------------------------- llms.txt

// llmstxt.org v2: H1 name, a blockquote summary, free prose, then H2 file
// lists of [name](url): notes.
const MCP_TOOLS = [
  ['sessions_find', 'filter the index by adapter, model, repo, time or outcome'],
  ['session_get', 'one session with its events, tokens and outcome rung'],
  ['blame_find', 'which session, model and prompt produced a line of code'],
  ['usage_summary', 'tokens and cost rolled up by day, week or month'],
  ['pacing_window', 'throughput over a moving window'],
  ['coverage_stats', 'which adapters are detected and what they yield'],
  ['outcome_rate', 'verified-outcome rate by model, adapter or repo, with an n floor'],
  ['session_handoff', 'what a session promised, decided and did not finish'],
  ['carry_forward', 'what the previous session left for this one'],
  ['forgotten_context', 'decisions compaction dropped, returned verbatim with receipts'],
  ['suspect_commits', 'commits whose session had no exit-0 test or a demoted claim'],
];

write(
  'llms.txt',
  `# AgentWorth

> Every agent says it's done. AgentWorth checks the git log. It reads the session logs that AI coding agents already leave in your dotfiles, and grades each claimed outcome against what actually happened: files changed, tests run, commits made, CI green. Native Rust, local-only, nothing uploaded.

AgentWorth indexes trajectories from 21 coding-agent harnesses into a local SQLite database, then scores each session on a five-rung evidence ladder — done claimed, artifact changed, test or build passed, commit observed, CI or deployment verified. Rungs 3 and above require a captured exit code, so "tests passed" means exit 0 rather than "a test command appeared in the transcript".

It never phones home. There is no account, no server and no upload; it reads files already on your disk and writes an index beside them.

## Install

\`\`\`
npx -y agentworth scan                        # index what is already on this machine
curl -fsSL ${SITE}/install.sh | sh   # or install the native binary
agentworth serve                              # open the local explorer
\`\`\`

## MCP server

Register with \`claude mcp add agentworth -- agentworth mcp\` (stdio). Redaction is on by default. Tools:

${MCP_TOOLS.map(([n, d]) => `- \`${n}\`: ${d}`).join('\n')}

## Docs

- [Docs](${SITE}/docs/): guides, reference, specs and research in one index
- [Changelog](${SITE}/changelog/): every release with the pull request behind each line
- [Releases RSS](${SITE}/changelog/rss.xml): the changelog as a feed
- [Reference](${SITE}/docs/reference/): every CLI command, API route, and MCP tool, generated from the code
- [Specs](${SITE}/docs/specs/): the design doc behind every feature, ${docs.specs.length} of them
- [Research](${SITE}/docs/research/): memos that fed a spec, context only
- [Archie](${SITE}/archie/): the mascot, his kit and the four colourways
- [Reference (markdown)](${SITE}/docs/reference.md): the same reference, plain text
- [Blog](${SITE}/blog/): measurements from our own index
- [Blog RSS](${SITE}/blog/rss.xml): the blog as a feed
- [Full text](${SITE}/llms-full.txt): this file plus every post, release, and the full reference
- [Source](${REPO}): Apache-2.0

## Guides

${docs.learn.map((d) => `- [${d.title}](${SITE}${docPath('learn', d.slug)}): ${d.description}`).join('\n')}

## Posts

${posts.map((p) => `- [${p.title}](${SITE}/blog/${p.slug}/): ${p.description}`).join('\n')}
`
);

// Nests docs/REFERENCE.md's own headings (# AgentWorth Reference, ## CLI, ### `agentworth
// scan`, ...) one level under llms-full.txt's H2 sections, so "# AgentWorth Reference"
// becomes a peer of "## Changelog" instead of a second top-level H1.
const demoteHeadings = (md) => md.replace(/^(#{1,5})(\s)/gm, (_m, hashes, sp) => `#${hashes}${sp}`);

const stripTags = (html) =>
  html
    .replace(/<\/(p|li|h[1-6]|tr|blockquote|pre)>/g, '\n')
    .replace(/<[^>]+>/g, '')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, '&')
    .replace(/\n{3,}/g, '\n\n')
    .trim();

write(
  'llms-full.txt',
  `# AgentWorth — full text

> Every agent says it's done. AgentWorth checks the git log. Generated ${downloads.fetchedAt} from ${SITE}. Source: ${REPO} (Apache-2.0).

## Install

    npx -y agentworth scan
    curl -fsSL ${SITE}/install.sh | sh
    agentworth serve

## MCP tools

${MCP_TOOLS.map(([n, d]) => `- ${n}: ${d}`).join('\n')}

${demoteHeadings(reference.markdown)}

## Blog posts

${posts
  .map(
    (p) => `### ${p.title}
${SITE}/blog/${p.slug}/ — ${humanDate(p.date)} — ${p.readingMinutes} min read

${p.description}

${stripTags(p.html)}
`
  )
  .join('\n---\n\n')}

## Changelog

${releases
  .map(
    (r) => `### ${r.version} — ${r.date}

${r.sections.map((s) => `${s.title}:\n${stripTags(s.html)}`).join('\n\n')}
`
  )
  .join('\n')}
`
);

write(
  'humans.txt',
  `/* TEAM */
  Unfoundbox Crew — ${REPO}

/* SITE */
  Standards: HTML5, CSS3, pre-rendered static HTML
  Components: React, Vite, TypeScript
  Typeface: Geist and Geist Mono
  Software: native Rust, SQLite

/* NUMBERS */
  Every figure on this site comes from one real index on one machine.
  Where a measurement is narrow, the page says how narrow.
`
);
console.log('wrote llms.txt, llms-full.txt and humans.txt');

// docs/REFERENCE.md served verbatim at /docs/reference.md (text/markdown, see vercel.json)
// -- the same committed file the page above renders and the CI staleness check verifies.
write('docs/reference.md', reference.markdown);
console.log('wrote docs/reference.md');

// ----------------------------------------------------------- search index

// One static file the ⌘K palette fetches on first open, then filters in memory.
// Keys are one letter because they repeat on every entry and this file is
// downloaded by a reader, not read by one: t=title, s=section, u=url,
// x=excerpt. Excerpts are already capped at 200 characters upstream.
const SECTION_LABELS = { learn: 'Learn', specs: 'Specs', research: 'Research' };

const searchIndex = [];
const seenEntry = new Set();
const addEntry = (t, section, u, x) => {
  const key = `${u}|${t}`;
  if (!t || seenEntry.has(key)) return;
  seenEntry.add(key);
  searchIndex.push({ t, s: section, u, x: x || '' });
};

for (const doc of allDocs) {
  const label = SECTION_LABELS[doc.section];
  const url = docPath(doc.section, doc.slug);
  addEntry(doc.title, label, url, doc.description);
  for (const h of doc.headings) {
    addEntry(h.text, `${label} · ${doc.title}`, `${url}#${h.id}`, h.excerpt);
  }
}

// The generated reference is one page with three groups of anchors; without
// these, searching for a flag or a tool name finds nothing.
const refId = (str) =>
  str
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');

for (const cmd of reference.cli) {
  addEntry(cmd.path, 'Reference · CLI', `/docs/reference/#${refId(cmd.path)}`, cmd.about);
}
for (const route of reference.api) {
  addEntry(
    `${route.method} ${route.path}`,
    'Reference · HTTP API',
    `/docs/reference/#${refId(`${route.method} ${route.path}`)}`,
    route.description
  );
}
for (const tool of reference.mcp) {
  addEntry(
    tool.name,
    'Reference · MCP',
    `/docs/reference/#${refId(tool.name)}`,
    (tool.description || '').slice(0, 200)
  );
}

for (const r of releases) {
  addEntry(
    `AgentWorth ${r.version}`,
    'Changelog',
    `/changelog/#${r.id}`,
    `${humanDate(r.date)} — ${r.changeCount} ${r.changeCount === 1 ? 'change' : 'changes'}`
  );
}
for (const p of posts) {
  addEntry(p.title, 'Blog', `/blog/${p.slug}/`, p.description);
}

write('docs/search-index.json', JSON.stringify(searchIndex));
console.log(
  `wrote docs/search-index.json: ${searchIndex.length} entries, ` +
    `${(JSON.stringify(searchIndex).length / 1024).toFixed(1)} KB`
);

// ------------------------------------------------------------ social cards

const lockup = readLockup();
for (const p of posts) {
  const svg = postCardSvg(
    {
      title: p.title,
      kicker: 'Blog',
      footer: `${humanDate(p.date)} · ${p.readingMinutes} min read`,
    },
    lockup
  );
  renderSvgToPng(svg, path.join(dist, `og/blog/${p.slug}.png`));
}
console.log(`rendered ${posts.length} post social cards`);

// ------------------------------------------------------------------- check

const missing = pages.filter((p) => !existsSync(path.join(dist, p.file)));
if (missing.length) {
  throw new Error(`prerender missed: ${missing.map((m) => m.file).join(', ')}`);
}
for (const p of pages) {
  const html = readFileSync(path.join(dist, p.file), 'utf8');
  if (html.includes('<!--@head-->') || html.includes('<!--@app-->')) {
    throw new Error(`${p.file}: a placeholder survived the build`);
  }
  if (!/<title>[^<]+<\/title>/.test(html)) throw new Error(`${p.file}: no title`);
  if (!html.includes('rel="canonical"')) throw new Error(`${p.file}: no canonical`);
  if (html.indexOf('<div id="root">') + 15 === html.indexOf('</div>')) {
    throw new Error(`${p.file}: root is empty, nothing was pre-rendered`);
  }
}
console.log(`checked ${pages.length} routes: title, canonical, non-empty root`);
