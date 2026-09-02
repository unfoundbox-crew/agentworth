// The typed view of content.generated.json, which `npm run build` writes from
// the root CHANGELOG.md and content/blog/*.md before Vite runs. It is not in
// git — regenerate with `npm run content`.
import generated from "./content.generated.json";

export interface ChangelogSection {
  title: string;
  html: string;
}

export interface Release {
  version: string;
  date: string;
  id: string;
  changeCount: number;
  sections: ChangelogSection[];
}

export interface PostLink {
  slug: string;
  title: string;
}

export interface Post {
  slug: string;
  title: string;
  date: string;
  description: string;
  tags: string[];
  author: string;
  canonical: string;
  readingMinutes: number;
  words: number;
  html: string;
  newer: PostLink | null;
  older: PostLink | null;
}

export interface CliArgDoc {
  name: string;
  positional: boolean;
  required: boolean;
  help: string;
  default: string | null;
  possibleValues: string[];
}

export interface CliCommandDoc {
  path: string;
  about: string;
  args: CliArgDoc[];
}

export interface ApiParamDoc {
  name: string;
  description: string;
}

export interface ApiRouteDoc {
  method: string;
  path: string;
  description: string;
  queryParams: ApiParamDoc[];
}

export interface McpToolDoc {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
}

export interface Reference {
  version: string;
  generatedDate: string;
  globalFlags: CliArgDoc[];
  cli: CliCommandDoc[];
  api: ApiRouteDoc[];
  mcp: McpToolDoc[];
  /** The exact `docs/REFERENCE.md` text, inlined into `llms-full.txt`. Not rendered on the
   *  page itself -- the page renders the typed fields above so it can add anchors and nav. */
  markdown: string;
}

export interface Downloads {
  fetchedAt: string;
  npm: { downloads: number; start: string; end: string } | null;
  github: { assets: number; releases: number; latest: string | null } | null;
  offline: boolean;
}

export type DocSection = "learn" | "specs" | "research";

export interface DocHeading {
  id: string;
  depth: number;
  text: string;
  excerpt: string;
}

export interface DocLink {
  slug: string;
  title: string;
  section: DocSection;
}

export interface Doc {
  section: DocSection;
  slug: string;
  file: string;
  title: string;
  /** The spec's own `Status:` line, lifted out of the body. Guides have none. */
  status: string | null;
  description: string;
  headings: DocHeading[];
  html: string;
  prev: DocLink | null;
  next: DocLink | null;
}

export interface Docs {
  learn: Doc[];
  specs: Doc[];
  research: Doc[];
}

export interface SiteContent {
  releases: Release[];
  posts: Post[];
  reference: Reference;
  docs: Docs;
  downloads: Downloads;
}

export const content = generated as unknown as SiteContent;
export const { releases, posts, reference, docs, downloads } = content;

/** Directory path for one docs page. Mirrors `docPath` in scripts/content.mjs. */
export const docPath = (section: DocSection, slug: string): string =>
  `/docs/${section}/${slug}/`;

export const humanDate = (d: string): string =>
  new Date(`${d}T00:00:00Z`).toLocaleDateString("en-US", {
    year: "numeric",
    month: "long",
    day: "numeric",
    timeZone: "UTC",
  });
