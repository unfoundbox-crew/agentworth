import React from "react";
import { LandingPage } from "./components/LandingPage";
import { ChangelogPage } from "./components/ChangelogPage";
import { ReferencePage } from "./components/ReferencePage";
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
