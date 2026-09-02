//! `agentworth docs`: generates `docs/REFERENCE.md` (and its JSON twin, `docs/reference.json`,
//! which `apps/web` builds the `/docs/reference/` page from) directly from the code:
//!
//! - **CLI** section: walks the `clap::Command` tree `crate::app::cli_command()` builds --
//!   every subcommand and flag comes from `#[derive(Parser)]`'s own introspection, not
//!   hand-typed text.
//! - **API** section: walks `crate::server::routes::route_entries()`, the same data structure
//!   `create_router` builds the live `/api/*` surface from.
//! - **MCP** section: walks `AgentWorthMcpServer::tool_router().list_all()`, which carries each
//!   tool's rmcp-derived JSON schema -- building a `ToolRouter` doesn't touch storage, so this
//!   works without a running server.
//!
//! A CI step regenerates this output and fails if it differs from the committed
//! `docs/REFERENCE.md` (see `.github/workflows/ci.yml`), so the reference cannot go stale.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result};
use clap::Id;
use serde::Serialize;

/// One flag or positional argument on a CLI command.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliArgDoc {
    /// Display form: `--limit, -l` for an option, `PATHS` for a positional.
    pub name: String,
    pub positional: bool,
    pub required: bool,
    pub help: String,
    pub default: Option<String>,
    pub possible_values: Vec<String>,
}

/// One clap subcommand, at any depth (e.g. both `agentworth config` and `agentworth config
/// set`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliCommandDoc {
    /// Full invocation, e.g. `"agentworth config set"`.
    pub path: String,
    pub about: String,
    pub args: Vec<CliArgDoc>,
}

/// One HTTP query parameter, mirroring `server::routes::QueryParamDoc`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiParamDoc {
    pub name: String,
    pub description: String,
}

/// One registered `/api/*` route.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiRouteDoc {
    pub method: String,
    pub path: String,
    pub description: String,
    pub query_params: Vec<ApiParamDoc>,
}

/// One MCP tool, with its rmcp-derived JSON schema reused as-is.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolDoc {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// The complete generated reference: CLI, API, and MCP surfaces plus the header metadata
/// every rendering (markdown or JSON) carries.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceDoc {
    pub version: String,
    pub generated_date: String,
    pub global_flags: Vec<CliArgDoc>,
    pub cli: Vec<CliCommandDoc>,
    pub api: Vec<ApiRouteDoc>,
    pub mcp: Vec<McpToolDoc>,
    /// Old spelling -> new spelling, for the pre-0.1.16 CLI commands and MCP tools that
    /// still run but no longer appear in help. Read from `crate::app`'s own tables, so this
    /// cannot drift from what the binary actually accepts.
    pub old_spellings: Vec<OldSpelling>,
}

/// One retired name and what replaced it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OldSpelling {
    /// `cli` or `mcp`.
    pub surface: String,
    pub old: String,
    pub new: String,
    /// The release the old name stops working in.
    pub removed_in: String,
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Builds the full reference from the code: clap's command tree, the axum route table, and
/// the rmcp tool router. None of the three requires a running server or an open database.
pub fn build_reference_doc() -> ReferenceDoc {
    let mut root = crate::app::cli_command();
    // "Prepare for introspecting" (the method's own doc comment) -- without this, a
    // subcommand's `get_arguments()` would not yet carry the parent's `global = true` flags
    // propagated down to it.
    root.build();

    // The five `global = true` flags (verbose/db_path/no_json/no_color/plain) plus clap's own
    // auto-generated `--help`/`--version`. Documented once at the top instead of repeated on
    // every one of the ~30 subcommands they propagate to.
    let global_ids: HashSet<Id> = root
        .get_arguments()
        .filter(|a| !matches!(a.get_id().as_str(), "help" | "version"))
        .map(|a| a.get_id().clone())
        .collect();
    let global_flags = collect_args(&root, &HashSet::new());

    let mut cli = Vec::new();
    collect_commands(&root, "agentworth", &global_ids, &mut cli);

    let api = crate::server::routes::route_entries()
        .into_iter()
        .map(|entry| ApiRouteDoc {
            method: entry.method.to_string(),
            path: format!("/api{}", entry.path),
            description: entry.description.to_string(),
            query_params: entry
                .query_params
                .iter()
                .map(|p| ApiParamDoc {
                    name: p.name.to_string(),
                    description: p.description.to_string(),
                })
                .collect(),
        })
        .collect();

    // Hidden things stay out of the reference: a retired tool name appears once, in the
    // old-spellings table, not documented a second time as a tool of its own.
    let mcp = crate::mcp::AgentWorthMcpServer::tool_router()
        .list_all()
        .into_iter()
        .filter(|tool| {
            !crate::app::OLD_MCP_TOOL_NAMES
                .iter()
                .any(|(old, _)| *old == tool.name.as_ref())
        })
        .map(|tool| McpToolDoc {
            name: tool.name.to_string(),
            description: tool.description.map(|d| d.to_string()).unwrap_or_default(),
            input_schema: serde_json::Value::Object((*tool.input_schema).clone()),
        })
        .collect();

    let old_spellings = crate::app::OLD_CLI_SPELLINGS
        .iter()
        .map(|(old, new)| OldSpelling {
            surface: "cli".to_string(),
            old: (*old).to_string(),
            new: (*new).to_string(),
            removed_in: crate::app::HIDDEN_ALIASES_REMOVED_IN.to_string(),
        })
        .chain(
            crate::app::OLD_MCP_TOOL_NAMES
                .iter()
                .map(|(old, new)| OldSpelling {
                    surface: "mcp".to_string(),
                    old: (*old).to_string(),
                    new: (*new).to_string(),
                    removed_in: crate::app::HIDDEN_ALIASES_REMOVED_IN.to_string(),
                }),
        )
        .collect();

    ReferenceDoc {
        version: VERSION.to_string(),
        generated_date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        global_flags,
        cli,
        api,
        mcp,
        old_spellings,
    }
}

/// Recursively walks `cmd`'s subcommands, appending one `CliCommandDoc` per subcommand at
/// every depth (so `agentworth config` and `agentworth config set` both get an entry).
/// clap auto-adds a `help` pseudo-subcommand once a command has any subcommands; it is
/// skipped here, along with anything explicitly hidden.
fn collect_commands(
    cmd: &clap::Command,
    prefix: &str,
    global_ids: &HashSet<Id>,
    out: &mut Vec<CliCommandDoc>,
) {
    for sub in cmd.get_subcommands() {
        if sub.get_name() == "help" || sub.is_hide_set() {
            continue;
        }
        let path = format!("{prefix} {}", sub.get_name());
        let about = sub
            .get_about()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let args = collect_args(sub, global_ids);
        out.push(CliCommandDoc { path: path.clone(), about, args });
        collect_commands(sub, &path, global_ids, out);
    }
}

fn collect_args(cmd: &clap::Command, exclude: &HashSet<Id>) -> Vec<CliArgDoc> {
    cmd.get_arguments()
        .filter(|a| !matches!(a.get_id().as_str(), "help" | "version"))
        .filter(|a| !exclude.contains(a.get_id()))
        .map(|a| {
            let positional = a.is_positional();
            let name = if positional {
                a.get_value_names()
                    .and_then(|names| names.first())
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| a.get_id().to_string().to_uppercase())
            } else {
                let mut parts = Vec::new();
                if let Some(l) = a.get_long() {
                    parts.push(format!("--{l}"));
                }
                if let Some(s) = a.get_short() {
                    parts.push(format!("-{s}"));
                }
                if parts.is_empty() {
                    a.get_id().to_string()
                } else {
                    parts.join(", ")
                }
            };
            CliArgDoc {
                name,
                positional,
                required: a.is_required_set(),
                help: a.get_help().map(|h| h.to_string()).unwrap_or_default(),
                default: a
                    .get_default_values()
                    .first()
                    .map(|v| v.to_string_lossy().to_string()),
                possible_values: a
                    .get_possible_values()
                    .into_iter()
                    .map(|pv| pv.get_name().to_string())
                    .collect(),
            }
        })
        .collect()
}

/// Markdown table cells can't carry a raw `|` or newline without breaking the row.
fn md_cell(s: &str) -> String {
    if s.is_empty() {
        return "-".to_string();
    }
    s.replace('|', "\\|").replace('\n', "<br>")
}

fn render_args_table(out: &mut String, args: &[CliArgDoc]) {
    if args.is_empty() {
        return;
    }
    let _ = writeln!(out, "| Flag | Required | Help | Default | Values |");
    let _ = writeln!(out, "|---|---|---|---|---|");
    for a in args {
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} |",
            a.name,
            if a.required { "yes" } else { "no" },
            md_cell(&a.help),
            a.default.as_deref().map(md_cell).unwrap_or_else(|| "-".to_string()),
            if a.possible_values.is_empty() {
                "-".to_string()
            } else {
                a.possible_values.join(", ")
            },
        );
    }
    let _ = writeln!(out);
}

/// Renders the full reference as the markdown that becomes `docs/REFERENCE.md`.
pub fn render_markdown(doc: &ReferenceDoc) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# AgentWorth Reference");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "_Generated by `agentworth docs --write` from v{} on {}. Do not edit by hand -- \
         regenerate with `agentworth docs --write` (see `apps/cli/src/commands/docs.rs`)._",
        doc.version, doc.generated_date
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Three sections, each read straight from the running code: every `clap` subcommand \
         and flag, every route the local API server registers, and every MCP tool this \
         binary exposes over stdio."
    );
    let _ = writeln!(out);

    let _ = writeln!(out, "## CLI");
    let _ = writeln!(out);
    let _ = writeln!(out, "### Global flags");
    let _ = writeln!(out);
    let _ = writeln!(out, "Accepted before the subcommand on every command below.");
    let _ = writeln!(out);
    render_args_table(&mut out, &doc.global_flags);

    for cmd in &doc.cli {
        let _ = writeln!(out, "### `{}`", cmd.path);
        let _ = writeln!(out);
        if !cmd.about.is_empty() {
            let _ = writeln!(out, "{}", cmd.about);
            let _ = writeln!(out);
        }
        render_args_table(&mut out, &cmd.args);
    }

    if !doc.old_spellings.is_empty() {
        let removed_in = doc
            .old_spellings
            .first()
            .map(|s| s.removed_in.clone())
            .unwrap_or_default();
        let _ = writeln!(out, "### Old spellings");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "The commands and MCP tools below were renamed in 0.1.16. Every old name still \
             runs and reaches the same handler; none of them appears in `--help` any more, \
             and all of them stop working in {removed_in}."
        );
        let _ = writeln!(out);
        let _ = writeln!(out, "| Surface | Old | New |");
        let _ = writeln!(out, "|---|---|---|");
        for s in &doc.old_spellings {
            let _ = writeln!(out, "| {} | `{}` | `{}` |", s.surface, s.old, s.new);
        }
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "## HTTP API");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Registered by `agentworth serve` under `http://localhost:<port>` (default port \
         {}). Every route below lives in one table, `server::routes::route_entries()`, that \
         also builds the live router -- so this list cannot list a route the server doesn't \
         actually serve.",
        crate::server::DEFAULT_PORT
    );
    let _ = writeln!(out);
    for route in &doc.api {
        let _ = writeln!(out, "### `{} {}`", route.method, route.path);
        let _ = writeln!(out);
        let _ = writeln!(out, "{}", route.description);
        let _ = writeln!(out);
        if !route.query_params.is_empty() {
            let _ = writeln!(out, "| Query Param | Description |");
            let _ = writeln!(out, "|---|---|");
            for p in &route.query_params {
                let _ = writeln!(out, "| `{}` | {} |", p.name, md_cell(&p.description));
            }
            let _ = writeln!(out);
        }
    }

    let _ = writeln!(out, "## MCP Tools");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Register once with `claude mcp add agentworth --scope user -- agentworth mcp` \
         (stdio). Read-only; redaction is on by default for every tool (see \
         `docs/specs/mcp-server.md`)."
    );
    let _ = writeln!(out);
    for tool in &doc.mcp {
        let _ = writeln!(out, "### `{}`", tool.name);
        let _ = writeln!(out);
        let _ = writeln!(out, "{}", tool.description);
        let _ = writeln!(out);
        render_mcp_params_table(&mut out, &tool.input_schema);
        let _ = writeln!(out, "<details><summary>JSON schema</summary>");
        let _ = writeln!(out);
        let _ = writeln!(out, "```json");
        let _ = writeln!(
            out,
            "{}",
            serde_json::to_string_pretty(&tool.input_schema).unwrap_or_default()
        );
        let _ = writeln!(out, "```");
        let _ = writeln!(out);
        let _ = writeln!(out, "</details>");
        let _ = writeln!(out);
    }

    out
}

/// A short params table pulled from the schema's top-level `properties`/`required`, ahead of
/// the full JSON schema (kept below in a `<details>` block) -- most readers want "what do I
/// pass" before "the exact schema".
fn render_mcp_params_table(out: &mut String, schema: &serde_json::Value) {
    let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) else {
        return;
    };
    if properties.is_empty() {
        return;
    }
    let required: HashSet<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let _ = writeln!(out, "| Param | Required | Type | Description |");
    let _ = writeln!(out, "|---|---|---|---|");
    for (name, prop) in properties {
        let ty = schema_type_label(prop);
        let description = prop.get("description").and_then(|d| d.as_str()).unwrap_or("-");
        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} |",
            name,
            if required.contains(name.as_str()) { "yes" } else { "no" },
            md_cell(&ty),
            md_cell(description),
        );
    }
    let _ = writeln!(out);
}

fn schema_type_label(prop: &serde_json::Value) -> String {
    if let Some(t) = prop.get("type").and_then(|t| t.as_str()) {
        return t.to_string();
    }
    if let Some(arr) = prop.get("type").and_then(|t| t.as_array()) {
        // Joined with "or", not "|" -- this string lands directly in a markdown table cell,
        // and an un-escaped pipe there would silently split the row into extra columns.
        return arr
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" or ");
    }
    if prop.get("$ref").is_some() {
        return "object".to_string();
    }
    "-".to_string()
}

/// Writes `docs/REFERENCE.md` and `docs/reference.json`, both relative to the current
/// directory, which must be the repository root (the same assumption the CI step and this
/// command's own `--help` text make).
pub fn run_docs_command(format: &str, write: bool) -> Result<()> {
    let doc = build_reference_doc();

    if write {
        let markdown = render_markdown(&doc);
        let json = serde_json::to_string_pretty(&doc)? + "\n";

        let docs_dir = Path::new("docs");
        if !docs_dir.is_dir() {
            anyhow::bail!(
                "docs/ directory not found in the current directory -- run `agentworth docs \
                 --write` from the repository root"
            );
        }
        std::fs::write(docs_dir.join("REFERENCE.md"), &markdown)
            .context("failed to write docs/REFERENCE.md")?;
        std::fs::write(docs_dir.join("reference.json"), &json)
            .context("failed to write docs/reference.json")?;
        eprintln!("wrote docs/REFERENCE.md and docs/reference.json");
        return Ok(());
    }

    match format {
        "json" => println!("{}", serde_json::to_string_pretty(&doc)?),
        _ => print!("{}", render_markdown(&doc)),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Independently recounts clap's own tree (not via `collect_commands`) and axum's route
    /// table, then checks the generated markdown names every one of them by its exact path --
    /// catches both "a command/route silently went missing from the render" and "the counts
    /// drifted apart".
    #[test]
    fn markdown_lists_every_clap_subcommand_and_every_registered_route() {
        let doc = build_reference_doc();
        let markdown = render_markdown(&doc);

        fn count_subcommands(cmd: &clap::Command) -> usize {
            let mut n = 0;
            for sub in cmd.get_subcommands() {
                if sub.get_name() == "help" || sub.is_hide_set() {
                    continue;
                }
                n += 1 + count_subcommands(sub);
            }
            n
        }
        let mut root = crate::app::cli_command();
        root.build();
        let expected_commands = count_subcommands(&root);
        assert_eq!(
            doc.cli.len(),
            expected_commands,
            "build_reference_doc's CLI list should match a fresh walk of the clap tree"
        );
        assert!(expected_commands > 20, "sanity floor: the CLI has ~30 subcommands");
        for cmd in &doc.cli {
            let heading = format!("### `{}`", cmd.path);
            assert!(
                markdown.contains(&heading),
                "markdown is missing a heading for `{}`",
                cmd.path
            );
        }

        let expected_routes = crate::server::routes::route_entries().len();
        assert_eq!(
            doc.api.len(),
            expected_routes,
            "build_reference_doc's API list should match server::routes::route_entries()"
        );
        for route in &doc.api {
            let heading = format!("### `{} {}`", route.method, route.path);
            assert!(
                markdown.contains(&heading),
                "markdown is missing a heading for `{} {}`",
                route.method,
                route.path
            );
        }

        // Every registered tool except the retired names, which are listed once in the
        // old-spellings table instead of documented twice.
        let expected_tools = crate::mcp::AgentWorthMcpServer::tool_router().list_all().len()
            - crate::app::OLD_MCP_TOOL_NAMES.len();
        assert_eq!(doc.mcp.len(), expected_tools);
        for (old, _) in crate::app::OLD_MCP_TOOL_NAMES {
            assert!(
                !doc.mcp.iter().any(|t| t.name == *old),
                "retired tool `{old}` should not be in the reference"
            );
            assert!(
                markdown.contains(&format!("| mcp | `{old}` |")),
                "retired tool `{old}` should appear in the old-spellings table"
            );
        }
        for tool in &doc.mcp {
            assert!(markdown.contains(&format!("### `{}`", tool.name)));
        }
    }

    /// Snapshot of one section's rendering, on hand-built fixtures rather than the live CLI/
    /// API/MCP surfaces -- this should only ever change when someone deliberately changes
    /// `render_markdown`'s output shape, not every time a subcommand is added.
    #[test]
    fn render_markdown_snapshot_of_one_command_one_route_one_tool() {
        let doc = ReferenceDoc {
            version: "9.9.9".to_string(),
            generated_date: "2026-01-01".to_string(),
            global_flags: vec![],
            old_spellings: vec![],
            cli: vec![CliCommandDoc {
                path: "agentworth widget".to_string(),
                about: "Turn a gear.".to_string(),
                args: vec![CliArgDoc {
                    name: "--force".to_string(),
                    positional: false,
                    required: false,
                    help: "Force the turn".to_string(),
                    default: None,
                    possible_values: vec![],
                }],
            }],
            api: vec![ApiRouteDoc {
                method: "GET".to_string(),
                path: "/api/widget".to_string(),
                description: "Fetch the widget.".to_string(),
                query_params: vec![ApiParamDoc {
                    name: "color".to_string(),
                    description: "Filter by color".to_string(),
                }],
            }],
            mcp: vec![McpToolDoc {
                name: "widget_find".to_string(),
                description: "Find a widget.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "color": { "type": "string", "description": "Filter by color" }
                    },
                    "required": ["color"]
                }),
            }],
        };

        let markdown = render_markdown(&doc);

        assert!(markdown.contains("### `agentworth widget`\n\nTurn a gear.\n"));
        assert!(markdown.contains(
            "| Flag | Required | Help | Default | Values |\n\
             |---|---|---|---|---|\n\
             | `--force` | no | Force the turn | - | - |\n"
        ));
        assert!(markdown.contains("### `GET /api/widget`\n\nFetch the widget.\n"));
        assert!(markdown.contains("| `color` | Filter by color |"));
        assert!(markdown.contains("### `widget_find`\n\nFind a widget.\n"));
        assert!(markdown.contains(
            "| Param | Required | Type | Description |\n\
             |---|---|---|---|\n\
             | `color` | yes | string | Filter by color |\n"
        ));
    }
}
