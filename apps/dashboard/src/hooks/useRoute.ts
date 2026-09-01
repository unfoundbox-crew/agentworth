import { useCallback, useEffect, useState } from 'react';

export interface Route {
  /** Current location.pathname, e.g. "/" or "/s/sess_1a2b3c". */
  path: string;
  /** Decoded session id when path is /s/<id>, otherwise null. Ids are
   *  opaque strings (most are UUIDs, some are derived from folder names),
   *  so no character-set assumption is made beyond percent-decoding. */
  sessionId: string | null;
  /** Push an app-relative path via the History API and re-render. */
  navigate: (to: string) => void;
}

const ROUTE_CHANGE_EVENT = 'agentworth-route-change';

function parsePath(pathname: string): { path: string; sessionId: string | null } {
  if (pathname.startsWith('/s/')) {
    const raw = pathname.slice('/s/'.length);
    let sessionId: string | null = null;
    try {
      sessionId = raw ? decodeURIComponent(raw) : null;
    } catch {
      sessionId = raw || null;
    }
    return { path: pathname, sessionId };
  }
  return { path: pathname, sessionId: null };
}

/**
 * Dependency-free History API router (no react-router). The Rust server
 * already serves index.html for unmatched non-/api paths, so pushState
 * navigation and a hard reload/deep link both resolve correctly.
 */
export function useRoute(): Route {
  const [state, setState] = useState(() =>
    parsePath(typeof window !== 'undefined' ? window.location.pathname : '/')
  );

  useEffect(() => {
    const onChange = () => setState(parsePath(window.location.pathname));
    window.addEventListener('popstate', onChange);
    window.addEventListener(ROUTE_CHANGE_EVENT, onChange);
    return () => {
      window.removeEventListener('popstate', onChange);
      window.removeEventListener(ROUTE_CHANGE_EVENT, onChange);
    };
  }, []);

  const navigate = useCallback((to: string) => {
    if (to === window.location.pathname) return;
    window.history.pushState(null, '', to);
    window.dispatchEvent(new Event(ROUTE_CHANGE_EVENT));
  }, []);

  return { path: state.path, sessionId: state.sessionId, navigate };
}
