import React from "react";
import { SiteHeader, SiteFooter, InstallLine, REPO } from "./SiteChrome";
import { DocsNav, DocsSearch } from "./DocsShell";
import { docPath, type Doc } from "../content";

const SECTION_LABEL: Record<string, string> = {
  learn: "Learn",
  specs: "Specs",
  research: "Research",
};

const SECTION_INDEX: Record<string, string> = {
  learn: "/docs/",
  specs: "/docs/specs/",
  research: "/docs/research/",
};

const SOURCE_DIR: Record<string, string> = {
  learn: "apps/web/content/docs/learn",
  specs: "docs/specs",
  research: "docs/research",
};

export const DocsArticlePage: React.FC<{ doc: Doc }> = ({ doc }) => {
  const label = SECTION_LABEL[doc.section];
  return (
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
            <a href={SECTION_INDEX[doc.section]}>{label}</a>
            <span aria-hidden="true">/</span>
            <span aria-current="page">{doc.title}</span>
          </nav>

          <header className="page-head docs-head">
            <p className="kicker">{label}</p>
            <h1>{doc.title}</h1>
            {/* A spec's description is its own opening paragraph, which the body
                renders a few lines further down; repeating it here would be the
                same sentence twice. The guides carry a written front-matter
                description, which is not in their body at all. */}
            {doc.section === "learn" && doc.description && (
              <p className="lede">{doc.description}</p>
            )}
            {doc.status && (
              <p className="doc-status mono">
                <span>Status</span> {doc.status}
              </p>
            )}
            <DocsSearch />
          </header>

          <div className="docs-layout">
            <DocsNav section={doc.section} slug={doc.slug} />

            <div className="docs-main">
              <article
                className="prose doc-body"
                dangerouslySetInnerHTML={{ __html: doc.html }}
              />

              <p className="doc-source mono">
                <a href={`${REPO}/blob/main/${SOURCE_DIR[doc.section]}/${doc.file}`}>
                  {SOURCE_DIR[doc.section]}/{doc.file} on GitHub
                </a>
              </p>

              {(doc.prev || doc.next) && (
                <nav className="post-nav" aria-label={`More in ${label}`}>
                  {doc.prev ? (
                    <a className="pn newer" href={docPath(doc.prev.section, doc.prev.slug)}>
                      <span>Previous</span>
                      <b>{doc.prev.title}</b>
                    </a>
                  ) : (
                    <span />
                  )}
                  {doc.next && (
                    <a className="pn older" href={docPath(doc.next.section, doc.next.slug)}>
                      <span>Next</span>
                      <b>{doc.next.title}</b>
                    </a>
                  )}
                </nav>
              )}
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
};
