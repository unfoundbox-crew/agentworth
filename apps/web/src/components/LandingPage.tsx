import React, { useRef, useState } from "react";
import { trackEvent } from "../analytics";
import { ThemeToggle } from "@ui/ThemeToggle";
import { VerdictBoard } from "./VerdictBoard";
import { CacheCliffWidget } from "./CacheCliffWidget";
import { VerdictStamp } from "./VerdictStamp";
import { ArchieMascot } from "./ArchieMascot";
import { IconArrowRight, IconCheck, IconCopy, IconGithub, IconShieldCheck } from "@ui/icons";
import { useScrollReveal } from "../hooks/useScrollReveal";
import { APP_VERSION } from "../version";

interface LandingPageProps {
  onOpenExplorer?: () => void;
}

const INVARIANTS = [
  {
    title: "Never upload user data",
    body: "Scanning works completely offline. Raw histories remain on your disk untouched. Zero network telemetry.",
  },
  {
    title: "No raw-log duplication",
    body: "AgentWorth never copies multi-gigabyte transcripts into SQLite. It stores only derived indexes, scores, and fingerprints.",
  },
  {
    title: "Streaming JSONL parsing",
    body: "Bounded-memory parsers handle 100+ GB logs without crashing. Rescans skip unchanged files in milliseconds via SHA-256.",
  },
  {
    title: "Deterministic verification",
    body: "Never trusts self-claimed completion. Scores are backed by compiler exit codes, positive diffs, and git commits.",
  },
];

const COMMANDS = [
  {
    cmd: "$ agentworth stats",
    lines: [
      ["Sessions", "9,713"],
      ["Total tokens", "77.9B"],
      ["Cache read", "97.2%"],
      ["List equivalent", "$33,234"],
    ],
    body: "Full token math across input, output, cache-read, and cache-creation.",
  },
  {
    cmd: "$ agentworth usage --pacing",
    lines: [
      ["Pacing window", "5 hours"],
      ["Burn rate", "1.4M tok/hr"],
      ["Cache hit", "98.1%"],
      ["5h spend", "$4.18"],
    ],
    body: "Real-time burn-rate pacing aligned with Anthropic's 5-hour rate limits.",
  },
  {
    cmd: "$ agentworth blame src/api.ts",
    lines: [
      ["L10-45", "Claude Opus"],
      ["Session", "#89312 (Rung 5)"],
      ["Modified", "2026-08-31"],
      ["Diff", "+45 -12 lines"],
    ],
    body: "Trace any line of source code back to the exact agent session and prompt that wrote it.",
  },
];

export const LandingPage: React.FC<LandingPageProps> = ({ onOpenExplorer }) => {
  const [copied, setCopied] = useState(false);
  const mainRef = useRef<HTMLElement>(null);
  useScrollReveal(mainRef);

  const installCommand = "npx agentworth";

  const handleCopy = (cmd: string) => {
    navigator.clipboard.writeText(cmd);
    setCopied(true);
    trackEvent("npx_command_copied", { command: cmd });
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div>
      <div className="bg-grid" aria-hidden="true" />
      <a className="skip-link" href="#main">
        Skip to content
      </a>

      <header className="topbar">
        <span className="wordmark">
          <span className="dot" />
          AgentWorth
        </span>
        <div className="flex items-center gap-1.5 sm:gap-3">
          {onOpenExplorer && (
            <button
              onClick={onOpenExplorer}
              aria-label="Launch explorer"
              className="flex items-center gap-1.5 px-2.5 sm:px-3 py-1.5 rounded-lg bg-ink text-ground text-xs font-mono font-semibold hover:opacity-85 transition-opacity"
            >
              <span className="hidden sm:inline">Launch explorer</span>
              <IconArrowRight size={13} />
            </button>
          )}
          <a
            href="https://github.com/unfoundbox-crew/agentworth"
            target="_blank"
            rel="noreferrer"
            title="GitHub repository"
            aria-label="GitHub repository"
            className="p-2 rounded-lg text-muted hover:text-ink hover:bg-surface transition-colors"
          >
            <IconGithub size={16} />
          </a>
          <div className="hidden sm:block h-4 w-px bg-border" />
          <ThemeToggle />
        </div>
      </header>

      <main id="main" ref={mainRef}>
        {/* Hero */}
        <section className="hero">
          <div className="shell prose-shell" style={{ maxWidth: 820, paddingInline: 0, marginInline: "auto" }}>
            <span className="eyebrow">The verdict layer for AI coding agents</span>
            <h1 className="thesis" style={{ maxWidth: "18ch" }}>
              Every agent says it&apos;s done. AgentWorth checks the git log.
            </h1>
            <p className="dek" style={{ maxWidth: "60ch" }}>
              AgentWorth reads the session logs already on your disk &mdash; Claude Code,
              Codex, Cursor, Antigravity and <em>20+ CLIs</em> &mdash; and grades every
              session against evidence it can check: files changed, tests passed,
              commits landed, CI green. Native Rust, local SQLite, zero telemetry.
            </p>

            <div className="hero-meta" style={{ marginBottom: 32 }}>
              <span>100% offline</span>
              <span>Local SQLite WAL</span>
              <span>Zero telemetry</span>
              <span>Apache-2.0</span>
            </div>

            <div
              className="flex items-center justify-between gap-3 rounded-xl border border-border bg-surface p-3"
              style={{ maxWidth: 420 }}
            >
              <code className="font-mono text-sm font-semibold text-ink select-all">
                $ {installCommand}
              </code>
              <button
                onClick={() => handleCopy(installCommand)}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-ink text-ground font-mono text-xs font-semibold hover:opacity-85 transition-opacity shrink-0"
              >
                {copied ? <IconCheck size={13} /> : <IconCopy size={13} />}
                <span>{copied ? "Copied" : "Copy"}</span>
              </button>
            </div>

            {/* Hero visual: a real scored session card */}
            <figure className="diagram" style={{ marginTop: 48 }}>
              <div className="diagram-frame">
                <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 pb-4 border-b border-border font-mono">
                  <div className="flex items-center gap-3">
                    <span className="w-2 h-2 rounded-full bg-accent inline-block" aria-hidden="true" />
                    <span className="font-semibold text-xs text-ink">Session audit #98893-claude</span>
                    <span className="text-[10px] px-1.5 py-0.5 rounded border border-border text-muted">
                      claude-opus-5
                    </span>
                  </div>
                  <VerdictStamp status="ci_or_deployment_verified" size="sm" />
                </div>

                <div
                  className="grid grid-cols-2 sm:grid-cols-4 gap-4 py-4 border-b border-border-soft font-mono text-xs"
                  style={{ fontVariantNumeric: "tabular-nums" }}
                >
                  <div>
                    <div className="text-[10px] text-faint uppercase">Outcome rung</div>
                    <div className="font-semibold text-ink mt-0.5">Rung 5 &middot; CI green</div>
                  </div>
                  <div>
                    <div className="text-[10px] text-faint uppercase">Composite score</div>
                    <div className="font-semibold text-ink mt-0.5">94.2 / 100</div>
                  </div>
                  <div>
                    <div className="text-[10px] text-faint uppercase">Token volume</div>
                    <div className="font-semibold text-ink mt-0.5">14.2M (97.4% cache)</div>
                  </div>
                  <div>
                    <div className="text-[10px] text-faint uppercase">List equivalent</div>
                    <div className="font-semibold text-ink mt-0.5">$6.12 USD</div>
                  </div>
                </div>

                <div className="pt-4 font-mono text-xs">
                  <div className="text-faint text-[10px] font-semibold uppercase mb-2">
                    Empirical evidence quoted from disk
                  </div>
                  <div className="p-2.5 rounded-lg border border-border-soft bg-ground text-text text-[11px]">
                    <code>✓ Git commit 4c901e8 observed → CI test-suite exit code 0 on branch main</code>
                  </div>
                </div>
              </div>
              <figcaption>
                A real session, scored against evidence it can check on disk &mdash; not a
                self-report the agent typed into the chat.
              </figcaption>
            </figure>
          </div>
        </section>

        {/* The ladder */}
        <section className="sec" id="ladder">
          <div className="shell">
            <span className="eyebrow">01 &mdash; The verdict ladder</span>
            <h2 className="sec-title">Five rungs, from claim to CI-verified.</h2>
            <p className="lede">
              Every session lands on a rung backed by evidence AgentWorth can check on
              disk &mdash; never on what the agent says it did.
            </p>
            <VerdictBoard />
          </div>
        </section>

        {/* The cache cliff */}
        <section className="sec" id="cache-cliff">
          <div className="shell">
            <span className="eyebrow">02 &mdash; The cache cliff</span>
            <h2 className="sec-title">Long sessions get expensive fast.</h2>
            <p className="lede">
              Cache writes are cheap. Cache misses, past a context-window cliff, are not.
              This is what that curve actually looks like.
            </p>
            <CacheCliffWidget />
          </div>
        </section>

        {/* What you get today */}
        <section className="sec" id="features">
          <div className="shell">
            <span className="eyebrow">03 &mdash; What you get today</span>
            <h2 className="sec-title">Three real CLI commands. No fabricated data.</h2>
            <p className="lede">Shipped in the native Rust binary today.</p>

            <div className="term-grid cols-3">
              {COMMANDS.map((block) => (
                <div key={block.cmd} className="term-card">
                  <div className="font-mono text-[11px] text-muted mb-3">{block.cmd}</div>
                  <div className="space-y-1.5 font-mono text-[11px] mb-4" style={{ fontVariantNumeric: "tabular-nums" }}>
                    {block.lines.map(([k, v]) => (
                      <div key={k} className="flex justify-between">
                        <span className="text-muted">{k}</span>
                        <span className="text-ink font-semibold">{v}</span>
                      </div>
                    ))}
                  </div>
                  <p>{block.body}</p>
                </div>
              ))}
            </div>
          </div>
        </section>

        {/* Local means local */}
        <section className="sec" id="invariants">
          <div className="shell">
            <span className="eyebrow">04 &mdash; AGENTS.md contract</span>
            <h2 className="sec-title">Local means local. Always.</h2>
            <p className="lede">Four invariants AgentWorth&apos;s own contributor contract enforces.</p>

            <div className="term-grid">
              {INVARIANTS.map((inv, i) => (
                <div key={inv.title} className="term-card">
                  <h3 className="flex items-center gap-2">
                    <IconShieldCheck size={16} className="text-accent shrink-0" />
                    <span>
                      {i + 1}. {inv.title}
                    </span>
                  </h3>
                  <p>{inv.body}</p>
                </div>
              ))}
            </div>
          </div>
        </section>

        {/* Roadmap */}
        <section className="sec" id="roadmap">
          <div className="shell prose-shell" style={{ maxWidth: 820, paddingInline: 0, marginInline: "auto" }}>
            <div className="flex items-center justify-between gap-2">
              <span className="eyebrow" style={{ marginBottom: 0 }}>
                05 &mdash; Phase 2 roadmap
              </span>
              <VerdictStamp status="not_built" size="sm" />
            </div>
            <h2 className="sec-title" style={{ marginTop: 14 }}>
              Policy engine: route, explain, run.
            </h2>
            <p className="body-text">
              Transparently marked <strong>not built</strong>. Routing off your own
              repo&apos;s verified build/test/CI history, computed locally on your machine.
            </p>

            <div className="provenance-note">
              <span className="label">Preview &mdash; not shipped</span>
              <p className="font-mono" style={{ fontVariantNumeric: "tabular-nums" }}>
                $ aw route --task &apos;fix race condition in queue&apos;
                <br />
                RECOMMENDED: Claude Sonnet 4.6 (92.4% success on async Rust tasks in this repo)
                <br />
                Estimated cost: $0.18 &middot; Expected turns: 4 &middot; Context cache: warm
              </p>
            </div>
          </div>
        </section>
      </main>

      <footer className="sec">
        <div className="shell flex flex-col sm:flex-row items-center justify-between gap-8">
          <ArchieMascot />
          <div className="footer" style={{ padding: 0, border: 0, flex: 1 }}>
            <p>
              <span className="dot" />
              <span>
                AgentWorth v{APP_VERSION} &middot; Apache-2.0 license &middot; native Rust core
              </span>
              <a
                href="https://github.com/unfoundbox-crew/agentworth"
                target="_blank"
                rel="noreferrer"
                className="ml-auto inline-flex items-center gap-1 text-ink hover:text-accent transition-colors"
              >
                <IconGithub size={12} />
                <span>GitHub</span>
              </a>
            </p>
          </div>
        </div>
      </footer>
    </div>
  );
};
