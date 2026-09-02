import React, { useEffect, useRef, useState } from "react";
import { IconCheck, IconCopy } from "@ui/icons";
import { trackEvent } from "../analytics";
import { SiteHeader, SiteFooter, SCAN_CMD, CURL_CMD } from "./SiteChrome";

/**
 * The outcome distribution from one real machine.
 *
 * Source: `agentworth stats` on the author's laptop, 2026-09-02, over an
 * index of 2,960 sessions. Every figure here is copied from that output —
 * nothing on this page is illustrative. All six rows together are the whole
 * index: 7 + 449 + 808 + 120 + 86 + 1,490 = 2,960, and the percentages sum
 * to 100.0. Keep it that way — a column a reader can add up is the point.
 */
const RUNGS = [
  { label: "Nothing verified, or still running", n: 1490, pct: 50.3, evidence: false },
  { label: "The agent said it was done", n: 7, pct: 0.2, evidence: false },
  { label: "Some files on disk changed", n: 449, pct: 15.2, evidence: false },
  { label: "A test or a build passed", n: 808, pct: 27.3, evidence: true },
  { label: "A commit landed in git", n: 120, pct: 4.1, evidence: true },
  { label: "CI or a deploy went green", n: 86, pct: 2.9, evidence: true },
];

const HARNESSES =
  "Claude Code · Codex · Cursor · Antigravity · Gemini · Goose · Aider · " +
  "Cline & Roo-Code · Windsurf · OpenCode · Grok · Kimi · Qwen · DeepSeek · " +
  "MiniMax · Zhipu · Manus · Hermes · OpenClaw · Herdr · Pi";

function CopyBlock({ cmd, id }: { cmd: string; id: string }) {
  const [copied, setCopied] = useState(false);

  const copy = () => {
    navigator.clipboard?.writeText(cmd).catch(() => undefined);
    setCopied(true);
    trackEvent("install_command_copied", { command: cmd, position: id });
    setTimeout(() => setCopied(false), 1800);
  };

  return (
    <div className="install">
      <code>
        <span className="sigil" aria-hidden="true">
          $
        </span>
        {cmd}
      </code>
      <button type="button" onClick={copy} aria-label={`Copy: ${cmd}`}>
        {copied ? <IconCheck size={13} /> : <IconCopy size={13} />}
        <span>{copied ? "Copied" : "Copy"}</span>
      </button>
    </div>
  );
}

/** Adds `.in` to every `.reveal` as it enters the viewport, once. */
function useReveal() {
  const root = useRef<HTMLElement>(null);

  useEffect(() => {
    const nodes = root.current?.querySelectorAll<HTMLElement>(".reveal");
    if (!nodes?.length) return;
    if (!("IntersectionObserver" in window)) {
      nodes.forEach((n) => n.classList.add("in"));
      return;
    }
    const io = new IntersectionObserver(
      (entries) => {
        entries.forEach((e) => {
          if (e.isIntersecting) {
            e.target.classList.add("in");
            io.unobserve(e.target);
          }
        });
      },
      { rootMargin: "0px 0px -12% 0px", threshold: 0.05 }
    );
    nodes.forEach((n) => io.observe(n));
    return () => io.disconnect();
  }, []);

  return root;
}

export const LandingPage: React.FC = () => {
  const main = useReveal();

  const verifiedPct = 34.3;
  const indexed = 2960;

  return (
    <>
      <a className="skip" href="#main">
        Skip to content
      </a>

      <SiteHeader />

      <main id="main" ref={main}>
        <section className="hero">
          <div className="wrap hero-grid">
            <div>
              <p className="kicker rise" style={{ ["--i" as string]: 0 }}>
                Runs on your machine. Sends nothing anywhere.
              </p>

              <h1 className="rise" style={{ ["--i" as string]: 1 }}>
                <span className="setup">Every agent says it&rsquo;s done.</span>
                AgentWorth checks the git log.
              </h1>

              <p className="lede rise" style={{ ["--i" as string]: 2 }}>
                Claude Code, Codex, Cursor and eighteen others already write
                down everything they do, in dot-directories you have never
                opened. AgentWorth reads those logs and checks each claim
                against what actually happened &mdash;{" "}
                <b>files changed, tests run, commits made, CI green</b>.
              </p>

              <div className="rise" style={{ ["--i" as string]: 3 }}>
                <CopyBlock cmd={SCAN_CMD} id="hero" />
                <p className="alt-install">
                  <span className="sep">or</span>{" "}
                  <code>{CURL_CMD}</code>
                </p>
              </div>

              <p className="trust rise" style={{ ["--i" as string]: 4 }}>
                <b>It never phones home.</b> No account, no server, no upload
                &mdash; it reads files that are already on your disk and writes
                an index next to them.
              </p>
            </div>

            <div className="ladder rise" style={{ ["--i" as string]: 3 }}>
              <div className="ladder-head">
                <h2>How far did it actually get?</h2>
                <p>
                  {indexed.toLocaleString()} sessions on one laptop, graded
                </p>
              </div>

              {RUNGS.map((r, i) => (
                <React.Fragment key={r.label}>
                  {i === 3 && (
                    <div className="ladder-rule">Evidence starts here</div>
                  )}
                  <div className={`rung${r.evidence ? " evidence" : ""}`}>
                    <span
                      className="bar"
                      style={{ ["--w" as string]: `${r.pct * 1.24}%` }}
                      aria-hidden="true"
                    />
                    <span className="label">{r.label}</span>
                    <span className="n">
                      {r.n.toLocaleString()} <em>{r.pct}%</em>
                    </span>
                  </div>
                </React.Fragment>
              ))}

              <p className="ladder-foot">
                Half of them never got far enough to tell, and the two rows
                above the line are things an agent can say without doing much.
                Only the last three left a trace someone else can check.{" "}
                <b>{verifiedPct}% of all {indexed.toLocaleString()} cleared
                that line.</b>
              </p>
            </div>
          </div>
        </section>

        <section className="shot wrap">
          <div className="shot-head reveal">
            <h2>Then open it and look.</h2>
            <p>
              <code className="mono">agentworth serve</code> runs a local
              explorer on your own index. Every session, what it claimed, and
              the evidence behind the claim.
            </p>
          </div>

          <figure className="reveal" style={{ margin: 0 }}>
            <div className="plate">
              <img
                src="/explorer-1440.webp"
                width={1440}
                height={560}
                alt="The AgentWorth explorer showing one session scored 89, with its outcome ladder: CI or deployment green, commit observed in git log, test or build passed, artifact changed on disk, and done claimed by the agent."
                loading="lazy"
                decoding="async"
              />
            </div>
            <figcaption>
              A real session from the index above &mdash; a 56-minute
              claude-sonnet-5 run that scored 89, with the command behind each
              rung quoted from the transcript.
            </figcaption>
          </figure>
        </section>

        <section className="wrap">
          <div className="facts">
            <div className="fact reveal">
              <h3>21 harnesses</h3>
              <p>
                One index across all of them, so the answer to{" "}
                <b>&ldquo;what has been running on this machine?&rdquo;</b> is
                a single command.
              </p>
              <p className="harnesses">{HARNESSES}</p>
            </div>

            <div className="fact reveal">
              <h3>Blame, for agents</h3>
              <p>
                <code>agentworth blame src/main.rs</code> names the session,
                the model and the prompt behind a line of code &mdash; months
                after whoever ran it forgot.
              </p>
            </div>

            <div className="fact reveal">
              <h3>Where the tokens went</h3>
              <p>
                Input, output, cache reads and cache writes, rolled up by day,
                week or month. Your own agent can ask too:{" "}
                <code>claude mcp add agentworth -- agentworth mcp</code>.
              </p>
            </div>
          </div>
        </section>

        <section className="close wrap">
          <h2>Point it at your own machine.</h2>
          <CopyBlock cmd={SCAN_CMD} id="close" />
          <p className="alt-install">
            <span className="sep">or</span> <code>{CURL_CMD}</code>
          </p>
        </section>
      </main>

      <SiteFooter />
    </>
  );
};
