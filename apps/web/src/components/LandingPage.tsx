import React, { useEffect, useRef, useState } from "react";
import { IconCheck, IconCopy } from "@ui/icons";
import { trackEvent } from "../analytics";
import { SiteHeader, SiteFooter, SCAN_CMD, CURL_CMD } from "./SiteChrome";

/**
 * One machine's receipt, copied from `archie stats --json` and
 * `archie stats usage --period month --json` on the author's laptop on
 * 2026-09-04 (index of 3,981 sessions, 2026-02-15 to 2026-09-03). Nothing
 * here is illustrative. The dollar figure is the API list-price equivalent
 * of the tokens, the same basis `archie stats usage` reports; the account
 * behind most of it is a flat subscription. Verified means the session left
 * a test, build, commit or CI result somebody else can check (rung 3 and up).
 */
const RECEIPT = {
  span: "2026-02-15 to 2026-09-03",
  sessions: "3,981",
  harnesses: "3 of 21",
  tokens: "114.6 B",
  input: "331 M",
  output: "303 M",
  cacheWrites: "2.5 B",
  cacheReads: "111.4 B",
  listPrice: "$48,463",
  verified: "1,938 (48.7%)",
  topModel: "claude-sonnet-5",
};

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
                <span className="setup">You&rsquo;re burning tokens you can&rsquo;t see.</span>
                Archie reads the receipts.
              </h1>

              <p className="lede rise" style={{ ["--i" as string]: 2 }}>
                Which model made the mistake, what it cost, which run to
                trust. The answers are in transcripts nobody reads: Claude
                Code, Codex, Cursor and eighteen others write down everything
                they do. AgentWorth reads those logs and turns every run into
                a receipt &mdash; <b>tokens, cost, blunders, and what was
                actually verified</b>.
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

            <div className="receipt rise" style={{ ["--i" as string]: 3 }}>
              <div className="receipt-head">
                <span>* * * FLIGHT RECEIPT * * *</span>
                <span className="muted">one laptop &middot; {RECEIPT.span}</span>
              </div>
              <div className="receipt-body">
                <div className="row">
                  <span className="k">SESSIONS INDEXED</span>
                  <span className="dots" aria-hidden="true" />
                  <span className="v">{RECEIPT.sessions}</span>
                </div>
                <div className="row">
                  <span className="k">HARNESSES ON THIS MACHINE</span>
                  <span className="dots" aria-hidden="true" />
                  <span className="v">{RECEIPT.harnesses}</span>
                </div>
                <div className="row">
                  <span className="k">TOTAL TOKENS</span>
                  <span className="dots" aria-hidden="true" />
                  <span className="v">{RECEIPT.tokens}</span>
                </div>
                <div className="row sub">
                  <span className="k">├─ input</span>
                  <span className="dots" aria-hidden="true" />
                  <span className="v">{RECEIPT.input}</span>
                </div>
                <div className="row sub">
                  <span className="k">├─ output</span>
                  <span className="dots" aria-hidden="true" />
                  <span className="v">{RECEIPT.output}</span>
                </div>
                <div className="row sub">
                  <span className="k">├─ cache writes</span>
                  <span className="dots" aria-hidden="true" />
                  <span className="v">{RECEIPT.cacheWrites}</span>
                </div>
                <div className="row sub">
                  <span className="k">└─ cache reads</span>
                  <span className="dots" aria-hidden="true" />
                  <span className="v">{RECEIPT.cacheReads}</span>
                </div>
                <div className="row">
                  <span className="k">AT API LIST PRICE</span>
                  <span className="dots" aria-hidden="true" />
                  <span className="v">{RECEIPT.listPrice}</span>
                </div>
                <div className="row">
                  <span className="k">VERIFIED: TEST, COMMIT OR CI</span>
                  <span className="dots" aria-hidden="true" />
                  <span className="v">{RECEIPT.verified}</span>
                </div>
                <div className="row">
                  <span className="k">TOP MODEL</span>
                  <span className="dots" aria-hidden="true" />
                  <span className="v">{RECEIPT.topModel}</span>
                </div>
              </div>
              <p className="receipt-foot">
                [flown] read from local disk, nothing uploaded. Yours prints
                with <code className="mono">archie stats</code>.
              </p>
            </div>
          </div>
        </section>

        <section className="shot wrap">
          <div className="shot-head reveal">
            <h2>Archie reads them.</h2>
            <p>
              <code className="mono">archie scan</code> indexes every agent
              history on the machine. <code className="mono">archie stats</code>{" "}
              prints the receipt: tokens by kind, cost, the models and
              harnesses behind them, and how many runs left proof.
            </p>
          </div>

          <figure className="reveal" style={{ margin: 0 }}>
            <div className="plate">
              <video
                className="term-demo"
                width={1440}
                height={900}
                poster="/stats-1440.webp"
                autoPlay
                muted
                loop
                playsInline
                preload="none"
              >
                <source src="/stats-demo-1440.webm" type="video/webm" />
                <source src="/stats-demo-1440.mp4" type="video/mp4" />
                <img
                  src="/stats-demo-1440.gif"
                  width={1440}
                  height={900}
                  alt="A terminal running archie stats: 3,981 sessions, 114.6 B tokens, an evidence ladder from unflown to CI verified, and the adapters and models behind the totals."
                  loading="lazy"
                  decoding="async"
                />
              </video>
            </div>
            <figcaption>
              The same laptop as the receipt above, on 2026-09-04. Three
              harnesses, seven months, one command.
            </figcaption>
          </figure>

          <figure className="reveal" style={{ margin: 0, marginTop: "clamp(28px, 4vw, 44px)" }}>
            <div className="plate">
              <video
                className="term-demo"
                width={1440}
                height={900}
                poster="/sessions-demo-1440-poster.webp"
                autoPlay
                muted
                loop
                playsInline
                preload="none"
              >
                <source src="/sessions-demo-1440.webm" type="video/webm" />
                <source src="/sessions-demo-1440.mp4" type="video/mp4" />
                <img
                  src="/sessions-demo-1440.gif"
                  width={1440}
                  height={900}
                  alt="A terminal running archie session list --limit 20: twenty recent sessions, each with its evidence ladder, adapter, model, score, duration, and tokens."
                  loading="lazy"
                  decoding="async"
                />
              </video>
            </div>
            <figcaption>
              Twenty of those sessions, one line each. The dots are the
              same ladder, one row per run.
            </figcaption>
          </figure>

          <div className="shot-head reveal" style={{ marginTop: "clamp(48px, 7vw, 88px)" }}>
            <h2>Then open it and look.</h2>
            <p>
              <code className="mono">archie serve</code> runs a local explorer
              on your own index. Every run, what it cost, what it claimed, and
              the evidence behind the claim.
            </p>
          </div>

          <figure className="reveal" style={{ margin: 0 }}>
            <div className="plate">
              <img
                src="/explorer-1440.webp"
                width={1440}
                height={1040}
                alt="The AgentWorth explorer showing one session, its evidence ladder, and the commands behind each rung quoted from the transcript."
                loading="lazy"
                decoding="async"
              />
            </div>
            <figcaption>
              One real session: a 25-minute claude-sonnet-5 run on this repo,
              with the command behind each piece of evidence quoted from its
              transcript.
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
                <code>archie repo blame src/main.rs</code> names the session,
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
