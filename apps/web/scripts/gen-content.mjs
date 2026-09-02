// Writes src/content.generated.json before Vite runs, so the page components
// can import content as an ordinary module and both builds — client and SSR —
// see exactly the same bytes.
//
// AGENTWORTH_OFFLINE=1 skips the network entirely, for a build with no
// connection. The download lines are then omitted rather than guessed.
import { writeFileSync } from 'node:fs';
import path from 'node:path';
import { parseChangelog, parsePosts, parseReference, webRoot } from './content.mjs';
import { fetchDownloads } from './downloads.mjs';

const releases = parseChangelog();
const posts = parsePosts();
const reference = parseReference();
const downloads = await fetchDownloads({
  offline: process.env.AGENTWORTH_OFFLINE === '1',
});

const out = path.join(webRoot, 'src/content.generated.json');
writeFileSync(out, JSON.stringify({ releases, posts, reference, downloads }, null, 2));

console.log(
  `content: ${releases.length} releases, ${posts.length} posts, ` +
    `reference ${reference.cli.length} commands/${reference.api.length} routes/${reference.mcp.length} tools, ` +
    `downloads npm=${downloads.npm?.downloads ?? 'n/a'} gh=${downloads.github?.assets ?? 'n/a'}`
);
