use std::path::PathBuf;
use std::sync::Arc;

use agentworth_adapter_sdk::ScanOptions;
use agentworth_core::{ScanSummary, Scanner};
use agentworth_outcomes::evaluate_trace_outcomes;
use agentworth_storage::{SessionFilter, SessionOrderBy, Storage};
use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use console::style;
use serde_json::json;
use tracing_subscriber::EnvFilter;

#[path = "commands/merge.rs"]
mod merge;
#[path = "commands/watch.rs"]
mod watch;
#[path = "commands/cache_doctor.rs"]
mod cache_doctor;
#[path = "commands/blind_spots.rs"]
mod blind_spots;
#[path = "commands/threat_digest.rs"]
mod threat_digest;
#[path = "commands/autopsy.rs"]
mod autopsy;
#[path = "commands/recall.rs"]
mod recall;
#[path = "commands/bisect.rs"]
mod bisect;
#[path = "commands/pr_blame.rs"]
mod pr_blame;
// Public because the local API's /api/config routes write through the same validation
// and the same file this module owns (see server/routes.rs).
#[path = "commands/config.rs"]
pub mod config;
#[path = "commands/version_info.rs"]
mod version_info;
#[path = "commands/self_test.rs"]
mod self_test;
// Declared here rather than in `commands/mod.rs`, which `lib.rs` glob-re-exports: a public
// `commands::handoff` would collide with the `crate::handoff` module this command renders.
#[path = "commands/handoff.rs"]
mod handoff_command;
// Same collision, same fix: `commands::forgotten` would clash with `crate::forgotten`.
#[path = "commands/forgotten.rs"]
mod forgotten_command;
// Same collision, same fix again: `commands::asks` would clash with `crate::asks`.
#[path = "commands/asks.rs"]
mod asks_command;

#[derive(Parser, Debug)]
#[command(
    name = "agentworth",
    author,
    version,
    about = "Discover, normalize, and understand AI-agent histories locally."
)]
struct Cli {
    #[arg(short, long, global = true, help = "Enable verbose debug logging")]
    verbose: bool,

    #[arg(
        long,
        global = true,
        help = "Custom path for the local SQLite index database"
    )]
    db_path: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        help = "Force text output even if persisted config defaults to JSON (see `archie config`)"
    )]
    no_json: bool,

    #[arg(
        long,
        global = true,
        help = "Disable colour. NO_COLOR in the environment does the same thing"
    )]
    no_color: bool,

    #[arg(
        long,
        global = true,
        help = "No colour and ASCII-only glyphs, at identical column positions"
    )]
    plain: bool,

    /// Nothing typed at all opens the cockpit on a terminal, and prints the overview
    /// anywhere else. See `Action::Tui` and `run_cockpit_command`.
    #[command(subcommand)]
    command: Option<Commands>,
}

/// Every pre-0.1.16 top-level spelling still runs; it is only hidden from `--help` and from
/// the generated reference. Grep this constant to find everything that goes when the grace
/// period ends: the `#[command(hide = true)]` variants below, their arms in `normalize`, and
/// the hidden MCP tool aliases in `apps/cli/src/mcp/server.rs`.
pub const HIDDEN_ALIASES_REMOVED_IN: &str = "v0.1.18";

/// Old top-level command -> the noun-verb spelling that replaced it.
///
/// One table, three readers: the alias test at the bottom of this file walks it, `agentworth
/// docs` prints it into the reference, and a person reading `normalize` can check the two
/// against each other. A hidden variant added to `Commands` without a row here fails
/// `every_hidden_command_has_a_row`.
pub const OLD_CLI_SPELLINGS: &[(&str, &str)] = &[
    ("traces", "session list"),
    ("inspect", "session show"),
    ("export", "session export"),
    ("receipt", "session receipt"),
    ("handoff", "session handoff"),
    ("forgotten", "session forgotten"),
    ("loose-ends", "session loose-ends"),
    ("asks", "session asks"),
    ("cache-doctor", "session cache"),
    ("bisect", "session bisect"),
    ("search", "session search"),
    ("recall", "session recall"),
    ("audit", "session audit"),
    ("blunder", "session blunder"),
    ("autopsy", "session autopsy"),
    ("watch", "session watch"),
    ("blind-spots", "session list --unproven"),
    ("threat-digest", "session risk"),
    ("matrix", "agent list"),
    ("blame", "repo blame"),
    ("pr-blame", "repo pr-blame"),
    ("suspect", "repo suspect"),
    ("blunder-blame", "repo blunder-blame"),
    ("usage", "stats usage"),
];

/// Old MCP tool name -> the tool it became. Both names stay registered and dispatch to one
/// handler (`apps/cli/src/mcp/server.rs`); `archie docs` leaves the old ones out of the
/// generated reference.
pub const OLD_MCP_TOOL_NAMES: &[(&str, &str)] = &[
    ("sessions_find", "session_list"),
    ("session_get", "session_show"),
    ("blame_find", "repo_blame"),
    ("usage_summary", "stats_usage"),
    ("pacing_window", "window_show"),
    ("coverage_stats", "agent_list"),
    ("outcome_rate", "stats_outcomes"),
    ("carry_forward", "session_carry_forward"),
    ("forgotten_context", "session_forgotten"),
    ("suspect_commits", "repo_suspect"),
];

/// `archie completions --help`. The three install lines are clap_complete's own documented
/// ones (crate 4.6.9, verified on docs.rs 2026-09-02), which is why they source the binary
/// rather than a committed file: the crate states that the shell code and the binary must
/// match version for version, so re-source on upgrade instead of checking a script into a
/// dotfile repo.
const COMPLETIONS_LONG_ABOUT: &str = "\
Write a static completion script -- commands, flags and fixed value lists -- to stdout.

Live values (session ids, repositories, models) need the dynamic completer instead, which
the binary answers itself:

  # bash
  echo \"source <(COMPLETE=bash archie)\" >> ~/.bashrc
  # zsh
  echo \"source <(COMPLETE=zsh archie)\" >> ~/.zshrc
  # fish
  echo \"COMPLETE=fish archie | source\" >> ~/.config/fish/completions/archie.fish

Re-source after an upgrade: the shell code and the binary have to be the same version.";

#[derive(Subcommand, Debug, PartialEq)]
enum Commands {
    /// Scan and index agent histories from the local system
    Scan(ScanArgs),

    /// Machine-wide summary statistics. `stats usage` rolls spend up by period; `stats
    /// outcomes` reports the verified-outcome rate
    Stats {
        #[command(subcommand)]
        action: Option<StatsCommand>,

        #[command(flatten)]
        args: StatsArgs,
    },

    /// Everything that acts on sessions: list them, read one, hand one over
    Session {
        #[command(subcommand)]
        action: SessionCommand,
    },

    /// The agent adapters: what each one extracts, and how one is doing on this machine
    Agent {
        #[command(subcommand)]
        action: AgentCommand,
    },

    /// Repositories: what an agent wrote where, and which commits nothing proved
    Repo {
        #[command(subcommand)]
        action: RepoCommand,
    },

    /// The rolling burn-rate window: what is being spent right now
    Window {
        #[command(subcommand)]
        action: WindowCommand,
    },

    /// Start the local API server and interactive explorer UI
    Serve(ServeArgs),

    /// Start the read-only MCP server over stdio, for a coding agent to query this machine's
    /// session index mid-session (see docs/specs/mcp-server.md). Register it once with
    /// `claude mcp add agentworth --scope user -- archie mcp`.
    Mcp,

    /// Check local environment, adapter discoveries, and SQLite database health
    Doctor(DoctorArgs),

    /// Generate CLI, HTTP API, and MCP tool reference documentation from the code itself
    /// (see docs/REFERENCE.md). Nothing here is hand-written prose: the CLI section walks
    /// the clap command tree, the API section walks the axum route table, and the MCP
    /// section walks the rmcp tool router -- so the reference cannot drift from the code.
    Docs(DocsArgs),

    /// Get, set, or list persisted CLI defaults (~/.agentworth/config.toml)
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Print version details: binary version, npm install detection, and a live
    /// check for a newer release
    Version(VersionArgs),

    /// Check for a newer AgentWorth release and show exactly how to get it
    Update(UpdateArgs),

    /// Write a shell completion script to stdout
    #[command(long_about = COMPLETIONS_LONG_ABOUT)]
    Completions(CompletionsArgs),

    /// Merge another local SQLite index database into this index
    Merge(MergeArgs),

    /// Open the cockpit: the same grammar, full screen, with a cursor. A bare `archie`
    /// does this too; `archie tui` is the explicit spelling. Off a terminal, under
    /// `--plain`, under `TERM=dumb`, or with JSON output, both print the overview and
    /// exit 0 instead of taking the terminal over
    Tui(TuiArgs),

    // -------------------------------------------------------------------------
    // Hidden aliases for the pre-0.1.16 spellings. See HIDDEN_ALIASES_REMOVED_IN.
    // Each one carries the same args struct as the noun-verb spelling it maps to,
    // so `normalize` below is the only place that knows both names.
    // -------------------------------------------------------------------------
    #[command(hide = true)]
    Traces(SessionListArgs),

    #[command(hide = true)]
    Inspect(SessionShowArgs),

    #[command(hide = true)]
    Export(ExportArgs),

    #[command(hide = true)]
    Receipt(ReceiptArgs),

    #[command(hide = true)]
    Handoff(HandoffArgs),

    #[command(hide = true)]
    Forgotten(ForgottenArgs),

    #[command(name = "loose-ends", hide = true)]
    LooseEnds(LooseEndsArgs),

    #[command(hide = true)]
    Asks(AsksArgs),

    #[command(name = "cache-doctor", hide = true)]
    CacheDoctor(SessionRefArgs),

    #[command(hide = true)]
    Bisect(SessionRefArgs),

    #[command(hide = true)]
    Search(SearchArgs),

    #[command(hide = true)]
    Recall(RecallArgs),

    #[command(hide = true)]
    Audit(AuditArgs),

    #[command(hide = true)]
    Blunder(BlunderArgs),

    #[command(hide = true)]
    Autopsy(AutopsyArgs),

    #[command(hide = true)]
    Watch(WatchArgs),

    #[command(name = "blind-spots", hide = true)]
    BlindSpots(BlindSpotsArgs),

    #[command(name = "threat-digest", hide = true)]
    ThreatDigest(RiskArgs),

    #[command(hide = true)]
    Matrix(AgentListArgs),

    #[command(hide = true)]
    Blame(BlameArgs),

    #[command(name = "pr-blame", hide = true)]
    PrBlame(PrBlameArgs),

    #[command(hide = true)]
    Suspect(SuspectCliArgs),

    #[command(name = "blunder-blame", hide = true)]
    BlunderBlame(BlunderBlameArgs),

    #[command(hide = true)]
    Usage(UsageArgs),
}

#[derive(Subcommand, Debug, PartialEq)]
enum SessionCommand {
    /// List indexed sessions with optional filtering
    List(SessionListArgs),

    /// Read one session in detail, with its timeline
    Show(SessionShowArgs),

    /// Export one session safely, in JSON or ATIF format
    Export(ExportArgs),

    /// Render one session's Flight Receipt, as ANSI or SVG
    Receipt(ReceiptArgs),

    /// Hand a session over: what it promised and dropped, decided, changed, ran, and proved
    Handoff(HandoffArgs),

    /// What compaction dropped: decisions this session made and its own summaries did not keep
    Forgotten(ForgottenArgs),

    /// The handoff's loose-ends section alone: what a session said it would do and did not
    #[command(name = "loose-ends")]
    LooseEnds(LooseEndsArgs),

    /// The questions you asked and where their answers are -- built so you never have to
    /// re-scroll or re-ask because the answer landed several messages later
    Asks(AsksArgs),

    /// Diagnose turn-by-turn prompt caching dynamics and identify cache drop root causes
    Cache(SessionRefArgs),

    /// Pinpoint the exact turning point where a session's trajectory turned negative
    Bisect(SessionRefArgs),

    /// Semantic vector search across indexed trajectory turns with ASCII thermal receipts
    Search(SearchArgs),

    /// Semantically recall past solutions joined with outcome validation and cost
    Recall(RecallArgs),

    /// Safety and threat audit detecting forbidden commands, leaked variables, sweeps, and
    /// fake claims, machine-wide over every session
    Audit(AuditArgs),

    /// Discover top agent blunders, render thermal receipts, and export to the Hall of Blunders
    Blunder(BlunderArgs),

    /// Surface recurring human correction and steering phrases across all sessions
    Autopsy(AutopsyArgs),

    /// Watch active session transcripts and detect doom loops or file edit thrashing
    Watch(WatchArgs),

    /// Rank indexed sessions by real secret/credential exposure risk, by category and severity
    Risk(RiskArgs),
}

#[derive(Subcommand, Debug, PartialEq)]
enum AgentCommand {
    /// Extraction capabilities and coverage across every registered adapter
    List(AgentListArgs),

    /// One adapter in detail: what it extracts, whether it is present here, and what it
    /// has actually put in the index
    Show(AgentShowArgs),
}

#[derive(Subcommand, Debug, PartialEq)]
enum RepoCommand {
    /// The repositories and workspaces this index holds, by session count
    List(RepoListArgs),

    /// Trace file modifications back to the session, model, and prompt that authored them
    Blame(BlameArgs),

    /// Annotate changed PR files with AI agent authoring provenance and outcome validation
    #[command(name = "pr-blame")]
    PrBlame(PrBlameArgs),

    /// List commits on this branch whose authoring session never proved anything, so you
    /// know where to look twice before pushing. Prints a list and a prompt, never a patch
    Suspect(SuspectCliArgs),

    /// Bridge AI Code Blame with the Hall of Blunders: trace a recorded blunder forward
    /// to the exact files it blame-attributes to, or a file's blame history backward to
    /// any recorded blunders in the sessions blamed for it
    #[command(name = "blunder-blame")]
    BlunderBlame(BlunderBlameArgs),
}

#[derive(Subcommand, Debug, PartialEq)]
enum WindowCommand {
    /// The current rolling window: burn rate, active models, quota headroom
    Show(WindowShowArgs),

    /// The recent rolling windows, newest first
    List(WindowListArgs),
}

#[derive(Subcommand, Debug, PartialEq)]
enum StatsCommand {
    /// Deep usage, pacing, and token expenditure rollups
    Usage(UsageArgs),

    /// Verified-outcome rate by model, adapter, or repo: of the sessions that claimed done,
    /// what share left evidence a test, build, commit or CI run can be pointed at
    Outcomes(OutcomesArgs),
}

// -----------------------------------------------------------------------------
// Argument structs. Shared by the noun-verb spelling and its hidden alias, so the
// two can never drift apart on a flag.
// -----------------------------------------------------------------------------

#[derive(clap::Args, Debug, PartialEq)]
struct ScanArgs {
    /// Optional specific paths or directories to scan
    #[arg(value_name = "PATHS")]
    paths: Vec<PathBuf>,

    /// Force rescanning and re-indexing of unchanged source files
    #[arg(short, long)]
    force: bool,

    /// Keep storing/pruning near-empty stub sessions instead of filtering them out
    #[arg(long)]
    include_stubs: bool,

    /// Output scan results as formatted JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct StatsArgs {
    /// Output summary statistics as formatted JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq, Default)]
struct SessionListArgs {
    /// Maximum number of sessions to display (default 20, or persisted `config limit`)
    #[arg(short, long)]
    limit: Option<usize>,

    /// Filter by adapter name (e.g. claude_code, codex, gemini, opencode)
    #[arg(short, long, add = clap_complete::engine::ArgValueCandidates::new(crate::completions::adapter_candidates))]
    adapter: Option<String>,

    /// Filter by model substring (e.g. sonnet, gpt-4o, gemini-2.5)
    #[arg(short, long, add = clap_complete::engine::ArgValueCandidates::new(crate::completions::model_candidates))]
    model: Option<String>,

    /// Include 1-event session stubs in the listing
    #[arg(long)]
    all_stubs: bool,

    /// Only sessions whose completion claims were never independently corroborated by
    /// tests or CI -- the blind spots
    #[arg(long, conflicts_with_all = ["adapter", "model", "all_stubs"])]
    unproven: bool,

    /// Output as formatted JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct BlindSpotsArgs {
    /// Maximum number of sessions to list (default 20, or persisted `config limit`)
    #[arg(short, long)]
    limit: Option<usize>,

    /// Output results as JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct SessionShowArgs {
    /// The session to read, by full ID or a unique prefix. With nothing given on a TTY, a
    /// picker lists the newest sessions; elsewhere, pass an ID or `--last`
    #[arg(value_name = "SESSION_ID", add = clap_complete::engine::ArgValueCandidates::new(crate::completions::session_candidates))]
    session_id: Option<String>,

    /// The newest session for this directory's repository. The default when no ID is given
    /// and stdout is not a TTY
    #[arg(long)]
    last: bool,

    /// Alias of `--last`
    #[arg(long)]
    current: bool,

    /// Output raw trace structure as formatted JSON
    #[arg(long)]
    json: bool,
}

/// The plainest session-taking shape: one session and an output format. `session cache` and
/// `session bisect` both take exactly this.
#[derive(clap::Args, Debug, PartialEq)]
struct SessionRefArgs {
    /// The session to act on, by full ID or a unique prefix. With nothing given on a TTY, a
    /// picker lists the newest sessions; elsewhere, pass an ID or `--last`
    #[arg(value_name = "SESSION_ID", add = clap_complete::engine::ArgValueCandidates::new(crate::completions::session_candidates))]
    session_id: Option<String>,

    /// The newest session for this directory's repository. The default when no ID is given
    /// and stdout is not a TTY
    #[arg(long)]
    last: bool,

    /// Alias of `--last`
    #[arg(long)]
    current: bool,

    /// Output findings as formatted JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct ExportArgs {
    /// The session to export, by full ID or a unique prefix. With nothing given on a TTY, a
    /// picker lists the newest sessions; elsewhere, pass an ID or `--last`
    #[arg(value_name = "SESSION_ID", add = clap_complete::engine::ArgValueCandidates::new(crate::completions::session_candidates))]
    session_id: Option<String>,

    /// Export the newest session for this directory's repository. The default when
    /// no ID is given and stdout is not a TTY
    #[arg(long)]
    last: bool,

    /// Alias of `--last`
    #[arg(long)]
    current: bool,

    /// Apply redaction to mask secrets, API keys, tokens, emails, and home paths
    #[arg(short, long)]
    redact: bool,

    /// Export format: json (default), atif, receipt, or svg
    #[arg(short, long, default_value = "json", value_parser = ["json", "atif", "receipt", "terminal", "ansi", "svg"])]
    format: String,

    /// Optional file path to write export output to (defaults to stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[derive(clap::Args, Debug, PartialEq)]
struct ReceiptArgs {
    /// The session to render a flight receipt for, by full ID or a unique prefix.
    /// With nothing given on a TTY, a picker lists the newest sessions; elsewhere,
    /// pass an ID or `--last`
    #[arg(value_name = "SESSION_ID", add = clap_complete::engine::ArgValueCandidates::new(crate::completions::session_candidates))]
    session_id: Option<String>,

    /// Render the newest session for this directory's repository. The default when no ID
    /// is given and stdout is not a TTY
    #[arg(long)]
    last: bool,

    /// Alias of `--last`
    #[arg(long)]
    current: bool,

    /// Output format: terminal (default), ansi, svg, receipt, or json
    #[arg(short, long, default_value = "terminal", value_parser = ["terminal", "ansi", "svg", "receipt", "json"])]
    format: String,

    /// Optional file path to write receipt or SVG output to (defaults to stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[derive(clap::Args, Debug, PartialEq)]
struct SearchArgs {
    /// Search query (natural language or code snippet)
    query: String,

    /// Maximum number of results to return (default 10, or persisted `config limit`)
    #[arg(short, long)]
    limit: Option<usize>,

    /// Minimum similarity score threshold (0.0 to 1.0)
    #[arg(long, default_value_t = 0.0)]
    min_score: f32,

    /// Filter by chunk kind (summary, error_recovery, tool_invocation, apology_panic, code_lineage)
    #[arg(short, long, add = clap_complete::engine::ArgValueCandidates::new(crate::completions::chunk_kind_candidates))]
    kind: Option<String>,

    /// Output results as JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct RecallArgs {
    /// Search query to match against previous trajectories
    query: String,

    /// Maximum number of results to return (default 5, or persisted `config limit`)
    #[arg(short, long)]
    limit: Option<usize>,

    /// Minimum similarity score threshold (0.0 to 1.0)
    #[arg(long, default_value_t = 0.0)]
    min_score: f32,

    /// Output results as JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct AuditArgs {
    /// Restrict audit to safety and threat vectors only
    #[arg(long)]
    safety: bool,

    /// Output audit results as formatted JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct BlunderArgs {
    /// Number of top blunder exhibits to retrieve and display (default: 5)
    #[arg(short, long, default_value_t = 5)]
    top: usize,

    /// Submit redacted blunder receipts to the public Hall of Blunders at stfuopus.lol
    #[arg(short, long)]
    submit: bool,

    /// Output blunder exhibits as formatted JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct AutopsyArgs {
    /// Minimum number of occurrences across sessions to report (default: 2)
    #[arg(short, long, default_value_t = 2)]
    min_occurrences: usize,

    /// Output results as JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct WatchArgs {
    /// Polling interval in seconds (default: 3)
    #[arg(short, long, default_value_t = 3)]
    interval_secs: u64,

    /// Run a single poll check and exit immediately
    #[arg(long)]
    poll_once: bool,

    /// Output findings as formatted JSON
    #[arg(long)]
    json: bool,

    /// Custom path directories to monitor
    #[arg(short, long)]
    paths: Vec<PathBuf>,
}

#[derive(clap::Args, Debug, PartialEq)]
struct RiskArgs {
    /// Maximum number of sessions to show in the report (default 20, or persisted `config
    /// limit`) -- every indexed session is still scanned; this only trims the displayed list
    #[arg(short, long)]
    limit: Option<usize>,

    /// Only include sessions whose worst finding is at least this severity
    #[arg(long, default_value = "low", value_parser = ["low", "medium", "high", "critical"])]
    min_severity: String,

    /// Output results as JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct ServeArgs {
    /// Port to bind the server to
    #[arg(short, long, default_value_t = crate::DEFAULT_PORT)]
    port: u16,

    /// Automatically open the Web UI in the default browser
    #[arg(long)]
    open: bool,

    /// Optional path to custom web frontend dist directory
    #[arg(long)]
    dist: Option<PathBuf>,
}

#[derive(clap::Args, Debug, PartialEq)]
struct DoctorArgs {
    /// Output diagnostic report as formatted JSON
    #[arg(long)]
    json: bool,

    /// Run the real release workflow end to end -- scan, stats, usage, sessions, show,
    /// handoff, forgotten, and an MCP round trip -- against the real index
    /// on this machine, with no network, and report pass/fail/slow and timing for
    /// each step. Exits non-zero if any step fails
    #[arg(long)]
    self_test: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct VersionArgs {
    /// Skip the live GitHub-releases update check (fully local, no network call)
    #[arg(long)]
    offline: bool,

    /// Output as formatted JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct UpdateArgs {
    /// Skip the live GitHub-releases check and just show install-method guidance
    #[arg(long)]
    offline: bool,

    /// Output as formatted JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct MergeArgs {
    /// Path to the source SQLite database file to merge from
    source_db: PathBuf,

    /// Output results as JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq, Default)]
struct TuiArgs {
    /// Print the overview as JSON and exit, without opening anything
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct DocsArgs {
    /// Output format when printing to stdout (ignored with --write, which always
    /// writes both forms)
    #[arg(long, default_value = "markdown", value_parser = ["markdown", "json"])]
    format: String,

    /// Write docs/REFERENCE.md and docs/reference.json (relative to the current
    /// directory, which must be the repository root) instead of printing to stdout
    #[arg(long)]
    write: bool,
}

/// `archie completions <shell>`. Static scripts only; the live-value completions
/// (session ids, repos, models) come from `COMPLETE=<shell> archie` instead -- see this
/// command's long help.
#[derive(clap::Args, Debug, PartialEq)]
struct CompletionsArgs {
    /// Shell to generate a completion script for
    #[arg(value_name = "SHELL")]
    shell: clap_complete::aot::Shell,
}

#[derive(clap::Args, Debug, PartialEq)]
struct HandoffArgs {
    /// Session to hand over, by full ID or a unique prefix. With nothing given on a
    /// TTY, a picker lists the newest sessions; elsewhere, pass an ID or `--last`
    #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::completions::session_candidates))]
    session_id: Option<String>,

    /// Hand over the newest session for this repository. The default when no ID is
    /// given and stdout is not a TTY
    #[arg(long)]
    last: bool,

    /// Alias of `--last`
    #[arg(long)]
    current: bool,

    /// Mask secrets, paths, and this session's own repository name before printing
    #[arg(short, long)]
    redact: bool,

    /// Emit the same markdown the `session_handoff` MCP tool returns
    #[arg(long)]
    markdown: bool,

    /// Line budget for `--markdown` (default 60, ceiling 120)
    #[arg(long)]
    max_lines: Option<usize>,

    /// Output the structured handoff as JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct ForgottenArgs {
    /// Session to diff, by full ID or a unique prefix. With nothing given on a TTY, a
    /// picker lists the newest sessions; elsewhere, defaults to the newest session
    /// indexed for this directory's repository (same as `--last`)
    #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::completions::session_candidates))]
    session_id: Option<String>,

    /// Diff the newest session for this directory's repository. The default when no
    /// ID is given and stdout is not a TTY
    #[arg(long)]
    last: bool,

    /// Alias of `--last`
    #[arg(long)]
    current: bool,

    /// One 1-based compaction round. Defaults to every round.
    #[arg(long)]
    round: Option<u32>,

    /// Any of decision, rejected, reason. Repeatable. Defaults to all three.
    #[arg(long = "class", value_name = "CLASS", add = clap_complete::engine::ArgValueCandidates::new(crate::completions::forgotten_class_candidates))]
    classes: Vec<String>,

    /// How many statements to return, newest first (default 20, ceiling 200)
    #[arg(long)]
    limit: Option<usize>,

    /// Mask secrets, paths, and this session's own repository name before printing
    #[arg(short, long)]
    redact: bool,

    /// Output the structured diff as JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct AsksArgs {
    /// Session to index, by full ID, a unique prefix, or a raw JSONL file path (parsed
    /// directly if it isn't an indexed session). With nothing given on a TTY, a picker
    /// lists the newest sessions; elsewhere, pass an ID or `--last`.
    #[arg(value_name = "SESSION_ID", conflicts_with_all = ["session", "current", "last"],
          add = clap_complete::engine::ArgValueCandidates::new(crate::completions::session_candidates))]
    session_id: Option<String>,

    /// The same session, named with a flag. Kept because `archie session asks --session <id>`
    /// is the spelling that shipped in #97.
    #[arg(long, conflicts_with_all = ["current", "last"],
          add = clap_complete::engine::ArgValueCandidates::new(crate::completions::session_candidates))]
    session: Option<String>,

    /// Resolve the newest session for this directory's repository, falling back to the
    /// newest session anywhere.
    #[arg(long)]
    last: bool,

    /// Alias of `--last`.
    #[arg(long)]
    current: bool,

    /// Only questions asked at or after this time: RFC 3339, `YYYY-MM-DD`, or a relative
    /// duration like `2h`, `30m`, `1d`, `3w`.
    #[arg(long)]
    since: Option<String>,

    /// Only questions that are not `answered` -- still open, or flagged back to you.
    #[arg(long)]
    unanswered: bool,

    /// Output the structured index as JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct LooseEndsArgs {
    /// Session to check, by full ID or a unique prefix. With nothing given on a TTY, a
    /// picker lists the newest sessions; elsewhere, pass an ID or `--last`
    #[arg(add = clap_complete::engine::ArgValueCandidates::new(crate::completions::session_candidates))]
    session_id: Option<String>,

    /// Check the newest session for this repository. The default when no ID is given and
    /// stdout is not a TTY
    #[arg(long)]
    last: bool,

    /// Alias of `--last`
    #[arg(long)]
    current: bool,

    /// Mask secrets, paths, and this session's own repository name before printing
    #[arg(short, long)]
    redact: bool,

    /// Print the copyable prompt to hand to an agent that has the repository open
    #[arg(long)]
    prompt: bool,

    /// Output the loose ends as JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct AgentListArgs {
    /// Output the coverage matrix as formatted JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct AgentShowArgs {
    /// Adapter name (e.g. claude_code, codex, gemini, opencode)
    #[arg(value_name = "ADAPTER", add = clap_complete::engine::ArgValueCandidates::new(crate::completions::adapter_candidates))]
    adapter: String,

    /// Output as formatted JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct RepoListArgs {
    /// Maximum number of repositories to display (default 20, or persisted `config limit`)
    #[arg(short, long)]
    limit: Option<usize>,

    /// Output as formatted JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct BlameArgs {
    /// Target file path or pattern to search
    file_path: String,

    /// Output results as JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct PrBlameArgs {
    /// List of files to check (if omitted, infers from git diff)
    files: Vec<String>,

    /// Output results as JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct SuspectCliArgs {
    /// Path to a git checkout. Defaults to the current directory
    #[arg(long)]
    repo: Option<PathBuf>,

    /// A date (RFC 3339 or YYYY-MM-DD) or a git ref to measure from. Defaults to the
    /// branch's upstream, then origin/main
    #[arg(long)]
    since: Option<String>,

    /// Branch to walk. Defaults to HEAD
    #[arg(long)]
    branch: Option<String>,

    /// Ref to diff against, if you want to name it separately from --since
    #[arg(long)]
    base: Option<String>,

    /// How long before a commit a session's file touch still counts as authoring it
    #[arg(long)]
    window_hours: Option<i64>,

    /// Print a ready-to-install pre-push hook and exit. The hook never blocks a push
    #[arg(long)]
    hook: bool,

    /// Print only the copyable prompt, and only when something is suspect. What the
    /// hook runs
    #[arg(long)]
    quiet: bool,

    /// Output the full report as JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct BlunderBlameArgs {
    /// Blame -> blunder direction: file path or pattern. Checks every session AI
    /// Code Blame attributes this file to for a recorded blunder
    #[arg(long, conflicts_with = "session")]
    file: Option<String>,

    /// Blunder -> blame direction: one specific session ID, by full ID or a unique
    /// prefix. Resolves it to the files AI Code Blame attributes to that session
    #[arg(long, conflicts_with = "file",
          add = clap_complete::engine::ArgValueCandidates::new(crate::completions::session_candidates))]
    session: Option<String>,

    /// Blunder -> blame direction for the newest session in this repository, same as
    /// `--session` with that session's ID
    #[arg(long, conflicts_with = "file")]
    last: bool,

    /// Alias of `--last`
    #[arg(long, conflicts_with = "file")]
    current: bool,

    /// In default mode (no --file or --session), number of top blunders to bridge
    #[arg(short, long, default_value_t = 5)]
    top: usize,

    /// Output results as JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct UsageArgs {
    /// Rollup period: day, week, month, year, or all -- `all` is one row per group across
    /// all time, with no period column. Single-letter aliases d/w/m/y also work. Default
    /// day, or persisted `config period`.
    #[arg(short, long, value_parser = parse_period_arg)]
    period: Option<String>,

    /// Show the rolling pacing window instead of the rollup. `archie window show` is the
    /// spelling that survives; this flag is kept for scripts written against `usage`.
    #[arg(long, hide = true)]
    pacing: bool,

    /// Pacing window duration in hours
    #[arg(long, default_value_t = 5)]
    hours: i64,

    /// Alert and highlight if window spend exceeds this threshold in USD
    #[arg(long)]
    alert_above: Option<f64>,

    /// Maximum number of periods to keep (default 30 for day, 26 for week, 24 for month,
    /// unbounded for year) -- or, under `--period all`, the number of top groups by
    /// spend to keep (default 20, or persisted `config limit`). Counts periods, not rows:
    /// a day with two adapters is one period, not two.
    #[arg(short, long)]
    limit: Option<usize>,

    /// Group the rollup by adapter (default; most informative when nearly every session
    /// shares one adapter, e.g. Claude Code), model (usually the useful one), or repo
    #[arg(long, default_value = "adapter", value_parser = ["adapter", "model", "repo"])]
    by: String,

    /// Only include sessions started at or after this time: an absolute date
    /// (`2026-08-01` or RFC 3339), or a relative shorthand (`1d`, `7d`, `2w`, `3m`)
    #[arg(long)]
    since: Option<String>,

    /// Output usage data as JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct OutcomesArgs {
    /// Group the rate by model (default), adapter, or repo
    #[arg(long, default_value = "model", value_parser = ["model", "adapter", "repo"])]
    by: String,

    /// Suppress groups with fewer than this many sessions (default 20)
    #[arg(long)]
    min_n: Option<usize>,

    /// Only sessions started at or after this time: an absolute date (`2026-08-01` or
    /// RFC 3339), or a relative shorthand (`1d`, `7d`, `2w`, `3m`)
    #[arg(long)]
    since: Option<String>,

    /// Only sessions started before this time, same formats as `--since`
    #[arg(long)]
    until: Option<String>,

    /// Include 1-event session stubs in the population
    #[arg(long)]
    include_stubs: bool,

    /// Output as formatted JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct WindowShowArgs {
    /// Window duration in hours
    #[arg(long, default_value_t = 5)]
    hours: i64,

    /// Alert and highlight if window spend exceeds this threshold in USD
    #[arg(long)]
    alert_above: Option<f64>,

    /// Output as formatted JSON
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args, Debug, PartialEq)]
struct WindowListArgs {
    /// Window duration in hours
    #[arg(long, default_value_t = 5)]
    hours: i64,

    /// How many consecutive windows to show, newest first (default 6)
    #[arg(short, long)]
    limit: Option<usize>,

    /// Output as formatted JSON
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand, Debug, PartialEq)]
enum ConfigAction {
    /// List every persisted config key and its current value
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Print the persisted value for one config key
    Get {
        /// Config key: json, limit, period, archie.accessory, or archie.colourway
        key: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Persist a default value for one config key
    Set {
        /// Config key: json, limit, period, archie.accessory, or archie.colourway
        key: String,

        /// Value to store (json: true/false, limit: a number, period: day/week/month,
        /// archie.accessory: lamp/goggles/none, archie.colourway: C1/C2/C3/C4)
        value: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// What `run()` actually dispatches on: the noun-verb grammar with every hidden alias
/// already folded into it. Two spellings of the same command produce the same `Action`,
/// which is what makes the alias table testable rather than a promise (see the tests at
/// the bottom of this file).
#[derive(Debug, PartialEq)]
enum Action {
    Session(SessionCommand),
    Agent(AgentCommand),
    Repo(RepoCommand),
    Window(WindowCommand),
    Stats {
        action: Option<StatsCommand>,
        args: StatsArgs,
    },
    Scan(ScanArgs),
    Serve(ServeArgs),
    Mcp,
    Doctor(DoctorArgs),
    Docs(DocsArgs),
    Config(ConfigAction),
    Version(VersionArgs),
    Update(UpdateArgs),
    Completions(CompletionsArgs),
    Merge(MergeArgs),
    Tui(TuiArgs),
}

/// The only place both spellings of a command are named. Everything below the divider is
/// a pre-0.1.16 alias and goes when `HIDDEN_ALIASES_REMOVED_IN` arrives.
fn normalize(command: Commands) -> Action {
    match command {
        Commands::Session { action } => Action::Session(action),
        Commands::Agent { action } => Action::Agent(action),
        Commands::Repo { action } => Action::Repo(action),
        Commands::Window { action } => Action::Window(action),
        Commands::Stats { action, args } => Action::Stats { action, args },
        Commands::Scan(a) => Action::Scan(a),
        Commands::Serve(a) => Action::Serve(a),
        Commands::Mcp => Action::Mcp,
        Commands::Doctor(a) => Action::Doctor(a),
        Commands::Docs(a) => Action::Docs(a),
        Commands::Config { action } => Action::Config(action),
        Commands::Version(a) => Action::Version(a),
        Commands::Update(a) => Action::Update(a),
        Commands::Completions(a) => Action::Completions(a),
        Commands::Merge(a) => Action::Merge(a),
        Commands::Tui(a) => Action::Tui(a),

        // ---- hidden aliases ----
        Commands::Traces(a) => Action::Session(SessionCommand::List(a)),
        Commands::Inspect(a) => Action::Session(SessionCommand::Show(a)),
        Commands::Export(a) => Action::Session(SessionCommand::Export(a)),
        Commands::Receipt(a) => Action::Session(SessionCommand::Receipt(a)),
        Commands::Handoff(a) => Action::Session(SessionCommand::Handoff(a)),
        Commands::Forgotten(a) => Action::Session(SessionCommand::Forgotten(a)),
        Commands::LooseEnds(a) => Action::Session(SessionCommand::LooseEnds(a)),
        Commands::Asks(a) => Action::Session(SessionCommand::Asks(a)),
        Commands::CacheDoctor(a) => Action::Session(SessionCommand::Cache(a)),
        Commands::Bisect(a) => Action::Session(SessionCommand::Bisect(a)),
        Commands::Search(a) => Action::Session(SessionCommand::Search(a)),
        Commands::Recall(a) => Action::Session(SessionCommand::Recall(a)),
        Commands::Audit(a) => Action::Session(SessionCommand::Audit(a)),
        Commands::Blunder(a) => Action::Session(SessionCommand::Blunder(a)),
        Commands::Autopsy(a) => Action::Session(SessionCommand::Autopsy(a)),
        Commands::Watch(a) => Action::Session(SessionCommand::Watch(a)),
        Commands::ThreatDigest(a) => Action::Session(SessionCommand::Risk(a)),
        // `blind-spots` was never a listing of its own -- it is `session list` with one
        // filter on, which is why the spec turned it into a flag.
        Commands::BlindSpots(a) => Action::Session(SessionCommand::List(SessionListArgs {
            limit: a.limit,
            unproven: true,
            json: a.json,
            ..SessionListArgs::default()
        })),
        Commands::Matrix(a) => Action::Agent(AgentCommand::List(a)),
        Commands::Blame(a) => Action::Repo(RepoCommand::Blame(a)),
        Commands::PrBlame(a) => Action::Repo(RepoCommand::PrBlame(a)),
        Commands::Suspect(a) => Action::Repo(RepoCommand::Suspect(a)),
        Commands::BlunderBlame(a) => Action::Repo(RepoCommand::BlunderBlame(a)),
        Commands::Usage(a) => Action::Stats {
            action: Some(StatsCommand::Usage(a)),
            args: StatsArgs { json: false },
        },
    }
}

/// The name this process was actually invoked as.
///
/// One executable answers to `archie`, `agwt` and `agentworth`, and clap only knows the
/// root command's name (`agentworth`). Every completion script has to name the binary the
/// user typed: `source <(COMPLETE=zsh archie)` emitting `#compdef agentworth` registers
/// completions against a name that is not on the command line, so Tab does nothing.
fn invoked_binary_name() -> String {
    std::env::args_os()
        .next()
        .map(PathBuf::from)
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "archie".to_string())
}

pub fn run() -> Result<()> {
    // Dynamic shell completion runs before argv is parsed: with COMPLETE=<shell> set, the
    // binary answers the shell's completion request and exits, and with it unset this is a
    // no-op. Nothing here touches the index -- the per-argument completers in
    // `crate::completions` open their own read-only connection only when a value is asked for.
    clap_complete::env::CompleteEnv::with_factory(Cli::command)
        .bin(invoked_binary_name())
        .complete();

    let cli = Cli::parse();

    // Initialize tracing
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("warn")
    };
    // `archie mcp` speaks JSON-RPC over stdout -- any stray tracing line there would
    // corrupt the protocol stream for whatever client spawned this process (the same reason
    // every rmcp stdio example logs to stderr). Every other subcommand keeps the existing
    // stdout default.
    if matches!(cli.command, Some(Commands::Mcp)) {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .with_writer(std::io::stderr)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .init();
    }

    // Persisted user defaults (`~/.agentworth/config.toml`, see `archie config`). A
    // corrupt config file should not brick every command, so fall back to built-in
    // defaults with a warning rather than erroring out.
    let persisted_config = config::load_config().unwrap_or_else(|e| {
        eprintln!(
            "warning: failed to load persisted config ({}), using built-in defaults",
            e
        );
        Default::default()
    });
    let no_json = cli.no_json;
    let resolve_json = |flag: bool| config::resolve_json(flag, no_json, persisted_config.json);
    let ui = crate::ui::Ui::detect(cli.no_color, cli.plain);
    // `Ui` resolves --plain/--no-color/NO_COLOR once above; every raw `console::style(...)`
    // call across the older commands (the ones not yet rendering through `ui::views`) builds
    // its own colour decision independently and never saw those flags, so `archie session blunder
    // --plain` still printed ANSI codes. Forcing the global switch here to the same verdict
    // makes every `style()` call in the binary agree with `Ui`, immediately, without waiting
    // on each command's own redesign.
    console::set_colors_enabled(ui.color() != crate::ui::ColorMode::None);
    console::set_colors_enabled_stderr(ui.color() != crate::ui::ColorMode::None);

    // A bare `archie` is `archie tui`: the cockpit on a terminal, the overview anywhere
    // else. Spec section 3, and the answer to its open question 4.
    let action = match cli.command {
        Some(command) => normalize(command),
        None => Action::Tui(TuiArgs::default()),
    };

    match action {
        Action::Tui(a) => {
            run_cockpit_command(resolve_json(a.json), cli.db_path, &ui)?;
        }
        Action::Scan(a) => {
            run_scan_command(a.paths, a.force, a.include_stubs, resolve_json(a.json), cli.db_path, &ui)?;
        }
        Action::Stats { action: None, args } => {
            run_stats_command(resolve_json(args.json), cli.db_path, &ui)?;
        }
        Action::Stats { action: Some(StatsCommand::Usage(a)), .. } => {
            let period = config::resolve_period(a.period, persisted_config.period.clone(), "day")?;
            // Periods, not rows (see `UsageReport`'s doc comment): each period kind gets its
            // own sane default, and "year" has no cap because there are rarely more than a
            // handful of them to begin with. `--period all` has no period axis, so its
            // `limit` caps *groups* instead, at the same 20 every other command defaults to.
            let builtin_limit_default = match period.as_str() {
                "day" => 30,
                "week" => 26,
                "month" => 24,
                "year" => usize::MAX,
                _ => 20,
            };
            let limit = config::resolve_limit(a.limit, persisted_config.limit, builtin_limit_default);
            let since = a.since.as_deref().map(parse_since_arg).transpose()?;
            run_usage_command(UsageCommandArgs {
                period: &period,
                pacing: a.pacing,
                hours: a.hours,
                alert_above: a.alert_above,
                limit,
                by: &a.by,
                since,
                json: resolve_json(a.json),
                db_path: cli.db_path,
                ui: &ui,
            })?;
        }
        Action::Stats { action: Some(StatsCommand::Outcomes(a)), .. } => {
            let since = a.since.as_deref().map(parse_since_arg).transpose()?;
            let until = a.until.as_deref().map(parse_since_arg).transpose()?;
            run_stats_outcomes_command(
                &a.by,
                a.min_n,
                since,
                until,
                a.include_stubs,
                resolve_json(a.json),
                cli.db_path,
                &ui,
            )?;
        }
        Action::Window(WindowCommand::Show(a)) => {
            run_usage_command(UsageCommandArgs {
                period: "day",
                pacing: true,
                hours: a.hours,
                alert_above: a.alert_above,
                limit: 30,
                by: "adapter",
                since: None,
                json: resolve_json(a.json),
                db_path: cli.db_path,
                ui: &ui,
            })?;
        }
        Action::Window(WindowCommand::List(a)) => {
            let limit = config::resolve_limit(a.limit, persisted_config.limit, 6);
            run_window_list_command(a.hours, limit, resolve_json(a.json), cli.db_path, &ui)?;
        }
        Action::Session(SessionCommand::List(a)) => {
            let limit = config::resolve_limit(a.limit, persisted_config.limit, 20);
            if a.unproven {
                blind_spots::run_blind_spots_command(limit, resolve_json(a.json), cli.db_path, &ui)?;
            } else {
                run_traces_command(
                    limit,
                    a.adapter,
                    a.model,
                    a.all_stubs,
                    resolve_json(a.json),
                    cli.db_path,
                    &ui,
                )?;
            }
        }
        Action::Session(SessionCommand::Show(a)) => {
            let json = resolve_json(a.json);
            run_inspect_command(a.session_id, a.last, a.current, json, cli.db_path.clone(), &ui)?;
        }
        Action::Session(SessionCommand::Export(a)) => {
            run_export_command(
                a.session_id,
                a.last,
                a.current,
                a.redact,
                &a.format,
                a.output.as_deref(),
                cli.db_path,
                &ui,
            )?;
        }
        Action::Session(SessionCommand::Receipt(a)) => {
            crate::run_receipt_command(
                a.session_id,
                a.last,
                a.current,
                &a.format,
                a.output,
                cli.db_path,
                &ui,
            )?;
        }
        Action::Session(SessionCommand::Search(a)) => {
            let limit = config::resolve_limit(a.limit, persisted_config.limit, 10);
            crate::run_search_command(
                &a.query,
                limit,
                a.min_score,
                a.kind,
                resolve_json(a.json),
                cli.db_path,
            )?;
        }
        Action::Session(SessionCommand::Audit(a)) => {
            crate::run_audit_command(a.safety, resolve_json(a.json), cli.db_path, &ui)?;
        }
        Action::Session(SessionCommand::Blunder(a)) => {
            crate::run_blunder_command(a.top, a.submit, resolve_json(a.json), cli.db_path, &ui)?;
        }
        Action::Session(SessionCommand::Handoff(a)) => {
            handoff_command::run_handoff_command(
                a.session_id,
                a.last,
                a.current,
                a.redact,
                a.max_lines,
                a.markdown,
                resolve_json(a.json),
                cli.db_path,
                &ui,
            )?;
        }
        Action::Session(SessionCommand::Forgotten(a)) => {
            forgotten_command::run_forgotten_command(
                a.session_id,
                a.last,
                a.current,
                a.round,
                a.classes,
                a.limit,
                a.redact,
                resolve_json(a.json),
                cli.db_path,
                &ui,
            )?;
        }
        Action::Session(SessionCommand::Asks(a)) => {
            asks_command::run_asks_command(
                a.session_id.or(a.session),
                a.last,
                a.current,
                a.since,
                a.unanswered,
                resolve_json(a.json),
                cli.db_path,
                &ui,
            )?;
        }
        Action::Session(SessionCommand::LooseEnds(a)) => {
            handoff_command::run_loose_ends_command(
                a.session_id,
                a.last,
                a.current,
                a.redact,
                a.prompt,
                resolve_json(a.json),
                cli.db_path,
                &ui,
            )?;
        }
        Action::Session(SessionCommand::Cache(a)) => {
            cache_doctor::run_cache_doctor_command(
                a.session_id,
                a.last,
                a.current,
                resolve_json(a.json),
                cli.db_path,
                &ui,
            )?;
        }
        Action::Session(SessionCommand::Bisect(a)) => {
            bisect::run_bisect_command(
                a.session_id,
                a.last,
                a.current,
                resolve_json(a.json),
                cli.db_path,
                &ui,
            )?;
        }
        Action::Session(SessionCommand::Watch(a)) => {
            watch::run_watch_command(
                a.interval_secs,
                a.poll_once,
                resolve_json(a.json),
                a.paths,
                cli.db_path,
                &ui,
            )?;
        }
        Action::Session(SessionCommand::Risk(a)) => {
            let limit = config::resolve_limit(a.limit, persisted_config.limit, 20);
            threat_digest::run_threat_digest_command(
                limit,
                &a.min_severity,
                resolve_json(a.json),
                cli.db_path,
                &ui,
            )?;
        }
        Action::Session(SessionCommand::Autopsy(a)) => {
            autopsy::run_autopsy_command(a.min_occurrences, resolve_json(a.json), cli.db_path)?;
        }
        Action::Session(SessionCommand::Recall(a)) => {
            let limit = config::resolve_limit(a.limit, persisted_config.limit, 5);
            recall::run_recall_command(&a.query, limit, a.min_score, resolve_json(a.json), cli.db_path)?;
        }
        Action::Agent(AgentCommand::List(a)) => {
            run_matrix_command(resolve_json(a.json), cli.db_path, &ui)?;
        }
        Action::Agent(AgentCommand::Show(a)) => {
            run_agent_show_command(&a.adapter, resolve_json(a.json), cli.db_path, &ui)?;
        }
        Action::Repo(RepoCommand::List(a)) => {
            let limit = config::resolve_limit(a.limit, persisted_config.limit, 20);
            run_repo_list_command(limit, resolve_json(a.json), cli.db_path, &ui)?;
        }
        Action::Repo(RepoCommand::Blame(a)) => {
            run_blame_command(&a.file_path, resolve_json(a.json), cli.db_path, &ui)?;
        }
        Action::Repo(RepoCommand::PrBlame(a)) => {
            pr_blame::run_pr_blame_command(a.files, resolve_json(a.json), cli.db_path, &ui)?;
        }
        Action::Repo(RepoCommand::Suspect(a)) => {
            crate::commands::suspect::run_suspect_command(
                crate::commands::suspect::SuspectArgs {
                    repo: a.repo,
                    since: a.since,
                    branch: a.branch,
                    base: a.base,
                    window_hours: a.window_hours,
                    json: resolve_json(a.json),
                    hook: a.hook,
                    quiet: a.quiet,
                },
                cli.db_path,
                &ui,
            )?;
        }
        Action::Repo(RepoCommand::BlunderBlame(a)) => {
            crate::run_blunder_blame_command(
                a.file,
                a.session,
                a.last,
                a.current,
                a.top,
                resolve_json(a.json),
                cli.db_path,
                &ui,
            )?;
        }
        Action::Serve(a) => {
            let storage = open_storage(cli.db_path)?;
            let dist_path = crate::server::resolve_dist_dir(a.dist)?;
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(crate::start_server(storage, a.port, a.open, dist_path))?;
        }
        Action::Mcp => {
            let storage = open_storage(cli.db_path)?;
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(crate::run_mcp_server(storage))?;
        }
        Action::Merge(a) => {
            merge::run_merge_command(a.source_db, resolve_json(a.json), cli.db_path, &ui)?;
        }
        Action::Doctor(a) => {
            if a.self_test {
                self_test::run_self_test_command(resolve_json(a.json), cli.db_path, &ui)?;
            } else {
                run_doctor_command(resolve_json(a.json), cli.db_path, &ui)?;
            }
        }
        Action::Version(a) => {
            version_info::run_version_command(resolve_json(a.json), a.offline)?;
        }
        Action::Update(a) => {
            version_info::run_update_command(resolve_json(a.json), a.offline)?;
        }
        Action::Completions(a) => {
            let mut cmd = Cli::command();
            clap_complete::aot::generate(
                a.shell,
                &mut cmd,
                invoked_binary_name(),
                &mut std::io::stdout(),
            );
        }
        Action::Config(action) => match action {
            ConfigAction::List { json } => config::run_config_list(resolve_json(json))?,
            ConfigAction::Get { key, json } => config::run_config_get(&key, resolve_json(json))?,
            ConfigAction::Set { key, value, json } => {
                config::run_config_set(&key, &value, resolve_json(json))?
            }
        },
        Action::Docs(a) => {
            crate::commands::docs::run_docs_command(&a.format, a.write)?;
        }
    }

    Ok(())
}

/// Clap `value_parser` for `usage --period`: canonicalizes `d`/`w`/`m`/`y` to their full
/// words via `config::normalize_period`, so `archie stats usage --period y` and `--period year`
/// parse identically.
fn parse_period_arg(s: &str) -> std::result::Result<String, String> {
    config::normalize_period(s).map(str::to_string).ok_or_else(|| {
        format!("invalid period {s:?}: expected one of day, week, month, year, all (or d/w/m/y)")
    })
}

/// Parse `usage --since`: an absolute date (RFC 3339, or bare `YYYY-MM-DD` read as midnight
/// UTC) or a relative shorthand (`<n>d`, `<n>w`, `<n>m` for days/weeks/months ago, e.g. `7d`,
/// `2w`, `3m`), anchored to now.
fn parse_since_arg(value: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    use chrono::{Months, TimeZone, Utc};

    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(value) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        if let Some(naive) = date.and_hms_opt(0, 0, 0) {
            return Ok(Utc.from_utc_datetime(&naive));
        }
    }
    if value.len() >= 2 {
        let (count, unit) = value.split_at(value.len() - 1);
        if let Ok(n) = count.parse::<u32>() {
            let now = Utc::now();
            let parsed = match unit {
                "d" => Some(now - chrono::Duration::days(n as i64)),
                "w" => Some(now - chrono::Duration::weeks(n as i64)),
                "m" => now.checked_sub_months(Months::new(n)),
                _ => None,
            };
            if let Some(dt) = parsed {
                return Ok(dt);
            }
        }
    }
    anyhow::bail!(
        "invalid --since {value:?}: expected a date (2026-08-01, or RFC 3339), or a relative \
         shorthand (1d, 7d, 2w, 3m)"
    );
}

/// The full clap command tree for `Cli`, for `archie docs` to introspect. Not exposing
/// `Cli` itself keeps its construction (parsing argv) owned entirely by `run()` above; this
/// is just the read-only `clap::Command` metadata `CommandFactory` derives for free.
pub fn cli_command() -> clap::Command {
    Cli::command()
}

fn open_storage(db_path: Option<PathBuf>) -> Result<Arc<Storage>> {
    if let Some(path) = db_path {
        Ok(Arc::new(Storage::open_path(&path)?))
    } else {
        Ok(Arc::new(Storage::open_default()?))
    }
}

// -----------------------------------------------------------------------------
// Command: Scan
// -----------------------------------------------------------------------------

fn run_scan_command(
    paths: Vec<PathBuf>,
    force: bool,
    include_stubs: bool,
    json: bool,
    db_path: Option<PathBuf>,
    ui: &crate::ui::Ui,
) -> Result<()> {
    let storage = open_storage(db_path)?;

    // The first run is the only one with something to introduce, so the banner is gated on
    // an empty index rather than on a flag. Stubs count: an index holding only stubs has
    // still been scanned once, and a second introduction is a lie about which run this is.
    if !json
        && storage
            .get_aggregate_stats(true)
            .map(|s| s.total_sessions == 0)
            .unwrap_or(false)
    {
        let index = storage
            .db_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "an in-memory index".to_string());
        print!("{}", crate::ui::views::scan_first_run(ui, &index));
    }

    let scanner = Scanner::new(storage.clone());
    let options = ScanOptions {
        custom_paths: paths,
        force,
        include_stubs,
    };

    // A stream that cannot move the cursor gets frame 1 once and nothing after it: a
    // loop that scrolls is a loop that lies. --json gets nothing at all.
    // --plain asks for the same treatment a pipe gets, so it is part of the test.
    let repaints = console::Term::stdout().is_term() && !ui.ascii();
    let mut progress = ScanProgress::new(ui, !json, repaints);

    let summary = scanner.run_scan(&options, |current, total| {
        progress.tick(current, total);
    })?;
    progress.clear();

    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        print_scan_summary(&summary, ui);
    }

    Ok(())
}

/// Three lines, redrawn in place at 8 fps. Faster than that is noise, and it costs a
/// redraw on every terminal on the machine.
struct ScanProgress<'a> {
    ui: &'a crate::ui::Ui,
    /// False under --json: Archie never appears on machine-readable output.
    show: bool,
    /// Whether the terminal can repaint in place. When it cannot, frame 1 prints once.
    repaints: bool,
    frame: u64,
    drawn: bool,
    last: std::time::Instant,
}

const SCAN_FRAME_MS: u64 = 125;

impl<'a> ScanProgress<'a> {
    fn new(ui: &'a crate::ui::Ui, show: bool, repaints: bool) -> Self {
        ScanProgress {
            ui,
            show,
            repaints,
            frame: 0,
            drawn: false,
            last: std::time::Instant::now() - std::time::Duration::from_millis(SCAN_FRAME_MS),
        }
    }

    fn tick(&mut self, current: usize, total: usize) {
        if !self.show {
            return;
        }
        if !self.repaints {
            if !self.drawn {
                println!(
                    "{}",
                    crate::ui::views::scan_progress(self.ui, 0, "agent histories", current, total)
                );
                self.drawn = true;
            }
            return;
        }
        if self.last.elapsed().as_millis() < SCAN_FRAME_MS as u128 {
            return;
        }
        self.last = std::time::Instant::now();
        let term = console::Term::stdout();
        if self.drawn {
            let _ = term.move_cursor_up(crate::ui::views::scan_progress_lines(self.ui));
            let _ = term.clear_to_end_of_screen();
        }
        print!(
            "{}",
            crate::ui::views::scan_progress(self.ui, self.frame, "agent histories", current, total)
        );
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
        self.frame += 1;
        self.drawn = true;
    }

    /// The progress line is cleared in one frame, per --motion-exit. A stream that cannot
    /// repaint keeps its one printed frame -- there is nothing there to clear.
    fn clear(&mut self) {
        if self.drawn && self.repaints {
            let term = console::Term::stdout();
            let _ = term.move_cursor_up(crate::ui::views::scan_progress_lines(self.ui));
            let _ = term.clear_to_end_of_screen();
            self.drawn = false;
        }
    }
}
// -----------------------------------------------------------------------------
// Command: Stats
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Default, serde::Serialize)]
struct VerdictBreakdown {
    ci_or_deployment_verified: usize,
    commit_observed: usize,
    test_or_build_passed: usize,
    artifact_changed: usize,
    done_claimed: usize,
    unverified: usize,
    real_verified_tasks: usize,
    real_verified_rate: f64,
}

/// Reads the verdict-rung counts straight from the index (`Storage::verdict_breakdown`) rather
/// than reparsing every transcript on disk -- the old version here called `Scanner::load_trace`
/// and re-ran outcome detection per session, which took 17s against a 2,960-session index.
/// Every non-stub scanned session already carries a trustworthy `primary_outcome` (#85/#81),
/// so the storage-layer aggregate query answers the same question.
fn compute_verdict_breakdown(storage: &Arc<Storage>, total_sessions: usize) -> VerdictBreakdown {
    let counts = storage.verdict_breakdown().unwrap_or_default();

    let mut breakdown = VerdictBreakdown {
        ci_or_deployment_verified: counts.ci_or_deployment_verified,
        commit_observed: counts.commit_observed,
        test_or_build_passed: counts.test_or_build_passed,
        artifact_changed: counts.artifact_changed,
        done_claimed: counts.done_claimed,
        unverified: counts.unverified,
        real_verified_tasks: counts.real_verified_tasks,
        real_verified_rate: 0.0,
    };

    if total_sessions > 0 {
        breakdown.real_verified_rate =
            (breakdown.real_verified_tasks as f64 / total_sessions as f64) * 100.0;
    }

    breakdown
}

fn run_stats_command(json: bool, db_path: Option<PathBuf>, ui: &crate::ui::Ui) -> Result<()> {
    let storage = open_storage(db_path)?;
    // false: compute_verdict_breakdown below iterates list_sessions_filtered's stub-excluded
    // default, so stats.total_sessions must exclude stubs too or real_verified_rate divides by
    // an inflated, mismatched denominator (docs/DECISION-INBOX.md, stats/stub-count-mismatch).
    let stats = storage.get_aggregate_stats(false)?;
    let top_repos = storage.get_top_repositories()?;
    let verdict = compute_verdict_breakdown(&storage, stats.total_sessions);

    if json {
        let json_output = json!({
            "total_sessions": stats.total_sessions,
            "total_events": stats.total_events,
            "date_range": {
                "first_session_at": stats.first_session_at,
                "last_session_at": stats.last_session_at,
            },
            "token_usage": {
                "input_tokens": stats.token_usage.input_tokens,
                "output_tokens": stats.token_usage.output_tokens,
                "cache_read_tokens": stats.token_usage.cache_read_tokens,
                "cache_creation_tokens": stats.token_usage.cache_creation_tokens,
                "total_tokens": stats.token_usage.total(),
            },
            "verdict_breakdown": {
                "ci_or_deployment_verified": verdict.ci_or_deployment_verified,
                "commit_observed": verdict.commit_observed,
                "test_or_build_passed": verdict.test_or_build_passed,
                "artifact_changed": verdict.artifact_changed,
                "done_claimed": verdict.done_claimed,
                "unverified": verdict.unverified,
                "real_verified_tasks": verdict.real_verified_tasks,
                "real_verified_rate": verdict.real_verified_rate,
            },
            "sessions_by_adapter": stats.sessions_by_adapter,
            "models_usage_count": stats.models_usage_count,
            "tools_usage_count": stats.tools_usage_count,
            "top_repositories": top_repos.iter().map(|(path, count)| json!({
                "repository": path,
                "sessions_count": count
            })).collect::<Vec<_>>()
        });
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else {
        let _ = &top_repos;
        print!(
            "{}",
            build_stats_view(&stats, &verdict, storage.db_path(), ui)
        );
    }

    Ok(())
}

/// Sort a name->count map into the descending order every table on the screen uses.
fn ranked(map: &std::collections::BTreeMap<String, usize>, take: usize) -> Vec<(String, usize)> {
    let mut v: Vec<(String, usize)> = map.iter().map(|(k, c)| (k.clone(), *c)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.truncate(take);
    v
}

fn build_stats_view(
    stats: &agentworth_storage::AggregateStats,
    verdict: &VerdictBreakdown,
    db_path: Option<&std::path::Path>,
    ui: &crate::ui::Ui,
) -> String {
    let db = db_path.map(|p| p.to_string_lossy().to_string());
    crate::ui::views::stats(ui, &stats_view(stats, verdict, db.as_deref(), true))
}

/// The `stats` screen's data, apart from how it is drawn -- `overview` below reuses it with
/// the closing line off, so the window block lands inside the same screen.
fn stats_view<'a>(
    stats: &agentworth_storage::AggregateStats,
    verdict: &VerdictBreakdown,
    db_path: Option<&'a str>,
    show_next: bool,
) -> crate::ui::views::StatsView<'a> {
    let tokens = &stats.token_usage;
    crate::ui::views::StatsView {
        db_path,
        total_sessions: stats.total_sessions,
        total_events: stats.total_events as u64,
        first_day: stats.first_session_at.map(|d| d.format("%Y-%m-%d").to_string()),
        last_day: stats.last_session_at.map(|d| d.format("%Y-%m-%d").to_string()),
        rungs: [
            verdict.unverified,
            verdict.done_claimed,
            verdict.artifact_changed,
            verdict.test_or_build_passed,
            verdict.commit_observed,
            verdict.ci_or_deployment_verified,
        ],
        verified: verdict.real_verified_tasks,
        input_tokens: tokens.input_tokens,
        output_tokens: tokens.output_tokens,
        cache_read_tokens: tokens.cache_read_tokens,
        cache_write_tokens: tokens.cache_creation_tokens,
        adapters: ranked(&stats.sessions_by_adapter, 3),
        models: ranked(&stats.models_usage_count, 3)
            .into_iter()
            .map(|(m, c)| (crate::ui::views::short_model(&m), c))
            .collect(),
        tools: ranked(&stats.tools_usage_count, 3),
        show_next,
    }
}

// -----------------------------------------------------------------------------
// Command: Traces
// -----------------------------------------------------------------------------

/// Turns already-indexed sessions into `(badge, score, outcome, session)` rows for `traces`
/// and its `--json` payload.
///
/// `SessionSummary` already carries both `primary_outcome` and `composite_score` straight
/// from the index -- scan wrote them once, at index time. This used to call
/// `scanner.load_trace` (a disk read + full adapter reparse) for every displayed row just to
/// recompute the same two numbers; on `--limit` set high, or `--all-stubs` against a large
/// index, that reparse cost dominated the whole command. See
/// `test_traces_reads_the_index_without_reparsing_every_transcript` below.
fn build_trace_rows(
    storage: &Arc<Storage>,
    sessions: Vec<agentworth_storage::SessionSummary>,
) -> Vec<(
    String,
    f64,
    Option<agentworth_schema::OutcomeKind>,
    agentworth_storage::SessionSummary,
)> {
    // `sessions.composite_score` is `NULL` only for a row a scan never actually scored
    // (see `crates/core/src/lib.rs`'s scan loop: "every session that gets stored gets both
    // passes, unconditionally" -- a `NULL` here means something other than a scan wrote the
    // row, and the next scan's `needs_backfill` pulls it back through to fix it). That's
    // rare and self-healing, but the old code recomputed a real score for every row via
    // `Scanner::load_trace` regardless, so falling back to the same per-row reparse ONLY for
    // that rare unscored case keeps `--json` byte-identical without giving up the fast path
    // for the overwhelming common (scored) case this fix exists for. `primary_outcome` being
    // `None` is not the same kind of gap -- it means "the detector found no outcome
    // evidence", the normal shape of an unverified session -- so it's read straight off the
    // index either way.
    let scanner = Scanner::new(storage.clone());
    sessions
        .into_iter()
        .map(|s| {
            let highest_kind = s.primary_outcome.as_deref().and_then(outcome_kind_from_str);
            let badge = match highest_kind {
                Some(agentworth_schema::OutcomeKind::CiOrDeploymentVerified) => "[CI_VERIFIED]".to_string(),
                Some(agentworth_schema::OutcomeKind::CommitObserved) => "[COMMITTED]".to_string(),
                Some(agentworth_schema::OutcomeKind::TestOrBuildPassed) => "[TEST_PASSED]".to_string(),
                Some(agentworth_schema::OutcomeKind::ArtifactChanged) => "[ARTIFACT]".to_string(),
                Some(agentworth_schema::OutcomeKind::DoneClaimed) => "[CLAIM_ONLY]".to_string(),
                None => "[UNVERIFIED]".to_string(),
            };
            let score_val = match s.composite_score {
                Some(score) => score * 100.0,
                None => scanner
                    .load_trace(&s.session_id)
                    .map(|t| agentworth_scoring::TraceScorer::default().score(&t).composite_score * 100.0)
                    .unwrap_or(0.0),
            };
            (badge, score_val, highest_kind, s)
        })
        .collect()
}

fn run_traces_command(
    limit: usize,
    adapter: Option<String>,
    model: Option<String>,
    all_stubs: bool,
    json: bool,
    db_path: Option<PathBuf>,
    ui: &crate::ui::Ui,
) -> Result<()> {
    let storage = open_storage(db_path)?;

    let filter = SessionFilter {
        adapter,
        model,
        limit: None,
        include_stubs: if all_stubs { Some(true) } else { None },
        order_by: Some(SessionOrderBy::StartedAtDesc),
        ..Default::default()
    };

    let all_sessions = storage.list_sessions_filtered(&filter)?;
    let filtered_sessions: Vec<_> = all_sessions
        .into_iter()
        .filter(|s| all_stubs || s.total_events > 1)
        .take(limit)
        .collect();

    let rows = build_trace_rows(&storage, filtered_sessions);

    if json {
        // When filtering by model, the session-wide `total_tokens` can be misleading
        // for a multi-model session (it's every model's usage, not just the matched
        // one) — so also surface the matched model's own contribution.
        let model_needle = filter.model.as_deref().map(|m| m.to_lowercase());

        let json_rows: Vec<_> = rows
            .iter()
            .map(|(badge, score, kind, s)| {
                let model_filter_tokens: Option<u64> = model_needle.as_deref().and_then(|needle| {
                    match storage.get_session_model_usage(&s.session_id) {
                        Ok(usages) if !usages.is_empty() => Some(
                            usages
                                .iter()
                                .filter(|(m, _)| m.to_lowercase().contains(needle))
                                .map(|(_, u)| u.total())
                                .sum(),
                        ),
                        _ => None,
                    }
                });

                json!({
                    "session_id": s.session_id,
                    "adapter": s.adapter,
                    "source_path": s.source_path,
                    "started_at": s.started_at,
                    "duration_seconds": s.duration_seconds,
                    "total_tokens": s.total_tokens,
                    "total_events": s.total_events,
                    "tool_calls_count": s.tool_calls_count,
                    "models_used": s.models_used,
                    "verdict_badge": badge,
                    // Use the canonical snake_case encoding (see agentworth_outcomes::outcome_kind_name),
                    // not `{:?}` Debug formatting -- Debug prints the raw PascalCase variant name, which
                    // is exactly the encoding mismatch this codebase has already shipped as a bug once.
                    "primary_outcome": kind.map(agentworth_outcomes::outcome_kind_name),
                    "score": score,
                    "model_filter_tokens": model_filter_tokens,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_rows)?);
    } else if rows.is_empty() {
        print!(
            "{}",
            crate::ui::views::error(
                ui,
                "archie session list",
                "No sessions found in index.",
                "",
                &[],
                &[(
                    "archie scan".to_string(),
                    "discover and index agent histories".to_string()
                )],
            )
        );
    } else {
        let indexed = storage.get_aggregate_stats(all_stubs).map(|s| s.total_sessions).unwrap_or(rows.len());
        print!("{}", build_traces_view(&rows, indexed, limit, ui));
    }

    Ok(())
}

fn build_traces_view(
    rows: &[(
        String,
        f64,
        Option<agentworth_schema::OutcomeKind>,
        agentworth_storage::SessionSummary,
    )],
    indexed: usize,
    limit: usize,
    ui: &crate::ui::Ui,
) -> String {
    let view_rows: Vec<crate::ui::views::TraceRow> = rows
        .iter()
        .map(|(_, score, kind, s)| crate::ui::views::TraceRow {
            session_id: s.session_id.clone(),
            adapter: s.adapter.clone(),
            model: s
                .models_used
                .first()
                .map(|m| crate::ui::views::short_model(m))
                .unwrap_or_else(|| "-".to_string()),
            score: *score,
            rung: outcome_rung(*kind),
            duration_seconds: s.duration_seconds,
            total_tokens: s.total_tokens,
        })
        .collect();
    crate::ui::views::traces(
        ui,
        &format!("archie session list --limit {}", limit),
        indexed,
        &view_rows,
    )
}

/// Parses `sessions.primary_outcome`'s stored snake_case string (`"done_claimed"`, ...) back
/// into the enum, the reverse of `agentworth_outcomes::outcome_kind_name`. `OutcomeKind`
/// derives `Deserialize` with `rename_all = "snake_case"`, so this is exact, not a guess --
/// an unrecognised or unset value reads as `None` (unverified) rather than an error, since a
/// row with no outcome yet is the common, expected case, not a data problem.
fn outcome_kind_from_str(s: &str) -> Option<agentworth_schema::OutcomeKind> {
    serde_json::from_value(json!(s)).ok()
}

/// The ladder position of an outcome. `None` is rung 0 — unverified, not missing.
fn outcome_rung(kind: Option<agentworth_schema::OutcomeKind>) -> usize {
    match kind {
        Some(agentworth_schema::OutcomeKind::CiOrDeploymentVerified) => 5,
        Some(agentworth_schema::OutcomeKind::CommitObserved) => 4,
        Some(agentworth_schema::OutcomeKind::TestOrBuildPassed) => 3,
        Some(agentworth_schema::OutcomeKind::ArtifactChanged) => 2,
        Some(agentworth_schema::OutcomeKind::DoneClaimed) => 1,
        None => 0,
    }
}

fn format_duration(seconds: f64) -> String {
    if seconds >= 3600.0 {
        let hrs = (seconds / 3600.0).floor();
        let mins = ((seconds % 3600.0) / 60.0).floor();
        format!("{}h {}m", hrs, mins)
    } else if seconds >= 60.0 {
        let mins = (seconds / 60.0).floor();
        let secs = (seconds % 60.0).floor();
        format!("{}m {:02}s", mins, secs)
    } else {
        format!("{:.1}s", seconds)
    }
}

// -----------------------------------------------------------------------------
// Command: Inspect
// -----------------------------------------------------------------------------

fn run_inspect_command(
    session_id: Option<String>,
    last: bool,
    current: bool,
    json: bool,
    db_path: Option<PathBuf>,
    ui: &crate::ui::Ui,
) -> Result<()> {
    let storage = open_storage(db_path.clone())?;
    let scanner = Scanner::new(storage.clone());

    let arg = crate::ui::picker::SessionArg::new(session_id, last, current);
    let resolved_id = match crate::ui::picker::resolve(&storage, ui, json, &arg)? {
        crate::ui::picker::Resolved::Id(id) => id,
        crate::ui::picker::Resolved::Ambiguous { input, candidates } => {
            crate::ui::picker::exit_ambiguous(ui, json, &input, &candidates)
        }
        crate::ui::picker::Resolved::NotFound(input) => {
            if json {
                anyhow::bail!(
                    "Session '{}' not found in SQLite index. Try running 'archie scan' first.",
                    input
                );
            }
            print!("{}", inspect_not_found(&input, db_path, ui));
            std::process::exit(1);
        }
    };
    let trace = crate::ui::with_status(ui, "loading session", || scanner.load_trace(&resolved_id))?;

    if json {
        println!("{}", serde_json::to_string_pretty(&trace)?);
    } else {
        print_inspect_view(&trace);
    }

    Ok(())
}

/// The `archie session show` screen, as a string.
///
/// One rendering, two readers: the command prints it, and the cockpit's session screen
/// shows it in a viewport. Nothing here is TUI-only, which is the spec's binding rule.
pub(crate) fn inspect_view(trace: &agentworth_schema::AgentWorthTrace) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let scorer = agentworth_scoring::TraceScorer::default();
    let score = scorer.score(trace);
    let outcomes = evaluate_trace_outcomes(trace);

    out.push('\n');
    let _ = writeln!(
        out,
        "{}",
        style("╔════════════════════════════════════════════════════════════════════════════════╗")
            .bold()
    );
    let _ = writeln!(
        out,
        "║ {:<78} ║",
        style(format!("AgentWorth Session Trace: {}", trace.session_id))
            .bold()
            .cyan()
    );
    let _ = writeln!(
        out,
        "{}",
        style("╠════════════════════════════════════════════════════════════════════════════════╣")
            .bold()
    );
    let _ = writeln!(
        out,
        "║ Adapter:     {:<20} Started:    {:<30} ║",
        style(&trace.adapter).green().bold(),
        trace.started_at.to_rfc3339()
    );
    if let Some(ended) = trace.ended_at {
        let _ = writeln!(
        out,
            "║ Duration:    {:<20} Ended:      {:<30} ║",
            trace
                .stats
                .duration_seconds
                .map(format_duration)
                .unwrap_or_else(|| "-".to_string()),
            ended.to_rfc3339()
        );
    }
    let _ = writeln!(
        out,
        "║ Score:       {:<20} Events:     {:<30} ║",
        style(format!("{:.1} / 100", score.composite_score * 100.0))
            .bold()
            .yellow(),
        trace.stats.total_events
    );

    let tokens = &trace.stats.token_usage;
    let _ = writeln!(
        out,
        "║ Tokens:      {:<20} Breakdown: {:<30} ║",
        style(format_number(tokens.total())).bold().magenta(),
        format!(
            "{} in / {} out / {} cache",
            format_number(tokens.input_tokens),
            format_number(tokens.output_tokens),
            format_number(tokens.cache_read_tokens)
        )
    );

    if !trace.stats.models_used.is_empty() {
        let _ = writeln!(
        out,
            "║ Models:      {:<66} ║",
            style(trace.stats.models_used.join(", ")).green()
        );
    }

    if !trace.stats.tools_used.is_empty() {
        let tools_str = trace
            .stats
            .tools_used
            .iter()
            .map(|(k, v)| format!("{}({})", k, v))
            .collect::<Vec<_>>()
            .join(", ");
        let display = if tools_str.chars().count() > 66 {
            format!("{}...", agentworth_schema::text::truncate_chars(&tools_str, 63))
        } else {
            tools_str
        };
        let _ = writeln!(out, "║ Tools:       {:<66} ║", style(display).yellow());
    }

    if let Some(strongest) = agentworth_outcomes::highest_outcome(&outcomes) {
        let (rung_num, rung_label) = match strongest.kind {
            agentworth_schema::OutcomeKind::CiOrDeploymentVerified => (5, "CI or Deployment Verified"),
            agentworth_schema::OutcomeKind::CommitObserved => (4, "Commit Observed"),
            agentworth_schema::OutcomeKind::TestOrBuildPassed => (3, "Test or Build Passed"),
            agentworth_schema::OutcomeKind::ArtifactChanged => (2, "Artifact Changed"),
            agentworth_schema::OutcomeKind::DoneClaimed => (1, "Done Claimed"),
        };

        let _ = writeln!(
        out,
            "{}",
            style("╠════════════════════════════════════════════════════════════════════════════════╣").bold()
        );
        let _ = writeln!(
        out,
            "║ Highest Outcome Reached: {:<53} ║",
            style(format!("Rung {} - {} (Confidence: {:.0}%)", rung_num, rung_label, strongest.confidence * 100.0)).bold().green()
        );
        let _ = writeln!(
        out,
            "║ Supporting Evidence:                                                           ║"
        );

        let supporting: Vec<_> = outcomes.iter().filter(|o| o.kind == strongest.kind).collect();
        for ev in &supporting {
            let summary_display = if ev.summary.chars().count() > 64 {
                format!("{}...", agentworth_schema::text::truncate_chars(&ev.summary, 61))
            } else {
                ev.summary.clone()
            };
            let _ = writeln!(
        out,
                "║   • {:<72} ║",
                style(format!("{} (conf: {:.0}%)", summary_display, ev.confidence * 100.0)).cyan()
            );
        }

        let other_signals: Vec<_> = outcomes.iter().filter(|o| o.kind != strongest.kind).collect();
        if !other_signals.is_empty() {
            let _ = writeln!(
        out,
                "║ Precursor / Secondary Evidence Signals ({}):                                   ║",
                other_signals.len()
            );
            for ev in other_signals.iter().take(3) {
                let summary_display = if ev.summary.chars().count() > 48 {
                    format!("{}...", agentworth_schema::text::truncate_chars(&ev.summary, 45))
                } else {
                    ev.summary.clone()
                };
                let _ = writeln!(
        out,
                    "║   - [{:<20}] {:<48} ║",
                    format!("{:?}", ev.kind),
                    style(summary_display).dim()
                );
            }
        }
    }

    let _ = writeln!(
        out,
        "║ Source:      {:<66} ║",
        style(&trace.provenance.source_path).dim()
    );
    let _ = writeln!(
        out,
        "{}",
        style("╚════════════════════════════════════════════════════════════════════════════════╝")
            .bold()
    );
    out.push('\n');

    let _ = writeln!(
        out,
        "{}",
        style("─ Session Timeline ─────────────────────────────────────────────────────────────")
            .bold()
    );

    for ev in &trace.events {
        let ts = ev.timestamp.format("%H:%M:%S").to_string();
        let seq = format!("[{:03}]", ev.sequence);

        match &ev.payload {
            agentworth_schema::EventPayload::UserMessage { content } => {
                let _ = writeln!(
        out,
                    "{} {} 👤 {}",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("USER PROMPT").bold().blue()
                );
                for line in content.lines() {
                    let _ = writeln!(out, "   │ {}", line);
                }
                let _ = writeln!(out, "   │");
            }
            agentworth_schema::EventPayload::AssistantMessage { content, thinking } => {
                if let Some(th) = thinking {
                    let _ = writeln!(
        out,
                        "{} {} 🧠 {}",
                        style(&seq).dim(),
                        style(&ts).dim(),
                        style("ASSISTANT THINKING").dim().cyan()
                    );
                    for line in th.lines() {
                        let _ = writeln!(out, "   │ {}", style(line).dim());
                    }
                    let _ = writeln!(out, "   │");
                }
                if !content.is_empty() {
                    let _ = writeln!(
        out,
                        "{} {} 💬 {}",
                        style(&seq).dim(),
                        style(&ts).dim(),
                        style("ASSISTANT").bold().green()
                    );
                    for line in content.lines() {
                        let _ = writeln!(out, "   │ {}", line);
                    }
                    let _ = writeln!(out, "   │");
                }
            }
            agentworth_schema::EventPayload::ModelInvocation {
                model,
                token_usage,
                cost_usd,
                latency_ms,
                ..
            } => {
                let cost_str = cost_usd
                    .map(|c| format!(" (${:.4})", c))
                    .unwrap_or_default();
                let latency_str = latency_ms.map(|l| format!(" {}ms", l)).unwrap_or_default();
                let _ = writeln!(
        out,
                    "{} {} 🤖 {} {}{}",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style(format!("MODEL ({})", model)).magenta().bold(),
                    style(format!(
                        "Tokens: {} in, {} out{}",
                        token_usage.input_tokens, token_usage.output_tokens, cost_str
                    ))
                    .dim(),
                    style(latency_str).dim()
                );
                let _ = writeln!(out, "   │");
            }
            agentworth_schema::EventPayload::ModelSwitch(ms) => {
                let from_str = ms.from_model.as_deref().unwrap_or("auto");
                let _ = writeln!(
        out,
                    "{} {} 🔀 {} {} -> {}",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("MODEL SWITCH:").magenta().bold(),
                    style(from_str).dim(),
                    style(&ms.to_model).magenta().bold()
                );
                if let Some(ref reason) = ms.reason {
                    let _ = writeln!(out, "   │ reason: {}", style(reason).dim());
                }
                let _ = writeln!(out, "   │");
            }
            agentworth_schema::EventPayload::ToolCall(tc) => {
                let _ = writeln!(
        out,
                    "{} {} ⚡ {} {}",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("TOOL CALL:").bold().yellow(),
                    style(&tc.name).yellow().bold()
                );
                let args_str = serde_json::to_string_pretty(&tc.arguments).unwrap_or_default();
                for line in args_str.lines() {
                    let _ = writeln!(out, "   │ {}", style(line).dim());
                }
                let _ = writeln!(out, "   │");
            }
            agentworth_schema::EventPayload::ToolResult(tr) => {
                let status = if tr.is_error {
                    style("[ERROR]").red().bold()
                } else {
                    style("[OK]").green().bold()
                };
                let name = tr.name.as_deref().unwrap_or("Tool");
                let _ = writeln!(
        out,
                    "{} {} 📥 {} {} {}",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("TOOL RESULT:").bold().yellow(),
                    style(name).yellow(),
                    status
                );
                let out_str = if let Some(s) = tr.output.as_str() {
                    s.to_string()
                } else {
                    serde_json::to_string_pretty(&tr.output).unwrap_or_default()
                };
                for line in out_str.lines().take(15) {
                    let _ = writeln!(out, "   │ {}", style(line).dim());
                }
                if out_str.lines().count() > 15 {
                    let _ = writeln!(out, "   │ {}", style("... (truncated output)").dim().italic());
                }
                let _ = writeln!(out, "   │");
            }
            agentworth_schema::EventPayload::ShellCommand(sc) => {
                let _ = writeln!(
        out,
                    "{} {} 💻 {} {}",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("SHELL COMMAND:").bold().cyan(),
                    style(&sc.command).cyan().bold()
                );
                if let Some(ref cwd) = sc.cwd {
                    let _ = writeln!(out, "   │ cwd: {}", style(cwd).dim());
                }
                if let Some(code) = sc.exit_code {
                    let _ = writeln!(out, "   │ exit: {}", code);
                }
                let _ = writeln!(out, "   │");
            }
            agentworth_schema::EventPayload::FileAction {
                path,
                action,
                lines_changed,
                ..
            } => {
                let _ = writeln!(
        out,
                    "{} {} 📝 {} {:?} {}",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("FILE ACTION:").bold().magenta(),
                    action,
                    style(path).bold()
                );
                if let Some(lines) = lines_changed {
                    let _ = writeln!(out, "   │ lines changed: {}", lines);
                }
                let _ = writeln!(out, "   │");
            }
            agentworth_schema::EventPayload::OutcomeEvidence(oe) => {
                let _ = writeln!(
        out,
                    "{} {} 🏆 {} {:?} (confidence: {:.0}%)",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("OUTCOME EVIDENCE:").bold().green(),
                    oe.kind,
                    oe.confidence * 100.0
                );
                let _ = writeln!(out, "   │ {}", style(&oe.summary).green());
                let _ = writeln!(out, "   │");
            }
            agentworth_schema::EventPayload::Error {
                message,
                is_recovered,
            } => {
                let _ = writeln!(
        out,
                    "{} {} ⚠️  {} (recovered: {})",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("ERROR:").bold().red(),
                    is_recovered
                );
                let _ = writeln!(out, "   │ {}", style(message).red());
                let _ = writeln!(out, "   │");
            }
            agentworth_schema::EventPayload::HumanIntervention(hi) => {
                let _ = writeln!(
        out,
                    "{} {} 🛑 {} {}",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("HUMAN INTERVENTION:").bold().red(),
                    hi.action
                );
                if let Some(ref details) = hi.details {
                    let _ = writeln!(out, "   │ details: {}", details);
                }
                let _ = writeln!(out, "   │");
            }
            agentworth_schema::EventPayload::Compaction(c) => {
                let tokens_str = match (c.pre_tokens, c.post_tokens) {
                    (Some(pre), Some(post)) => format!(" {} -> {} tokens", pre, post),
                    _ => String::new(),
                };
                let _ = writeln!(
        out,
                    "{} {} 🗜️  {} trigger={}{}",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("COMPACTION:").bold().cyan(),
                    style(&c.trigger).cyan().bold(),
                    style(tokens_str).dim()
                );
                if let Some(dropped) = c.dropped_tokens {
                    let _ = writeln!(out, "   │ dropped: {} tokens", dropped);
                }
                if let Some(duration) = c.duration_ms {
                    let _ = writeln!(out, "   │ duration: {}ms", duration);
                }
                let _ = writeln!(out, "   │");
            }
            agentworth_schema::EventPayload::Custom { kind, data } => {
                let _ = writeln!(
        out,
                    "{} {} 📦 {} {}",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("CUSTOM EVENT:").dim(),
                    kind
                );
                let _ = writeln!(out, "   │ {}", data);
                let _ = writeln!(out, "   │");
            }
        }
    }
    let _ = writeln!(
        out,
        "{}",
        style("────────────────────────────────────────────────────────────────────────────────")
            .bold()
    );
    out.push('\n');
    out
}

fn print_inspect_view(trace: &agentworth_schema::AgentWorthTrace) {
    print!("{}", inspect_view(trace));
}

// -----------------------------------------------------------------------------
// Command: Export
// -----------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn run_export_command(
    session_id: Option<String>,
    last: bool,
    current: bool,
    redact: bool,
    format: &str,
    output: Option<&std::path::Path>,
    db_path: Option<PathBuf>,
    ui: &crate::ui::Ui,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let scanner = Scanner::new(storage.clone());

    let arg = crate::ui::picker::SessionArg::new(session_id, last, current);
    // `export` has no --json; its machine-readable formats are the ones that emit JSON, and
    // those must not get a text table from the picker when no session was named.
    let session_id = crate::ui::picker::resolve_or_exit(
        &storage,
        ui,
        matches!(format, "json" | "atif"),
        "session export",
        &arg,
    )?;
    let session_id = session_id.as_str();

    let mut trace = crate::ui::with_status(ui, "loading session", || scanner.load_trace(session_id))?;

    if redact {
        trace = agentworth_redaction::redact_trace(&trace);
    }

    let output_content = match format.to_lowercase().as_str() {
        "atif" => agentworth_export_atif::export_to_atif(&trace, true)?,
        "svg" => {
            let scorer = agentworth_scoring::TraceScorer::default();
            let score = scorer.score(&trace);
            crate::render_svg_receipt(&trace, &score)
        }
        "receipt" | "terminal" | "ansi" => {
            let scorer = agentworth_scoring::TraceScorer::default();
            let score = scorer.score(&trace);
            crate::render_terminal_receipt(&trace, &score)
        }
        _ => serde_json::to_string_pretty(&trace)?,
    };


    if let Some(out_path) = output {
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create export directory {:?}", parent))?;
        }
        std::fs::write(out_path, output_content.as_bytes())
            .with_context(|| format!("Failed to write export file {:?}", out_path))?;
        // No glyph here: the allowed set (ui/mod.rs's module doc) has no check mark, and this
        // line is confirmation prose, not a rung or a primary number, so it takes no accent.
        eprintln!(
            "{}",
            ui.paint(
                crate::ui::Role::Label,
                &format!("Exported session '{}' ({}) to {:?}", session_id, format, out_path)
            )
        );
    } else {
        println!("{}", output_content);
    }

    Ok(())
}

fn format_number(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn print_scan_summary(summary: &ScanSummary, ui: &crate::ui::Ui) {
    let view = crate::ui::views::ScanView {
        discovered: summary.discovered_sources,
        scanned: summary.scanned_sessions,
        skipped: summary.skipped_unchanged,
        backfilled: summary.backfilled_sessions,
        reparsed: summary.reparsed_sessions,
        errors: summary.errors_encountered,
        total_indexed: summary.total_indexed_sessions,
        pruned: summary.stub_sessions_removed,
        total_tokens: summary.aggregate_stats.token_usage.total(),
        adapters: ranked(&summary.aggregate_stats.sessions_by_adapter, 5),
    };
    print!("{}", crate::ui::views::scan_summary(ui, &view));
}

/// The not-found screen for `inspect`: the failing noun, the nearest ids, the two
/// commands that resolve it.
fn inspect_not_found(
    session_id: &str,
    db_path: Option<PathBuf>,
    ui: &crate::ui::Ui,
) -> String {
    let nearest: Vec<String> = open_storage(db_path)
        .ok()
        .and_then(|s| {
            s.list_sessions_filtered(&SessionFilter {
                limit: None,
                order_by: Some(SessionOrderBy::StartedAtDesc),
                ..Default::default()
            })
            .ok()
        })
        .map(|sessions| {
            let needle = session_id.to_lowercase();
            let mut hits: Vec<String> = sessions
                .iter()
                .filter(|s| s.session_id.to_lowercase().contains(&needle))
                .map(|s| format!("{}\t{}", s.session_id, s.started_at.format("%b %e %H:%M")))
                .collect();
            if hits.is_empty() {
                hits = sessions
                    .iter()
                    .take(3)
                    .map(|s| format!("{}\t{}", s.session_id, s.started_at.format("%b %e %H:%M")))
                    .collect();
            }
            hits.truncate(3);
            hits
        })
        .unwrap_or_default();

    crate::ui::views::error(
        ui,
        &format!("archie session show {}", session_id),
        &format!("No indexed session starts with {}.", session_id),
        "Closest three:",
        &nearest,
        &[
            (
                "archie session list --limit 20".to_string(),
                "list what is indexed".to_string(),
            ),
            (
                "archie scan".to_string(),
                "re-index, if it should be here".to_string(),
            ),
        ],
    )
}

fn run_doctor_command(json_output: bool, custom_db_path: Option<PathBuf>, ui: &crate::ui::Ui) -> Result<()> {
    let storage_res = open_storage(custom_db_path.clone());
    let mut storage_healthy = false;
    let mut total_indexed = 0;
    let mut db_size_bytes = 0;
    let db_path_display = if let Some(ref p) = custom_db_path {
        p.display().to_string()
    } else {
        match std::env::var("HOME") {
            Ok(h) => PathBuf::from(h).join(".agentworth").join("agentworth.db").display().to_string(),
            Err(_) => "agentworth.db".to_string(),
        }
    };

    if let Ok(st) = &storage_res {
        storage_healthy = true;
        // true: this is a raw index-health count ("total_indexed_sessions" under "storage"),
        // matching `archie scan`'s own "Total Indexed... in SQLite index" promise -- not a
        // "real activity" metric.
        if let Ok(stats) = st.get_aggregate_stats(true) {
            total_indexed = stats.total_sessions;
        }
        let actual_path = PathBuf::from(&db_path_display);
        if let Ok(meta) = std::fs::metadata(&actual_path) {
            db_size_bytes = meta.len();
        }
    }

    // Inspect adapters discovery
    let adapters: Vec<Box<dyn agentworth_adapter_sdk::AgentAdapter>> = vec![
        Box::new(agentworth_adapters::AiderAdapter::new()),
        Box::new(agentworth_adapters::ClaudeCodeAdapter::new()),
        Box::new(agentworth_adapters::ClineAdapter::new()),
        Box::new(agentworth_adapters::CursorAdapter::new()),
        Box::new(agentworth_adapters::GeminiAdapter::new()),
        Box::new(agentworth_adapters::CodexAdapter::new()),
        Box::new(agentworth_adapters::DeepSeekAdapter::new()),
        Box::new(agentworth_adapters::GooseAdapter::new()),
        Box::new(agentworth_adapters::PiAdapter::new()),
        Box::new(agentworth_adapters::HerdrAdapter::new()),
        Box::new(agentworth_adapters::HermesAdapter::new()),
        Box::new(agentworth_adapters::KimiAdapter::new()),
        Box::new(agentworth_adapters::ManusAdapter::new()),
        Box::new(agentworth_adapters::MiniMaxAdapter::new()),
        Box::new(agentworth_adapters::OpenClawAdapter::new()),
        Box::new(agentworth_adapters::GrokAdapter::new()),
        Box::new(agentworth_adapters::OpenCodeAdapter::new()),
        Box::new(agentworth_adapters::QwenAdapter::new()),
        Box::new(agentworth_adapters::WindsurfAdapter::new()),
        Box::new(agentworth_adapters::ZhipuAdapter::new()),
    ];

    let scan_opts = ScanOptions::default();
    let mut detections = Vec::new();
    for a in &adapters {
        if let Ok(d) = a.detect(&scan_opts) {
            detections.push(d);
        }
    }

    if json_output {
        let report = json!({
            "status": if storage_healthy { "healthy" } else { "degraded" },
            "environment": {
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "version": env!("CARGO_PKG_VERSION"),
            },
            "storage": {
                "path": db_path_display,
                "healthy": storage_healthy,
                "size_bytes": db_size_bytes,
                "total_indexed_sessions": total_indexed,
            },
            "adapters": detections.iter().map(|d| {
                json!({
                    "adapter": d.adapter_name,
                    "is_present": d.is_present,
                    "confidence": d.confidence,
                    "discovered_roots": d.discovered_roots.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                })
            }).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    let view = crate::ui::views::DoctorView {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        db_path: db_path_display,
        storage_healthy,
        db_size_bytes,
        total_indexed,
        adapters: detections
            .iter()
            .map(|d| crate::ui::views::DoctorAdapterRow {
                name: d.adapter_name.to_string(),
                detected: d.is_present,
                roots: d.discovered_roots.len(),
            })
            .collect(),
    };
    print!("{}", crate::ui::views::doctor(ui, &view));

    Ok(())
}

// -----------------------------------------------------------------------------
// Command: Matrix
// -----------------------------------------------------------------------------

fn run_matrix_command(json_output: bool, _db_path: Option<PathBuf>, ui: &crate::ui::Ui) -> Result<()> {
    // Derived from the registry (`agentworth_adapters::all_adapters()`) rather than a
    // hand-copied list here: a newly-registered adapter now shows up in this table without
    // anyone remembering to add it in a second place.
    let (rows_data, real_coverage_rate) = matrix_rows_data();

    if json_output {
        let json_arr: Vec<_> = rows_data
            .iter()
            .map(|(name, source_root, caps, detected)| {
                json!({
                    "adapter": name,
                    "source_root": source_root,
                    "extraction": {
                        "prompts": caps.prompts,
                        "tokens": caps.tokens,
                        "tools": caps.tools,
                        "shell": caps.shell,
                        "diffs": caps.diffs,
                        "thinking": caps.thinking,
                        "outcomes": caps.outcomes,
                    },
                    "is_detected": detected,
                    "status": if *detected { "detected" } else { "available" }
                })
            })
            .collect();

        let output = json!({
            "total_adapters": rows_data.len(),
            "coverage_rate": real_coverage_rate,
            "adapters": json_arr,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    print!("{}", build_matrix_view(&rows_data, &real_coverage_rate, ui));

    Ok(())
}

/// What `archie agent list` draws, as a string. The cockpit's agents screen shows the same
/// thing in a viewport.
fn build_matrix_view(
    rows_data: &[(
        &str,
        &'static str,
        agentworth_adapter_sdk::AdapterCapabilities,
        bool,
    )],
    coverage_rate: &str,
    ui: &crate::ui::Ui,
) -> String {
    // Only `claude_code`'s parser populates `compaction_count`/`CompactionEvent` today
    // (`crates/adapters/src/claude.rs`) -- add an adapter's name here only once its parser
    // does the same, so this column stays a grounded fact rather than a guess.
    let compaction_tracking: std::collections::HashSet<&'static str> =
        ["claude_code"].into_iter().collect();

    let rows: Vec<crate::ui::views::MatrixRow> = rows_data
        .iter()
        .map(|(name, _source_root, caps, is_detected)| crate::ui::views::MatrixRow {
            adapter: (*name).to_string(),
            detected: *is_detected,
            parse: caps.prompts,
            outcomes: caps.outcomes,
            // Both are required for the shared recovery-loop detector's failure ->
            // corrective-action -> recovery pattern to have anything to work with (it
            // walks ToolCall/ToolResult and ShellCommand events; see
            // `crates/outcomes/src/recovery.rs`).
            recoveries: caps.tools && caps.shell,
            compaction: compaction_tracking.contains(*name),
        })
        .collect();
    crate::ui::views::matrix(ui, coverage_rate, &rows)
}

/// The adapter registry's own answer to `agent list`, shared by the command, its JSON and
/// the cockpit.
fn matrix_rows_data() -> (
    Vec<(
        &'static str,
        &'static str,
        agentworth_adapter_sdk::AdapterCapabilities,
        bool,
    )>,
    String,
) {
    let adapters: Vec<Box<dyn agentworth_adapter_sdk::AgentAdapter>> =
        agentworth_adapters::all_adapters();
    let scan_opts = ScanOptions::default();
    let mut rows_data = Vec::new();
    let mut total_supported = 0usize;
    let mut total_possible = 0usize;

    for adapter in &adapters {
        let name = adapter.name();
        let caps = adapter.capabilities();
        let (sup, poss) = caps.score();
        total_supported += sup;
        total_possible += poss;
        let is_detected = adapter.detect(&scan_opts).map(|d| d.is_present).unwrap_or(false);
        rows_data.push((name, adapter_source_root(name), caps, is_detected));
    }

    let coverage = format!(
        "{:.1}%",
        (total_supported as f64 / total_possible as f64) * 100.0
    );
    (rows_data, coverage)
}

// -----------------------------------------------------------------------------
// Command: Usage & Pacing
// -----------------------------------------------------------------------------

struct UsageCommandArgs<'a> {
    period: &'a str,
    pacing: bool,
    hours: i64,
    alert_above: Option<f64>,
    limit: usize,
    by: &'a str,
    since: Option<chrono::DateTime<chrono::Utc>>,
    json: bool,
    db_path: Option<PathBuf>,
    ui: &'a crate::ui::Ui,
}

/// Noun for one period, for the footer's "N of M <noun>s" phrasing.
fn period_noun(period: &str) -> &'static str {
    match period {
        "week" => "week",
        "month" => "month",
        "year" => "year",
        _ => "day",
    }
}

fn run_usage_command(args: UsageCommandArgs) -> Result<()> {
    let UsageCommandArgs {
        period,
        pacing,
        hours,
        alert_above,
        limit,
        by,
        since,
        json,
        db_path,
        ui,
    } = args;
    let storage = open_storage(db_path)?;

    if pacing {
        let p = storage.get_pacing_window(hours)?;
        let alert_triggered = alert_above
            .map(|t| p.estimated_cost_usd >= t)
            .unwrap_or(false);

        if json {
            let mut val = serde_json::to_value(&p)?;
            if let Some(threshold) = alert_above {
                val["alert_threshold_usd"] = serde_json::json!(threshold);
                val["alert_triggered"] = serde_json::json!(alert_triggered);
            }
            println!("{}", serde_json::to_string_pretty(&val)?);
            return Ok(());
        }

        print!(
            "{}",
            build_pacing_view(&p, hours, alert_above, alert_triggered, ui)
        );
        return Ok(());
    }

    let period_kind = agentworth_storage::UsagePeriodKind::parse(period)
        .expect("period already validated by clap's value_parser / config::resolve_period");
    let group_by = agentworth_storage::UsageGroupBy::parse(by)
        .expect("by already validated by clap's value_parser");
    let report = storage.get_usage_report(period_kind, group_by, since, limit)?;
    let cost_basis = crate::cost_basis::CostBasis::detect();

    if json {
        let mut val = serde_json::to_value(&report)?;
        if let Some(obj) = val.as_object_mut() {
            obj.insert("cost_basis".to_string(), json!(cost_basis.cost_basis));
            if let Some(tier) = &cost_basis.subscription_tier {
                obj.insert("subscription_tier".to_string(), json!(tier));
            }
        }
        println!("{}", serde_json::to_string_pretty(&val)?);
        return Ok(());
    }

    if report.rows.is_empty() {
        print!("{}", no_usage_records(ui));
        return Ok(());
    }

    let who_head = match group_by {
        agentworth_storage::UsageGroupBy::Adapter => "ADAPTER",
        agentworth_storage::UsageGroupBy::Model => "MODEL",
        agentworth_storage::UsageGroupBy::Repo => "REPO",
    };
    let show_period = period_kind != agentworth_storage::UsagePeriodKind::All;

    let rows: Vec<crate::ui::views::UsageRow> = report
        .rows
        .iter()
        .map(|r| crate::ui::views::UsageRow {
            period: r.period.clone().unwrap_or_default(),
            who: r.group.clone(),
            sessions: r.session_count,
            input: r.input_tokens,
            output: r.output_tokens,
            cache_read: r.cache_read_tokens,
            cost_usd: r.estimated_cost_usd,
            measured: r.total_tokens > 0,
        })
        .collect();

    let truncation_note = report.truncated.then(|| {
        if show_period {
            let noun = period_noun(period);
            let plural = if report.periods_total == 1 { noun.to_string() } else { format!("{noun}s") };
            format!(
                "last {} of {} {plural}: totals cover the shown periods only.",
                report.periods_shown, report.periods_total
            )
        } else {
            let noun = who_head.to_lowercase();
            let plural = if report.periods_total == 1 { noun } else { format!("{noun}s") };
            format!(
                "top {} of {} {plural} by spend: totals cover the shown groups only.",
                report.periods_shown, report.periods_total
            )
        }
    });

    let mut command = format!("archie stats usage --period {period}");
    if by != "adapter" {
        command.push_str(&format!(" --by {by}"));
    }
    let cost_note = cost_basis.label_long();

    print!(
        "{}",
        crate::ui::views::usage(
            ui,
            &crate::ui::views::UsageView {
                command: &command,
                who_head,
                period_noun: period_noun(period),
                rows: &rows,
                show_period,
                cost_note: &cost_note,
                truncation_note: truncation_note.as_deref(),
            }
        )
    );

    Ok(())
}

fn no_usage_records(ui: &crate::ui::Ui) -> String {
    crate::ui::views::error(
        ui,
        "archie stats usage",
        "No usage records in the index.",
        "",
        &[],
        &[(
            "archie scan".to_string(),
            "index local sessions first".to_string(),
        )],
    )
}

fn build_pacing_view(
    p: &agentworth_storage::PacingSummary,
    hours: i64,
    alert_above: Option<f64>,
    alert_triggered: bool,
    ui: &crate::ui::Ui,
) -> String {
    use crate::ui::{compact, thousands, views, Role};
    let i = ui.inner();
    let mut out = String::new();

    out.push_str(&ui.header(
        &format!("archie window show --hours {}", hours),
        &format!(
            "{} {} {}",
            p.started_at.format("%Y-%m-%d %H:%M"),
            ui.arrow(),
            p.ended_at.format("%H:%M")
        ),
    ));
    out.push('\n');

    for (label, value, role) in [
        (
            "sessions active",
            thousands(p.session_count as u64),
            Role::Value,
        ),
        ("events", thousands(p.total_events as u64), Role::Value),
        ("tokens consumed", compact(p.total_tokens), Role::Value),
        (
            "burn velocity",
            format!("{:.1}M / hour", p.burn_rate_tokens_per_hour / 1_000_000.0),
            Role::Value,
        ),
        (
            "cache hit ratio",
            format!("{:.1}%", p.cache_hit_ratio),
            Role::Value,
        ),
        (
            "estimated cost",
            format!("${:.2}", p.estimated_cost_usd),
            Role::Verified,
        ),
    ] {
        out.push_str(&format!(
            "{}\n",
            ui.leaders(&format!("  {}", label), &value, i, role)
        ));
    }

    if let Some(threshold) = alert_above {
        out.push('\n');
        let (role, line) = if alert_triggered {
            (
                Role::Error,
                format!(
                    "Window spend ${:.2} is over the ${:.2} alarm.",
                    p.estimated_cost_usd, threshold
                ),
            )
        } else {
            (
                Role::Label,
                format!(
                    "Window spend ${:.2} is under the ${:.2} alarm.",
                    p.estimated_cost_usd, threshold
                ),
            )
        };
        out.push_str(&format!("  {}\n", ui.paint(role, &line)));
    }

    let _ = views::RUNG_LABELS;
    out.push('\n');
    out.push_str(&ui.next("archie stats usage --period day", "the same spend, by day"));
    out
}

// -----------------------------------------------------------------------------
// Command: Blame
// -----------------------------------------------------------------------------

fn run_blame_command(
    file_path: &str,
    json: bool,
    db_path: Option<PathBuf>,
    ui: &crate::ui::Ui,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let matches = storage.find_sessions_for_blame(file_path)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&matches)?);
        return Ok(());
    }

    // The question is not who edited the file, it is which edit is trustworthy — so each
    // row's ladder position comes straight from `sessions.primary_outcome`, the same index
    // column `traces` and `pr-blame` already read for the same question. This used to call
    // `scanner.load_trace` per row (a disk read + full adapter reparse) just to recompute an
    // outcome the scan already wrote to the index; `get_session_by_id` is one indexed SQLite
    // lookup instead.
    let rows: Vec<crate::ui::views::BlameRow> = matches
        .iter()
        .map(|m| {
            let rung = storage
                .get_session_by_id(&m.session_id)
                .ok()
                .flatten()
                .and_then(|s| s.primary_outcome)
                .as_deref()
                .and_then(outcome_kind_from_str)
                .map(Some)
                .map(outcome_rung)
                .unwrap_or(0);
            crate::ui::views::BlameRow {
                when: m.modified_at.format("%b %e %H:%M").to_string(),
                rung,
                session_id: m.session_id.clone(),
                model: m
                    .model
                    .clone()
                    .or_else(|| m.models_used.first().cloned())
                    .map(|s| crate::ui::views::short_model(&s))
                    .unwrap_or_else(|| "-".to_string()),
                tool_calls: m.tool_calls_count,
                action: m.action.clone(),
            }
        })
        .collect();

    print!("{}", crate::ui::views::blame(ui, file_path, &rows));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_schema::{AgentWorthTrace, EventPayload, NormalizedEvent, Provenance, TokenUsage};
    use chrono::{Duration, Utc};
    use tempfile::NamedTempFile;

    #[test]
    fn test_traces_all_stubs_filter_wiring() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Storage::open_path(tmp.path()).unwrap();

        // Session 1: A stub session (only 1 event: UserMessage)
        let start = Utc::now();
        let prov1 = Provenance::new("/test/stub.jsonl", "claude_code", 10, 100, "fp1");
        let mut trace1 = AgentWorthTrace::new("sess-stub-1", "claude_code", prov1, start);
        trace1.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::UserMessage {
                content: "help".to_string(),
            },
        ));
        trace1.stats.total_events = 1;
        storage.upsert_trace(&trace1).unwrap();

        // Session 2: A multi-turn session (2 events)
        let prov2 = Provenance::new("/test/full.jsonl", "claude_code", 20, 200, "fp2");
        let mut trace2 = AgentWorthTrace::new("sess-full-2", "claude_code", prov2, start);
        trace2.events.push(NormalizedEvent::new(
            1,
            start,
            EventPayload::UserMessage {
                content: "write tests".to_string(),
            },
        ));
        trace2.events.push(NormalizedEvent::new(
            2,
            start,
            EventPayload::AssistantMessage {
                content: "tests written".to_string(),
                thinking: None,
            },
        ));
        trace2.stats.total_events = 2;
        trace2.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
        storage.upsert_trace(&trace2).unwrap();

        // Without all_stubs (default): stubs excluded (total_events > 1)
        let default_filter = SessionFilter {
            include_stubs: None,
            ..Default::default()
        };
        let default_results = storage.list_sessions_filtered(&default_filter).unwrap();
        assert_eq!(default_results.len(), 1);
        assert_eq!(default_results[0].session_id, "sess-full-2");

        // With all_stubs: stubs included
        let all_stubs_filter = SessionFilter {
            include_stubs: Some(true),
            ..Default::default()
        };
        let all_stubs_results = storage.list_sessions_filtered(&all_stubs_filter).unwrap();
        assert_eq!(all_stubs_results.len(), 2);
    }

    #[test]
    fn test_compute_verdict_breakdown_scans_beyond_default_limit() {
        // Regression test: compute_verdict_breakdown asks list_sessions_filtered for every
        // session via `limit: None`, then divides its bucket counts against the true total.
        // list_sessions_filtered used to silently resolve `limit: None` to 50, so on any index
        // bigger than that the buckets summed to 50 instead of the real count. This seeds more
        // than 50 non-stub sessions and asserts the buckets cover all of them.
        let tmp = NamedTempFile::new().unwrap();
        let storage = Storage::open_path(tmp.path()).unwrap();
        let start = Utc::now();

        const SESSION_COUNT: i64 = 60;
        for i in 0..SESSION_COUNT {
            let prov = Provenance::new(
                format!("/test/verdict_{}.jsonl", i),
                "claude_code",
                10,
                100,
                format!("fp_verdict_{}", i),
            );
            let mut trace = AgentWorthTrace::new(
                format!("sess-verdict-{}", i),
                "claude_code",
                prov,
                start + Duration::seconds(i),
            );
            // Non-stub: list_sessions_filtered's default excludes total_events <= 1 or
            // total_tokens <= 0.
            trace.stats.total_events = 2;
            trace.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
            storage.upsert_trace(&trace).unwrap();
        }

        let storage = Arc::new(storage);
        let total_sessions = storage.get_aggregate_stats(false).unwrap().total_sessions;
        assert_eq!(total_sessions, SESSION_COUNT as usize);

        let breakdown = compute_verdict_breakdown(&storage, total_sessions);
        let scanned = breakdown.ci_or_deployment_verified
            + breakdown.commit_observed
            + breakdown.test_or_build_passed
            + breakdown.artifact_changed
            + breakdown.done_claimed
            + breakdown.unverified;

        assert_eq!(
            scanned, SESSION_COUNT as usize,
            "compute_verdict_breakdown must scan every session, not silently cap at 50"
        );
    }

    #[test]
    fn test_stats_reads_the_index_without_reparsing_every_transcript() {
        // Timing guard: `compute_verdict_breakdown` used to call `Scanner::load_trace` (a
        // real disk read + adapter re-parse) for every session on every `archie stats`
        // invocation -- 17s measured against a 2,960-session index. It now reads
        // `sessions.primary_outcome` straight out of the index (`Storage::verdict_breakdown`),
        // so this must stay fast against 3,000 sessions.
        //
        // Every source_path below is nonexistent, so if a reparse ever creeps back in here,
        // `Scanner::load_trace` fails loudly on the missing file rather than this test
        // silently passing anyway.
        // In-memory, not `NamedTempFile` + `open_path`: seeding 3,000 rows through real
        // per-call transactions is dominated by disk fsync otherwise, which would make this
        // guard's own setup slower than the 2s budget it's checking `compute_verdict_breakdown`
        // against (`test_discover_blunders_surfaces_blunder_beyond_old_5000_cap` in
        // apps/cli/src/commands/blunder.rs seeds 5,000 rows the same way for the same reason).
        let storage = Storage::open_in_memory().unwrap();
        let start = Utc::now();

        const SESSION_COUNT: i64 = 3000;
        let outcomes = [
            Some("ci_or_deployment_verified"),
            Some("commit_observed"),
            Some("test_or_build_passed"),
            Some("artifact_changed"),
            Some("done_claimed"),
            None,
        ];
        for i in 0..SESSION_COUNT {
            let prov = Provenance::new(
                format!("/nonexistent/verdict_timing_{}.jsonl", i),
                "claude_code",
                10,
                100,
                format!("fp_timing_{}", i),
            );
            let mut trace = AgentWorthTrace::new(
                format!("sess-timing-{}", i),
                "claude_code",
                prov,
                start + Duration::seconds(i),
            );
            trace.stats.total_events = 2;
            trace.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
            storage
                .upsert_session(&trace, outcomes[(i % 6) as usize], Some(0.5), 1)
                .unwrap();
        }

        let storage = Arc::new(storage);
        let total_sessions = storage.get_aggregate_stats(false).unwrap().total_sessions;
        assert_eq!(total_sessions, SESSION_COUNT as usize);

        let started = std::time::Instant::now();
        let breakdown = compute_verdict_breakdown(&storage, total_sessions);
        let elapsed = started.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "compute_verdict_breakdown took {:?} against {} sessions -- it must read the \
             index, not reparse every transcript",
            elapsed,
            SESSION_COUNT
        );

        // 3000 sessions split evenly across the 6 buckets above -- 500 each.
        assert_eq!(breakdown.ci_or_deployment_verified, 500);
        assert_eq!(breakdown.commit_observed, 500);
        assert_eq!(breakdown.test_or_build_passed, 500);
        assert_eq!(breakdown.artifact_changed, 500);
        assert_eq!(breakdown.done_claimed, 500);
        assert_eq!(breakdown.unverified, 500);
        assert_eq!(breakdown.real_verified_tasks, 1500);
    }

    #[test]
    fn test_burn_alarm_threshold_triggering() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Storage::open_path(tmp.path()).unwrap();

        let now = Utc::now();
        let prov = Provenance::new("/tmp/test.jsonl", "claude_code", 100, 1000, "fp1");
        let mut trace = AgentWorthTrace::new("sess-pacing-1", "claude_code", prov, now - Duration::hours(1));

        trace.stats.token_usage = TokenUsage {
            input_tokens: 10_000_000,   // 10M input = $30.00
            output_tokens: 2_000_000,  // 2M output = $30.00
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        };
        trace.stats.total_events = 2;

        trace.events.push(NormalizedEvent::new(
            1,
            now - Duration::hours(1),
            EventPayload::UserMessage {
                content: "run simulation".to_string(),
            },
        ));

        storage.upsert_trace(&trace).unwrap();

        let pacing = storage.get_pacing_window(5).unwrap();
        assert!(pacing.estimated_cost_usd >= 50.0);

        // Alert threshold $100 -> NOT triggered
        let alert_triggered_high = pacing.estimated_cost_usd >= 100.0;
        assert!(!alert_triggered_high);

        // Alert threshold $20 -> TRIGGERED
        let alert_triggered_low = pacing.estimated_cost_usd >= 20.0;
        assert!(alert_triggered_low);
    }

    #[test]
    fn test_cli_binary_alias_parsing() {
        use clap::Parser;

        // The same argv under each of the three binary names it answers to.
        for argv0 in ["agentworth", "agwt", "archie"] {
            let parsed = Cli::try_parse_from([argv0, "doctor", "--json"]).unwrap();
            match parsed.command {
                Some(Commands::Doctor(a)) => {
                    assert!(a.json);
                    assert!(!a.self_test);
                }
                _ => panic!("Expected Doctor command"),
            }
        }

        // Test parsing "doctor --self-test"
        let parsed_self_test = Cli::try_parse_from(["agentworth", "doctor", "--self-test"]).unwrap();
        match parsed_self_test.command {
            Some(Commands::Doctor(a)) => {
                assert!(!a.json);
                assert!(a.self_test);
            }
            _ => panic!("Expected Doctor command"),
        }

        // Test usage pacing with alert-above, on the hidden alias
        let parsed_usage = Cli::try_parse_from(["agwt", "usage", "--pacing", "--alert-above", "50.0"]).unwrap();
        match parsed_usage.command {
            Some(Commands::Usage(a)) => {
                assert!(a.pacing);
                assert_eq!(a.alert_above, Some(50.0));
            }
            _ => panic!("Expected Usage command"),
        }
    }

    #[test]
    fn test_traces_reads_the_index_without_reparsing_every_transcript() {
        // Timing guard: `run_traces_command` used to call `Scanner::load_trace` for every
        // displayed row to recompute a badge and score that `SessionSummary.primary_outcome`
        // / `composite_score` already carry. `build_trace_rows` now reads those two fields
        // straight off the summary -- no `Scanner` in scope at all -- so this must stay fast
        // against `--limit` set to the whole 3,000-session index. Nonexistent source paths:
        // if a `scanner.load_trace` call ever creeps back into this row-building path, it
        // fails loudly on the missing file rather than this test silently passing anyway.
        let storage = Storage::open_in_memory().unwrap();
        let start = Utc::now();

        const SESSION_COUNT: i64 = 3000;
        let outcomes = [
            Some("ci_or_deployment_verified"),
            Some("commit_observed"),
            Some("test_or_build_passed"),
            Some("artifact_changed"),
            Some("done_claimed"),
            None,
        ];
        for i in 0..SESSION_COUNT {
            let prov = Provenance::new(
                format!("/nonexistent/traces_timing_{}.jsonl", i),
                "claude_code",
                10,
                100,
                format!("fp_traces_timing_{}", i),
            );
            let mut trace = AgentWorthTrace::new(
                format!("sess-traces-timing-{}", i),
                "claude_code",
                prov,
                start + Duration::seconds(i),
            );
            trace.stats.total_events = 2;
            trace.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
            storage
                .upsert_session(&trace, outcomes[(i % 6) as usize], Some(0.5), 1)
                .unwrap();
        }

        let sessions = storage
            .list_sessions_filtered(&SessionFilter {
                limit: None,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(sessions.len(), SESSION_COUNT as usize);

        let storage = Arc::new(storage);
        let started = std::time::Instant::now();
        let rows = build_trace_rows(&storage, sessions);
        let elapsed = started.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "build_trace_rows took {:?} against {} sessions -- it must read \
             primary_outcome/composite_score off the index, not reparse every transcript",
            elapsed,
            SESSION_COUNT
        );
        assert_eq!(rows.len(), SESSION_COUNT as usize);
        // Every fifth row (i % 6 == 4) is "done_claimed" -- rung 1, badge [CLAIM_ONLY].
        let done_claimed_row = rows.iter().find(|(_, _, kind, _)| {
            matches!(kind, Some(agentworth_schema::OutcomeKind::DoneClaimed))
        });
        assert_eq!(done_claimed_row.unwrap().0, "[CLAIM_ONLY]");
    }

    /// `--json`'s `score` field must stay byte-identical to the pre-fix behaviour even for a
    /// `NULL composite_score` row -- a row a scan never actually scored (see
    /// `build_trace_rows`'s doc comment: "every session that gets stored gets both passes,
    /// unconditionally", so `NULL` here can only mean something other than a scan wrote it).
    /// The old code recomputed a real score from the trace file for every row regardless of
    /// what the index held; this proves the `None`-score fallback path still does exactly
    /// that, rather than silently reporting 0.0.
    #[test]
    fn test_traces_recomputes_a_score_only_for_the_rare_null_composite_score_row() {
        let tmp = NamedTempFile::new().unwrap();
        let storage = Arc::new(Storage::open_path(tmp.path()).unwrap());

        // A real on-disk session, scanned normally, so `sessions_scanned`'s row carries a
        // real `composite_score` -- this is the fixture the *fallback* has to succeed
        // against, so the temp dir (not a nonexistent path) is deliberate here, unlike the
        // timing guards above.
        let dir = tempfile::tempdir().unwrap();
        let session_path = dir.path().join("unscored-session.jsonl");
        std::fs::write(
            &session_path,
            concat!(
                r#"{"type":"user","timestamp":"2026-01-01T00:00:00Z","content":"fix the bug"}"#,
                "\n",
                r#"{"type":"assistant","timestamp":"2026-01-01T00:00:02Z","model":"claude-3-5-sonnet-20241022","usage":{"input_tokens":100,"output_tokens":20,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test"}}]}"#,
                "\n",
                r#"{"type":"tool_result","timestamp":"2026-01-01T00:00:04Z","tool_use_id":"t1","content":"test result: ok. 3 passed; 0 failed","is_error":false}"#,
                "\n",
            ),
        )
        .unwrap();

        // `Scanner::new` registers every adapter, and more than one of them can match this
        // same generic-looking JSONL well enough to also index it under its own derived
        // session_id -- scoping to the one adapter this fixture is actually shaped for is
        // what keeps this a single-session fixture (mirrors
        // `pr_blame::tests::test_annotate_pr_files_prices_non_sonnet_model_correctly`).
        let scanner = Scanner::with_adapters(
            vec![Box::new(agentworth_adapters::ClaudeCodeAdapter::new())],
            storage.clone(),
        );
        let summary = scanner
            .run_scan(
                &ScanOptions { custom_paths: vec![session_path.clone()], force: true, ..Default::default() },
                |_, _| {},
            )
            .expect("scan the real session");
        assert_eq!(summary.scanned_sessions, 1);

        let scored = storage
            .list_sessions_filtered(&SessionFilter { limit: None, ..Default::default() })
            .unwrap();
        assert_eq!(scored.len(), 1);
        let real_score = scored[0]
            .composite_score
            .expect("a real scan always scores what it stores");
        assert!(real_score > 0.0, "this session has real evidence, it must not score zero");

        // Now simulate the anomalous "written by something other than a scan" row this
        // fallback exists for: same session, same file on disk, but re-written with `None`
        // for the score the way a non-scan writer (or a pre-scoring-era row) would leave it.
        let trace = scanner.load_trace(&scored[0].session_id).unwrap();
        storage
            .upsert_session(&trace, scored[0].primary_outcome.as_deref(), None, 1)
            .unwrap();

        let unscored = storage
            .list_sessions_filtered(&SessionFilter { limit: None, ..Default::default() })
            .unwrap();
        assert_eq!(unscored.len(), 1);
        assert!(unscored[0].composite_score.is_none(), "fixture must be unscored now");

        let rows = build_trace_rows(&storage, unscored);
        assert_eq!(rows.len(), 1);
        // `build_trace_rows` fell back to `Scanner::load_trace` for this one `None`-score
        // row, recomputed the same real score the initial scan found, and did not silently
        // report 0.0 -- proving `--json`'s `score` field stays correct for this row, not
        // just fast for the common already-scored case.
        assert!(
            (rows[0].1 - real_score * 100.0).abs() < 1e-9,
            "expected the recomputed score {} to match the original scan's {}",
            rows[0].1,
            real_score * 100.0
        );
    }

    #[test]
    fn test_blame_reads_the_index_without_reparsing_every_transcript() {
        // Same shape as the `traces` guard above: `run_blame_command` used to call
        // `Scanner::load_trace` per matched row just to get the outcome rung.
        // `get_session_by_id` is a SQLite lookup, not a disk reparse, so this stays fast
        // against a 3,000-session index even though every source path below is nonexistent.
        let storage = Storage::open_in_memory().unwrap();
        let start = Utc::now();

        const SESSION_COUNT: i64 = 3000;
        for i in 0..SESSION_COUNT {
            let prov = Provenance::new(
                format!("/nonexistent/blame_timing_{}.jsonl", i),
                "claude_code",
                10,
                100,
                format!("fp_blame_timing_{}", i),
            );
            let mut trace = AgentWorthTrace::new(
                format!("sess-blame-timing-{}", i),
                "claude_code",
                prov,
                start + Duration::seconds(i),
            );
            trace.stats.total_events = 2;
            trace.stats.token_usage = TokenUsage::new(100, 20, 0, 0);
            storage
                .upsert_session(&trace, Some("commit_observed"), Some(0.7), 1)
                .unwrap();
        }
        let storage = Arc::new(storage);

        let started = std::time::Instant::now();
        let mut rungs_summed = 0usize;
        for i in 0..SESSION_COUNT {
            let session_id = format!("sess-blame-timing-{}", i);
            let rung = storage
                .get_session_by_id(&session_id)
                .ok()
                .flatten()
                .and_then(|s| s.primary_outcome)
                .as_deref()
                .and_then(outcome_kind_from_str)
                .map(Some)
                .map(outcome_rung)
                .unwrap_or(0);
            rungs_summed += rung;
        }
        let elapsed = started.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "the blame rung lookup took {:?} against {} sessions -- it must read \
             primary_outcome off the index, not reparse every transcript",
            elapsed,
            SESSION_COUNT
        );
        // Every session is "commit_observed" -- rung 4.
        assert_eq!(rungs_summed, 4 * SESSION_COUNT as usize);
    }
}

// -----------------------------------------------------------------------------
// Command: repo list
// -----------------------------------------------------------------------------

/// The repositories this index holds. `repo` is not a stored column -- it is derived from
/// `sessions.source_path` (`extract_repository_or_workspace`), which is why the count comes
/// from `Storage::get_top_repositories` rather than a `GROUP BY`.
fn run_repo_list_command(
    limit: usize,
    json: bool,
    db_path: Option<PathBuf>,
    ui: &crate::ui::Ui,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let ranked = storage.get_top_repositories()?;
    let total_sessions: usize = ranked.iter().map(|(_, n)| *n).sum();

    if json {
        let rows: Vec<_> = ranked
            .iter()
            .take(limit)
            .map(|(repo, sessions)| json!({ "repo": repo, "sessions": sessions }))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "total_repos": ranked.len(),
                "total_sessions": total_sessions,
                "repos": rows,
            }))?
        );
        return Ok(());
    }

    print!("{}", build_repo_list_view(&ranked, total_sessions, limit, "", ui).0);
    Ok(())
}

/// `archie repo list` as a string, plus the repository behind each row so the cockpit's
/// cursor knows what it is pointing at. `filter` is the cockpit's `/`; the command passes
/// an empty one, which keeps every row.
fn build_repo_list_view(
    ranked: &[(String, usize)],
    total_sessions: usize,
    limit: usize,
    filter: &str,
    ui: &crate::ui::Ui,
) -> (String, Vec<String>) {
    let needle = filter.to_lowercase();
    let kept: Vec<(String, usize)> = ranked
        .iter()
        .filter(|(repo, _)| needle.is_empty() || repo.to_lowercase().contains(&needle))
        .take(limit)
        .cloned()
        .collect();
    let rows: Vec<crate::ui::views::RepoListRow<'_>> = kept
        .iter()
        .map(|(repo, sessions)| crate::ui::views::RepoListRow {
            repo,
            sessions: *sessions,
        })
        .collect();
    let text = crate::ui::views::repo_list(
        ui,
        &crate::ui::views::RepoListView {
            total_repos: ranked.len(),
            total_sessions,
            rows: &rows,
        },
    );
    (text, kept.iter().map(|(repo, _)| repo.clone()).collect())
}

// -----------------------------------------------------------------------------
// Command: agent show
// -----------------------------------------------------------------------------

fn run_agent_show_command(
    adapter_name: &str,
    json: bool,
    db_path: Option<PathBuf>,
    ui: &crate::ui::Ui,
) -> Result<()> {
    let adapters = agentworth_adapters::all_adapters();
    let adapter = adapters
        .iter()
        .find(|a| a.name() == adapter_name)
        .with_context(|| {
            let names: Vec<&str> = adapters.iter().map(|a| a.name()).collect();
            format!(
                "no adapter named '{adapter_name}'. Registered adapters: {}",
                names.join(", ")
            )
        })?;

    let caps = adapter.capabilities();
    let detected = adapter
        .detect(&ScanOptions::default())
        .map(|d| d.is_present)
        .unwrap_or(false);
    let source_root = adapter_source_root(adapter_name);

    let storage = open_storage(db_path)?;
    let stats = storage.get_aggregate_stats(false)?;
    let indexed_sessions = stats
        .sessions_by_adapter
        .get(adapter_name)
        .copied()
        .unwrap_or(0);

    let sessions = storage.list_sessions_filtered(&SessionFilter {
        adapter: Some(adapter_name.to_string()),
        order_by: Some(SessionOrderBy::StartedAtDesc),
        ..Default::default()
    })?;
    let indexed_tokens: u64 = sessions.iter().map(|s| s.total_tokens).sum();
    let mut models: Vec<String> = sessions
        .iter()
        .flat_map(|s| s.models_used.iter().cloned())
        .collect();
    models.sort();
    models.dedup();

    let capabilities: [(&str, bool); 7] = [
        ("prompts", caps.prompts),
        ("tokens", caps.tokens),
        ("tools", caps.tools),
        ("shell", caps.shell),
        ("diffs", caps.diffs),
        ("thinking", caps.thinking),
        ("outcomes", caps.outcomes),
    ];

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "adapter": adapter_name,
                "source_root": source_root,
                "is_detected": detected,
                "extraction": {
                    "prompts": caps.prompts,
                    "tokens": caps.tokens,
                    "tools": caps.tools,
                    "shell": caps.shell,
                    "diffs": caps.diffs,
                    "thinking": caps.thinking,
                    "outcomes": caps.outcomes,
                },
                "indexed_sessions": indexed_sessions,
                "indexed_tokens": indexed_tokens,
                "models": models,
            }))?
        );
        return Ok(());
    }

    print!(
        "{}",
        crate::ui::views::agent_show(
            ui,
            &crate::ui::views::AgentShowView {
                adapter: adapter_name,
                source_root,
                detected,
                capabilities: &capabilities,
                indexed_sessions,
                indexed_tokens,
                models: &models,
            }
        )
    );
    Ok(())
}

// -----------------------------------------------------------------------------
// Command: window list
// -----------------------------------------------------------------------------

/// Recent rolling windows.
///
/// Read the simplest correct way rather than through a new bucketing query in storage: one
/// bounded `list_sessions_filtered` over exactly the span the requested windows cover, then
/// bucketed here. The span is `hours * limit` wide, so this reads recent rows, never the
/// index. Anchored on the newest indexed session the same way `Storage::get_pacing_window`
/// anchors, so a machine that has been idle for a day still shows its last active windows.
fn run_window_list_command(
    hours: i64,
    limit: usize,
    json: bool,
    db_path: Option<PathBuf>,
    ui: &crate::ui::Ui,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let hours = hours.max(1);
    let limit = limit.clamp(1, 200);

    let Some((anchor, rows)) = window_buckets(&storage, hours, limit)? else {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({ "window_hours": hours, "windows": [] }))?
            );
        } else {
            print!("{}", empty_window_list_view(hours, ui));
        }
        return Ok(());
    };
    if json {
        let out: Vec<_> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let end = anchor - chrono::Duration::hours(hours * i as i64);
                json!({
                    "started_at": (end - chrono::Duration::hours(hours)).to_rfc3339(),
                    "ended_at": end.to_rfc3339(),
                    "session_count": r.sessions,
                    "total_tokens": r.total_tokens,
                    "estimated_cost_usd": r.estimated_cost_usd,
                    "burn_rate_tokens_per_hour": r.burn_rate_tokens_per_hour,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "window_hours": hours,
                "anchored_at": anchor.to_rfc3339(),
                "windows": out,
            }))?
        );
        return Ok(());
    }

    let anchor_label = anchor.format("%b %e %H:%M").to_string();
    print!(
        "{}",
        crate::ui::views::window_list(
            ui,
            &crate::ui::views::WindowListView {
                hours,
                anchor: &anchor_label,
                rows: &rows,
            }
        )
    );
    Ok(())
}

/// The windows themselves, anchored and bucketed. `None` when nothing is indexed.
///
/// One bucketing pass, three readers: the command's table, its `--json`, and the cockpit's
/// windows screen. The span read is `hours * limit` wide, so this reads recent rows and
/// never the whole index.
fn window_buckets(
    storage: &Arc<Storage>,
    hours: i64,
    limit: usize,
) -> Result<Option<(chrono::DateTime<chrono::Utc>, Vec<crate::ui::views::WindowListRow>)>> {
    let newest = storage.list_sessions_filtered(&SessionFilter {
        limit: Some(1),
        order_by: Some(SessionOrderBy::StartedAtDesc),
        ..Default::default()
    })?;
    let Some(anchor) = newest.first().map(|s| s.started_at) else {
        return Ok(None);
    };

    let span = chrono::Duration::hours(hours * limit as i64);
    let sessions = storage.list_sessions_filtered(&SessionFilter {
        start_date: Some(anchor - span),
        order_by: Some(SessionOrderBy::StartedAtDesc),
        ..Default::default()
    })?;

    let mut buckets: Vec<(usize, u64, f64)> = vec![(0, 0, 0.0); limit];
    for s in &sessions {
        let elapsed_hours = (anchor - s.started_at).num_seconds() as f64 / 3600.0;
        let index = (elapsed_hours / hours as f64).floor().max(0.0) as usize;
        if index >= limit {
            continue;
        }
        buckets[index].0 += 1;
        buckets[index].1 += s.total_tokens;
        // Priced from the same per-model usage rows `stats usage` prices from, so the two
        // surfaces cannot disagree about what a window cost.
        let per_model: std::collections::BTreeMap<String, agentworth_schema::TokenUsage> = storage
            .get_session_model_usage(&s.session_id)
            .unwrap_or_default()
            .into_iter()
            .collect();
        buckets[index].2 += agentworth_storage::estimate_total_cost_from_per_model_usage(&per_model);
    }

    let rows = buckets
        .iter()
        .enumerate()
        .map(|(i, (sessions, tokens, cost))| {
            let end = anchor - chrono::Duration::hours(hours * i as i64);
            let start = end - chrono::Duration::hours(hours);
            crate::ui::views::WindowListRow {
                label: format!("{} to {}", start.format("%b %e %H:%M"), end.format("%H:%M")),
                sessions: *sessions,
                total_tokens: *tokens,
                estimated_cost_usd: *cost,
                burn_rate_tokens_per_hour: *tokens as f64 / hours as f64,
            }
        })
        .collect();
    Ok(Some((anchor, rows)))
}

fn empty_window_list_view(hours: i64, ui: &crate::ui::Ui) -> String {
    crate::ui::views::window_list(
        ui,
        &crate::ui::views::WindowListView {
            hours,
            anchor: "nothing indexed",
            rows: &[],
        },
    )
}

/// `archie window list` as a string, for the cockpit's windows screen.
fn build_window_list_view(
    storage: &Arc<Storage>,
    hours: i64,
    limit: usize,
    ui: &crate::ui::Ui,
) -> Result<String> {
    let hours = hours.max(1);
    let limit = limit.clamp(1, 200);
    Ok(match window_buckets(storage, hours, limit)? {
        Some((anchor, rows)) => crate::ui::views::window_list(
            ui,
            &crate::ui::views::WindowListView {
                hours,
                anchor: &anchor.format("%b %e %H:%M").to_string(),
                rows: &rows,
            },
        ),
        None => empty_window_list_view(hours, ui),
    })
}

// -----------------------------------------------------------------------------
// Command: stats outcomes
// -----------------------------------------------------------------------------

/// `outcome_rate` on the CLI. The aggregate itself already exists in storage and behind the
/// MCP tool; this is the surface a person can type.
#[allow(clippy::too_many_arguments)]
fn run_stats_outcomes_command(
    by: &str,
    min_n: Option<usize>,
    since: Option<chrono::DateTime<chrono::Utc>>,
    until: Option<chrono::DateTime<chrono::Utc>>,
    include_stubs: bool,
    json: bool,
    db_path: Option<PathBuf>,
    ui: &crate::ui::Ui,
) -> Result<()> {
    use agentworth_storage::OutcomeRateGroupBy;

    let storage = open_storage(db_path)?;
    let group_by = match by {
        "adapter" => OutcomeRateGroupBy::Adapter,
        "repo" => OutcomeRateGroupBy::Repo,
        _ => OutcomeRateGroupBy::Model,
    };
    // The same floor `outcome_rate` uses over MCP (docs/specs/verified-outcome-rate.md).
    let min_n = min_n.unwrap_or(20);
    let result = storage.get_outcome_rate(group_by, since, until, min_n, include_stubs)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    let rows: Vec<crate::ui::views::StatsOutcomesRow<'_>> = result
        .rows
        .iter()
        .map(|r| crate::ui::views::StatsOutcomesRow {
            key: &r.key,
            n: r.n,
            verified: r.verified,
            rate: r.rate,
            reason: r.reason.as_deref(),
        })
        .collect();

    let counted_at = result.receipt.counted_at.format("%Y-%m-%d %H:%M UTC").to_string();
    print!(
        "{}",
        crate::ui::views::stats_outcomes(
            ui,
            &crate::ui::views::StatsOutcomesView {
                group_by: by,
                min_n: result.min_n,
                baseline_n: result.baseline.n,
                baseline_rate: result.baseline.rate,
                suppressed_groups: result.suppressed_groups,
                rows: &rows,
                counted_at: &counted_at,
            }
        )
    );
    Ok(())
}

/// Where an adapter keeps its history by default. One table, shared by `agent list` and
/// `agent show`, so the two can't disagree about where to look.
fn adapter_source_root(name: &str) -> &'static str {
    match name {
        "aider" => "~/.aider* / chat history",
        "claude_code" => "~/.claude/projects/",
        "cline" => "~/.config/Code/.../cline/",
        "codex" => "~/.codex/sessions/",
        "cursor" => "~/.cursor/ / workspaceStorage",
        "deepseek" => "~/.deepseek/",
        "gemini" => "~/.gemini/ / antigravity",
        "goose" => "~/.config/goose/sessions/",
        "grok" => "~/.grok/ / ~/.xai/",
        "herdr" => "~/.herdr/",
        "hermes" => "~/.hermes/",
        "kimi" => "~/.kimi/",
        "manus" => "~/.manus/",
        "minimax" => "~/.minimax/",
        "openclaw" => "~/.openclaw/",
        "opencode" => "~/.opencode/",
        "pi" => "~/.pi/",
        "qwen" => "~/.qwen/",
        "windsurf" => "~/.codeium/windsurf/",
        "zhipu" => "~/.zhipu/ / codegeex/",
        _ => "~/.<agent>/",
    }
}


// -----------------------------------------------------------------------------
// The cockpit
// -----------------------------------------------------------------------------

/// A bare `archie`, and `archie tui`.
///
/// The spec's rule, and the answer to its open question 4: on a real terminal this opens
/// the cockpit; off one, or under `--plain`, `TERM=dumb` or JSON output, it prints the
/// overview and exits 0. Nothing here writes -- the cockpit is read-only, permanently.
fn run_cockpit_command(json: bool, db_path: Option<PathBuf>, ui: &crate::ui::Ui) -> Result<()> {
    use crate::ui::cockpit;

    // `Storage::open_path` creates the file it cannot find, so "no index" is a count of
    // zero rather than a failed open -- an open that does fail is something else (a
    // permission, a corrupt file) and is reported the same way, with its own text.
    let (storage, missing) = match open_storage(db_path) {
        Ok(s) => {
            let empty = s
                .get_aggregate_stats(true)
                .map(|a| a.total_sessions == 0)
                .unwrap_or(true);
            (Some(s), if empty { Some(String::new()) } else { None })
        }
        Err(e) => (None, Some(e.to_string())),
    };

    if let Some(detail) = missing {
        // Byte for byte the screen `archie session list` prints against an empty index.
        let text = crate::ui::views::error(
            ui,
            "archie",
            cockpit::NO_INDEX_LINE,
            &detail,
            &[],
            &[(
                "archie scan".to_string(),
                "discover and index agent histories".to_string(),
            )],
        );
        if cockpit::should_open(ui, json) {
            return cockpit::run_message(&text);
        }
        print!("{}", text);
        return Ok(());
    }

    let storage = storage.expect("an index that is not missing is open");
    let screens = CockpitScreens {
        storage: storage.clone(),
        ui: *ui,
    };

    if json {
        let stats = storage.get_aggregate_stats(false)?;
        let verdict = compute_verdict_breakdown(&storage, stats.total_sessions);
        let window = storage.get_pacing_window(cockpit::WINDOW_HOURS).ok();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "total_sessions": stats.total_sessions,
                "total_events": stats.total_events,
                "verified_rate": verdict.real_verified_rate,
                "total_tokens": stats.token_usage.total(),
                "window": window.map(|w| json!({
                    "hours": w.window_hours,
                    "session_count": w.session_count,
                    "total_tokens": w.total_tokens,
                    "burn_rate_tokens_per_hour": w.burn_rate_tokens_per_hour,
                    "estimated_cost_usd": w.estimated_cost_usd,
                })),
            }))?
        );
        return Ok(());
    }

    if cockpit::should_open(ui, json) {
        return cockpit::run(ui, &screens);
    }
    print!("{}", crate::ui::cockpit::Screens::overview(&screens)?);
    Ok(())
}

/// Every cockpit screen, fetched the way its printed command fetches it.
///
/// This is the whole of the cockpit's contact with storage: each method calls a builder the
/// CLI already uses, so a screen cannot drift from the command that prints the same thing.
struct CockpitScreens {
    storage: Arc<Storage>,
    ui: crate::ui::Ui,
}

impl crate::ui::cockpit::Screens for CockpitScreens {
    fn overview(&self) -> Result<String> {
        let ui = &self.ui;
        let stats = self.storage.get_aggregate_stats(false)?;
        let verdict = compute_verdict_breakdown(&self.storage, stats.total_sessions);
        let db = self
            .storage
            .db_path()
            .map(|p| p.to_string_lossy().to_string());
        let view = stats_view(&stats, &verdict, db.as_deref(), false);

        let window = self
            .storage
            .get_pacing_window(crate::ui::cockpit::WINDOW_HOURS)
            .ok()
            .map(|p| crate::ui::views::OverviewWindow {
                hours: p.window_hours,
                span: format!(
                    "{} {} {}",
                    p.started_at.format("%Y-%m-%d %H:%M"),
                    ui.arrow(),
                    p.ended_at.format("%H:%M")
                ),
                sessions: p.session_count,
                events: p.total_events,
                tokens: p.total_tokens,
                burn_rate_tokens_per_hour: p.burn_rate_tokens_per_hour,
                cache_hit_ratio: p.cache_hit_ratio,
                estimated_cost_usd: p.estimated_cost_usd,
            });
        Ok(crate::ui::views::overview(ui, &view, window.as_ref()))
    }

    fn sessions(&self, filter: &str) -> Result<(String, Vec<String>)> {
        let limit = crate::ui::cockpit::LIST_LIMIT;
        let sessions = self.storage.list_sessions_filtered(&SessionFilter {
            limit: None,
            order_by: Some(SessionOrderBy::StartedAtDesc),
            ..Default::default()
        })?;
        let needle = filter.to_lowercase();
        let kept: Vec<_> = sessions
            .into_iter()
            .filter(|s| s.total_events > 1)
            .filter(|s| {
                needle.is_empty()
                    || s.session_id.to_lowercase().contains(&needle)
                    || s.adapter.to_lowercase().contains(&needle)
                    || s.source_path.to_lowercase().contains(&needle)
                    || s
                        .models_used
                        .iter()
                        .any(|m| m.to_lowercase().contains(&needle))
            })
            .take(limit)
            .collect();

        let rows = build_trace_rows(&self.storage, kept);
        let ids: Vec<String> = rows
            .iter()
            .map(|(_, _, _, s)| s.session_id.clone())
            .collect();
        let indexed = self
            .storage
            .get_aggregate_stats(false)
            .map(|s| s.total_sessions)
            .unwrap_or(rows.len());
        Ok((build_traces_view(&rows, indexed, limit, &self.ui), ids))
    }

    fn session(&self, id: &str, tab: crate::ui::cockpit::Tab) -> Result<String> {
        use crate::ui::cockpit::Tab;
        match tab {
            Tab::Show => {
                let scanner = Scanner::new(self.storage.clone());
                let trace = scanner.load_trace(id)?;
                Ok(inspect_view(&trace))
            }
            Tab::Handoff => handoff_command::view_for(&self.storage, &self.ui, id),
            Tab::Asks => asks_command::view_for(&self.storage, &self.ui, id),
            Tab::Forgotten => forgotten_command::view_for(&self.storage, &self.ui, id),
            Tab::Receipt => {
                let scanner = Scanner::new(self.storage.clone());
                let trace = scanner.load_trace(id)?;
                let score = agentworth_scoring::TraceScorer::default().score(&trace);
                Ok(crate::commands::receipt::render_terminal_receipt_with(
                    &trace, &score, &self.ui,
                ))
            }
        }
    }

    fn agents(&self) -> Result<String> {
        let (rows_data, coverage) = matrix_rows_data();
        Ok(build_matrix_view(&rows_data, &coverage, &self.ui))
    }

    fn repos(&self, filter: &str) -> Result<(String, Vec<String>)> {
        let ranked = self.storage.get_top_repositories()?;
        let total_sessions: usize = ranked.iter().map(|(_, n)| *n).sum();
        Ok(build_repo_list_view(
            &ranked,
            total_sessions,
            crate::ui::cockpit::LIST_LIMIT,
            filter,
            &self.ui,
        ))
    }

    fn windows(&self) -> Result<String> {
        build_window_list_view(
            &self.storage,
            crate::ui::cockpit::WINDOW_HOURS,
            crate::ui::cockpit::WINDOW_COUNT,
            &self.ui,
        )
    }
}


#[cfg(test)]
mod grammar_tests {
    use super::*;
    use clap::Parser;

    fn action(argv: &[&str]) -> Action {
        let mut full = vec!["agentworth"];
        full.extend_from_slice(argv);
        normalize(
            Cli::try_parse_from(full)
                .unwrap_or_else(|e| panic!("{argv:?} did not parse: {e}"))
                .command
                .unwrap_or_else(|| panic!("{argv:?} parsed to no subcommand")),
        )
    }

    /// Every hidden alias, and the noun-verb spelling it has to be indistinguishable from.
    /// One table rather than one test per command: a command added to the grammar without a
    /// row here is caught by `every_hidden_command_has_a_row` below, and a row that stops
    /// agreeing is caught here.
    const ALIAS_PAIRS: &[(&[&str], &[&str])] = &[
        (&["traces", "--limit", "5"], &["session", "list", "--limit", "5"]),
        (&["inspect", "abc123"], &["session", "show", "abc123"]),
        (&["inspect", "--last", "--json"], &["session", "show", "--last", "--json"]),
        (
            &["export", "abc", "--format", "atif"],
            &["session", "export", "abc", "--format", "atif"],
        ),
        (
            &["receipt", "abc", "--format", "svg"],
            &["session", "receipt", "abc", "--format", "svg"],
        ),
        (&["handoff", "abc", "--redact"], &["session", "handoff", "abc", "--redact"]),
        (
            &["forgotten", "abc", "--round", "2"],
            &["session", "forgotten", "abc", "--round", "2"],
        ),
        (&["loose-ends", "abc"], &["session", "loose-ends", "abc"]),
        (&["asks", "--session", "abc"], &["session", "asks", "--session", "abc"]),
        (&["cache-doctor", "abc"], &["session", "cache", "abc"]),
        (&["bisect", "abc"], &["session", "bisect", "abc"]),
        (&["search", "a query"], &["session", "search", "a query"]),
        (&["recall", "a query"], &["session", "recall", "a query"]),
        (&["audit", "--safety"], &["session", "audit", "--safety"]),
        (&["blunder", "--top", "3"], &["session", "blunder", "--top", "3"]),
        (&["autopsy"], &["session", "autopsy"]),
        (&["watch", "--poll-once"], &["session", "watch", "--poll-once"]),
        (
            &["threat-digest", "--min-severity", "high"],
            &["session", "risk", "--min-severity", "high"],
        ),
        (&["blind-spots"], &["session", "list", "--unproven"]),
        (
            &["blind-spots", "--limit", "7", "--json"],
            &["session", "list", "--unproven", "--limit", "7", "--json"],
        ),
        (&["matrix", "--json"], &["agent", "list", "--json"]),
        (&["blame", "src/lib.rs"], &["repo", "blame", "src/lib.rs"]),
        (&["pr-blame", "src/lib.rs"], &["repo", "pr-blame", "src/lib.rs"]),
        (&["suspect", "--quiet"], &["repo", "suspect", "--quiet"]),
        (&["blunder-blame", "--last"], &["repo", "blunder-blame", "--last"]),
        (&["usage", "--period", "week"], &["stats", "usage", "--period", "week"]),
    ];

    #[test]
    fn every_hidden_alias_dispatches_to_its_new_spelling() {
        for (old, new) in ALIAS_PAIRS {
            assert_eq!(
                action(old),
                action(new),
                "`{}` and `{}` must reach the same handler",
                old.join(" "),
                new.join(" ")
            );
        }
    }

    /// The alias table and the clap tree have to describe the same set. Without this, a
    /// hidden variant could quietly exist with nothing documenting what replaced it.
    #[test]
    fn every_hidden_command_has_a_row() {
        let mut cmd = cli_command();
        cmd.build();
        let hidden: Vec<String> = cmd
            .get_subcommands()
            .filter(|s| s.is_hide_set() && s.get_name() != "help")
            .map(|s| s.get_name().to_string())
            .collect();

        for name in &hidden {
            assert!(
                OLD_CLI_SPELLINGS.iter().any(|(old, _)| old == name),
                "hidden command `{name}` has no row in OLD_CLI_SPELLINGS"
            );
        }
        for (old, _) in OLD_CLI_SPELLINGS {
            assert!(
                hidden.iter().any(|n| n == old),
                "OLD_CLI_SPELLINGS lists `{old}`, which is not a hidden command"
            );
        }
        assert!(
            ALIAS_PAIRS.len() >= hidden.len(),
            "every hidden command needs at least one ALIAS_PAIRS row"
        );
    }

    /// The nouns and the machine-level commands stay visible. A `hide = true` landing on one
    /// of these by accident would empty out `--help` without failing anything else.
    #[test]
    fn the_noun_tree_and_the_top_level_are_visible() {
        let mut cmd = cli_command();
        cmd.build();
        let visible: Vec<String> = cmd
            .get_subcommands()
            .filter(|s| !s.is_hide_set())
            .map(|s| s.get_name().to_string())
            .collect();

        for expected in [
            "session", "agent", "repo", "window", "stats", "scan", "serve", "mcp", "doctor",
            "docs", "config", "version", "update", "completions", "merge", "tui",
        ] {
            assert!(
                visible.iter().any(|n| n == expected),
                "`{expected}` should be visible in --help; visible: {visible:?}"
            );
        }
    }

    /// A bare `archie` is the cockpit, and `archie tui` is the same thing said out loud.
    /// Both must reach `Action::Tui`; the difference between them is a terminal check, not
    /// a different command.
    #[test]
    fn a_bare_invocation_and_the_tui_verb_are_the_same_action() {
        let bare = Cli::try_parse_from(["agentworth"]).expect("a bare archie must parse");
        assert!(bare.command.is_none(), "a bare archie should take no subcommand");
        assert!(matches!(action(&["tui"]), Action::Tui(_)));
        assert!(matches!(action(&["tui", "--json"]), Action::Tui(TuiArgs { json: true })));
    }

    /// `--unproven` is a filter on the listing, not a second listing wearing a flag, so it
    /// must refuse the filters it would silently ignore.
    #[test]
    fn unproven_refuses_the_filters_it_would_ignore() {
        assert!(Cli::try_parse_from(["agentworth", "session", "list", "--unproven", "--adapter", "codex"]).is_err());
        assert!(Cli::try_parse_from(["agentworth", "session", "list", "--unproven", "--model", "sonnet"]).is_err());
    }

    /// `window show` and the `usage --pacing` flag it replaced both end at the pacing path.
    #[test]
    fn window_show_and_usage_pacing_agree_on_the_window() {
        let Action::Stats { action: Some(StatsCommand::Usage(usage)), .. } =
            action(&["usage", "--pacing", "--hours", "3"])
        else {
            panic!("`usage --pacing` should normalize to stats usage");
        };
        let Action::Window(WindowCommand::Show(window)) = action(&["window", "show", "--hours", "3"])
        else {
            panic!("`window show` should normalize to the window noun");
        };
        assert!(usage.pacing);
        assert_eq!(usage.hours, window.hours);
    }

    #[test]
    fn the_new_verbs_parse() {
        assert!(matches!(
            action(&["agent", "show", "claude_code"]),
            Action::Agent(AgentCommand::Show(_))
        ));
        assert!(matches!(action(&["repo", "list"]), Action::Repo(RepoCommand::List(_))));
        assert!(matches!(
            action(&["window", "list", "--hours", "2"]),
            Action::Window(WindowCommand::List(_))
        ));
        assert!(matches!(
            action(&["stats", "outcomes", "--by", "adapter"]),
            Action::Stats { action: Some(StatsCommand::Outcomes(_)), .. }
        ));
        assert!(matches!(action(&["stats"]), Action::Stats { action: None, .. }));
    }
}
