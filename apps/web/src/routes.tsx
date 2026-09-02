import React from "react";
import { LandingPage } from "./components/LandingPage";
import { ChangelogPage } from "./components/ChangelogPage";
import { ReferencePage } from "./components/ReferencePage";
import { ArchiePage } from "./components/ArchiePage";
import { NotFoundPage } from "./components/NotFoundPage";
import { BlogIndexPage } from "./components/BlogIndexPage";
import { BlogPostPage } from "./components/BlogPostPage";
import { DocsHomePage } from "./components/DocsHomePage";
import { DocsIndexPage } from "./components/DocsIndexPage";
import { DocsArticlePage } from "./components/DocsArticlePage";
import { docs, docPath, posts } from "./content";

export interface Route {
  /** Directory path, always with a trailing slash. `/` is the root. */
  path: string;
  element: React.ReactElement;
}

export const routes: Route[] = [
  { path: "/", element: <LandingPage /> },
  { path: "/changelog/", element: <ChangelogPage /> },
  { path: "/docs/", element: <DocsHomePage /> },
  { path: "/docs/reference/", element: <ReferencePage /> },
  {
    path: "/docs/specs/",
    element: (
      <DocsIndexPage
        section="specs"
        crumb="Specs"
        title="The design doc behind every feature."
        lede="Each one states the problem in the words of the person who has it, measures the thing before building it, and says plainly what it deliberately does not do. Read straight out of the repository, unedited."
        docs={docs.specs}
        sourceDir="docs/specs"
      />
    ),
  },
  {
    path: "/docs/research/",
    element: (
      <DocsIndexPage
        section="research"
        crumb="Research"
        title="Memos that fed a spec."
        lede="Every claim carries its source, and unverified means no primary source was found, not false. Context only — none of these decides anything."
        docs={docs.research}
        sourceDir="docs/research"
      />
    ),
  },
  ...[...docs.learn, ...docs.specs, ...docs.research].map((doc) => ({
    path: docPath(doc.section, doc.slug),
    element: <DocsArticlePage doc={doc} />,
  })),
  { path: "/archie/", element: <ArchiePage /> },
  // Pre-rendered to dist/404.html, not to a route directory. It is in this list so the
  // prerenderer and the client both build it from the same component.
  { path: "/404/", element: <NotFoundPage /> },
  { path: "/blog/", element: <BlogIndexPage /> },
  ...posts.map((post) => ({
    path: `/blog/${post.slug}/`,
    element: <BlogPostPage post={post} />,
  })),
];

/** Tolerates a missing trailing slash so a hand-typed /blog still hydrates. */
export function routeFor(pathname: string): Route | undefined {
  const wanted = pathname.endsWith("/") ? pathname : `${pathname}/`;
  return routes.find((r) => r.path === wanted);
}

/** The route the browser is on, or the 404 — which is what the host served if no other
 *  route matched, and which needs React to name the address it could not find. */
export function routeForOr404(pathname: string): Route {
  return routeFor(pathname) ?? routeFor("/404/")!;
}
