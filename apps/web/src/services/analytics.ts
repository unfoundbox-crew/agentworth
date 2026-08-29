/**
 * Lightweight, privacy-first telemetry for the public marketing landing page.
 * Strictly no-ops during offline local development / agentworth serve.
 */

let isInitialized = false;

export function initAnalytics() {
  if (typeof window === 'undefined') return;

  // Only initialize on production domain or when explicit key is provided
  const posthogKey = (import.meta as any).env?.VITE_POSTHOG_KEY || (window as any).__POSTHOG_KEY__;
  const isLocalhost = window.location.hostname === 'localhost' || window.location.hostname === '127.0.0.1';

  if (!posthogKey || isLocalhost) {
    return;
  }

  if (isInitialized) return;

  try {
    const posthogHost = (import.meta as any).env?.VITE_POSTHOG_HOST || 'https://us.i.posthog.com';
    
    // Dynamic load to keep bundle zero-overhead
    const script = document.createElement('script');
    script.async = true;
    script.src = `${posthogHost}/static/array.js`;
    script.onload = () => {
      if ((window as any).posthog) {
        (window as any).posthog.init(posthogKey, {
          api_host: posthogHost,
          autocapture: false,
          capture_pageview: true,
          disable_session_recording: true,
          persistence: 'memory',
        });
        isInitialized = true;
      }
    };
    document.head.appendChild(script);
  } catch (err) {
    console.debug('Analytics init skipped:', err);
  }
}

export function trackEvent(eventName: string, properties?: Record<string, any>) {
  if (typeof window === 'undefined') return;
  try {
    if (isInitialized && (window as any).posthog) {
      (window as any).posthog.capture(eventName, properties);
    }
  } catch {
    // Silent no-op
  }
}
