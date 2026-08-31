import React from "react";
import { OutcomeKind } from "../types";

export type VerdictStatus =
  | OutcomeKind
  | "shipped"
  | "partial"
  | "not_built"
  | "overclaimed"
  | "no_verdict"
  | "not_measured";

interface VerdictStampProps {
  status: VerdictStatus | string;
  size?: "sm" | "md" | "lg";
  rotated?: boolean;
  className?: string;
}

export const VerdictStamp: React.FC<VerdictStampProps> = ({
  status,
  size = "md",
  rotated = false,
  className = "",
}) => {
  const normalized = status.toLowerCase();

  // Ink is truth (Black). Red is doubt (Stamp Red).
  const isInkBlack =
    normalized === "ci_or_deployment_verified" ||
    normalized === "ci verified" ||
    normalized === "commit_observed" ||
    normalized === "committed" ||
    normalized === "test_or_build_passed" ||
    normalized === "tested" ||
    normalized === "artifact_changed" ||
    normalized === "shipped" ||
    normalized === "shipped & true";

  const isPartial = normalized === "partial" || normalized === "rung 2";

  let label = "NO VERDICT";
  let subtitle = "UNRESOLVED";

  switch (normalized) {
    case "ci_or_deployment_verified":
    case "ci verified":
      label = "CI VERIFIED";
      subtitle = "RUNG 5 · REPO GREEN";
      break;
    case "commit_observed":
    case "committed":
      label = "COMMITTED";
      subtitle = "RUNG 4 · GIT LOG";
      break;
    case "test_or_build_passed":
    case "tested":
      label = "TESTED";
      subtitle = "RUNG 3 · EXIT CODE 0";
      break;
    case "artifact_changed":
      label = "ARTIFACT CHANGED";
      subtitle = "RUNG 2 · FILES WRITTEN";
      break;
    case "done_claimed":
    case "claim only":
      label = "CLAIM ONLY";
      subtitle = "RUNG 1 · AGENT SAID DONE";
      break;
    case "shipped":
    case "shipped & true":
      label = "SHIPPED & TRUE";
      subtitle = "VERIFIED IN PROD";
      break;
    case "partial":
      label = "PARTIAL";
      subtitle = "SAY SO HONESTLY";
      break;
    case "not_built":
      label = "NOT BUILT";
      subtitle = "PHASE 2 ROADMAP";
      break;
    case "overclaimed":
      label = "OVERCLAIMED";
      subtitle = "AUDIT FAILED";
      break;
    case "not_measured":
      label = "NOT MEASURED";
      subtitle = "NO METRIC ON FILE";
      break;
    case "unresolved":
    case "no_verdict":
    default:
      label = "NO VERDICT";
      subtitle = "ON FILE";
      break;
  }

  const sizeClasses = {
    sm: "px-2 py-0.5 text-[10px] tracking-wider",
    md: "px-3 py-1.5 text-xs tracking-widest",
    lg: "px-4 py-2 text-sm tracking-widest font-extrabold",
  };

  const rotationClass = rotated ? "-rotate-2" : "";

  if (isInkBlack) {
    return (
      <span
        className={`inline-flex flex-col items-center justify-center font-mono border-2 border-black dark:border-white bg-white dark:bg-black text-black dark:text-white uppercase select-none ${sizeClasses[size]} ${rotationClass} shadow-[2px_2px_0px_0px_rgba(0,0,0,1)] dark:shadow-[2px_2px_0px_0px_rgba(255,255,255,1)] ${className}`}
      >
        <span className="font-extrabold leading-tight">{label}</span>
        {size !== "sm" && (
          <span className="text-[9px] font-medium tracking-normal text-neutral-600 dark:text-neutral-400 mt-0.5">
            {subtitle}
          </span>
        )}
      </span>
    );
  }

  if (isPartial) {
    return (
      <span
        className={`inline-flex flex-col items-center justify-center font-mono border-2 border-amber-600 dark:border-amber-500 bg-amber-50 dark:bg-amber-950/40 text-amber-900 dark:text-amber-300 uppercase select-none ${sizeClasses[size]} ${rotationClass} shadow-[2px_2px_0px_0px_rgba(217,119,6,0.8)] ${className}`}
      >
        <span className="font-extrabold leading-tight">{label}</span>
        {size !== "sm" && (
          <span className="text-[9px] font-medium tracking-normal text-amber-700 dark:text-amber-400 mt-0.5">
            {subtitle}
          </span>
        )}
      </span>
    );
  }

  // Red is doubt: unverified, unmeasured, not built
  return (
    <span
      className={`inline-flex flex-col items-center justify-center font-mono border-2 border-dashed border-red-600 dark:border-red-500 bg-red-50/70 dark:bg-red-950/40 text-red-700 dark:text-red-400 uppercase select-none ${sizeClasses[size]} ${rotationClass} shadow-[2px_2px_0px_0px_rgba(220,38,38,0.8)] ${className}`}
    >
      <span className="font-extrabold leading-tight">{label}</span>
      {size !== "sm" && (
        <span className="text-[9px] font-medium tracking-normal text-red-600/80 dark:text-red-400/80 mt-0.5">
          {subtitle}
        </span>
      )}
    </span>
  );
};
