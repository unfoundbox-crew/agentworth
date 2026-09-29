import { useCallback, useEffect, useState } from 'react';

/**
 * Dependency-free History API router for the deck shell (same shape as
 * apps/dashboard/src/hooks/useRoute.ts). No react-router.
 *
 * The deck is served under `/home` (`vite.config.ts` base, rust-embed SPA
 * fallback on `/home/*`). App-merge P1 deep links are the paths *inside*
 * that mount: `/insights` and `/s/<id>`. Full browser URLs are therefore
 * `/home/insights` and `/home/s/<id>`. Root-relative `/insights` and
 * `/s/<id>` are also accepted so a post-merge move of the deck to `/`
 * does not break pasted links.
 *
 * `#insights` remains a read-side alias for older bookmarks; new writes
 * go to the path form.
 */

/** Mount prefix the binary serves the deck under today. */
export const HOME_BASE = '/home';

export const INSIGHTS_PATH = '/insights';
export const INSIGHTS_HASH = '#insights';

const ROUTE_CHANGE_EVENT = 'agentworth-home-route-change';

export interface HomeRoute {
  /** App path relative to the home mount, e.g. "/", "/insights", "/s/sess_1". */
  path: string;
  /** Decoded session id when path is /s/<id>, otherwise null. */
  sessionId: string | null;
  /** True when the URL selects the insights phase (path or legacy hash). */
  insights: boolean;
  /** Push an app-relative path via the History API and re-render. */
  navigate: (to: string) => void;
}

export interface ParsedHomeRoute {
  path: string;
  sessionId: string | null;
  insights: boolean;
}

/** Strip the `/home` mount so `/home/insights` and `/insights` parse the same. */
export function stripHomeBase(pathname: string): string {
  if (!pathname || pathname === '/') return '/';
  if (pathname === HOME_BASE || pathname === `${HOME_BASE}/`) return '/';
  if (pathname.startsWith(`${HOME_BASE}/`)) {
    const rest = pathname.slice(HOME_BASE.length);
    return rest.length === 0 ? '/' : rest;
  }
  return pathname;
}

/** Join an app-relative path onto the home mount for History API writes. */
export function homeHref(appPath: string): string {
  const normalized =
    !appPath || appPath === '/'
      ? '/'
      : appPath.startsWith('/')
        ? appPath
        : `/${appPath}`;
  if (normalized === '/') return `${HOME_BASE}/`;
  return `${HOME_BASE}${normalized}`;
}

function decodeSessionId(raw: string): string | null {
  if (!raw) return null;
  try {
    return decodeURIComponent(raw);
  } catch {
    return raw;
  }
}

/**
 * Pure parse of an app-relative path (already stripped of `/home`).
 * Ids are opaque — percent-decode only, no character-set assumption.
 */
export function parseAppPath(appPath: string): ParsedHomeRoute {
  const path = !appPath || appPath === '' ? '/' : appPath.startsWith('/') ? appPath : `/${appPath}`;

  if (path === INSIGHTS_PATH || path.startsWith(`${INSIGHTS_PATH}/`)) {
    return { path: INSIGHTS_PATH, sessionId: null, insights: true };
  }

  if (path.startsWith('/s/')) {
    const raw = path.slice('/s/'.length);
    const idPart = raw.split('/')[0] ?? '';
    const sessionId = decodeSessionId(idPart);
    return {
      path: sessionId ? `/s/${encodeURIComponent(sessionId)}` : '/s/',
      sessionId,
      insights: false,
    };
  }

  return { path, sessionId: null, insights: false };
}

/**
 * Parse a browser location into a home route. Path wins; `#insights` is a
 * legacy alias when the path itself is not already the insights phase.
 */
export function parseHomeLocation(
  pathname: string,
  hash?: string | null
): ParsedHomeRoute {
  const parsed = parseAppPath(stripHomeBase(pathname));
  if (parsed.insights) return parsed;
  // Legacy hash alias only on the deck root — never on /s/<id>.
  if (
    !parsed.sessionId &&
    (hash === INSIGHTS_HASH || hash === 'insights')
  ) {
    return { ...parsed, insights: true };
  }
  return parsed;
}

/** Legacy hash-only probe; prefer parseHomeLocation. */
export function parseInsightsDeepLink(hash: string | null | undefined): boolean {
  return hash === INSIGHTS_HASH || hash === 'insights';
}

/**
 * Mirror insights open/closed onto the path (replaceState only — same
 * discipline as the old hash sync so i/Escape do not pollute history).
 * Clears a legacy `#insights` hash when writing the path form.
 *
 * Closing insights while the URL is `/s/<id>` leaves the session deep link
 * alone (session UI is P2; the route must still round-trip).
 */
export function syncInsightsPath(open: boolean): void {
  if (typeof window === 'undefined') return;
  const current = parseHomeLocation(window.location.pathname, window.location.hash);

  if (!open && current.sessionId) {
    // Session deep link owns the path; only clear a leftover hash.
    if (window.location.hash) {
      window.history.replaceState(null, '', window.location.pathname + window.location.search);
      window.dispatchEvent(new Event(ROUTE_CHANGE_EVENT));
    }
    return;
  }

  const nextHref = homeHref(open ? INSIGHTS_PATH : '/');
  const nowHref = window.location.pathname + (window.location.hash || '');
  // Treat `/home` and `/home/` as the same closed state.
  const closedNow =
    !open &&
    (stripHomeBase(window.location.pathname) === '/' || stripHomeBase(window.location.pathname) === '') &&
    !window.location.hash;
  const openNow = open && stripHomeBase(window.location.pathname) === INSIGHTS_PATH && !window.location.hash;
  if (closedNow || openNow) return;
  if (nowHref === nextHref) return;

  window.history.replaceState(null, '', nextHref + window.location.search);
  window.dispatchEvent(new Event(ROUTE_CHANGE_EVENT));
}

/** @deprecated Prefer syncInsightsPath; hashes are no longer written. */
export function syncInsightsHash(open: boolean): void {
  syncInsightsPath(open);
}

function readWindowRoute(): ParsedHomeRoute {
  if (typeof window === 'undefined') return { path: '/', sessionId: null, insights: false };
  return parseHomeLocation(window.location.pathname, window.location.hash);
}

/**
 * Hook: current home route + navigate. pushState for explicit navigation;
 * insights phase toggles still use syncInsightsPath (replaceState) from Deck.
 */
export function useRoute(): HomeRoute {
  const [state, setState] = useState<ParsedHomeRoute>(readWindowRoute);

  useEffect(() => {
    const onChange = () => setState(readWindowRoute());
    window.addEventListener('popstate', onChange);
    window.addEventListener('hashchange', onChange);
    window.addEventListener(ROUTE_CHANGE_EVENT, onChange);
    return () => {
      window.removeEventListener('popstate', onChange);
      window.removeEventListener('hashchange', onChange);
      window.removeEventListener(ROUTE_CHANGE_EVENT, onChange);
    };
  }, []);

  const navigate = useCallback((to: string) => {
    const href = homeHref(to);
    if (href === window.location.pathname && !window.location.hash) return;
    window.history.pushState(null, '', href);
    window.dispatchEvent(new Event(ROUTE_CHANGE_EVENT));
  }, []);

  return {
    path: state.path,
    sessionId: state.sessionId,
    insights: state.insights,
    navigate,
  };
}
