import React, { useState } from "react";
import { AggregateStats, OutcomeKind } from "../types";
import { CheckCircle2, GitCommit, Terminal, FileCode, MessageSquare } from "lucide-react";

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
    description: "The agent explicitly said “I have completed the task” with no verifiable tool execution.",
    evidenceCriterion: "Assistant message text matches completion heuristics (“All done”, “Fixed”).",
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

  const unverifiedTotal = dist.done_claimed + dist.unresolved;

  return (
    <div className={`panel ${className}`}>
      {/* Panel head */}
      <div className="panel-head">
        <div className="panel-kicker">
          <span className="tag-pill">Evidence ladder</span>
          <span className="status-pill is-neutral">
            <span className="dot" />
            {isMeasured ? `${verifiedCount.toLocaleString()} verified / ${totalSessions.toLocaleString()} total` : "Awaiting scan"}
          </span>
        </div>
        <h2>Deterministic task outcome rungs</h2>
        <p>Every agent says it&apos;s done. AgentWorth verifies the diff, the compiler exit code, and the git log.</p>
      </div>

      {/* The ladder — five ordered rungs, one continuous confidence hierarchy */}
      <div className="ladder" role="group" aria-label="Outcome verification rungs, ordered by confidence">
        {RUNGS.map((rung, i) => {
          const count = isMeasured ? (dist as any)[rung.kind] ?? 0 : undefined;
          const pct = isMeasured && totalSessions > 0 && count !== undefined ? (count / totalSessions) * 100 : undefined;
          const isActive = activeRung === rung.kind;
          const nextIsUnverified = i < RUNGS.length - 1 && !RUNGS[i + 1].isVerified;

          return (
            <React.Fragment key={rung.kind}>
              <button
                type="button"
                data-rung={rung.rung}
                onClick={() => handleRungClick(rung.kind)}
                aria-pressed={isActive}
                className={`ladder-rung ${isActive ? "is-active" : ""}`}
              >
                <span className="ladder-node" aria-hidden="true" />
                <span className="ladder-body">
                  <span className="meta">
                    <rung.icon className="w-3 h-3" />
                    Rung {rung.rung} &middot; {rung.shortLabel}
                  </span>
                  <span className="title">{rung.title}</span>
                  <span className="desc">{rung.description}</span>
                </span>
                <span className="ladder-stat">
                  <span className="count">{count !== undefined ? count.toLocaleString() : "—"}</span>
                  <span className="pct">{pct !== undefined ? `${pct.toFixed(1)}% of total` : "not scanned"}</span>
                </span>
              </button>
              {i < RUNGS.length - 1 && (
                <div className={`ladder-connector ${nextIsUnverified ? "is-cliff" : ""}`} aria-hidden="true" />
              )}
            </React.Fragment>
          );
        })}
      </div>

      {/* Outcome distribution — one segment per rung, same colour logic as the ladder */}
      <div className="mt-7">
        <div className="flex justify-between items-baseline mb-2">
          <span className="eyebrow" style={{ marginBottom: 0 }}>
            Outcome volume distribution
          </span>
          <span className="text-xs font-mono text-faint">
            {isMeasured ? `${totalSessions.toLocaleString()} sessions indexed` : "no verdict calculated yet"}
          </span>
        </div>

        {isMeasured && totalSessions > 0 ? (
          <div className="dist-bar">
            <div
              className="seg bg-success"
              style={{ width: `${(dist.ci_or_deployment_verified / totalSessions) * 100}%` }}
              title={`CI Verified: ${dist.ci_or_deployment_verified}`}
            />
            <div
              className="seg bg-success/80"
              style={{ width: `${(dist.commit_observed / totalSessions) * 100}%` }}
              title={`Committed: ${dist.commit_observed}`}
            />
            <div
              className="seg bg-success/60"
              style={{ width: `${(dist.test_or_build_passed / totalSessions) * 100}%` }}
              title={`Tested: ${dist.test_or_build_passed}`}
            />
            <div
              className="seg bg-success/35"
              style={{ width: `${(dist.artifact_changed / totalSessions) * 100}%` }}
              title={`Artifact Changed: ${dist.artifact_changed}`}
            />
            <div
              className="seg bg-danger"
              style={{ width: `${(unverifiedTotal / totalSessions) * 100}%` }}
              title={`Claim Only / Unresolved: ${unverifiedTotal}`}
            />
          </div>
        ) : (
          <div className="dist-bar items-center justify-center">
            <span className="text-[10px] font-mono text-faint px-3">
              Run `agentworth scan` to calculate an outcome verdict
            </span>
          </div>
        )}

        <div className="flex flex-wrap items-center gap-x-5 gap-y-2 text-xs text-muted mt-3">
          <span className="flex items-center gap-1.5">
            <span className="w-2 h-2 rounded-sm bg-success inline-block" />
            CI Verified (R5)
          </span>
          <span className="flex items-center gap-1.5">
            <span className="w-2 h-2 rounded-sm bg-success/80 inline-block" />
            Committed (R4)
          </span>
          <span className="flex items-center gap-1.5">
            <span className="w-2 h-2 rounded-sm bg-success/60 inline-block" />
            Tested (R3)
          </span>
          <span className="flex items-center gap-1.5">
            <span className="w-2 h-2 rounded-sm bg-success/35 inline-block" />
            Artifact Changed (R2)
          </span>
          <span className="flex items-center gap-1.5 text-danger font-medium">
            <span className="w-2 h-2 rounded-sm bg-danger inline-block" />
            Unverified / Claim (R1/R0)
          </span>
        </div>
      </div>

      {/* Selected rung detail */}
      <div className="mt-7 pt-6 border-t border-border">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 mb-3">
          <div className="flex items-center gap-2">
            <selectedInfo.icon className="w-4 h-4 text-ink" />
            <span className="font-semibold text-sm text-ink">
              Rung {selectedInfo.rung} &mdash; {selectedInfo.title}
            </span>
          </div>
          <span className={`status-pill ${selectedInfo.isVerified ? "is-good" : "is-bad"}`}>
            <span className="dot" />
            {selectedInfo.isVerified ? "Verified outcome" : "Unverified claim"}
          </span>
        </div>

        <p className="text-sm text-text leading-relaxed mb-3">{selectedInfo.description}</p>

        <div className="grid gap-2.5 sm:grid-cols-2">
          <div className="quote-block">
            <span className="quote-label">Promotion criterion</span>
            {selectedInfo.evidenceCriterion}
          </div>
          <div className="quote-block">
            <span className="quote-label">Quoted trace evidence</span>
            &ldquo;{selectedInfo.quotedTraceExample}&rdquo;
          </div>
        </div>
      </div>
    </div>
  );
};
