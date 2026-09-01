import React from "react";
import { IconArrowUpRight, IconGithub } from "./icons";
import { ArchieMascot } from "./ArchieMascot";
import { APP_VERSION } from "../version";

export const Footer: React.FC = () => {
  return (
    <footer className="sec">
      <div className="shell">
        <span className="eyebrow">Local means local</span>

        <div className="grid grid-cols-1 lg:grid-cols-12 gap-8 items-start">
          <div className="lg:col-span-7 rounded-xl border border-border bg-surface p-6 sm:p-7 space-y-4">
            <h3 className="text-xl font-bold tracking-tight text-ink">
              Zero telemetry. Zero cloud duplication.
            </h3>

            <p className="body-text">
              AgentWorth never uploads your code, logs, or sessions. It scans dotfiles on
              your machine and stores an index in a local SQLite database at{" "}
              <code className="font-mono text-[0.9em]">~/.agentworth/agentworth.db</code>.
              Raw transcripts remain the source of truth.
            </p>

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 text-[11px] pt-2">
              <div className="p-2.5 rounded-lg border border-border-soft bg-ground text-muted">
                <strong className="text-ink font-semibold">Read-only parsing.</strong> Never
                modifies original log files.
              </div>
              <div className="p-2.5 rounded-lg border border-border-soft bg-ground text-muted">
                <strong className="text-ink font-semibold">Incremental index.</strong> SHA-256
                skip on unchanged JSONL.
              </div>
              <div className="p-2.5 rounded-lg border border-border-soft bg-ground text-muted">
                <strong className="text-ink font-semibold">Bounded memory.</strong> Multi-gigabyte
                logs stream line by line.
              </div>
              <div className="p-2.5 rounded-lg border border-border-soft bg-ground text-muted">
                <strong className="text-ink font-semibold">Zero network.</strong> Runs with
                airplane mode enabled.
              </div>
            </div>

            <div className="flex items-center gap-3 text-xs pt-2 font-mono">
              <a
                href="https://github.com/unfoundbox-crew/agentworth"
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center gap-1 font-semibold text-ink hover:text-accent transition-colors"
              >
                <span>View Rust source</span>
                <IconArrowUpRight size={13} />
              </a>
              <span className="text-faint">&middot;</span>
              <a
                href="https://github.com/unfoundbox-crew/agentworth/blob/main/LICENSE"
                target="_blank"
                rel="noreferrer"
                className="font-semibold text-ink hover:text-accent transition-colors"
              >
                Apache-2.0 license
              </a>
            </div>
          </div>

          <div className="lg:col-span-5 flex justify-center lg:justify-end">
            <ArchieMascot className="w-full" />
          </div>
        </div>

        <div className="footer" style={{ paddingBottom: 0 }}>
          <p>
            <span className="dot" />
            <span>
              AgentWorth &mdash; the verdict layer for AI coding agents &middot; v{APP_VERSION}
            </span>
            <a
              href="https://github.com/unfoundbox-crew/agentworth"
              target="_blank"
              rel="noreferrer"
              className="ml-auto inline-flex items-center gap-1 text-ink hover:text-accent transition-colors"
            >
              <IconGithub size={13} />
              <span>GitHub</span>
            </a>
          </p>
        </div>
      </div>
    </footer>
  );
};
