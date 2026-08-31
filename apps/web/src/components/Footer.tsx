import React from "react";
import { Github, ArrowUpRight } from "lucide-react";
import { ArchieMascot } from "./ArchieMascot";

export const Footer: React.FC = () => {
  return (
    <footer className="border-t-2 border-black dark:border-white bg-[#f8f9fa] dark:bg-[#0a0a0c] text-black dark:text-white font-mono text-xs py-12 transition-colors">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 space-y-10">
        
        {/* Architecture & Archie Mascot Row */}
        <div className="grid grid-cols-1 lg:grid-cols-12 gap-8 items-start">
          
          {/* Left: Zero Telemetry Guarantee */}
          <div className="lg:col-span-7 border-2 border-black dark:border-white bg-white dark:bg-[#121215] p-6 sm:p-7 shadow-[4px_4px_0px_0px_rgba(0,0,0,1)] dark:shadow-[4px_4px_0px_0px_rgba(255,255,255,1)] space-y-4">
            <div className="flex items-center gap-2">
              <span className="text-[10px] uppercase font-bold px-2 py-0.5 border border-black dark:border-white bg-black dark:bg-white text-white dark:text-black">
                LOCAL MEANS LOCAL
              </span>
              <span className="text-xs font-bold uppercase text-neutral-500 dark:text-neutral-400">
                100% OFFLINE & PRIVATE
              </span>
            </div>

            <h3 className="text-xl font-extrabold tracking-tight">
              Zero Telemetry. Zero Cloud Duplication.
            </h3>

            <p className="text-neutral-700 dark:text-neutral-300 text-xs leading-relaxed font-sans">
              AgentWorth never uploads your code, logs, or sessions. It scans dotfiles on your machine and stores an index in a local SQLite database at <code>~/.agentworth/agentworth.db</code>. Raw transcripts remain the source of truth.
            </p>

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 text-[11px] pt-2">
              <div className="p-2 border border-neutral-300 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900">
                <strong>1. Read-Only Parsing:</strong> Never modifies original log files.
              </div>
              <div className="p-2 border border-neutral-300 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900">
                <strong>2. Incremental Index:</strong> SHA-256 skip on unchanged JSONL.
              </div>
              <div className="p-2 border border-neutral-300 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900">
                <strong>3. Bounded Memory:</strong> Multi-gigabyte logs stream line-by-line.
              </div>
              <div className="p-2 border border-neutral-300 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900">
                <strong>4. Zero Network:</strong> Runs with airplane mode enabled.
              </div>
            </div>

            <div className="flex items-center space-x-3 text-xs pt-2">
              <a
                href="https://github.com/unfoundbox-crew/agentworth"
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center space-x-1 font-bold underline hover:text-neutral-600 dark:hover:text-neutral-300"
              >
                <span>View Rust source</span>
                <ArrowUpRight className="w-3.5 h-3.5" />
              </a>
              <span className="text-neutral-400">·</span>
              <a
                href="https://github.com/unfoundbox-crew/agentworth/blob/main/LICENSE"
                target="_blank"
                rel="noreferrer"
                className="font-bold underline hover:text-neutral-600 dark:hover:text-neutral-300"
              >
                Apache-2.0 License
              </a>
            </div>
          </div>

          {/* Right: Archie the Mascot */}
          <div className="lg:col-span-5 flex justify-center lg:justify-end">
            <ArchieMascot className="w-full" />
          </div>
        </div>

        {/* Bottom Bar */}
        <div className="flex flex-col sm:flex-row items-center justify-between gap-4 pt-6 border-t border-neutral-300 dark:border-neutral-800 text-neutral-500 text-[11px]">
          <div className="flex items-center space-x-2">
            <span className="font-bold text-black dark:text-white">AGENTWORTH</span>
            <span>— The verdict layer for AI coding agents</span>
          </div>

          <div className="flex items-center space-x-4">
            <span>v0.1.2 · Native Rust Core</span>
            <a
              href="https://github.com/unfoundbox-crew/agentworth"
              target="_blank"
              rel="noreferrer"
              className="text-black dark:text-white font-semibold hover:underline flex items-center space-x-1"
            >
              <Github className="w-3.5 h-3.5" />
              <span>GitHub</span>
            </a>
          </div>
        </div>

      </div>
    </footer>
  );
};
