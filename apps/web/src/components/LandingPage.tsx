import React, { useEffect, useRef, useState } from "react";
import { IconCheck, IconCopy } from "@ui/icons";
import { trackEvent } from "../analytics";
import { SiteHeader, SiteFooter, SCAN_CMD, CURL_CMD } from "./SiteChrome";
import { Terminal } from "./Terminal";

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
  span: "Feb to Sep 2026",
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

/**
 * Real output of `archie stats --no-json` and
 * `archie session list --limit 20 --no-json` on the author's laptop on
 * 2026-09-04, ANSI stripped. Numbers match RECEIPT above. Colour classes
 * carry the CLI's real semantics: t-accent is rungs 3-5 and VERIFIED
 * (the dashboard's own evidence ladder uses --mv-success for the same
 * state), t-dim is rungs 0-2 and asides, t-bold is headers and the
 * command echoed back.
 */
export const STATS_LINES: React.ReactNode[] = [
  <span className="term-line"><span className="t-bold">{"archie stats"}</span>{"                                       ~/.agentworth/agentworth.db"}</span>,
  <span className="term-line"><span className="t-dim">{"------------------------------------------------------------------------------"}</span></span>,
  <span className="term-line">{"\u00A0"}</span>,
  <span className="term-line">{"  "}<span className="t-bold">{"3,981 sessions"}</span>{"           2026-02-15 -> 2026-09-03           "}<span className="t-bold">{"1,772,808 events"}</span></span>,
  <span className="term-line">{"\u00A0"}</span>,
  <span className="term-line"><span className="t-bold">{"  EVIDENCE LADDER                                           SESSIONS     SHARE"}</span></span>,
  <span className="term-line"><span className="t-dim">{"  ----------------------------------------------------------------------------"}</span></span>,
  <span className="term-line"><span className="t-accent">{"  5  CI or deployment verified ...........................       157      3.9%"}</span></span>,
  <span className="term-line"><span className="t-accent">{"  4  commit observed .....................................     1,526     38.3%"}</span></span>,
  <span className="term-line"><span className="t-accent">{"  3  test or build passed ................................       255      6.4%"}</span></span>,
  <span className="term-line"><span className="t-dim">{"  ---------------------------- the evidence line -----------------------------"}</span></span>,
  <span className="term-line"><span className="t-dim">{"  2  artifact changed ....................................       278      7.0%"}</span></span>,
  <span className="term-line"><span className="t-dim">{"  1  done claimed ........................................         4      0.1%"}</span></span>,
  <span className="term-line"><span className="t-dim">{"  0  unflown .............................................     1,761     44.2%"}</span></span>,
  <span className="term-line"><span className="t-dim">{"  ----------------------------------------------------------------------------"}</span></span>,
  <span className="term-line"><span className="t-accent">{"     VERIFIED    rung 3 and up                                 1,938     48.7%"}</span></span>,
  <span className="term-line">{"\u00A0"}</span>,
  <span className="term-line"><span className="t-bold">{"  TOKENS                                                          114.6B TOTAL"}</span></span>,
  <span className="term-line"><span className="t-dim">{"  ----------------------------------------------------------------------------"}</span></span>,
  <span className="term-line">{"  cache read  "}<span className="t-dim">{"#############################################."}</span>{"    111.4B   97.2%"}</span>,
  <span className="term-line">{"  cache write "}<span className="t-dim">{"#............................................."}</span>{"      2.5B    2.2%"}</span>,
  <span className="term-line">{"  input       "}<span className="t-dim">{".............................................."}</span>{"    331.0M    0.3%"}</span>,
  <span className="term-line">{"  output      "}<span className="t-dim">{".............................................."}</span>{"    303.4M    0.3%"}</span>,
  <span className="term-line">{"\u00A0"}</span>,
  <span className="term-line"><span className="t-bold">{"  ADAPTERS                   MODELS                     TOOLS"}</span></span>,
  <span className="term-line">{"  -------------------------  -------------------------  ----------------------"}</span>,
  <span className="term-line">{"  claude_code    3,251  82%  sonnet-5            1,589  Bash           138,248"}</span>,
  <span className="term-line">{"  codex            659  17%  opus-5                949  Edit            23,715"}</span>,
  <span className="term-line">{"  opencode          71   2%  opus-4-8              338  Read            22,943"}</span>,
  <span className="term-line">{"\u00A0"}</span>,
  <span className="term-line"><span className="t-dim">{"  Next  "}</span><span className="t-bold">{"archie session list --limit 20"}</span>{"   the newest sessions, ladder first"}</span>,
];

export const SESSIONS_LINES: React.ReactNode[] = [
  <span className="term-line"><span className="t-bold">{"archie session list --limit 20"}</span>{"                        3,981 indexed - 20 shown"}</span>,
  <span className="term-line"><span className="t-dim">{"------------------------------------------------------------------------------"}</span></span>,
  <span className="term-line">{"\u00A0"}</span>,
  <span className="term-line"><span className="t-bold">{"  EVIDENCE  SESSION         ADAPTER      MODEL        SCORE      DUR    TOKENS"}</span></span>,
  <span className="term-line"><span className="t-dim">{"  ----------------------------------------------------------------------------"}</span></span>,
  <span className="term-line">{"  "}<span className="t-accent">{"###.."}</span>{"     agent-afe5fa..  claude_code  sonnet-5        78   9m 06s      4.4M"}</span>,
  <span className="term-line">{"  "}<span className="t-accent">{"####."}</span>{"     agent-aafd76..  claude_code  sonnet-5        89  39m 00s     80.7M"}</span>,
  <span className="term-line">{"  "}<span className="t-accent">{"####."}</span>{"     agent-a1e9cb..  claude_code  sonnet-5        89  30m 44s     54.8M"}</span>,
  <span className="term-line">{"  "}<span className="t-dim">{"....."}</span>{"     agent-aa39b8..  claude_code  sonnet-5        33   2m 05s      2.3M"}</span>,
  <span className="term-line">{"  "}<span className="t-accent">{"####."}</span>{"     agent-abceb3..  claude_code  sonnet-5        89  33m 35s     71.3M"}</span>,
  <span className="term-line">{"  "}<span className="t-dim">{"....."}</span>{"     rollout-2026..  codex        5.6-terra       26      11s     27.1K"}</span>,
  <span className="term-line">{"  "}<span className="t-accent">{"####."}</span>{"     agent-afdf29..  claude_code  sonnet-5        89  30m 12s     56.7M"}</span>,
  <span className="term-line">{"  "}<span className="t-accent">{"####."}</span>{"     agent-acd400..  claude_code  sonnet-5        89  32m 47s     69.4M"}</span>,
  <span className="term-line">{"  "}<span className="t-accent">{"####."}</span>{"     agent-ae5351..  claude_code  sonnet-5        89  28m 11s     62.1M"}</span>,
  <span className="term-line">{"  "}<span className="t-accent">{"####."}</span>{"     agent-a73ec5..  claude_code  opus-5          90   1h 10m     57.4M"}</span>,
  <span className="term-line">{"  "}<span className="t-dim">{"....."}</span>{"     agent-a8dadf..  claude_code  opus-5          26  13m 45s     11.9M"}</span>,
  <span className="term-line">{"  "}<span className="t-accent">{"####."}</span>{"     agent-a8ad37..  claude_code  opus-5          89  32m 09s     30.8M"}</span>,
  <span className="term-line">{"  "}<span className="t-accent">{"####."}</span>{"     agent-a7ece9..  claude_code  opus-5          78   4m 30s      4.8M"}</span>,
  <span className="term-line">{"  "}<span className="t-accent">{"####."}</span>{"     agent-a47545..  claude_code  opus-5          78   5m 20s      6.7M"}</span>,
  <span className="term-line">{"  "}<span className="t-accent">{"####."}</span>{"     agent-a41eeb..  claude_code  sonnet-5        90  54m 38s    122.3M"}</span>,
  <span className="term-line">{"  "}<span className="t-accent">{"#####"}</span>{"     agent-a91551..  claude_code  sonnet-5        99  21m 56s     37.6M"}</span>,
  <span className="term-line">{"  "}<span className="t-accent">{"####."}</span>{"     agent-a623e1..  claude_code  opus-5          76   3m 07s      2.6M"}</span>,
  <span className="term-line">{"  "}<span className="t-dim">{"##..."}</span>{"     agent-a26c3c..  claude_code  sonnet-5        50   5m 52s      6.9M"}</span>,
  <span className="term-line">{"  "}<span className="t-accent">{"####."}</span>{"     agent-a1df66..  claude_code  sonnet-5        89  14m 52s     13.9M"}</span>,
  <span className="term-line">{"  "}<span className="t-dim">{"##..."}</span>{"     agent-a1935b..  claude_code  opus-5          50  25m 19s     29.0M"}</span>,
  <span className="term-line"><span className="t-dim">{"  ----------------------------------------------------------------------------"}</span></span>,
  <span className="term-line"><span className="t-dim">{"  15 of these 20 left evidence; the rest are still claims."}</span></span>,
  <span className="term-line"><span className="t-bold">{"  archie session show agent-a91551d9762035f6c"}</span></span>,
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
            <Terminal
              command="archie stats"
              lines={STATS_LINES}
              ariaLabel="A terminal running archie stats: 3,981 sessions, 114.6 B tokens, an evidence ladder from unflown to CI verified, and the adapters and models behind the totals."
            />
            <figcaption>
              The same laptop as the receipt above, on 2026-09-04. Three
              harnesses, seven months, one command.
            </figcaption>
          </figure>

          <figure className="reveal" style={{ margin: 0, marginTop: "clamp(28px, 4vw, 44px)" }}>
            <Terminal
              command="archie session list --limit 20"
              lines={SESSIONS_LINES}
              ariaLabel="A terminal running archie session list --limit 20: twenty recent sessions, each with its evidence ladder, adapter, model, score, duration, and tokens."
            />
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
                srcSet="/explorer-1440.webp 1x, /explorer-2880.webp 2x"
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
