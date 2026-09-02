import React from "react";
import { renderToString } from "react-dom/server";
import { routes, routeFor } from "./routes";

/** Every path the prerenderer should write an HTML file for. */
export const allRoutes = (): string[] => routes.map((r) => r.path);

export function render(pathname: string): string {
  const route = routeFor(pathname);
  if (!route) throw new Error(`no route for ${pathname}`);
  return renderToString(<React.StrictMode>{route.element}</React.StrictMode>);
}
