import React, { useId } from 'react';

/**
 * Real vendor/agent logos for the coverage matrix.
 *
 * Sourced from the Dashboard Icons project (homarr-labs/dashboard-icons,
 * https://github.com/homarr-labs/dashboard-icons), used under the Apache
 * License, Version 2.0 (https://www.apache.org/licenses/LICENSE-2.0).
 * Copyright (c) 2024 Bjorn Lammers, Meier Lukas, Thomas Camlong and Homarr
 * Labs. Per that project's own Legal notice: "All product names,
 * trademarks, and registered trademarks are the property of their
 * respective owners. Icons are used for identification purposes only and
 * do not imply endorsement."
 *
 * Paths below are copied from the project's `svg/<name>.svg` sources, run
 * through svgo, and hand-converted to JSX (kebab-case attrs to camelCase,
 * inline `style="..."` flattened to attributes). Two are further trimmed
 * from upstream, noted on the component: the Gemini mark drops a decorative
 * blurred-aurora background layer meant for large hero placement (not legible
 * at icon size, and it depended on a mask that didn't survive minification
 * cleanly), and OpenCode's two-tone mark is recolored to `currentColor` and
 * padded to a square viewBox. Single-color sources are recolored to
 * `currentColor` so they follow the surrounding text color in both themes;
 * genuinely multi-color brand marks keep their real hex/gradient values.
 *
 * Adapters with no icon in that catalog (aider, cline, cursor, goose,
 * herdr, manus, pi, windsurf, zhipu) fall back to a plain initials
 * monogram — see MonogramLogo below. Hermes (crates/adapters/src/hermes.rs)
 * also falls back: the catalog does have a match (`hermes-icon.svg`,
 * titled "NousResearch"), but it's a ~500-point organic illustration that
 * turns to mud at 16-20px and costs ~18KB even minified, so it isn't used
 * here.
 */

export interface AgentLogoProps {
  className?: string;
  size?: number;
}

// dashboard-icons "claude-ai" — single-color source, recolored via currentColor.
export const ClaudeLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => (
  <svg width={size} height={size} viewBox="0 0 1200 1200" fill="currentColor" className={className} aria-hidden="true">
    <path d="m233.96 800.215 234.684-131.678 3.947-11.436-3.947-6.363h-11.436l-39.221-2.416-134.094-3.624-116.296-4.832-112.67-6.04-28.35-6.04L0 592.751l2.738-17.477 23.84-16.027 34.147 2.98 75.463 5.155 113.235 7.812 82.147 4.832 121.692 12.644h19.329l2.738-7.812-6.604-4.832-5.154-4.832-117.182-79.41-126.845-83.92-66.443-48.321-35.92-24.484-18.12-22.953-7.813-50.093 32.618-35.92 43.812 2.98 11.195 2.98 44.375 34.147 94.792 73.37 123.786 91.167 18.12 15.06 7.249-5.154.886-3.624-8.135-13.61-67.329-121.692-71.838-123.785-31.974-51.302-8.456-30.765c-2.98-12.645-5.154-23.275-5.154-36.242l37.127-50.416 20.537-6.604 49.53 6.604 20.86 18.121 30.765 70.39 49.852 110.818 77.315 150.684 22.631 44.698 12.08 41.396 4.51 12.645h7.813v-7.248l6.362-84.886 11.759-104.215 11.436-134.094 3.946-37.772 18.685-45.262L697.53 24l28.994 13.852L750.363 72l-3.303 22.067-14.174 92.134-27.785 144.323-18.121 96.644h10.55l12.08-12.08 48.887-64.913 82.147-102.685 36.242-40.752 42.282-45.02 27.14-21.423h51.303l37.772 56.135-16.913 57.986-52.832 67.007-43.812 56.779-62.82 84.563-39.22 67.651 3.623 5.396 9.343-.886 141.906-30.201 76.671-13.852 91.49-15.705 41.396 19.329 4.51 19.65-16.269 40.189-97.852 24.16L959.84 601.45l-170.9 40.43-2.093 1.53 2.416 2.98 76.993 7.248 32.94 1.771h80.617l150.12 11.195 39.222 25.933 23.517 31.732-3.946 24.16-60.403 30.766-81.503-19.33-190.228-45.26-65.235-16.27h-9.02v5.397l54.362 53.154 99.624 89.96 124.752 115.973 6.362 28.671-16.027 22.63-16.912-2.415-109.611-82.47-42.282-37.127-95.758-80.618h-6.363v8.456l22.067 32.296 116.537 175.167 6.04 53.719-8.456 17.476-30.201 10.55-33.181-6.04-68.215-95.758-70.39-107.84-56.778-96.644-6.926 3.947-33.503 360.886-15.705 18.443L565.53 1200l-30.201-22.953-16.027-37.127 16.027-73.37 19.329-95.758 15.704-76.107 14.175-94.55 8.456-31.41-.563-2.094-6.927.886-71.275 97.852-108.402 146.497-85.772 91.812-20.537 8.134-35.597-18.443 3.301-32.94 19.893-29.315 118.712-151.007 71.597-93.583 46.228-54.04-.322-7.813h-2.738L205.289 929.396l-56.135 7.248-24.16-22.63 2.98-37.128 11.435-12.08 94.792-65.236-.322.323Z" />
  </svg>
);

// dashboard-icons "codex" — multi-color gradient orb, brand colors kept.
export const CodexLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => {
  const uid = useId().replace(/:/g, '');
  return (
    <svg width={size} height={size} viewBox="0 0 512 512" className={className} aria-hidden="true">
      <defs>
        <linearGradient id={`${uid}-a`}>
          <stop offset="0" stopColor="#d97700" />
          <stop offset=".2" stopColor="#a85c00" />
        </linearGradient>
        <linearGradient id={`${uid}-b`}>
          <stop offset="0" stopColor="#d97700" />
          <stop offset=".8" stopColor="#a85c00" />
        </linearGradient>
        <linearGradient href={`#${uid}-b`} id={`${uid}-d`} x1="221.125" x2="331.379" y1="191.955" y2="382.636" gradientTransform="translate(3 3)" gradientUnits="userSpaceOnUse" />
        <linearGradient href={`#${uid}-b`} id={`${uid}-e`} x1="173" x2="401" y1="155" y2="468" gradientTransform="translate(.5 .5)" gradientUnits="userSpaceOnUse" />
        <linearGradient href={`#${uid}-a`} id={`${uid}-f`} x1="190" x2="512" y1="190" y2="512" gradientUnits="userSpaceOnUse" />
      </defs>
      <g>
        <circle cx="256" cy="256" r="256" fill="#000" />
        <circle cx="256" cy="256" r="69" fill="none" stroke={`url(#${uid}-d)`} strokeWidth="58.5486" />
        <path fill={`url(#${uid}-e)`} d="M256 139c-93.648 0-173.75 47.591-206.182 114.803l-1.3 2.697 1.3 2.668C82.249 326.38 162.352 374 256 374s173.786-47.62 206.217-114.832l1.3-2.668-1.3-2.697C430.278 186.582 350.14 138.99 256.492 138.99Zm0 18.688c84.825 0 154.891 38.584 185.566 98.783-30.668 60.211-100.73 98.841-185.566 98.841s-154.862-38.63-185.53-98.84c30.675-60.2 100.705-98.784 185.53-98.784" />
        <path fill={`url(#${uid}-f)`} d="M244.444 38.097c-8.38-7.042-8.737-20.78-.185-27.973 8.635-7.265 24.032-5.03 28.509 7.493 2.685 7.514 1.187 15.656-6.522 21.1-6.436 4.547-15.465 4.399-21.802-.62m88.784 17.66c-8.38-7.042-8.737-20.779-.185-27.973 8.635-7.264 24.032-5.03 28.509 7.494 2.685 7.513 1.187 15.655-6.522 21.1-6.436 4.546-15.465 4.399-21.802-.62zm75.267 50.293c-8.38-7.042-8.737-20.78-.186-27.973 8.636-7.265 24.033-5.03 28.51 7.494 2.685 7.513 1.187 15.655-6.522 21.1-6.436 4.546-15.465 4.398-21.802-.62zm50.29 75.268c-8.38-7.042-8.736-20.78-.185-27.973 8.636-7.264 24.033-5.03 28.51 7.494 2.685 7.513 1.187 15.655-6.522 21.1-6.436 4.546-15.465 4.398-21.802-.62zm17.659 88.785c-8.38-7.042-8.737-20.78-.186-27.973 8.636-7.264 24.033-5.03 28.51 7.494 2.685 7.513 1.187 15.655-6.522 21.1-6.436 4.546-15.465 4.398-21.802-.62zm-17.69 88.77c-8.38-7.043-8.737-20.78-.185-27.974 8.635-7.264 24.032-5.03 28.509 7.494 2.685 7.513 1.188 15.655-6.522 21.1-6.436 4.546-15.464 4.398-21.802-.62m-50.287 75.255c-8.38-7.042-8.736-20.78-.185-27.973 8.635-7.264 24.032-5.03 28.509 7.494 2.685 7.513 1.188 15.655-6.522 21.1-6.436 4.546-15.464 4.398-21.802-.62zm-75.245 50.31c-8.38-7.042-8.737-20.78-.185-27.973 8.635-7.264 24.032-5.03 28.509 7.494 2.685 7.513 1.188 15.655-6.522 21.1-6.436 4.546-15.464 4.398-21.802-.62zm-88.782 17.628c-8.38-7.042-8.737-20.78-.185-27.973 8.635-7.264 24.032-5.03 28.509 7.494 2.685 7.513 1.187 15.655-6.522 21.1-6.436 4.546-15.465 4.398-21.802-.62zm-88.771-17.659c-8.38-7.042-8.737-20.78-.185-27.973 8.635-7.264 24.032-5.03 28.509 7.494 2.685 7.513 1.187 15.655-6.522 21.1-6.436 4.546-15.465 4.398-21.802-.62zm-75.276-50.264c-8.38-7.042-8.737-20.78-.185-27.973 8.635-7.264 24.032-5.03 28.509 7.494 2.685 7.513 1.187 15.655-6.522 21.1-6.436 4.546-15.465 4.398-21.802-.62zM30.13 358.866c-8.38-7.042-8.737-20.78-.185-27.973 8.635-7.264 24.032-5.03 28.509 7.494 2.685 7.513 1.187 15.655-6.522 21.1-6.436 4.546-15.465 4.398-21.802-.62zm-17.657-88.772c-8.38-7.042-8.737-20.78-.185-27.973 8.635-7.264 24.032-5.03 28.509 7.494 2.685 7.513 1.187 15.655-6.522 21.1-6.436 4.546-15.465 4.398-21.802-.62zm17.63-88.78c-8.38-7.043-8.736-20.78-.184-27.974 8.635-7.264 24.032-5.03 28.509 7.494 2.685 7.513 1.187 15.655-6.522 21.1-6.436 4.546-15.465 4.398-21.802-.62zm50.313-75.245c-8.38-7.042-8.737-20.78-.185-27.973 8.635-7.265 24.032-5.03 28.509 7.494 2.685 7.513 1.187 15.655-6.522 21.1-6.436 4.546-15.465 4.398-21.802-.62zm75.257-50.285c-8.38-7.042-8.737-20.78-.185-27.973 8.635-7.264 24.032-5.03 28.509 7.494 2.685 7.513 1.187 15.655-6.522 21.1-6.436 4.546-15.465 4.398-21.802-.62zM256 0C152.527 0 59.084 62.436 19.488 158.031c-39.595 95.596-17.665 205.815 55.5 278.98 73.166 73.166 183.385 95.096 278.98 55.5C449.565 452.917 512 359.474 512 256 512 114.9 397.1 0 256 0m0 48c115.16 0 208 92.84 208 208 0 84.197-50.615 159.947-128.4 192.166-77.786 32.219-167.134 14.44-226.67-45.096S31.615 254.186 63.834 176.4C96.053 98.615 171.804 48 256 48" />
      </g>
    </svg>
  );
};

// dashboard-icons "google-gemini" — trimmed to the sparkle mark + its linear
// gradient; see file header. Used for both the "gemini" and "antigravity"
// adapters (Antigravity is Google's own IDE on the same model family and has
// no separate mark in the catalog).
export const GeminiLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => {
  const gradId = useId().replace(/:/g, '');
  return (
    <svg width={size} height={size} viewBox="0 0 65 65" className={className} aria-hidden="true">
      <path
        fill={`url(#${gradId})`}
        d="M32.447 0c.68 0 1.273.465 1.439 1.125a39 39 0 0 0 1.999 5.905q3.23 7.5 8.854 13.125 5.626 5.626 13.125 8.855a39 39 0 0 0 5.906 1.999c.66.166 1.124.758 1.124 1.438s-.464 1.273-1.125 1.439a39 39 0 0 0-5.905 1.999q-7.5 3.23-13.125 8.854-5.625 5.626-8.854 13.125a39 39 0 0 0-2 5.906 1.485 1.485 0 0 1-1.438 1.124c-.68 0-1.272-.464-1.438-1.125a39 39 0 0 0-2-5.905q-3.227-7.5-8.854-13.125-5.625-5.625-13.125-8.854a39 39 0 0 0-5.905-2A1.485 1.485 0 0 1 0 32.448c0-.68.465-1.272 1.125-1.438a39 39 0 0 0 5.905-2q7.5-3.228 13.125-8.854 5.626-5.624 8.855-13.125a39 39 0 0 0 1.999-5.905A1.485 1.485 0 0 1 32.447 0"
      />
      <defs>
        <linearGradient id={gradId} x1="18.447" x2="52.153" y1="43.42" y2="15.004" gradientUnits="userSpaceOnUse">
          <stop stopColor="#4893fc" />
          <stop offset=".27" stopColor="#4893fc" />
          <stop offset=".777" stopColor="#969dff" />
          <stop offset="1" stopColor="#bd99fe" />
        </linearGradient>
      </defs>
    </svg>
  );
};

// dashboard-icons "deepseek" — single-color source, recolored via currentColor.
export const DeepSeekLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => (
  <svg width={size} height={size} viewBox="0 0 377.1 277.86" fill="currentColor" className={className} aria-hidden="true">
    <path d="M373.15 23.32c-4-1.95-5.72 1.77-8.06 3.66-.79.62-1.47 1.43-2.14 2.14-5.85 6.26-12.67 10.36-21.57 9.86-13.04-.71-24.16 3.38-33.99 13.37-2.09-12.31-9.04-19.66-19.6-24.38-5.54-2.45-11.13-4.9-14.99-10.23-2.71-3.78-3.44-8-4.81-12.16-.85-2.51-1.72-5.09-4.6-5.52-3.13-.5-4.36 2.14-5.58 4.34-4.93 8.99-6.82 18.92-6.65 28.97.43 22.58 9.97 40.56 28.89 53.37 2.16 1.46 2.71 2.95 2.03 5.09-1.29 4.4-2.82 8.68-4.19 13.09-.85 2.82-2.14 3.44-5.15 2.2-10.39-4.34-19.37-10.76-27.29-18.55-13.46-13.02-25.63-27.41-40.81-38.67-3.57-2.64-7.12-5.09-10.81-7.41-15.49-15.07 2.03-27.45 6.08-28.9 4.25-1.52 1.47-6.79-12.23-6.73-13.69.06-26.24 4.65-42.21 10.76-2.34.93-4.79 1.61-7.32 2.14-14.5-2.73-29.55-3.35-45.29-1.58-29.62 3.32-53.28 17.34-70.68 41.28C1.29 88.2-3.63 120.88 2.39 155c6.33 35.91 24.64 65.68 52.8 88.94 29.18 24.1 62.8 35.91 101.15 33.65 23.29-1.33 49.23-4.46 78.48-29.24 7.38 3.66 15.12 5.12 27.97 6.23 9.89.93 19.41-.5 26.79-2.02 11.55-2.45 10.75-13.15 6.58-15.13-33.87-15.78-26.44-9.36-33.2-14.54 17.21-20.41 43.15-41.59 53.3-110.19.79-5.46.11-8.87 0-13.3-.06-2.67.54-3.72 3.61-4.03 8.48-.96 16.72-3.29 24.28-7.47 21.94-12 30.78-31.69 32.87-55.33.31-3.6-.06-7.35-3.86-9.24ZM181.96 235.97c-32.83-25.83-48.74-34.33-55.31-33.96-6.14.34-5.04 7.38-3.69 11.97 1.41 4.53 3.26 7.66 5.85 11.63 1.78 2.64 3.01 6.57-1.78 9.49-10.57 6.58-28.95-2.2-29.82-2.64-21.38-12.59-39.26-29.24-51.87-52.01-12.16-21.92-19.23-45.43-20.39-70.52-.31-6.08 1.47-8.22 7.49-9.3 7.92-1.46 16.11-1.77 24.03-.62 33.49 4.9 62.01 19.91 85.9 43.63 13.65 13.55 23.97 29.71 34.61 45.49 11.3 16.78 23.48 32.75 38.97 45.84 5.46 4.59 9.83 8.09 14 10.67-12.59 1.4-33.62 1.71-47.99-9.68Zm15.73-101.32c0-2.7 2.15-4.84 4.87-4.84.6 0 1.16.12 1.66.31.67.25 1.29.62 1.77 1.18.87.84 1.36 2.08 1.36 3.35 0 2.7-2.15 4.84-4.85 4.84s-4.81-2.14-4.81-4.84m48.86 25.12c-3.13 1.27-6.26 2.39-9.27 2.51-4.67.22-9.77-1.68-12.55-4-4.3-3.6-7.36-5.61-8.67-11.94-.54-2.7-.23-6.85.25-9.24 1.12-5.15-.12-8.44-3.74-11.44-2.96-2.45-6.7-3.1-10.82-3.1-1.54 0-2.95-.68-4-1.24-1.72-.87-3.13-3.01-1.78-5.64.43-.84 2.53-2.92 3.02-3.29 5.58-3.19 12.03-2.14 18 .25 5.54 2.26 9.71 6.42 15.72 12.28 6.16 7.1 7.26 9.09 10.76 14.39 2.76 4.19 5.29 8.47 7.01 13.37 1.04 3.04-.31 5.55-3.94 7.1Z" />
  </svg>
);

// dashboard-icons "grok" — single-color source, recolored via currentColor.
export const GrokLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => (
  <svg width={size} height={size} viewBox="0.36 0.5 33.33 32" fill="currentColor" className={className} aria-hidden="true">
    <path d="m13.237 21.04 11.082-8.19c.543-.4 1.32-.244 1.578.38 1.363 3.288.754 7.241-1.957 9.955-2.71 2.714-6.482 3.31-9.93 1.954l-3.765 1.745c5.401 3.697 11.96 2.782 16.059-1.324 3.251-3.255 4.258-7.692 3.317-11.693l.008.009c-1.365-5.878.336-8.227 3.82-13.031q.123-.17.247-.345l-4.585 4.59v-.014L13.234 21.044m-2.284 1.987c-3.877-3.707-3.208-9.446.1-12.755 2.446-2.449 6.454-3.448 9.952-1.979L24.76 6.56c-.677-.49-1.545-1.017-2.54-1.387A12.465 12.465 0 0 0 8.675 7.901c-3.519 3.523-4.625 8.94-2.725 13.561 1.42 3.454-.907 5.898-3.251 8.364-.83.874-1.664 1.749-2.335 2.674l10.583-9.466" />
  </svg>
);

// dashboard-icons "kimi-ai" (Moonshot AI) — multi-color, brand colors kept.
export const KimiLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => (
  <svg width={size} height={size} viewBox="2589.93 529.3 4225.03 4225.03" fillRule="evenodd" clipRule="evenodd" className={className} aria-hidden="true">
    <rect width="4225.03" height="4225.03" x="2589.93" y="529.3" rx="304.59" ry="304.59" fill="#2b2a29" />
    <path fill="#fff" d="M4354.82 2395.15h-641.29V1351.48h-518.69v2700.35h518.69V2913.84h930.5c99.31 0 187.16-42.51 257.02-104.49 29.17-25.87 43.76-42.09 67.45-77.16l27.62-44.68v1364.32h521.84V2923.28c0-275.9-193.35-483.68-441.87-523.22-53.14-8.46-250.95-4.91-318.89-4.91 25.23-21.93 133.44-28.86 263.21-208.33 8.03-11.11 13.21-21.99 19.88-33.56 23.63-40.99 81.8-173.93 101.34-219.31l114.36-256.58c27.34-56.91 56.36-131.98 81.76-182.31 6.16-12.22 11.49-25.16 16.69-36.74 8.37-18.63 46-98.65 47.21-113.12h-578.42c-10.76 20.34-18.77 42.96-29.35 64.97-5.26 10.95-10.16 23.06-14.21 32.93l-390.21 889.24c-8.43 18.76-23.03 62.81-34.64 62.81" />
    <path fill="#0179ff" d="M5624.83 1533.81c0 162.36 81.74 224.33 81.74 242.06 0 23.25-11.97 27.5-32.81 55.21-11.61 15.44-24.68 29.28-33.21 45.39l81.58-3.31c11.75-.97 10.98-2.67 25.21-3.08l81.85-3.04c13.69-.7 11.45-2.49 25.14-3.15 93.1-4.5 197.72-15.93 261.76-77.75 118.61-114.51 125.67-360.29 17.22-468.46-45.13-45.01-112.6-81.03-178.4-82.52-17.05-.39-19.63-3.34-34.58-3.34-29.74 0-98.29 11.16-125.81 22.15-34.4 13.73-66.41 32.15-91.97 58.92-40.84 42.79-77.72 137.66-77.72 220.92" />
  </svg>
);

// dashboard-icons "minimax" — single-color source, recolored via currentColor.
export const MiniMaxLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => (
  <svg width={size} height={size} fill="currentColor" viewBox="0 0 24 24" className={className} aria-hidden="true">
    <path d="M11.43 3.92a.86.86 0 1 0-1.718 0v14.236a1.999 1.999 0 0 1-3.997 0V9.022a.86.86 0 1 0-1.718 0v3.87a1.999 1.999 0 0 1-3.997 0V11.49a.57.57 0 0 1 1.139 0v1.404a.86.86 0 0 0 1.719 0V9.022a1.999 1.999 0 0 1 3.997 0v9.134a.86.86 0 0 0 1.719 0V3.92a1.998 1.998 0 1 1 3.996 0v11.788a.57.57 0 1 1-1.139 0zm10.572 3.105a2 2 0 0 0-1.999 1.997v7.63a.86.86 0 0 1-1.718 0V3.923a1.999 1.999 0 0 0-3.997 0v16.16a.86.86 0 0 1-1.719 0V18.08a.57.57 0 1 0-1.138 0v2a1.998 1.998 0 0 0 3.996 0V3.92a.86.86 0 0 1 1.719 0v12.73a1.999 1.999 0 0 0 3.996 0V9.023a.86.86 0 1 1 1.72 0v6.686a.57.57 0 0 0 1.138 0V9.022a2 2 0 0 0-1.998-1.997" />
  </svg>
);

// dashboard-icons "qwen" (Alibaba) — multi-color gradient, brand colors kept.
// Both radial gradients in the source share identical stops, so one <defs>
// entry is reused for both shapes here instead of duplicating it.
export const QwenLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => {
  const gradId = useId().replace(/:/g, '');
  return (
    <svg width={size} height={size} viewBox="27.55 17.52 147.28 145.51" className={className} aria-hidden="true">
      <path
        fill={`url(#${gradId})`}
        d="M174.82 108.75 155.38 75l10.26-17.25c.82-1.44.82-3.22 0-4.66l-10.26-17.25a2.97 2.97 0 0 0-2.6-1.51h-37.9l-8.74-15.3a2.97 2.97 0 0 0-2.6-1.51H83.3c-1.09 0-2.08.58-2.6 1.51L61.26 52.77H41.02c-1.09 0-2.08.58-2.6 1.51L28.16 71.53a4.72 4.72 0 0 0 0 4.66l17.36 31.31-8.74 15.3a4.72 4.72 0 0 0 0 4.66l10.26 17.25c.52.93 1.51 1.51 2.6 1.51h37.9l8.74 15.3c.52.93 1.51 1.51 2.6 1.51h20.24c1.09 0 2.08-.58 2.6-1.51l19.44-33.74h17.36c1.09 0 2.08-.58 2.6-1.51l10.26-17.25c.82-1.44.82-3.22 0-4.66z"
      />
      <path fill="#fff" d="M119.12 163.03H98.88l-11.34-18.32h-37.9l11.62-18.32H80.7l-42.28-71.1h22.84L83.3 19.03l10.26 18.32L83.3 55.29h78.28l-10.26 17.25 19.44 33.74h-19.44l-10.16-17.94-39.98 74.69z" />
      <path fill={`url(#${gradId})`} d="M127.86 79.83H76.14l25.04 42.28z" />
      <defs>
        <radialGradient id={gradId} cx="0" cy="0" r="1" gradientTransform="rotate(90 0 100)scale(100)" gradientUnits="userSpaceOnUse">
          <stop stopColor="#665cee" />
          <stop offset="1" stopColor="#332e91" />
        </radialGradient>
      </defs>
    </svg>
  );
};

// dashboard-icons "openclaw" — confirmed by the catalog's own aliases
// (claude-bot, clawdbot, moltbot, open-claw, clawd-bot): this is the
// OpenClaw agent, not a naming coincidence. Multi-tone illustration
// (originally a <style>+class palette; resolved to plain fill attrs here
// since JSX can't hold raw CSS text without escaping every `{`).
export const OpenClawLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => (
  <svg width={size} height={size} viewBox="0 0 500 500" className={className} aria-hidden="true">
    <path fillRule="evenodd" fill="#f6f4f4" d="M166.5 52.5h7q2.75 2.99 1.5 7-21.27 45.61-20.5 96 39.99 2.76 72 26.5 7.87 6.86 13.5 15.5 42.88-56.39 103.5-92.5 47.35-25.46 101-25 14.52.38 23.5 11.5 3.19 7.74 2 16-1.81 7.18-4.5 14-1 0-1 1-5.04 6.05-9 13-1 0-1 1v1q-12.42 12.15-28.5 19.5-3.99 1.5-8 3-32.99-.99-63.5 11.5-24.85 10.02-44 28.5-1.85 16.65 1 33 3.03 21.55.5 43-1.05 32.24-16.5 60.5-4.66 8.32-11 15.5-.36-.9-1-1.5-.11-24.32-4-48-3.03-16.86-14.5-29.5-3.06 1.01-6 2.5-24.71-1.3-49 3-16.02 3.24-31 10-15.55 6.7-30.5 14.5-1 0-2 1-2.98-.5-5.5-2.5-2.98-2.5-6-5-6.96-6.44-13-14-1.11-2.5-3-4.5-1.15-1.53-2-3-9.35-16.29-13-35-2.15-16.24 3.5-32 3.35-8.44 9.5-15.5.09-1.42 1-2 10.19-11.09 24.5-16 20.62-6.94 40.5-16.5 8.94-4.9 14.5-13.5 1.14-9.62 2.5-19.5 3.98-19.29-.5-38-3.36-3.15-6.5-6.5-16.94-24.98-15.5-55.5.34-15.02 12.5-24.5 8.45-6.5 19-8"/>
    <path fill="#f6f4f4" d="M247.5 197.5q4.51-.66 9-1 4.51.13 9 0 5.03-.15 10 1 8.34-1.06 15.5 3 1.28 21.06 1 42-.31 25.19-1.5 50.5.27 3.53-1.5 6.5-6.5.99-13.5.5-3.72-.44-7.5-.5-6.32-.32-12.5-1.5.35-1.34 1-2.5-1.9-3.44-1.5-8-.94-40.9-1.5-82-2.79-4.53-6.5-8Z"/>
    <path fill="#0b0303" d="M181.5 165.5q34.36-1.68 47.5 30-1.42 18.29-16.5 29-19.5 10.11-38-2-14.55-13.28-13-33 3.35-16.19 20-24Z"/>
    <path fill="#0b0303" d="M311.5 168.5q19.62-1.42 32.5 13 9.4 15.86 3 33.5-8.24 17.24-27.5 20-19.02 1.02-31-13.5-9.94-15.63-3.5-33 6.29-14.75 21.5-19.5-2.9 1.61-5.5 3.5-16.72 12.99-11.5 33.5 5.68 18.13 25 18.5 18.55.19 26-17 5.5-15.11-3.5-28.5-11.79-16.19-33-16.5 3.6-.36 7.5-1Z"/>
    <path fillRule="evenodd" fill="#f00212" d="M280.5 235.5q3.78.06 7.5.5 6.99.5 13.5-.5-1.03 5.28-3.5 10-6.13 15.4-19.5 25.5-3.66 2.66-8 4-13.16-3.98-22-14.5-8.13-9.99-11-22.5 6.18-1.15 12.5-1 6.32 1.18 12.5 1.5 8.7 1.31 17.5.5.5-2.13 0-4Z"/>
    <path fillRule="evenodd" fill="#ef0011" d="M99.5 233.5q23.42-4.29 47.5-3 5.11.53 9.5 3 2.29 12.68 3.5 25.5.86 8.98 3 17.5.32 1.68-1 2.5-30.02 16.87-58.5 36-2.87-3.13-5-7-9.09-14.09-13-30-1.09-5.87-1-12 3.16-16.16 15-27.5Z"/>
    <path fillRule="evenodd" fill="#ba000d" d="M356.5 232.5q22.11-1.36 44 2 5.02.75 9 4 15.31 15.77 15.5 37.5-.13 1.5-.5 3-4.87 17.32-16 31-1.06.9-2 2-13.55-9.72-28-18-14.94-8.55-30.5-16-1.86-1.52-1.5-4 1.85-8.19 3.5-16.5 2.09-9.24 3-19 1.06-5.83 3.5-11Z"/>
    <path fillRule="evenodd" fill="#f3e2e2" d="M198.5 262.5q1.35-.15 2.5.5 21.94 22.52 51 32.5 26.76 8.35 54.5 4 26.03-5.06 46-22.5 5.51-4.79 10-10.5 22.28 12.42 43.5 26.5-4.14 8.62-9.5 16.5-2.34 2.79-4 6-.16 1.16-1 2-.71 1.31-1.5 2.5-.87.87-1.5 2-8.51 9.9-18.5 18-.98.34-1.5 1.5-24.4 18.34-53 27.5-8.31 2.09-16.5 4.5-9.05.98-18 2.5-13.34.65-26.5-1-15.13-1.02-29.5-6-16.62-4.66-31.5-13-2.13-.87-4-2-.87-.71-2-1.5-.71-.87-1.5-2-1.13-.87-2-2-.87-.71-1.5-2-.71-.87-1.5-2-.71-.87-1.5-2-.68-1.36-2-2-4.51-8.51-9-17-3.42-4.98-5.5-10.5-.87-2.13-2-4 20.85-13.99 42.5-26.5Z"/>
  </svg>
);

// dashboard-icons "opencode" — two-tone mark, recolored via currentColor and
// re-centered into a square viewBox (source was 240x300).
export const OpenCodeLogo: React.FC<AgentLogoProps> = ({ className = '', size = 20 }) => (
  <svg width={size} height={size} viewBox="-30 0 300 300" fill="currentColor" className={className} aria-hidden="true">
    <path fillOpacity=".55" d="M180 240H60V120h120z" />
    <path d="M180 60H60v180h120zm60 240H0V0h240z" />
  </svg>
);

/** Plain initials fallback for adapters with no icon in the Dashboard Icons catalog. */
export const MonogramLogo: React.FC<AgentLogoProps & { label: string }> = ({
  label,
  className = '',
  size = 20,
}) => (
  <svg width={size} height={size} viewBox="0 0 24 24" className={className} aria-hidden="true">
    <circle cx="12" cy="12" r="11" fill="currentColor" fillOpacity="0.12" />
    <text x="12" y="12.5" textAnchor="middle" dominantBaseline="middle" fontSize="9.5" fontWeight="600" fill="currentColor">
      {label}
    </text>
  </svg>
);

export const AgentLogoIcon: React.FC<{ adapterId: string; size?: number; className?: string }> = ({
  adapterId,
  size = 20,
  className = '',
}) => {
  switch (adapterId) {
    case 'claude_code':
      return <ClaudeLogo size={size} className={className} />;
    case 'codex':
      return <CodexLogo size={size} className={className} />;
    case 'gemini':
    case 'antigravity':
      return <GeminiLogo size={size} className={className} />;
    case 'deepseek':
      return <DeepSeekLogo size={size} className={className} />;
    case 'grok':
      return <GrokLogo size={size} className={className} />;
    case 'kimi':
      return <KimiLogo size={size} className={className} />;
    case 'minimax':
      return <MiniMaxLogo size={size} className={className} />;
    case 'qwen':
      return <QwenLogo size={size} className={className} />;
    case 'openclaw':
      return <OpenClawLogo size={size} className={className} />;
    case 'opencode':
      return <OpenCodeLogo size={size} className={className} />;
    case 'cursor':
      return <MonogramLogo label="Cu" size={size} className={className} />;
    case 'goose':
      return <MonogramLogo label="Go" size={size} className={className} />;
    case 'hermes':
      return <MonogramLogo label="He" size={size} className={className} />;
    case 'herdr':
      return <MonogramLogo label="Hr" size={size} className={className} />;
    case 'pi':
      return <MonogramLogo label="Pi" size={size} className={className} />;
    case 'zhipu':
      return <MonogramLogo label="Zh" size={size} className={className} />;
    case 'aider':
      return <MonogramLogo label="Ad" size={size} className={className} />;
    case 'cline':
      return <MonogramLogo label="Cl" size={size} className={className} />;
    case 'windsurf':
      return <MonogramLogo label="Ws" size={size} className={className} />;
    case 'manus':
      return <MonogramLogo label="Mn" size={size} className={className} />;
    default:
      return null;
  }
};

export const getAgentLogo = (adapterId: string, size = 20, className = '') => (
  <AgentLogoIcon adapterId={adapterId} size={size} className={className} />
);
