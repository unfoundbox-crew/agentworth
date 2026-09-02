import React from "react";
import { SiteHeader, SiteFooter } from "./SiteChrome";
import { Archie } from "@ui/Archie";

/**
 * The 404. Lamp off, ears flat: the page is missing, he looked, that is all.
 *
 * Pre-rendered to `dist/404.html` rather than to a route directory — a static host
 * serves that file for paths that match nothing, and a path that matches nothing is
 * the only way anyone gets here. The address is filled in on mount, because at build
 * time there is no address to name.
 */
export const NotFoundPage: React.FC = () => {
  const [path, setPath] = React.useState<string | null>(null);
  React.useEffect(() => setPath(window.location.pathname), []);

  return (
    <>
      <a className="skip" href="#main">
        Skip to content
      </a>
      <SiteHeader />

      <main id="main" className="archie">
        <div className="wrap">
          <section className="ar-hero">
            <div>
              <p className="kicker">404</p>
              <h1>Nothing at that address.</h1>
              <p className="lede">
                {path ? (
                  <>
                    Archie went and looked. There is no page at <code>{path}</code>.
                  </>
                ) : (
                  <>Archie went and looked. There is no page at that address.</>
                )}{" "}
                The reference lists every command, route and MCP tool AgentWorth
                actually has.
              </p>
              <p className="alt-install">
                <a href="/">agentworth.dev</a> <span className="sep">&middot;</span>{" "}
                <a href="/docs/reference/">Reference</a> <span className="sep">&middot;</span>{" "}
                <a href="/archie/">Archie</a>
              </p>
            </div>
            <div className="ar-hero-figure">
              <Archie pose="error" size={200} label="Archie, lamp off, having found nothing" />
            </div>
          </section>
        </div>
      </main>

      <SiteFooter />
    </>
  );
};
