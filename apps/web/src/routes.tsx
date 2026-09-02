import React from "react";
import { LandingPage } from "./components/LandingPage";
import { ChangelogPage } from "./components/ChangelogPage";
import { ReferencePage } from "./components/ReferencePage";
import { ArchiePage } from "./components/ArchiePage";
import { NotFoundPage } from "./components/NotFoundPage";
import { BlogIndexPage } from "./components/BlogIndexPage";
import { BlogPostPage } from "./components/BlogPostPage";
import { posts } from "./content";

export interface Route {
  /** Directory path, always with a trailing slash. `/` is the root. */
  path: string;
  element: React.ReactElement;
}

export const routes: Route[] = [
  { path: "/", element: <LandingPage /> },
  { path: "/changelog/", element: <ChangelogPage /> },
  { path: "/docs/reference/", element: <ReferencePage /> },
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
