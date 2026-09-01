import React from "react";

export interface IconProps {
  className?: string;
  size?: number;
}

/**
 * Small inline-SVG icon set for the landing/chrome components. No icon
 * library is installed for these files (design.md hard rule 3 forbids
 * emoji as markers), so every glyph here is hand-drawn on a shared 20x20
 * stroke grid — strokeWidth 1.6, round caps/joins, fill: none — except
 * IconGithub, which renders the actual GitHub brand mark as a filled path
 * (the same treatment AgentLogos.tsx gives third-party agent logos).
 */
const base = {
  width: 20,
  height: 20,
  viewBox: "0 0 20 20",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.6,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};

export const IconGithub: React.FC<IconProps> = ({ className = "", size = 20 }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" className={className} aria-hidden="true">
    <path d="M12 .5C5.73.5.5 5.73.5 12c0 5.09 3.29 9.39 7.86 10.91.57.1.78-.25.78-.55 0-.27-.01-1.16-.02-2.11-3.2.7-3.88-1.36-3.88-1.36-.53-1.34-1.29-1.7-1.29-1.7-1.05-.72.08-.71.08-.71 1.17.08 1.78 1.2 1.78 1.2 1.03 1.77 2.71 1.26 3.37.96.1-.75.4-1.26.73-1.55-2.55-.29-5.23-1.28-5.23-5.7 0-1.26.45-2.29 1.19-3.09-.12-.29-.52-1.46.11-3.05 0 0 .97-.31 3.18 1.18a11.05 11.05 0 0 1 5.79 0c2.2-1.49 3.17-1.18 3.17-1.18.64 1.59.24 2.76.12 3.05.74.8 1.19 1.83 1.19 3.09 0 4.43-2.69 5.41-5.25 5.69.41.36.78 1.07.78 2.15 0 1.55-.01 2.8-.01 3.18 0 .3.2.66.79.55A10.52 10.52 0 0 0 23.5 12C23.5 5.73 18.27.5 12 .5Z" />
  </svg>
);

export const IconCopy: React.FC<IconProps> = ({ className = "", size = 20 }) => (
  <svg {...base} width={size} height={size} className={className} aria-hidden="true">
    <rect x="7.5" y="7.5" width="9" height="9" rx="1.5" />
    <path d="M5 12.5h-.5A1.5 1.5 0 0 1 3 11V4.5A1.5 1.5 0 0 1 4.5 3H11a1.5 1.5 0 0 1 1.5 1.5V5" />
  </svg>
);

export const IconCheck: React.FC<IconProps> = ({ className = "", size = 20 }) => (
  <svg {...base} width={size} height={size} className={className} aria-hidden="true">
    <path d="M4 10.5l4 4 8-9" />
  </svg>
);

export const IconArrowRight: React.FC<IconProps> = ({ className = "", size = 20 }) => (
  <svg {...base} width={size} height={size} className={className} aria-hidden="true">
    <path d="M3.5 10h13M11 4.5l5.5 5.5-5.5 5.5" />
  </svg>
);

export const IconArrowUpRight: React.FC<IconProps> = ({ className = "", size = 20 }) => (
  <svg {...base} width={size} height={size} className={className} aria-hidden="true">
    <path d="M6 14L14 6M8 6h6v6" />
  </svg>
);

export const IconShieldCheck: React.FC<IconProps> = ({ className = "", size = 20 }) => (
  <svg {...base} width={size} height={size} className={className} aria-hidden="true">
    <path d="M10 2.3l6.2 2.25v4.9c0 4.15-2.66 7.05-6.2 8.05-3.54-1-6.2-3.9-6.2-8.05v-4.9L10 2.3Z" />
    <path d="M7 10.1l2.1 2.1 3.9-4.3" />
  </svg>
);

export const IconDatabase: React.FC<IconProps> = ({ className = "", size = 20 }) => (
  <svg {...base} width={size} height={size} className={className} aria-hidden="true">
    <ellipse cx="10" cy="5.2" rx="6" ry="2.3" />
    <path d="M4 5.2v9.6c0 1.27 2.69 2.3 6 2.3s6-1.03 6-2.3V5.2" />
    <path d="M4 10c0 1.27 2.69 2.3 6 2.3s6-1.03 6-2.3" />
  </svg>
);

export const IconRefresh: React.FC<IconProps & { spinning?: boolean }> = ({
  className = "",
  size = 20,
  spinning = false,
}) => (
  <svg
    {...base}
    width={size}
    height={size}
    className={`${spinning ? "animate-spin" : ""} ${className}`}
    aria-hidden="true"
  >
    <path d="M15.7 6.6A6 6 0 0 0 5.1 8.6" />
    <path d="M4.3 13.4A6 6 0 0 0 14.9 11.4" />
    <path d="M15.7 3.4v3.4h-3.4" />
    <path d="M4.3 16.6v-3.4h3.4" />
  </svg>
);
