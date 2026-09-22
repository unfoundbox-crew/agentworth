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

/**
 * The verdict stamp: a semantic pill (never the brand accent — design.md's
 * "semantic colour is separate from the accent" rule). Solid border = a
 * checkable fact (verified rungs 2-5). Dashed border = unverified,
 * unmeasured, or not built — reusing the same solid/dashed convention the
 * system's diagrams use for built vs. proposed.
 */
export const VerdictStamp: React.FC<VerdictStampProps> = ({
  status,
  size = "md",
  rotated = false,
  className = "",
}) => {
  const normalized = status.toLowerCase();

  const isGood =
    normalized === "ci_or_deployment_verified" ||
    normalized === "ci verified" ||
    normalized === "commit_observed" ||
    normalized === "committed" ||
    normalized === "test_or_build_passed" ||
    normalized === "tested" ||
    normalized === "artifact_changed" ||
    normalized === "shipped" ||
    normalized === "shipped & true";

  const isWarn = normalized === "partial" || normalized === "rung 2";

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
    sm: "px-2.5 py-1 text-[10px] gap-0.5",
    md: "px-3.5 py-1.5 text-xs gap-0.5",
    lg: "px-4 py-2 text-sm gap-1",
  };

  const rotationClass = rotated ? "-rotate-2" : "";

  const toneClass = isGood
    ? "border-success-border bg-success-soft text-success"
    : isWarn
    ? "border-warn-border bg-warn-soft text-warn"
    : "border-dashed border-border bg-surface-2 text-muted";

  // Neither good nor warn means no evidence reached us, not that something
  // failed — the same distinction the outcome dots and ladder now make.
  const subtitleToneClass = isGood ? "text-success/80" : isWarn ? "text-warn/80" : "text-faint";

  return (
    <span
      className={`inline-flex flex-col items-center justify-center font-mono border rounded-lg uppercase select-none tracking-wider ${toneClass} ${sizeClasses[size]} ${rotationClass} ${className}`}
    >
      <span className="font-bold leading-tight">{label}</span>
      {size !== "sm" && (
        <span className={`text-[9px] font-medium tracking-normal ${subtitleToneClass}`}>{subtitle}</span>
      )}
    </span>
  );
};
