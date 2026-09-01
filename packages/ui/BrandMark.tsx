import React from "react";

export interface BrandMarkProps {
  size?: number;
  className?: string;
}

/**
 * The AgentWorth mark — a thermal receipt with a torn bottom edge, two
 * neutral line items over one heavier violet bar (the total).
 *
 * Inlined from packages/ui/brand/mark.svg with the two fills swapped for
 * CSS variables (`--mv-ink` for the body, `--mv-accent` for the total) so
 * one component is theme-correct everywhere the token cascade reaches —
 * no separate `-dark` file to pick.
 */
export const BrandMark: React.FC<BrandMarkProps> = ({ size = 20, className = "" }) => (
  <svg
    width={size}
    height={size}
    viewBox="0 0 32 32"
    role="img"
    aria-label="AgentWorth"
    className={className}
  >
    <path
      fill="var(--mv-ink)"
      fillRule="evenodd"
      d="M7 6.4A2.4 2.4 0 0 1 9.4 4H22.6A2.4 2.4 0 0 1 25 6.4V25.6L22.75 23.2 20.5 25.6 18.25 23.2 16 25.6 13.75 23.2 11.5 25.6 9.25 23.2 7 25.6ZM11.5 8.8H20.5A1.1 1.1 0 0 1 20.5 11H11.5A1.1 1.1 0 0 1 11.5 8.8ZM11.5 13H16.3A1.1 1.1 0 0 1 16.3 15.2H11.5A1.1 1.1 0 0 1 11.5 13Z"
    />
    <path fill="var(--mv-accent)" d="M11.8 17.2H20.2A1.4 1.4 0 0 1 20.2 20H11.8A1.4 1.4 0 0 1 11.8 17.2Z" />
  </svg>
);
