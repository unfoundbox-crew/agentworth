/**
 * Lightweight, privacy-first telemetry for the public marketing landing page.
 * Supports PostHog and Google Analytics (GA4).
 * Strictly no-ops during offline local development / agentworth serve.
 */

let isPosthogInitialized = false;
let isGaInitialized = false;

export function initAnalytics() {
  if (typeof window === 'undefined') return;

  const isLocalhost =
    window.location.hostname === 'localhost' ||
    window.location.hostname === '127.0.0.1' ||
    window.location.hostname.endsWith('.local');

  if (isLocalhost) {
    return;
  }

  // 1. Initialize PostHog
  const posthogKey = (import.meta as any).env?.VITE_POSTHOG_KEY || (window as any).__POSTHOG_KEY__;
  if (posthogKey && !isPosthogInitialized) {
    try {
      const posthogHost = (import.meta as any).env?.VITE_POSTHOG_HOST || 'https://us.i.posthog.com';
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
          isPosthogInitialized = true;
        }
      };
      document.head.appendChild(script);
    } catch (err) {
      console.debug('PostHog init skipped:', err);
    }
  }

  // 2. Initialize Google Analytics 4 (GA4)
  const gaId = (import.meta as any).env?.VITE_GA_ID || (window as any).__GA_ID__;
  if (gaId && !isGaInitialized) {
    try {
      const gaScript = document.createElement('script');
      gaScript.async = true;
      gaScript.src = `https://www.googletagmanager.com/gtag/js?id=${gaId}`;
      gaScript.onload = () => {
        (window as any).dataLayer = (window as any).dataLayer || [];
        function gtag(...args: any[]) {
          (window as any).dataLayer.push(args);
        }
        (window as any).gtag = gtag;
        gtag('js', new Date());
        gtag('config', gaId, {
          anonymize_ip: true,
          cookie_flags: 'SameSite=None;Secure',
        });
        isGaInitialized = true;
      };
      document.head.appendChild(gaScript);
    } catch (err) {
      console.debug('GA4 init skipped:', err);
    }
  }
}

export function trackEvent(eventName: string, properties?: Record<string, any>) {
  if (typeof window === 'undefined') return;

  // Track to PostHog
  try {
    if (isPosthogInitialized && (window as any).posthog) {
      (window as any).posthog.capture(eventName, properties);
    }
  } catch {
    // Silent
  }

  // Track to GA4
  try {
    if (isGaInitialized && typeof (window as any).gtag === 'function') {
      (window as any).gtag('event', eventName, properties);
    }
  } catch {
    // Silent
  }
}
