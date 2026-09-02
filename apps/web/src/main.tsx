import React from 'react';
import ReactDOM from 'react-dom/client';
import { routeForOr404 } from './routes';
import { initAnalytics } from './analytics';
import './index.css';

initAnalytics();

const root = document.getElementById('root')!;
const route = routeForOr404(window.location.pathname);

// Every route is written to disk as real HTML by scripts/prerender.mjs, so the
// normal path is hydration. A URL the build did not emit is served 404.html by the
// host, which is the 404 component's own markup — so it hydrates too, and can name
// the address it could not find. createRoot covers an empty container.
const tree = <React.StrictMode>{route.element}</React.StrictMode>;
if (root.firstChild) {
  ReactDOM.hydrateRoot(root, tree);
} else {
  ReactDOM.createRoot(root).render(tree);
}
