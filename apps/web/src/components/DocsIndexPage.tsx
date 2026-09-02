import React from "react";
import { SiteHeader, SiteFooter, InstallLine, REPO } from "./SiteChrome";
import { DocsNav, DocsSearch } from "./DocsShell";
import { docPath, type Doc, type DocSection } from "../content";

interface Props {
  section: Exclude<DocSection, "learn">;
  title: string;
  crumb: string;
  lede: React.ReactNode;
  docs: Doc[];
  sourceDir: string;
}

/** The flat index sheet: one line item per page, its description under the title
 *  and its own status on the right. Used for /docs/specs/ and /docs/research/. */
export const DocsIndexPage: React.FC<Props> = ({
  section,
  title,
  crumb,
  lede,
  docs,
  sourceDir,
}) => (
  <>
    <a className="skip" href="#main">
      Skip to content
    </a>
    <SiteHeader current="docs" />

    <main id="main">
      <div className="wrap">
        <nav className="crumbs" aria-label="Breadcrumb">
          <a href="/">Home</a>
          <span aria-hidden="true">/</span>
          <a href="/docs/">Docs</a>
          <span aria-hidden="true">/</span>
          <span aria-current="page">{crumb}</span>
        </nav>

        <header className="page-head docs-head">
          <p className="kicker">{crumb}</p>
          <h1>{title}</h1>
          <p className="lede">{lede}</p>
          <DocsSearch />
        </header>

        <div className="docs-layout">
          <DocsNav section={section} />

          <div className="docs-main">
            <ul className="line-items">
              {docs.map((d) => (
                <li className="line-item" key={d.slug}>
                  <span className="li-main">
                    <a className="li-title" href={docPath(section, d.slug)}>
                      {d.title}
                    </a>
                    <span className="li-file mono">{d.file}</span>
                    {d.description && <p className="li-desc">{d.description}</p>}
                  </span>
                  {d.status && <span className="li-count mono">{d.status}</span>}
                </li>
              ))}
            </ul>

            <p className="sheet-total">
              <span>
                {docs.length} {docs.length === 1 ? "file" : "files"}, read straight out of{" "}
                <code className="mono">{sourceDir}</code>
              </span>
              <a href={`${REPO}/tree/main/${sourceDir}`}>Source on GitHub</a>
            </p>
          </div>
        </div>

        <section className="close-band">
          <h2>Point it at your own machine.</h2>
          <InstallLine />
        </section>
      </div>
    </main>

    <SiteFooter />
  </>
);
