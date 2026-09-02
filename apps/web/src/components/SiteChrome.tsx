import React from "react";
import { ThemeToggle } from "@ui/ThemeToggle";
import { BrandMark } from "@ui/BrandMark";
import { Wordmark } from "@ui/Wordmark";
import { IconGithub } from "@ui/icons";

export const REPO = "https://github.com/unfoundbox-crew/agentworth";
export const SCAN_CMD = "npx -y agentworth scan";
export const CURL_CMD = "curl -fsSL https://agentworth.dev/install.sh | sh";

/** `current` dims the link for the page you are already on. */
export const SiteHeader: React.FC<{
  current?: "changelog" | "reference" | "blog" | "archie";
}> = ({ current }) => (
  <header className="wrap">
    <nav className="nav">
      <a className="mark" href="/" aria-label="AgentWorth home">
        <BrandMark size={20} />
        <span className="wordmark">
          <Wordmark height={13} />
        </span>
      </a>
      <div className="nav-right">
        <a
          className="nav-link"
          href="/changelog/"
          aria-current={current === "changelog" ? "page" : undefined}
        >
          Changelog
        </a>
        <a
          className="nav-link"
          href="/docs/reference/"
          aria-current={current === "reference" ? "page" : undefined}
        >
          Reference
        </a>
        <a
          className="nav-link"
          href="/blog/"
          aria-current={current === "blog" ? "page" : undefined}
        >
          Blog
        </a>
        <a
          className="nav-link"
          href="/archie/"
          aria-current={current === "archie" ? "page" : undefined}
        >
          Archie
        </a>
        <a className="nav-link" href={REPO} target="_blank" rel="noreferrer">
          <IconGithub size={14} />
          <span className="nav-label">GitHub</span>
        </a>
        <ThemeToggle />
      </div>
    </nav>
  </header>
);

/** `signoff` replaces the licence line on the one page that has earned a sign-off. */
export const SiteFooter: React.FC<{ signoff?: React.ReactNode }> = ({ signoff }) => (
  <footer className="wrap">
    <div className="foot">
      <span>{signoff ?? "Apache-2.0 \u00b7 native Rust \u00b7 nothing uploaded"}</span>
      <span className="foot-links">
        <a href="/blog/rss.xml">Blog RSS</a>
        <a href="/changelog/rss.xml">Releases RSS</a>
        <a href={REPO} target="_blank" rel="noreferrer">
          github.com/unfoundbox-crew/agentworth
        </a>
      </span>
    </div>
  </footer>
);

/** The command the site always ends on. Copy is a client-only nicety; the
 *  text is in the HTML either way, which is what a reader without JS needs. */
export const InstallLine: React.FC<{ cmd?: string }> = ({ cmd = SCAN_CMD }) => (
  <div className="install install-static">
    <code>
      <span className="sigil" aria-hidden="true">
        $
      </span>
      {cmd}
    </code>
  </div>
);
