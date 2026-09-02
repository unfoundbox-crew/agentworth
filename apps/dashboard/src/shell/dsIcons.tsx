/**
 * SpacePilot design-system icons used by the dashboard shell.
 *
 * Ported from the owner's icon sprite (24x24 grid, 1.5px stroke, square
 * caps, miter joins, fill:none) — a different construction from the
 * hand-drawn round-capped set in packages/ui/icons.tsx, which belongs to
 * the marketing site and is left untouched. Only the icons this shell
 * actually consumes are ported here; add more from the sprite as needed
 * rather than porting all 27 up front.
 */

export interface DsIconProps {
  size?: number;
  className?: string;
}

/** Two stacked rows with a leading dot each — reads as a list of sessions. */
export function IconDock({ size = 14, className }: DsIconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="square"
      strokeLinejoin="miter"
      aria-hidden="true"
      className={className}
    >
      <rect x="3" y="5" width="18" height="6" rx="1" />
      <rect x="3" y="14" width="18" height="6" rx="1" />
      <circle cx="7" cy="8" r=".85" fill="currentColor" stroke="none" />
      <circle cx="7" cy="17" r=".85" fill="currentColor" stroke="none" />
    </svg>
  );
}

/** A cog — hub, rim and eight teeth. Settings. */
export function IconSettings({ size = 14, className }: DsIconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="square"
      strokeLinejoin="miter"
      aria-hidden="true"
      className={className}
    >
      <circle cx="12" cy="12" r="3.25" />
      <circle cx="12" cy="12" r="7" />
      <path d="M12 2.5v2.5M12 19v2.5M2.5 12h2.5M19 12h2.5" />
      <path d="M5.3 5.3 7 7M17 17l1.7 1.7M18.7 5.3 17 7M7 17l-1.7 1.7" />
    </svg>
  );
}

/** A gauge — arc with a needle. */
export function IconMeter({ size = 14, className }: DsIconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="square"
      strokeLinejoin="miter"
      aria-hidden="true"
      className={className}
    >
      <path d="M3.5 18a8.5 8.5 0 0 1 17 0" />
      <path d="M12 18 16.5 10.5" />
    </svg>
  );
}

/** A 2x2 grid of tiles — reads as a coverage matrix. */
export function IconCargo({ size = 14, className }: DsIconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="square"
      strokeLinejoin="miter"
      aria-hidden="true"
      className={className}
    >
      <rect x="3" y="3" width="7.5" height="7.5" rx="1" />
      <rect x="13.5" y="3" width="7.5" height="7.5" rx="1" />
      <rect x="3" y="13.5" width="7.5" height="7.5" rx="1" />
      <rect x="13.5" y="13.5" width="7.5" height="7.5" rx="1" />
    </svg>
  );
}

/** A magnifier / probe — circle with a handle, digging into a trace. */
export function IconProbe({ size = 14, className }: DsIconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="square"
      strokeLinejoin="miter"
      aria-hidden="true"
      className={className}
    >
      <circle cx="11" cy="11" r="6.5" />
      <path d="M15.8 15.8 20.5 20.5" />
    </svg>
  );
}

/** Arrow down into a tray. */
export function IconDownload({ size = 14, className }: DsIconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="square"
      strokeLinejoin="miter"
      aria-hidden="true"
      className={className}
    >
      <path d="M12 3v12M7 10.5 12 15.5l5-5M4 20h16" />
    </svg>
  );
}

/** Circle with a check — externally / machine-checked evidence. */
export function IconVerified({ size = 14, className }: DsIconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="square"
      strokeLinejoin="miter"
      aria-hidden="true"
      className={className}
    >
      <circle cx="12" cy="12" r="8.5" />
      <path d="M8 12.2 10.8 15 16 9.8" />
    </svg>
  );
}

/** Dashed circle — the design system's own word for "nothing has confirmed this yet". */
export function IconUnflown({ size = 14, className }: DsIconProps) {
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="square"
      strokeLinejoin="miter"
      aria-hidden="true"
      className={className}
    >
      <circle cx="12" cy="12" r="8.5" strokeDasharray="2.4 3" />
    </svg>
  );
}
