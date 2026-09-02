// Download counts, fetched once at build time and baked into the HTML as text.
// Never fetched from the browser: the site ships no runtime calls and no
// telemetry, and a counter that phones home on every page view would be both.
//
// Sources:
//   npm   https://api.npmjs.org/downloads/point/last-month/agentworth
//         -> {"downloads":N,"start":"…","end":"…","package":"agentworth"}
//   gh    GET /repos/{owner}/{repo}/releases, summing assets[].download_count
//
// Both are best-effort. A failed fetch returns null and the page omits the
// line rather than printing a number nobody measured.

const NPM_ENDPOINT = 'https://api.npmjs.org/downloads/point/last-month/agentworth';
const GH_ENDPOINT =
  'https://api.github.com/repos/unfoundbox-crew/agentworth/releases?per_page=100';

const TIMEOUT_MS = 8000;

async function getJson(url, headers = {}) {
  const ctl = new AbortController();
  const timer = setTimeout(() => ctl.abort(), TIMEOUT_MS);
  try {
    const res = await fetch(url, {
      signal: ctl.signal,
      headers: { 'user-agent': 'agentworth.dev-build', ...headers },
    });
    if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
    return await res.json();
  } finally {
    clearTimeout(timer);
  }
}

export async function fetchDownloads({ offline = false } = {}) {
  const fetchedAt = new Date().toISOString().slice(0, 10);
  if (offline) return { fetchedAt, npm: null, github: null, offline: true };

  const [npm, github] = await Promise.all([
    getJson(NPM_ENDPOINT)
      .then((d) => {
        // A package npm has never seen returns {"error":"… not found"}, not a
        // zero. Zero downloads and no such package are different facts, and
        // only the first one is safe to print.
        if (typeof d.downloads !== 'number') {
          throw new Error(d.error || 'no downloads field');
        }
        return { downloads: d.downloads, start: d.start, end: d.end };
      })
      .catch((e) => {
        console.warn(`  downloads: npm unavailable (${e.message})`);
        return null;
      }),
    getJson(GH_ENDPOINT, process.env.GITHUB_TOKEN
      ? { authorization: `Bearer ${process.env.GITHUB_TOKEN}` }
      : {})
      .then((rels) => ({
        // .sha256 files sit beside each tarball; counting them would roughly
        // double a number that is meant to mean "someone took a binary".
        assets: rels
          .flatMap((r) => r.assets ?? [])
          .filter((a) => a.name.endsWith('.tar.gz'))
          .reduce((n, a) => n + (a.download_count ?? 0), 0),
        releases: rels.length,
        latest: rels[0]?.tag_name ?? null,
      }))
      .catch((e) => {
        console.warn(`  downloads: GitHub releases unavailable (${e.message})`);
        return null;
      }),
  ]);

  return { fetchedAt, npm, github, offline: false };
}
