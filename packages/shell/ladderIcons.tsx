export interface DsIconProps {
  size?: number;
  className?: string;
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

/** Dashed circle — "nothing has confirmed this yet". */
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
