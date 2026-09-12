import React from "react";
import { SiteHeader, SiteFooter, SCAN_CMD } from "./SiteChrome";
import {
  Archie,
  ARCHIE_ACCESSORIES,
  ARCHIE_COLOURWAYS,
  ARCHIE_DEFAULT_ACCESSORY,
  ARCHIE_DEFAULT_COLOURWAY,
  type ArchieAccessory,
  type ArchieColourway,
  type ArchiePose,
} from "@ui/Archie";

interface Verb {
  pose: ArchiePose;
  title: string;
  body: string;
  cli: string;
  mcp: string;
}

const VERBS: Verb[] = [
  {
    pose: "digging",
    title: "Digs up what you forgot",
    body: "Decision-shaped sentences your own session dropped when it compacted. Quoted verbatim, newest first, each one carrying what happened next — so a decision that was acted on reads differently from one that was only stated.",
    cli: "archie session forgotten",
    mcp: "forgotten_context",
  },
  {
    pose: "fetching",
    title: "Fetches the last handoff",
    body: "The last few handoffs in this repo, so a fresh session's first tool call can be the catch-up instead of twenty minutes of re-reading. Every worktree answers to one repo key.",
    cli: "archie session handoff",
    mcp: "carry_forward",
  },
  {
    pose: "dropping",
    title: "Drops the open questions at your feet",
    body: "Every question the session asked, with a status: answered, flagged back to you, or still hanging. He puts it down in front of you and looks up.",
    cli: "archie session asks",
    mcp: "session_asks",
  },
];

const COLOURWAY_NAMES: Record<ArchieColourway, string> = {
  C1: "Mono",
  C2: "Dark",
  C3: "AgentWorth",
  C4: "Quiet",
};

/** Exactly the block the CLI prints, so the page cannot claim a shape the binary
 *  does not draw. Kept in sync by hand with packages/ui/brand/archie/archie-tui.txt. */
const TERMINAL_BLOCK = `┌─ archie ────────────────────────────────────────────────────────────────────────────── digging ──┐
│  ,---.   ~/.claude/projects            ──────────────────────·······  68%                        │
│ ( o o )  1,204 sessions · 38 repos     last dig 2m ago                                           │
│-*  '._.' ● 902 verified  ○ 14 off      3 asks still unanswered                                   │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘

 (*) archie  scanning  ──────────────────·······  68%  1,204 sessions`;

/**
 * The kit switch. Both controls do exactly what the assets are built for: the
 * accessory is `data-accessory` on the SVG, the colourway is an `.archie-cN` class on
 * an ancestor. Neither is a second drawing, and neither needs a network round trip.
 */
const KitSwitch: React.FC<{
  accessory: ArchieAccessory;
  colourway: ArchieColourway;
  onAccessory: (a: ArchieAccessory) => void;
  onColourway: (c: ArchieColourway) => void;
}> = ({ accessory, colourway, onAccessory, onColourway }) => (
  <div className="ar-try">
    <div className="ar-try-stage">
      <Archie
        pose="three-quarter"
        size={120}
        accessory={accessory}
        colourway={colourway}
        label={`Archie, ${colourway} ${COLOURWAY_NAMES[colourway]}, accessory ${accessory}`}
      />
    </div>
    <div className="ar-try-controls">
      <div className="ar-switch" role="group" aria-label="Accessory">
        <span className="ar-switch-label">Accessory</span>
        {ARCHIE_ACCESSORIES.map((value) => (
          <button
            key={value}
            type="button"
            aria-pressed={accessory === value}
            onClick={() => onAccessory(value)}
          >
            {value}
          </button>
        ))}
      </div>
      <div className="ar-switch" role="group" aria-label="Colourway">
        <span className="ar-switch-label">Colourway</span>
        {ARCHIE_COLOURWAYS.map((value) => (
          <button
            key={value}
            type="button"
            aria-pressed={colourway === value}
            onClick={() => onColourway(value)}
          >
            {value} {COLOURWAY_NAMES[value]}
          </button>
        ))}
      </div>
      <p className="ar-try-note">
        One drawing. The kit is <code>data-accessory</code> on the SVG and the colourway is
        a <code>.archie-c1</code>&hellip;<code>.archie-c4</code> class on any ancestor, so
        neither is ever a second file to keep in sync. He arrives bare, carrying the torch
        in a front paw; the head gear is a costume you switch on. C3 is the default
        colourway and the only one that ships on this site.
      </p>
      <p className="ar-try-note">
        Under 40px the torch stops being a drawing and becomes a smudge, so the SVG takes{" "}
        <code>data-size=&quot;small&quot;</code> and swaps it for a lit dot at the paw.{" "}
        <code>Archie.tsx</code> sets that for you from the size you ask for.
      </p>
      <p className="ar-try-note">
        <code>agentworth config set archie.colourway C4</code> stores a choice for the
        surfaces that draw him: the local dashboard, and anything else rendering the SVG.
        The terminal form does not read it — there the torch glyph <em>is</em> the state, so
        it is fixed.
      </p>
    </div>
  </div>
);

export const ArchiePage: React.FC = () => {
  const [accessory, setAccessory] = React.useState<ArchieAccessory>(ARCHIE_DEFAULT_ACCESSORY);
  const [colourway, setColourway] = React.useState<ArchieColourway>(ARCHIE_DEFAULT_COLOURWAY);

  return (
    <>
      <a className="skip" href="#main">
        Skip to content
      </a>
      <SiteHeader current="archie" />

      <main id="main" className="archie">
        <div className="wrap">
          <nav className="crumbs" aria-label="Breadcrumb">
            <a href="/">Home</a>
            <span aria-hidden="true">/</span>
            <span aria-current="page">Archie</span>
          </nav>

          <section className="ar-hero">
            <div>
              <p className="kicker">agentworth.dev/archie</p>
              <h1>The memory your agents don&rsquo;t have.</h1>
              <p className="lede">
                Archie digs through the session logs your coding agents already left on
                disk, and comes back with the one line you needed. Nothing leaves the
                machine.
              </p>
              <div className="ar-cmd">
                <div className="install install-static">
                  <code>
                    <span className="sigil" aria-hidden="true">
                      $
                    </span>
                    {SCAN_CMD}
                  </code>
                </div>
                <p className="alt-install">
                  or <code>cargo install agentworth-cli</code>
                </p>
              </div>
            </div>
            <div className="ar-hero-figure">
              <Archie
                pose="dropping"
                size={240}
                accessory={accessory}
                colourway={colourway}
                label="Archie, dropping a receipt at your feet"
              />
            </div>
          </section>

          <KitSwitch
            accessory={accessory}
            colourway={colourway}
            onAccessory={setAccessory}
            onColourway={setColourway}
          />

          <section className="ar-verbs">
            <h2 className="ar-section-head">What he does</h2>
            {VERBS.map((verb) => (
              <article className="ar-verb" key={verb.mcp}>
                <div className="ar-verb-tile">
                  <Archie
                    pose={verb.pose}
                    size={96}
                    accessory={accessory}
                    colourway={colourway}
                    label=""
                  />
                </div>
                <div>
                  <h3>{verb.title}</h3>
                  <p>{verb.body}</p>
                </div>
                <div className="ar-names">
                  <span className="ar-name is-cli">
                    <span className="ar-name-kind">CLI</span>
                    <code>{verb.cli}</code>
                  </span>
                  <span className="ar-name">
                    <span className="ar-name-kind">MCP</span>
                    <code>{verb.mcp}</code>
                  </span>
                </div>
              </article>
            ))}
          </section>

          <section className="ar-band">
            <div className="ar-band-tile">
              <Archie
                pose="digging"
                size={140}
                accessory={accessory}
                colourway={colourway}
                label=""
              />
            </div>
            <div>
              <h3>He works offline, on files you already have</h3>
              <p>
                Nothing is uploaded, no model reads your transcript, and the index lives
                beside the logs. He is digging through files you already have — nothing
                here is talking to a server.
              </p>
            </div>
          </section>

          <section>
            <h2 className="ar-section-head">In the terminal</h2>
            <div className="ar-term">
              <pre>{TERMINAL_BLOCK}</pre>
            </div>
          </section>
        </div>
      </main>

      <SiteFooter
        signoff={
          <span className="ar-signoff">
            <Archie pose="sleeping" size={36} accessory={accessory} colourway="C4" label="" />
            <span>good boy</span>
          </span>
        }
      />
    </>
  );
};
