import React from "react";
import { docs, docPath, releases, reference, type Doc, type DocSection } from "../content";
import { DocsSearch } from "./DocsSearch";

export const LEARN = docs.learn;
export const SPECS = docs.specs;
export const RESEARCH = docs.research;

/** Total page count for the Reference card and rail: the generated reference is
 *  one page, but three groups of items, and the count people want is the items. */
export const REFERENCE_ITEMS =
  reference.cli.length + reference.api.length + reference.mcp.length;

export interface DocsNavState {
  /** The page currently open, so its row can be marked. */
  section?: DocSection | "reference" | "home";
  slug?: string;
}

const Row: React.FC<{
  href: string;
  label: string;
  meta?: string;
  current?: boolean;
}> = ({ href, label, meta, current }) => (
  <li>
    <a href={href} aria-current={current ? "page" : undefined}>
      <span>{label}</span>
      {meta && <span className="rail-meta">{meta}</span>}
    </a>
  </li>
);

/** The rail's contents, rendered twice: once in the desktop sidebar, once
 *  inside the mobile selector. Same links, one definition. */
const NavBody: React.FC<DocsNavState> = ({ section, slug }) => {
  const inSpecs = section === "specs";
  const inResearch = section === "research";
  const list = (docsList: Doc[], of: DocSection) =>
    docsList.map((d) => (
      <Row
        key={d.slug}
        href={docPath(of, d.slug)}
        label={d.title}
        current={section === of && slug === d.slug}
      />
    ));

  return (
    <>
      <p className="rail-head">Learn</p>
      <ol>{list(LEARN, "learn")}</ol>

      <p className="rail-head">Reference</p>
      <ol>
        <Row href="/docs/reference/#cli" label="CLI" meta={String(reference.cli.length)} />
        <Row href="/docs/reference/#api" label="HTTP API" meta={String(reference.api.length)} />
        <Row href="/docs/reference/#mcp" label="MCP Tools" meta={String(reference.mcp.length)} />
      </ol>

      <p className="rail-head">Specs</p>
      <ol>
        {inSpecs ? (
          list(SPECS, "specs")
        ) : (
          <Row href="/docs/specs/" label="All specs" meta={String(SPECS.length)} />
        )}
      </ol>

      <p className="rail-head">Research</p>
      <ol>
        {inResearch ? (
          list(RESEARCH, "research")
        ) : (
          <Row href="/docs/research/" label="All memos" meta={String(RESEARCH.length)} />
        )}
      </ol>

      <p className="rail-head">Changelog</p>
      <ol>
        <Row href="/changelog/" label="Every release" meta={`v${releases[0].version}`} />
      </ol>
    </>
  );
};

/**
 * Desktop rail and mobile selector are two elements, not one that changes
 * shape. A `<details>` forced open by CSS is not reliable across engines, and
 * a JS-driven collapse would have to guess the viewport during server render.
 * Both are in the HTML; CSS shows exactly one.
 */
export const DocsNav: React.FC<DocsNavState> = (state) => (
  <>
    <aside className="docs-sidebar" aria-label="Documentation sections">
      <NavBody {...state} />
    </aside>
    <details className="docs-selector">
      <summary>All sections</summary>
      <div className="docs-selector-body">
        <NavBody {...state} />
      </div>
    </details>
  </>
);

export { DocsSearch };
