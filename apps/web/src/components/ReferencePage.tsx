import React from "react";
import { SiteHeader, SiteFooter, InstallLine, REPO } from "./SiteChrome";
import {
  reference,
  humanDate,
  type CliArgDoc,
  type CliCommandDoc,
  type ApiRouteDoc,
  type McpToolDoc,
} from "../content";

/** Stable anchor id for a command path, route, or tool name. */
const slugify = (s: string): string =>
  s
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");

const cmdId = (path: string) => slugify(path);
const routeId = (method: string, path: string) => slugify(`${method} ${path}`);
const toolId = (name: string) => slugify(name);

const ArgsTable: React.FC<{ args: CliArgDoc[] }> = ({ args }) => {
  if (args.length === 0) return null;
  return (
    <table>
      <thead>
        <tr>
          <th>Flag</th>
          <th>Required</th>
          <th>Help</th>
          <th>Default</th>
          <th>Values</th>
        </tr>
      </thead>
      <tbody>
        {args.map((a) => (
          <tr key={a.name}>
            <td>
              <code>{a.name}</code>
            </td>
            <td>{a.required ? "yes" : "no"}</td>
            <td>{a.help || "-"}</td>
            <td>{a.default ?? "-"}</td>
            <td>{a.possibleValues.length ? a.possibleValues.join(", ") : "-"}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
};

/** Type label for one JSON Schema property, matching the Rust renderer's
 *  `schema_type_label` (apps/cli/src/commands/docs.rs) so the page and the
 *  markdown never disagree about how a field is described. */
function schemaTypeLabel(prop: unknown): string {
  if (typeof prop !== "object" || prop === null) return "-";
  const p = prop as Record<string, unknown>;
  if (typeof p.type === "string") return p.type;
  if (Array.isArray(p.type)) return p.type.filter((t) => typeof t === "string").join(" or ");
  if (p.$ref !== undefined) return "object";
  return "-";
}

const SchemaParamsTable: React.FC<{ schema: Record<string, unknown> }> = ({ schema }) => {
  const properties = schema.properties as Record<string, Record<string, unknown>> | undefined;
  if (!properties || Object.keys(properties).length === 0) return null;
  const required = new Set((schema.required as string[] | undefined) ?? []);

  return (
    <table>
      <thead>
        <tr>
          <th>Param</th>
          <th>Required</th>
          <th>Type</th>
          <th>Description</th>
        </tr>
      </thead>
      <tbody>
        {Object.entries(properties).map(([name, prop]) => (
          <tr key={name}>
            <td>
              <code>{name}</code>
            </td>
            <td>{required.has(name) ? "yes" : "no"}</td>
            <td>{schemaTypeLabel(prop)}</td>
            <td>{(prop.description as string | undefined) ?? "-"}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
};

const CliSection: React.FC<{ commands: CliCommandDoc[]; globalFlags: CliArgDoc[] }> = ({
  commands,
  globalFlags,
}) => (
  <section id="cli" className="ref-group">
    <h2>CLI</h2>
    <article className="ref-item prose" id="cli-global-flags">
      <h3>Global flags</h3>
      <p>Accepted before the subcommand on every command below.</p>
      <ArgsTable args={globalFlags} />
    </article>
    {commands.map((cmd) => (
      <article className="ref-item prose" id={cmdId(cmd.path)} key={cmd.path}>
        <h3>
          <code>{cmd.path}</code>
        </h3>
        {cmd.about && <p>{cmd.about}</p>}
        <ArgsTable args={cmd.args} />
      </article>
    ))}
  </section>
);

const ApiSection: React.FC<{ routes: ApiRouteDoc[] }> = ({ routes }) => (
  <section id="api" className="ref-group">
    <h2>HTTP API</h2>
    <p className="ref-group-lede">
      Registered by <code>agentworth serve</code> under{" "}
      <code>http://localhost:&lt;port&gt;</code>.
    </p>
    {routes.map((route) => (
      <article
        className="ref-item prose"
        id={routeId(route.method, route.path)}
        key={`${route.method} ${route.path}`}
      >
        <h3>
          <span className="ref-method">{route.method}</span>{" "}
          <code>{route.path}</code>
        </h3>
        <p>{route.description}</p>
        {route.queryParams.length > 0 && (
          <table>
            <thead>
              <tr>
                <th>Query Param</th>
                <th>Description</th>
              </tr>
            </thead>
            <tbody>
              {route.queryParams.map((p) => (
                <tr key={p.name}>
                  <td>
                    <code>{p.name}</code>
                  </td>
                  <td>{p.description}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </article>
    ))}
  </section>
);

const McpSection: React.FC<{ tools: McpToolDoc[] }> = ({ tools }) => (
  <section id="mcp" className="ref-group">
    <h2>MCP Tools</h2>
    <p className="ref-group-lede">
      Register once with{" "}
      <code>claude mcp add agentworth --scope user -- agentworth mcp</code> (stdio). Read-only;
      redaction is on by default for every tool.
    </p>
    {tools.map((tool) => (
      <article className="ref-item prose" id={toolId(tool.name)} key={tool.name}>
        <h3>
          <code>{tool.name}</code>
        </h3>
        <p>{tool.description}</p>
        <SchemaParamsTable schema={tool.inputSchema} />
        <details className="ref-schema">
          <summary>JSON schema</summary>
          <pre>
            <code>{JSON.stringify(tool.inputSchema, null, 2)}</code>
          </pre>
        </details>
      </article>
    ))}
  </section>
);

export const ReferencePage: React.FC = () => {
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
            <span aria-current="page">Reference</span>
          </nav>

          <header className="page-head">
            <p className="kicker">Reference</p>
            <h1>Every command, route, and tool -- read straight from the code.</h1>
            <p className="lede">
              Generated by <code>agentworth docs --write</code> from v{reference.version}
              , {humanDate(reference.generatedDate)}. CI regenerates it on every change and
              fails the build if this page would disagree with the binary -- so nothing
              here is hand-typed prose. Also available as{" "}
              <a href="/docs/reference.md">plain markdown</a> and inlined in{" "}
              <a href="/llms-full.txt">llms-full.txt</a>.
            </p>
          </header>

          <div className="rel-layout">
            <article className="rel-main">
              <CliSection commands={reference.cli} globalFlags={reference.globalFlags} />
              <ApiSection routes={reference.api} />
              <McpSection tools={reference.mcp} />
            </article>

            <nav className="rel-rail ref-rail" aria-label="Reference sections">
              <p className="rail-head">CLI</p>
              <ol>
                <li>
                  <a href="#cli-global-flags">
                    <span className="rail-v">Global flags</span>
                  </a>
                </li>
                {reference.cli.map((cmd) => (
                  <li key={cmd.path}>
                    <a href={`#${cmdId(cmd.path)}`}>
                      <span className="rail-v">{cmd.path.replace(/^agentworth /, "")}</span>
                    </a>
                  </li>
                ))}
              </ol>

              <p className="rail-head">HTTP API</p>
              <ol>
                {reference.api.map((route) => (
                  <li key={`${route.method} ${route.path}`}>
                    <a href={`#${routeId(route.method, route.path)}`}>
                      <span className="rail-v">
                        {route.method} {route.path}
                      </span>
                    </a>
                  </li>
                ))}
              </ol>

              <p className="rail-head">MCP Tools</p>
              <ol>
                {reference.mcp.map((tool) => (
                  <li key={tool.name}>
                    <a href={`#${toolId(tool.name)}`}>
                      <span className="rail-v">{tool.name}</span>
                    </a>
                  </li>
                ))}
              </ol>

              <p className="rail-foot">
                <a href={`${REPO}/blob/main/docs/REFERENCE.md`}>Source on GitHub</a>
              </p>
            </nav>
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
