import React, { useState } from "react";
import { AggregateStats, OutcomeKind } from "../types";
import { VerdictStamp } from "./VerdictStamp";
import { CheckCircle2, GitCommit, Terminal, FileCode, MessageSquare, AlertTriangle } from "lucide-react";

interface VerdictBoardProps {
  stats?: AggregateStats;
  selectedRung?: OutcomeKind | null;
  onSelectRung?: (rung: OutcomeKind | null) => void;
  className?: string;
}

interface RungInfo {
  rung: number;
  kind: OutcomeKind;
  title: string;
  shortLabel: string;
  icon: React.ComponentType<{ className?: string }>;
  description: string;
  evidenceCriterion: string;
  quotedTraceExample: string;
  isVerified: boolean;
}

const RUNGS: RungInfo[] = [
  {
    rung: 5,
    kind: "ci_or_deployment_verified",
    title: "CI / Deployment Verified",
    shortLabel: "CI VERIFIED",
    icon: CheckCircle2,
    description: "Continuous integration status checks passed and PR/deployment succeeded.",
    evidenceCriterion: "GitHub Actions / CI webhook green on the resulting git SHA.",
    quotedTraceExample: "CI check run test-suite on commit 8f2a1b completed with status SUCCESS (0 failures).",
    isVerified: true,
  },
  {
    rung: 4,
    kind: "commit_observed",
    title: "Commit Observed",
    shortLabel: "COMMITTED",
    icon: GitCommit,
    description: "A real Git commit was authored and committed to repository history.",
    evidenceCriterion: "git commit command executed with exit code 0 or commit ref created.",
    quotedTraceExample: "Tool invocation Bash executed git commit -m fix: resolve lockup in worker -> [main 4c901e8].",
    isVerified: true,
  },
  {
    rung: 3,
    kind: "test_or_build_passed",
    title: "Test / Build Passed",
    shortLabel: "TESTED",
    icon: Terminal,
    description: "Compiler, build tool, or test runner executed and exited with code 0.",
    evidenceCriterion: "cargo test, pytest, vitest, or go test with exit_code: 0.",
    quotedTraceExample: "Shell command cargo test --workspace returned exit code 0 (42 passed; 0 failed).",
    isVerified: true,
  },
  {
    rung: 2,
    kind: "artifact_changed",
    title: "Artifact Changed",
    shortLabel: "ARTIFACTS",
    icon: FileCode,
    description: "Source code files were modified, created, or patched on the local filesystem.",
    evidenceCriterion: "File edit tool (Edit, replace_file_content, StrReplace) executed with positive diff.",
    quotedTraceExample: "Tool invocation Edit modified src/services/api.ts (+45 lines, -12 lines).",
    isVerified: true,
  },
  {
    rung: 1,
    kind: "done_claimed",
    title: "Claim Only",
    shortLabel: "CLAIM ONLY",
    icon: MessageSquare,
    description: "The agent explicitly said I have completed the task with no verifiable tool execution.",
    evidenceCriterion: "Assistant message text matches completion heuristics (All done, Fixed).",
    quotedTraceExample: "Assistant: I have finished writing the code. Everything should work as expected. (No build executed).",
    isVerified: false,
  },
];

export const VerdictBoard: React.FC<VerdictBoardProps> = ({
  stats,
  selectedRung: controlledRung,
  onSelectRung,
  className = "",
}) => {
  const [internalSelectedRung, setInternalSelectedRung] = useState<OutcomeKind | null>("ci_or_deployment_verified");

  const activeRung = controlledRung !== undefined ? controlledRung : internalSelectedRung;

  const handleRungClick = (kind: OutcomeKind) => {
    const next = activeRung === kind ? null : kind;
    if (onSelectRung) {
      onSelectRung(next);
    } else {
      setInternalSelectedRung(next);
    }
  };

  const dist = stats?.outcome_distribution || {
    ci_or_deployment_verified: 0,
    commit_observed: 0,
    test_or_build_passed: 0,
    artifact_changed: 0,
    done_claimed: 0,
    unresolved: 0,
  };

  const totalSessions = stats?.total_sessions || 0;
  const verifiedCount = stats?.verified_outcomes_count || 0;
  const isMeasured = totalSessions > 0 && (verifiedCount > 0 || Object.values(dist).some((v) => v > 0));

  const selectedInfo = RUNGS.find((r) => r.kind === activeRung) || RUNGS[0];

  return (
    <div
      className={`border-2 border-black dark:border-white bg-white dark:bg-[#121215] text-black dark:text-white p-6 sm:p-7 font-mono shadow-[6px_6px_0px_0px_rgba(0,0,0,1)] dark:shadow-[6px_6px_0px_0px_rgba(255,255,255,1)] ${className}`}
    >
      {/* Verdict Board Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 pb-5 border-b-2 border-dashed border-neutral-300 dark:border-neutral-700">
        <div>
          <div className="flex items-center gap-2 mb-1">
            <span className="text-xs font-bold uppercase tracking-widest text-neutral-500 dark:text-neutral-400">
              § THE VERDICT BOARD
            </span>
            <span className="text-[10px] px-2 py-0.5 border border-black dark:border-white font-bold bg-neutral-100 dark:bg-neutral-900 text-black dark:text-white">
              EVIDENCE LADDER
            </span>
          </div>
          <h2 className="text-xl sm:text-2xl font-extrabold tracking-tight">
            Deterministic Task Outcome Rungs
          </h2>
          <p className="text-xs text-neutral-600 dark:text-neutral-400 mt-1 font-sans">
            Every agent says it is done. AgentWorth verifies the diff, compiler exit code, and git log.
          </p>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          <VerdictStamp
            status={isMeasured ? "ci_or_deployment_verified" : "not_measured"}
            size="md"
            rotated={!isMeasured}
          />
        </div>
      </div>

      {/* 4 Main Cards: CI VERIFIED · COMMITTED · TESTED · CLAIM ONLY */}
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-3.5 my-6">
        {/* 1. CI VERIFIED */}
        <button
          onClick={() => handleRungClick("ci_or_deployment_verified")}
          className={`p-4 text-left border-2 transition-all ${
            activeRung === "ci_or_deployment_verified"
              ? "border-black dark:border-white bg-neutral-50 dark:bg-neutral-900 shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] dark:shadow-[4px_4px_0px_0px_rgba(255,255,255,1)] translate-x-[-1px] translate-y-[-1px]"
              : "border-neutral-300 dark:border-neutral-800 hover:border-black dark:hover:border-white bg-white dark:bg-[#151518]"
          }`}
        >
          <div className="flex items-center justify-between text-xs text-neutral-500 dark:text-neutral-400 mb-2">
            <span className="font-bold">RUNG 5</span>
            <CheckCircle2 className="w-4 h-4 text-black dark:text-white" />
          </div>
          <div className="text-sm font-extrabold tracking-tight text-black dark:text-white mb-1">
            CI VERIFIED
          </div>
          <div className="text-2xl font-black text-black dark:text-white">
            {isMeasured ? dist.ci_or_deployment_verified.toLocaleString() : "—"}
          </div>
          <div className="text-[10px] text-neutral-500 dark:text-neutral-400 mt-1">
            {isMeasured && totalSessions > 0
              ? `${((dist.ci_or_deployment_verified / totalSessions) * 100).toFixed(1)}% of total`
              : "CI green on SHA"}
          </div>
        </button>

        {/* 2. COMMITTED */}
        <button
          onClick={() => handleRungClick("commit_observed")}
          className={`p-4 text-left border-2 transition-all ${
            activeRung === "commit_observed"
              ? "border-black dark:border-white bg-neutral-50 dark:bg-neutral-900 shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] dark:shadow-[4px_4px_0px_0px_rgba(255,255,255,1)] translate-x-[-1px] translate-y-[-1px]"
              : "border-neutral-300 dark:border-neutral-800 hover:border-black dark:hover:border-white bg-white dark:bg-[#151518]"
          }`}
        >
          <div className="flex items-center justify-between text-xs text-neutral-500 dark:text-neutral-400 mb-2">
            <span className="font-bold">RUNG 4</span>
            <GitCommit className="w-4 h-4 text-black dark:text-white" />
          </div>
          <div className="text-sm font-extrabold tracking-tight text-black dark:text-white mb-1">
            COMMITTED
          </div>
          <div className="text-2xl font-black text-black dark:text-white">
            {isMeasured ? dist.commit_observed.toLocaleString() : "—"}
          </div>
          <div className="text-[10px] text-neutral-500 dark:text-neutral-400 mt-1">
            {isMeasured && totalSessions > 0
              ? `${((dist.commit_observed / totalSessions) * 100).toFixed(1)}% of total`
              : "Git commit authored"}
          </div>
        </button>

        {/* 3. TESTED */}
        <button
          onClick={() => handleRungClick("test_or_build_passed")}
          className={`p-4 text-left border-2 transition-all ${
            activeRung === "test_or_build_passed"
              ? "border-black dark:border-white bg-neutral-50 dark:bg-neutral-900 shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] dark:shadow-[4px_4px_0px_0px_rgba(255,255,255,1)] translate-x-[-1px] translate-y-[-1px]"
              : "border-neutral-300 dark:border-neutral-800 hover:border-black dark:hover:border-white bg-white dark:bg-[#151518]"
          }`}
        >
          <div className="flex items-center justify-between text-xs text-neutral-500 dark:text-neutral-400 mb-2">
            <span className="font-bold">RUNG 3</span>
            <Terminal className="w-4 h-4 text-black dark:text-white" />
          </div>
          <div className="text-sm font-extrabold tracking-tight text-black dark:text-white mb-1">
            TESTED
          </div>
          <div className="text-2xl font-black text-black dark:text-white">
            {isMeasured ? dist.test_or_build_passed.toLocaleString() : "—"}
          </div>
          <div className="text-[10px] text-neutral-500 dark:text-neutral-400 mt-1">
            {isMeasured && totalSessions > 0
              ? `${((dist.test_or_build_passed / totalSessions) * 100).toFixed(1)}% of total`
              : "Runner exit code 0"}
          </div>
        </button>

        {/* 4. CLAIM ONLY (Stamp Red) */}
        <button
          onClick={() => handleRungClick("done_claimed")}
          className={`p-4 text-left border-2 transition-all ${
            activeRung === "done_claimed"
              ? "border-red-600 dark:border-red-500 bg-red-50/40 dark:bg-red-950/30 shadow-[4px_4px_0px_0px_rgba(220,38,38,0.8)] translate-x-[-1px] translate-y-[-1px]"
              : "border-neutral-300 dark:border-neutral-800 hover:border-red-600 dark:hover:border-red-500 bg-white dark:bg-[#151518]"
          }`}
        >
          <div className="flex items-center justify-between text-xs text-red-600 dark:text-red-400 mb-2">
            <span className="font-bold">RUNG 1</span>
            <AlertTriangle className="w-4 h-4 text-red-600 dark:text-red-400" />
          </div>
          <div className="text-sm font-extrabold tracking-tight text-red-700 dark:text-red-400 mb-1">
            CLAIM ONLY
          </div>
          <div className="text-2xl font-black text-red-700 dark:text-red-400">
            {isMeasured ? dist.done_claimed.toLocaleString() : "—"}
          </div>
          <div className="text-[10px] text-red-600/80 dark:text-red-400/80 mt-1">
            {isMeasured && totalSessions > 0
              ? `${((dist.done_claimed / totalSessions) * 100).toFixed(1)}% unverified`
              : "Unbacked agent claim"}
          </div>
        </button>
      </div>

      {/* Outcome Distribution Stacked Bar */}
      <div className="mb-6 p-4 border border-neutral-300 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900/50">
        <div className="flex justify-between text-xs font-bold mb-2">
          <span>OUTCOME VOLUME DISTRIBUTION</span>
          <span className="text-neutral-500">
            {isMeasured
              ? `${verifiedCount.toLocaleString()} Verified / ${totalSessions.toLocaleString()} Total`
              : "AWAITING INDEX CALCULATION"}
          </span>
        </div>

        {/* Stacked bar segments */}
        <div className="h-4 w-full bg-neutral-200 dark:bg-neutral-800 flex overflow-hidden border border-black dark:border-neutral-700">
          {isMeasured && totalSessions > 0 ? (
            <>
              <div
                style={{ width: `${(dist.ci_or_deployment_verified / totalSessions) * 100}%` }}
                className="bg-black dark:bg-white h-full"
                title={`CI Verified: ${dist.ci_or_deployment_verified}`}
              />
              <div
                style={{ width: `${(dist.commit_observed / totalSessions) * 100}%` }}
                className="bg-neutral-700 dark:bg-neutral-300 h-full"
                title={`Committed: ${dist.commit_observed}`}
              />
              <div
                style={{ width: `${(dist.test_or_build_passed / totalSessions) * 100}%` }}
                className="bg-neutral-500 dark:bg-neutral-500 h-full"
                title={`Tested: ${dist.test_or_build_passed}`}
              />
              <div
                style={{ width: `${(dist.artifact_changed / totalSessions) * 100}%` }}
                className="bg-neutral-400 dark:bg-neutral-600 h-full"
                title={`Artifact Changed: ${dist.artifact_changed}`}
              />
              <div
                style={{ width: `${((dist.done_claimed + dist.unresolved) / totalSessions) * 100}%` }}
                className="bg-red-600 h-full"
                title={`Claim Only / Unresolved: ${dist.done_claimed + dist.unresolved}`}
              />
            </>
          ) : (
            <div className="w-full h-full flex items-center justify-center text-[9px] text-neutral-500 font-bold tracking-wider">
              — NO OUTCOME VERDICT CALCULATED YET (RUN AGENTWORTH SCAN) —
            </div>
          )}
        </div>

        {/* Legend */}
        <div className="flex flex-wrap items-center gap-4 text-[11px] text-neutral-600 dark:text-neutral-400 mt-2.5">
          <div className="flex items-center gap-1.5">
            <span className="w-2.5 h-2.5 bg-black dark:bg-white inline-block border border-black dark:border-white" />
            <span>CI Verified (R5)</span>
          </div>
          <div className="flex items-center gap-1.5">
            <span className="w-2.5 h-2.5 bg-neutral-700 dark:bg-neutral-300 inline-block" />
            <span>Committed (R4)</span>
          </div>
          <div className="flex items-center gap-1.5">
            <span className="w-2.5 h-2.5 bg-neutral-500 dark:bg-neutral-500 inline-block" />
            <span>Tested (R3)</span>
          </div>
          <div className="flex items-center gap-1.5">
            <span className="w-2.5 h-2.5 bg-neutral-400 dark:bg-neutral-600 inline-block" />
            <span>Artifact Changed (R2)</span>
          </div>
          <div className="flex items-center gap-1.5">
            <span className="w-2.5 h-2.5 bg-red-600 inline-block" />
            <span className="text-red-700 dark:text-red-400 font-bold">Unverified / Claim (R1/0)</span>
          </div>
        </div>
      </div>

      {/* Selected Rung Inspector Details Box */}
      <div className="p-4 sm:p-5 border-2 border-black dark:border-neutral-700 bg-white dark:bg-[#17171c]">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 mb-3">
          <div className="flex items-center gap-2">
            <selectedInfo.icon className="w-4 h-4 text-black dark:text-white" />
            <span className="font-bold text-sm text-black dark:text-white uppercase">
              Rung {selectedInfo.rung} — {selectedInfo.title}
            </span>
          </div>
          <span className={`text-[10px] px-2 py-0.5 font-bold uppercase ${
            selectedInfo.isVerified
              ? "bg-black dark:bg-white text-white dark:text-black"
              : "bg-red-600 text-white"
          }`}>
            {selectedInfo.isVerified ? "VERIFIED OUTCOME" : "UNVERIFIED CLAIM"}
          </span>
        </div>

        <p className="text-xs text-neutral-700 dark:text-neutral-300 font-sans leading-relaxed mb-3">
          {selectedInfo.description}
        </p>

        <div className="space-y-2 text-xs">
          <div className="p-2.5 bg-neutral-100 dark:bg-neutral-900 border border-neutral-300 dark:border-neutral-800">
            <span className="text-neutral-500 font-bold uppercase text-[10px] block mb-1">
              Promotion Criterion:
            </span>
            <code className="text-black dark:text-white select-all text-[11px]">
              {selectedInfo.evidenceCriterion}
            </code>
          </div>

          <div className="p-2.5 bg-neutral-100 dark:bg-neutral-900 border border-neutral-300 dark:border-neutral-800">
            <span className="text-neutral-500 font-bold uppercase text-[10px] block mb-1">
              Quoted Trace Evidence:
            </span>
            <code className="text-neutral-800 dark:text-neutral-200 select-all text-[11px]">
              &quot;{selectedInfo.quotedTraceExample}&quot;
            </code>
          </div>
        </div>
      </div>
    </div>
  );
};
