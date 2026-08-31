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
      className={`inline-block border-2 border-black dark:border-white bg-white dark:bg-[#121215] text-black dark:text-white p-4 sm:p-5 font-mono text-xs shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] dark:shadow-[4px_4px_0px_0px_rgba(255,255,255,1)] select-none ${className}`}
    >
      <div className="flex flex-col sm:flex-row items-center sm:items-start gap-4">
        {/* Monospace ASCII Mascot Card */}
        <pre className="text-black dark:text-white font-mono text-[13px] sm:text-[14px] leading-[1.25] shrink-0">
{`┌───────────┐
│ ( • _ • ) │
│  /| 🔎 |\ │
│  / |  | \ │
│   /    \  │
└───┴────┴──┘`}
        </pre>

        {/* Archie Voice & Responsibilities */}
        <div className="text-left space-y-2">
          <div className="font-bold text-sm text-black dark:text-white">
            "{quote}"
          </div>
          <div className="h-[1px] bg-neutral-300 dark:bg-neutral-700 w-full" />
          <ul className="space-y-1 text-xs text-neutral-700 dark:text-neutral-300">
            {bullets.map((bullet, idx) => (
              <li key={idx} className="flex items-center gap-1.5">
                <span className="text-black dark:text-white font-bold">•</span>
                <span>{bullet}</span>
              </li>
            ))}
          </ul>
        </div>
      </div>
    </div>
  );
};
