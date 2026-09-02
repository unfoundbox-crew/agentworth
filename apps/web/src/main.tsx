import React from 'react';
import ReactDOM from 'react-dom/client';
import { routeFor } from './routes';
import { initAnalytics } from './analytics';
import './index.css';

initAnalytics();

const root = document.getElementById('root')!;
const route = routeFor(window.location.pathname);

// Every route is written to disk as real HTML by scripts/prerender.mjs, so the
// normal path is hydration. createRoot is the fallback for a URL the build did
// not emit — there the container is empty and there is nothing to hydrate.
if (route) {
  const tree = <React.StrictMode>{route.element}</React.StrictMode>;
  if (root.firstChild) {
    ReactDOM.hydrateRoot(root, tree);
  } else {
    ReactDOM.createRoot(root).render(tree);
  }
}
