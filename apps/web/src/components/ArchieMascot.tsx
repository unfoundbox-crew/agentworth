import React from "react";

interface ArchieMascotProps {
  quote?: string;
  bullets?: string[];
  className?: string;
}

export const ArchieMascot: React.FC<ArchieMascotProps> = ({
  quote = "Your agents left receipts.",
  bullets = [
    "Digging through dotfiles",
    "Auditing token burn pacing",
    "Tracing line-by-line lineage",
  ],
  className = "",
}) => {
  return (
    <div
      className={`inline-block rounded-xl border border-border bg-surface text-ink p-4 sm:p-5 select-none ${className}`}
    >
      <div className="flex flex-col sm:flex-row items-center sm:items-start gap-4">
        <pre className="font-mono text-[13px] sm:text-[14px] leading-[1.25] shrink-0 text-ink">
{`┌───────────┐
│ ( • _ • ) │
│  /|  *  |\\│
│  / |  | \\ │
│   /    \\  │
└───┴────┴──┘`}
        </pre>

        <div className="text-left space-y-2">
          <div className="font-mono font-semibold text-sm text-ink">
            &quot;{quote}&quot;
          </div>
          <div className="h-px bg-border w-full" />
          <ul className="space-y-1 text-xs text-muted">
            {bullets.map((bullet, idx) => (
              <li key={idx} className="flex items-center gap-1.5">
                <span className="text-accent font-bold">&mdash;</span>
                <span>{bullet}</span>
              </li>
            ))}
          </ul>
        </div>
      </div>
    </div>
  );
};
