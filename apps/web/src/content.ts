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

export interface Downloads {
  fetchedAt: string;
  npm: { downloads: number; start: string; end: string } | null;
  github: { assets: number; releases: number; latest: string | null } | null;
  offline: boolean;
}

export interface SiteContent {
  releases: Release[];
  posts: Post[];
  downloads: Downloads;
}

export const content = generated as unknown as SiteContent;
export const { releases, posts, downloads } = content;

export const humanDate = (d: string): string =>
  new Date(`${d}T00:00:00Z`).toLocaleDateString("en-US", {
    year: "numeric",
    month: "long",
    day: "numeric",
    timeZone: "UTC",
  });
