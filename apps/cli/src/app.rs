use std::path::PathBuf;
use std::sync::Arc;

use agentworth_adapter_sdk::ScanOptions;
use agentworth_core::{ScanSummary, Scanner};
use agentworth_outcomes::evaluate_trace_outcomes;
use agentworth_storage::{SessionFilter, SessionOrderBy, Storage};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
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
#[path = "commands/config.rs"]
mod config;
#[path = "commands/version_info.rs"]
mod version_info;

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
        help = "Force text output even if persisted config defaults to JSON (see `agentworth config`)"
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

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Scan and index agent histories from the local system
    Scan {
        /// Optional specific paths or directories to scan
        #[arg(value_name = "PATHS")]
        paths: Vec<PathBuf>,

        /// Force rescanning and re-indexing of unchanged source files
        #[arg(short, long)]
        force: bool,

        /// Output scan results as formatted JSON
        #[arg(long)]
        json: bool,
    },

    /// Show machine-wide summary statistics of indexed traces
    Stats {
        /// Output summary statistics as formatted JSON
        #[arg(long)]
        json: bool,
    },

    /// List indexed traces with optional filtering
    Traces {
        /// Maximum number of traces to display (default 20, or persisted `config limit`)
        #[arg(short, long)]
        limit: Option<usize>,

        /// Filter by adapter name (e.g. claude_code, codex, gemini, opencode)
        #[arg(short, long)]
        adapter: Option<String>,

        /// Filter by model substring (e.g. sonnet, gpt-4o, gemini-2.5)
        #[arg(short, long)]
        model: Option<String>,

        /// Include 1-event session stubs in the listing
        #[arg(long)]
        all_stubs: bool,

        /// Output traces as formatted JSON
        #[arg(long)]
        json: bool,
    },

    /// Display extraction capabilities and coverage matrix across all 20 agent adapters
    Matrix {
        /// Output matrix as formatted JSON
        #[arg(long)]
        json: bool,
    },

    /// Inspect a specific trace session in detail with timeline visualization
    Inspect {
        /// The session ID to inspect
        session_id: String,

        /// Output raw trace structure as formatted JSON
        #[arg(long)]
        json: bool,
    },

    /// Export a trace session safely in JSON or ATIF format
    Export {
        /// The session ID to export
        session_id: String,

        /// Apply redaction to mask secrets, API keys, tokens, emails, and home paths
        #[arg(short, long)]
        redact: bool,

        /// Export format: json (default), atif, receipt, or svg
        #[arg(short, long, default_value = "json", value_parser = ["json", "atif", "receipt", "terminal", "ansi", "svg"])]
        format: String,

        /// Optional file path to write export output to (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Generate and render an authentic ANSI or SVG Flight Receipt for a trace session
    Receipt {
        /// The session ID to generate flight receipt for
        session_id: String,

        /// Output format: terminal (default), ansi, svg, receipt, or json
        #[arg(short, long, default_value = "terminal", value_parser = ["terminal", "ansi", "svg", "receipt", "json"])]
        format: String,

        /// Optional file path to write receipt or SVG output to (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },


    /// Semantic vector search across indexed trajectory turns with ASCII thermal receipts
    Search {
        /// Search query (natural language or code snippet)
        query: String,

        /// Maximum number of results to return (default 10, or persisted `config limit`)
        #[arg(short, long)]
        limit: Option<usize>,

        /// Minimum similarity score threshold (0.0 to 1.0)
        #[arg(long, default_value_t = 0.0)]
        min_score: f32,

        /// Filter by chunk kind (summary, error_recovery, tool_invocation, apology_panic, code_lineage)
        #[arg(short, long)]
        kind: Option<String>,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Safety and threat audit detecting forbidden commands, leaked variables, sweeps, and fake claims
    Audit {
        /// Restrict audit to safety and threat vectors only
        #[arg(long)]
        safety: bool,

        /// Output audit results as formatted JSON
        #[arg(long)]
        json: bool,
    },

    /// Discover top agent blunders, render thermal receipts, and export to the Hall of Blunders
    Blunder {
        /// Number of top blunder exhibits to retrieve and display (default: 5)
        #[arg(short, long, default_value_t = 5)]
        top: usize,

        /// Submit redacted blunder receipts to the public Hall of Blunders at stfuopus.lol
        #[arg(short, long)]
        submit: bool,

        /// Output blunder exhibits as formatted JSON
        #[arg(long)]
        json: bool,
    },

    /// Start the local API server and interactive explorer UI
    Serve {
        /// Port to bind the server to
        #[arg(short, long, default_value_t = crate::DEFAULT_PORT)]
        port: u16,

        /// Automatically open the Web UI in the default browser
        #[arg(long)]
        open: bool,

        /// Optional path to custom web frontend dist directory
        #[arg(long)]
        dist: Option<PathBuf>,
    },

    /// Start the read-only MCP server over stdio, for a coding agent to query this machine's
    /// session index mid-session (see docs/specs/mcp-server.md). Register it once with
    /// `claude mcp add agentworth --scope user -- agentworth mcp`.
    Mcp,

    /// View deep usage, pacing, and token expenditure rollups
    Usage {
        /// Rollup period: day, week, or month (default day, or persisted `config period`)
        #[arg(short, long, value_parser = ["day", "week", "month"])]
        period: Option<String>,

        /// Show 5-hour rolling pacing window (burn rate, active models, quota headroom)
        #[arg(long)]
        pacing: bool,

        /// Pacing window duration in hours
        #[arg(long, default_value_t = 5)]
        hours: i64,

        /// Alert and highlight if window spend exceeds this threshold in USD
        #[arg(long)]
        alert_above: Option<f64>,

        /// Maximum number of rows to display (default 20, or persisted `config limit`)
        #[arg(short, long)]
        limit: Option<usize>,

        /// Group the rollup by model instead of adapter (e.g. how many tokens
        /// each of claude-opus-5 / claude-sonnet-5 / claude-fable-5 used)
        #[arg(long)]
        by_model: bool,

        /// Output usage data as JSON
        #[arg(long)]
        json: bool,
    },

    /// Trace file modifications back to the AI agent session, model, and prompt that authored them
    Blame {
        /// Target file path or pattern to search
        file_path: String,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Check local environment, adapter discoveries, and SQLite database health
    Doctor {
        /// Output diagnostic report as formatted JSON
        #[arg(long)]
        json: bool,
    },

    /// Print version details: binary version, npm install detection, and a live
    /// check for a newer release
    Version {
        /// Skip the live GitHub-releases update check (fully local, no network call)
        #[arg(long)]
        offline: bool,

        /// Output as formatted JSON
        #[arg(long)]
        json: bool,
    },

    /// Check for a newer AgentWorth release and show exactly how to get it
    Update {
        /// Skip the live GitHub-releases check and just show install-method guidance
        #[arg(long)]
        offline: bool,

        /// Output as formatted JSON
        #[arg(long)]
        json: bool,
    },

    /// Merge another local SQLite index database into this index
    Merge {
        /// Path to the source SQLite database file to merge from
        source_db: PathBuf,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Watch active session transcripts and detect doom loops or file edit thrashing
    Watch {
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
    },

    /// Diagnose turn-by-turn prompt caching dynamics and identify cache drop root causes
    #[command(name = "cache-doctor")]
    CacheDoctor {
        /// Target session ID to inspect
        session_id: String,

        /// Output findings as formatted JSON
        #[arg(long)]
        json: bool,
    },

    /// List sessions whose completion claims were never independently corroborated by tests or CI
    #[command(name = "blind-spots")]
    BlindSpots {
        /// Maximum number of sessions to list (default 20, or persisted `config limit`)
        #[arg(short, long)]
        limit: Option<usize>,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Rank indexed sessions by real secret/credential exposure risk, by category and severity
    #[command(name = "threat-digest")]
    ThreatDigest {
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
    },

    /// Surface recurring human correction and steering phrases across all sessions
    Autopsy {
        /// Minimum number of occurrences across sessions to report (default: 2)
        #[arg(short, long, default_value_t = 2)]
        min_occurrences: usize,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Semantically recall past solutions joined with outcome validation and cost
    Recall {
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
    },

    /// Pinpoint the exact turning point where an agent session trajectory turned negative
    Bisect {
        /// Session ID to bisect
        session_id: String,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Annotate changed PR files with AI agent authoring provenance and outcome validation
    #[command(name = "pr-blame")]
    PrBlame {
        /// List of files to check (if omitted, infers from git diff)
        files: Vec<String>,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Bridge AI Code Blame with the Hall of Blunders: trace a recorded blunder forward
    /// to the exact files it blame-attributes to, or a file's blame history backward to
    /// any recorded blunders in the sessions blamed for it
    #[command(name = "blunder-blame")]
    BlunderBlame {
        /// Blame -> blunder direction: file path or pattern. Checks every session AI
        /// Code Blame attributes this file to for a recorded blunder
        #[arg(long, conflicts_with = "session")]
        file: Option<String>,

        /// Blunder -> blame direction: one specific session ID. Resolves it to the
        /// files AI Code Blame attributes to that session
        #[arg(long, conflicts_with = "file")]
        session: Option<String>,

        /// In default mode (no --file or --session), number of top blunders to bridge
        #[arg(short, long, default_value_t = 5)]
        top: usize,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Get, set, or list persisted CLI defaults (~/.agentworth/config.toml)
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigAction {
    /// List every persisted config key and its current value
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Print the persisted value for one config key
    Get {
        /// Config key: json, limit, or period
        key: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Persist a default value for one config key
    Set {
        /// Config key: json, limit, or period
        key: String,

        /// Value to store (json: true/false, limit: a number, period: day/week/month)
        value: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("warn")
    };
    // `agentworth mcp` speaks JSON-RPC over stdout -- any stray tracing line there would
    // corrupt the protocol stream for whatever client spawned this process (the same reason
    // every rmcp stdio example logs to stderr). Every other subcommand keeps the existing
    // stdout default.
    if matches!(cli.command, Commands::Mcp) {
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

    // Persisted user defaults (`~/.agentworth/config.toml`, see `agentworth config`). A
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

    match cli.command {
        Commands::Scan { paths, force, json } => {
            run_scan_command(paths, force, resolve_json(json), cli.db_path, &ui)?;
        }
        Commands::Stats { json } => {
            run_stats_command(resolve_json(json), cli.db_path, &ui)?;
        }
        Commands::Doctor { json } => {
            run_doctor_command(resolve_json(json), cli.db_path, &ui)?;
        }
        Commands::Version { offline, json } => {
            version_info::run_version_command(resolve_json(json), offline)?;
        }
        Commands::Update { offline, json } => {
            version_info::run_update_command(resolve_json(json), offline)?;
        }
        Commands::Matrix { json } => {
            run_matrix_command(resolve_json(json), cli.db_path, &ui)?;
        }
        Commands::Traces {
            limit,
            adapter,
            model,
            all_stubs,
            json,
        } => {
            let limit = config::resolve_limit(limit, persisted_config.limit, 20);
            run_traces_command(
                limit,
                adapter,
                model,
                all_stubs,
                resolve_json(json),
                cli.db_path,
                &ui,
            )?;
        }
        Commands::Inspect { session_id, json } => {
            let json = resolve_json(json);
            if let Err(e) = run_inspect_command(&session_id, json, cli.db_path.clone()) {
                if json {
                    return Err(e);
                }
                // An error screen is a navigation screen, so it replaces the anyhow dump
                // rather than following it. Every storage handle is already dropped here.
                print!("{}", inspect_not_found(&session_id, cli.db_path, &ui));
                std::process::exit(1);
            }
        }
        Commands::Export {
            session_id,
            redact,
            format,
            output,
        } => {
            run_export_command(&session_id, redact, &format, output.as_deref(), cli.db_path)?;
        }
        Commands::Receipt {
            session_id,
            format,
            output,
        } => {
            crate::run_receipt_command(&session_id, &format, output, cli.db_path, &ui)?;
        }

        Commands::Search {
            query,
            limit,
            min_score,
            kind,
            json,
        } => {
            let limit = config::resolve_limit(limit, persisted_config.limit, 10);
            crate::run_search_command(
                &query,
                limit,
                min_score,
                kind,
                resolve_json(json),
                cli.db_path,
            )?;
        }
        Commands::Audit { safety, json } => {
            crate::run_audit_command(safety, resolve_json(json), cli.db_path)?;
        }
        Commands::Blunder { top, submit, json } => {
            crate::run_blunder_command(top, submit, resolve_json(json), cli.db_path)?;
        }
        Commands::Usage {
            period,
            pacing,
            hours,
            alert_above,
            limit,
            by_model,
            json,
        } => {
            let period = config::resolve_period(period, persisted_config.period.clone(), "day")?;
            let limit = config::resolve_limit(limit, persisted_config.limit, 20);
            run_usage_command(UsageCommandArgs {
                period: &period,
                pacing,
                hours,
                alert_above,
                limit,
                by_model,
                json: resolve_json(json),
                db_path: cli.db_path,
                ui: &ui,
            })?;
        }
        Commands::Blame { file_path, json } => {
            run_blame_command(&file_path, resolve_json(json), cli.db_path, &ui)?;
        }
        Commands::Serve { port, open, dist } => {
            let storage = open_storage(cli.db_path)?;
            let dist_path = crate::server::resolve_dist_dir(dist)?;
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(crate::start_server(storage, port, open, dist_path))?;
        }
        Commands::Mcp => {
            let storage = open_storage(cli.db_path)?;
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(crate::run_mcp_server(storage))?;
        }
        Commands::Merge { source_db, json } => {
            merge::run_merge_command(source_db, resolve_json(json), cli.db_path)?;
        }
        Commands::Watch {
            interval_secs,
            poll_once,
            json,
            paths,
        } => {
            watch::run_watch_command(interval_secs, poll_once, resolve_json(json), paths, cli.db_path)?;
        }
        Commands::CacheDoctor { session_id, json } => {
            cache_doctor::run_cache_doctor_command(&session_id, resolve_json(json), cli.db_path)?;
        }
        Commands::BlindSpots { limit, json } => {
            let limit = config::resolve_limit(limit, persisted_config.limit, 20);
            blind_spots::run_blind_spots_command(limit, resolve_json(json), cli.db_path)?;
        }
        Commands::ThreatDigest {
            limit,
            min_severity,
            json,
        } => {
            let limit = config::resolve_limit(limit, persisted_config.limit, 20);
            threat_digest::run_threat_digest_command(
                limit,
                &min_severity,
                resolve_json(json),
                cli.db_path,
            )?;
        }
        Commands::Autopsy {
            min_occurrences,
            json,
        } => {
            autopsy::run_autopsy_command(min_occurrences, resolve_json(json), cli.db_path)?;
        }
        Commands::Recall {
            query,
            limit,
            min_score,
            json,
        } => {
            let limit = config::resolve_limit(limit, persisted_config.limit, 5);
            recall::run_recall_command(&query, limit, min_score, resolve_json(json), cli.db_path)?;
        }
        Commands::Bisect { session_id, json } => {
            bisect::run_bisect_command(&session_id, resolve_json(json), cli.db_path)?;
        }
        Commands::PrBlame { files, json } => {
            pr_blame::run_pr_blame_command(files, resolve_json(json), cli.db_path)?;
        }
        Commands::BlunderBlame {
            file,
            session,
            top,
            json,
        } => {
            crate::run_blunder_blame_command(
                file,
                session,
                top,
                resolve_json(json),
                cli.db_path,
            )?;
        }
        Commands::Config { action } => match action {
            ConfigAction::List { json } => config::run_config_list(resolve_json(json))?,
            ConfigAction::Get { key, json } => config::run_config_get(&key, resolve_json(json))?,
            ConfigAction::Set { key, value, json } => {
                config::run_config_set(&key, &value, resolve_json(json))?
            }
        },
    }

    Ok(())
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
    json: bool,
    db_path: Option<PathBuf>,
    ui: &crate::ui::Ui,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let scanner = Scanner::new(storage.clone());
    let options = ScanOptions {
        custom_paths: paths,
        force,
    };

    // Under a non-TTY nothing prints until the summary; the three-line block is redrawn in
    // place, so a stream that cannot move the cursor would otherwise collect one block per
    // frame.
    let animate = !json && console::Term::stdout().is_term();
    let mut progress = ScanProgress::new(ui, animate);

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
    animate: bool,
    frame: u64,
    drawn: bool,
    last: std::time::Instant,
}

const SCAN_FRAME_MS: u64 = 125;
const SCAN_BLOCK_LINES: usize = 3;

impl<'a> ScanProgress<'a> {
    fn new(ui: &'a crate::ui::Ui, animate: bool) -> Self {
        ScanProgress {
            ui,
            animate,
            frame: 0,
            drawn: false,
            last: std::time::Instant::now() - std::time::Duration::from_millis(SCAN_FRAME_MS),
        }
    }

    fn tick(&mut self, current: usize, total: usize) {
        if !self.animate || self.last.elapsed().as_millis() < SCAN_FRAME_MS as u128 {
            return;
        }
        self.last = std::time::Instant::now();
        let term = console::Term::stdout();
        if self.drawn {
            let _ = term.move_cursor_up(SCAN_BLOCK_LINES);
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

    /// The progress line is cleared in one frame, per --motion-exit.
    fn clear(&mut self) {
        if self.drawn {
            let term = console::Term::stdout();
            let _ = term.move_cursor_up(SCAN_BLOCK_LINES);
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

fn compute_verdict_breakdown(storage: &Arc<Storage>, total_sessions: usize) -> VerdictBreakdown {
    let mut breakdown = VerdictBreakdown::default();
    let scanner = Scanner::new(storage.clone());

    if let Ok(sessions) = storage.list_sessions_filtered(&SessionFilter {
        limit: None,
        ..Default::default()
    }) {
        for s in sessions {
            let mut detected_rung = None;
            if let Ok(trace) = scanner.load_trace(&s.session_id) {
                let outcomes = evaluate_trace_outcomes(&trace);
                if let Some(strongest) = agentworth_outcomes::highest_outcome(&outcomes) {
                    detected_rung = Some(strongest.kind);
                }
            }

            match detected_rung {
                Some(agentworth_schema::OutcomeKind::CiOrDeploymentVerified) => {
                    breakdown.ci_or_deployment_verified += 1;
                    breakdown.real_verified_tasks += 1;
                }
                Some(agentworth_schema::OutcomeKind::CommitObserved) => {
                    breakdown.commit_observed += 1;
                    breakdown.real_verified_tasks += 1;
                }
                Some(agentworth_schema::OutcomeKind::TestOrBuildPassed) => {
                    breakdown.test_or_build_passed += 1;
                    breakdown.real_verified_tasks += 1;
                }
                Some(agentworth_schema::OutcomeKind::ArtifactChanged) => {
                    breakdown.artifact_changed += 1;
                }
                Some(agentworth_schema::OutcomeKind::DoneClaimed) => {
                    breakdown.done_claimed += 1;
                }
                None => {
                    breakdown.unverified += 1;
                }
            }
        }
    }

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
    let tokens = &stats.token_usage;
    let view = crate::ui::views::StatsView {
        db_path: db.as_deref(),
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
    };
    crate::ui::views::stats(ui, &view)
}

// -----------------------------------------------------------------------------
// Command: Traces
// -----------------------------------------------------------------------------

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
    let scanner = Scanner::new(storage.clone());
    let scorer = agentworth_scoring::TraceScorer::default();

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

    let mut rows = Vec::new();
    for s in filtered_sessions {
        let mut badge = "[UNVERIFIED]".to_string();
        let mut score_val = 0.0;
        let mut highest_kind = None;

        if let Ok(trace) = scanner.load_trace(&s.session_id) {
            let outcomes = evaluate_trace_outcomes(&trace);
            let sc = scorer.score(&trace);
            score_val = sc.composite_score * 100.0;

            if let Some(strongest) = agentworth_outcomes::highest_outcome(&outcomes) {
                highest_kind = Some(strongest.kind);
                badge = match strongest.kind {
                    agentworth_schema::OutcomeKind::CiOrDeploymentVerified => "[CI_VERIFIED]".to_string(),
                    agentworth_schema::OutcomeKind::CommitObserved => "[COMMITTED]".to_string(),
                    agentworth_schema::OutcomeKind::TestOrBuildPassed => "[TEST_PASSED]".to_string(),
                    agentworth_schema::OutcomeKind::ArtifactChanged => "[ARTIFACT]".to_string(),
                    agentworth_schema::OutcomeKind::DoneClaimed => "[CLAIM_ONLY]".to_string(),
                };
            }
        }

        rows.push((badge, score_val, highest_kind, s));
    }

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
                "agentworth traces",
                "No sessions found in index.",
                "",
                &[],
                &[(
                    "agentworth scan".to_string(),
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
        &format!("agentworth traces --limit {}", limit),
        indexed,
        &view_rows,
    )
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

fn run_inspect_command(session_id: &str, json: bool, db_path: Option<PathBuf>) -> Result<()> {
    let storage = open_storage(db_path)?;
    let scanner = Scanner::new(storage.clone());

    let trace = scanner.load_trace(session_id)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&trace)?);
    } else {
        print_inspect_view(&trace);
    }

    Ok(())
}

fn print_inspect_view(trace: &agentworth_schema::AgentWorthTrace) {
    let scorer = agentworth_scoring::TraceScorer::default();
    let score = scorer.score(trace);
    let outcomes = evaluate_trace_outcomes(trace);

    println!();
    println!(
        "{}",
        style("╔════════════════════════════════════════════════════════════════════════════════╗")
            .bold()
    );
    println!(
        "║ {:<78} ║",
        style(format!("AgentWorth Session Trace: {}", trace.session_id))
            .bold()
            .cyan()
    );
    println!(
        "{}",
        style("╠════════════════════════════════════════════════════════════════════════════════╣")
            .bold()
    );
    println!(
        "║ Adapter:     {:<20} Started:    {:<30} ║",
        style(&trace.adapter).green().bold(),
        trace.started_at.to_rfc3339()
    );
    if let Some(ended) = trace.ended_at {
        println!(
            "║ Duration:    {:<20} Ended:      {:<30} ║",
            trace
                .stats
                .duration_seconds
                .map(format_duration)
                .unwrap_or_else(|| "-".to_string()),
            ended.to_rfc3339()
        );
    }
    println!(
        "║ Score:       {:<20} Events:     {:<30} ║",
        style(format!("{:.1} / 100", score.composite_score * 100.0))
            .bold()
            .yellow(),
        trace.stats.total_events
    );

    let tokens = &trace.stats.token_usage;
    println!(
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
        println!(
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
        let display = if tools_str.len() > 66 {
            format!("{}...", &tools_str[..63])
        } else {
            tools_str
        };
        println!("║ Tools:       {:<66} ║", style(display).yellow());
    }

    if let Some(strongest) = agentworth_outcomes::highest_outcome(&outcomes) {
        let (rung_num, rung_label) = match strongest.kind {
            agentworth_schema::OutcomeKind::CiOrDeploymentVerified => (5, "CI or Deployment Verified"),
            agentworth_schema::OutcomeKind::CommitObserved => (4, "Commit Observed"),
            agentworth_schema::OutcomeKind::TestOrBuildPassed => (3, "Test or Build Passed"),
            agentworth_schema::OutcomeKind::ArtifactChanged => (2, "Artifact Changed"),
            agentworth_schema::OutcomeKind::DoneClaimed => (1, "Done Claimed"),
        };

        println!(
            "{}",
            style("╠════════════════════════════════════════════════════════════════════════════════╣").bold()
        );
        println!(
            "║ Highest Outcome Reached: {:<53} ║",
            style(format!("Rung {} - {} (Confidence: {:.0}%)", rung_num, rung_label, strongest.confidence * 100.0)).bold().green()
        );
        println!(
            "║ Supporting Evidence:                                                           ║"
        );

        let supporting: Vec<_> = outcomes.iter().filter(|o| o.kind == strongest.kind).collect();
        for ev in &supporting {
            let summary_display = if ev.summary.len() > 64 {
                format!("{}...", &ev.summary[..61])
            } else {
                ev.summary.clone()
            };
            println!(
                "║   • {:<72} ║",
                style(format!("{} (conf: {:.0}%)", summary_display, ev.confidence * 100.0)).cyan()
            );
        }

        let other_signals: Vec<_> = outcomes.iter().filter(|o| o.kind != strongest.kind).collect();
        if !other_signals.is_empty() {
            println!(
                "║ Precursor / Secondary Evidence Signals ({}):                                   ║",
                other_signals.len()
            );
            for ev in other_signals.iter().take(3) {
                let summary_display = if ev.summary.len() > 48 {
                    format!("{}...", &ev.summary[..45])
                } else {
                    ev.summary.clone()
                };
                println!(
                    "║   - [{:<20}] {:<48} ║",
                    format!("{:?}", ev.kind),
                    style(summary_display).dim()
                );
            }
        }
    }

    println!(
        "║ Source:      {:<66} ║",
        style(&trace.provenance.source_path).dim()
    );
    println!(
        "{}",
        style("╚════════════════════════════════════════════════════════════════════════════════╝")
            .bold()
    );
    println!();

    println!(
        "{}",
        style("─ Session Timeline ─────────────────────────────────────────────────────────────")
            .bold()
    );

    for ev in &trace.events {
        let ts = ev.timestamp.format("%H:%M:%S").to_string();
        let seq = format!("[{:03}]", ev.sequence);

        match &ev.payload {
            agentworth_schema::EventPayload::UserMessage { content } => {
                println!(
                    "{} {} 👤 {}",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("USER PROMPT").bold().blue()
                );
                for line in content.lines() {
                    println!("   │ {}", line);
                }
                println!("   │");
            }
            agentworth_schema::EventPayload::AssistantMessage { content, thinking } => {
                if let Some(th) = thinking {
                    println!(
                        "{} {} 🧠 {}",
                        style(&seq).dim(),
                        style(&ts).dim(),
                        style("ASSISTANT THINKING").dim().cyan()
                    );
                    for line in th.lines() {
                        println!("   │ {}", style(line).dim());
                    }
                    println!("   │");
                }
                if !content.is_empty() {
                    println!(
                        "{} {} 💬 {}",
                        style(&seq).dim(),
                        style(&ts).dim(),
                        style("ASSISTANT").bold().green()
                    );
                    for line in content.lines() {
                        println!("   │ {}", line);
                    }
                    println!("   │");
                }
            }
            agentworth_schema::EventPayload::ModelInvocation {
                model,
                token_usage,
                cost_usd,
                latency_ms,
            } => {
                let cost_str = cost_usd
                    .map(|c| format!(" (${:.4})", c))
                    .unwrap_or_default();
                let latency_str = latency_ms.map(|l| format!(" {}ms", l)).unwrap_or_default();
                println!(
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
                println!("   │");
            }
            agentworth_schema::EventPayload::ModelSwitch(ms) => {
                let from_str = ms.from_model.as_deref().unwrap_or("auto");
                println!(
                    "{} {} 🔀 {} {} -> {}",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("MODEL SWITCH:").magenta().bold(),
                    style(from_str).dim(),
                    style(&ms.to_model).magenta().bold()
                );
                if let Some(ref reason) = ms.reason {
                    println!("   │ reason: {}", style(reason).dim());
                }
                println!("   │");
            }
            agentworth_schema::EventPayload::ToolCall(tc) => {
                println!(
                    "{} {} ⚡ {} {}",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("TOOL CALL:").bold().yellow(),
                    style(&tc.name).yellow().bold()
                );
                let args_str = serde_json::to_string_pretty(&tc.arguments).unwrap_or_default();
                for line in args_str.lines() {
                    println!("   │ {}", style(line).dim());
                }
                println!("   │");
            }
            agentworth_schema::EventPayload::ToolResult(tr) => {
                let status = if tr.is_error {
                    style("[ERROR]").red().bold()
                } else {
                    style("[OK]").green().bold()
                };
                let name = tr.name.as_deref().unwrap_or("Tool");
                println!(
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
                    println!("   │ {}", style(line).dim());
                }
                if out_str.lines().count() > 15 {
                    println!("   │ {}", style("... (truncated output)").dim().italic());
                }
                println!("   │");
            }
            agentworth_schema::EventPayload::ShellCommand(sc) => {
                println!(
                    "{} {} 💻 {} {}",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("SHELL COMMAND:").bold().cyan(),
                    style(&sc.command).cyan().bold()
                );
                if let Some(ref cwd) = sc.cwd {
                    println!("   │ cwd: {}", style(cwd).dim());
                }
                if let Some(code) = sc.exit_code {
                    println!("   │ exit: {}", code);
                }
                println!("   │");
            }
            agentworth_schema::EventPayload::FileAction {
                path,
                action,
                lines_changed,
                ..
            } => {
                println!(
                    "{} {} 📝 {} {:?} {}",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("FILE ACTION:").bold().magenta(),
                    action,
                    style(path).bold()
                );
                if let Some(lines) = lines_changed {
                    println!("   │ lines changed: {}", lines);
                }
                println!("   │");
            }
            agentworth_schema::EventPayload::OutcomeEvidence(oe) => {
                println!(
                    "{} {} 🏆 {} {:?} (confidence: {:.0}%)",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("OUTCOME EVIDENCE:").bold().green(),
                    oe.kind,
                    oe.confidence * 100.0
                );
                println!("   │ {}", style(&oe.summary).green());
                println!("   │");
            }
            agentworth_schema::EventPayload::Error {
                message,
                is_recovered,
            } => {
                println!(
                    "{} {} ⚠️  {} (recovered: {})",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("ERROR:").bold().red(),
                    is_recovered
                );
                println!("   │ {}", style(message).red());
                println!("   │");
            }
            agentworth_schema::EventPayload::HumanIntervention(hi) => {
                println!(
                    "{} {} 🛑 {} {}",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("HUMAN INTERVENTION:").bold().red(),
                    hi.action
                );
                if let Some(ref details) = hi.details {
                    println!("   │ details: {}", details);
                }
                println!("   │");
            }
            agentworth_schema::EventPayload::Compaction(c) => {
                let tokens_str = match (c.pre_tokens, c.post_tokens) {
                    (Some(pre), Some(post)) => format!(" {} -> {} tokens", pre, post),
                    _ => String::new(),
                };
                println!(
                    "{} {} 🗜️  {} trigger={}{}",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("COMPACTION:").bold().cyan(),
                    style(&c.trigger).cyan().bold(),
                    style(tokens_str).dim()
                );
                if let Some(dropped) = c.dropped_tokens {
                    println!("   │ dropped: {} tokens", dropped);
                }
                if let Some(duration) = c.duration_ms {
                    println!("   │ duration: {}ms", duration);
                }
                println!("   │");
            }
            agentworth_schema::EventPayload::Custom { kind, data } => {
                println!(
                    "{} {} 📦 {} {}",
                    style(&seq).dim(),
                    style(&ts).dim(),
                    style("CUSTOM EVENT:").dim(),
                    kind
                );
                println!("   │ {}", data);
                println!("   │");
            }
        }
    }
    println!(
        "{}",
        style("────────────────────────────────────────────────────────────────────────────────")
            .bold()
    );
    println!();
}

// -----------------------------------------------------------------------------
// Command: Export
// -----------------------------------------------------------------------------

fn run_export_command(
    session_id: &str,
    redact: bool,
    format: &str,
    output: Option<&std::path::Path>,
    db_path: Option<PathBuf>,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let scanner = Scanner::new(storage.clone());

    let mut trace = scanner.load_trace(session_id)?;

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
        eprintln!(
            "{} Exported session '{}' ({}) to {:?}",
            style("✔").green().bold(),
            session_id,
            format,
            out_path
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
        &format!("agentworth inspect {}", session_id),
        &format!("No indexed session starts with {}.", session_id),
        "Closest three:",
        &nearest,
        &[
            (
                "agentworth traces --limit 20".to_string(),
                "list what is indexed".to_string(),
            ),
            (
                "agentworth scan".to_string(),
                "re-index, if it should be here".to_string(),
            ),
        ],
    )
}

/// Archie, above a screen that has not yet been moved onto the grid. The old banner
/// carried a magnifying-glass emoji that is double-width in some terminals; no CLI output
/// carries an emoji any more.
fn print_archie_banner(ui: &crate::ui::Ui) {
    let art = crate::ui::archie(ui, &crate::ui::eyes(ui, crate::ui::EyeKind::Digging));
    println!();
    for line in art {
        println!("  {}", ui.paint(crate::ui::Role::Chrome, &line));
    }
    println!();
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
        // matching `agentworth scan`'s own "Total Indexed... in SQLite index" promise -- not a
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

    print_archie_banner(ui);
    println!(
        "{}",
        style("┌──────────────────────────────────────────────────────────┐").bold()
    );
    println!(
        "│ {}                   │",
        style("🩺 AgentWorth System Health & Diagnostics").bold()
    );
    println!(
        "{}",
        style("├──────────────────────────────────────────────────────────┤").bold()
    );
    println!("│ Environment:                                             │");
    println!(
        "│   • OS / Arch:        {:<34} │",
        style(format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH)).cyan()
    );
    println!(
        "│   • Binary Version:   {:<34} │",
        style(format!("v{}", env!("CARGO_PKG_VERSION"))).cyan()
    );
    println!(
        "{}",
        style("├──────────────────────────────────────────────────────────┤").bold()
    );
    println!("│ Local SQLite Index:                                      │");
    println!(
        "│   • Path:             {:<34} │",
        style(if db_path_display.len() > 34 {
            format!("...{}", &db_path_display[db_path_display.len() - 31..])
        } else {
            db_path_display.clone()
        }).cyan()
    );
    println!(
        "│   • Database State:   {:<34} │",
        if storage_healthy {
            style("✓ Healthy (WAL Mode)").green().bold()
        } else {
            style("✗ Not Found / Uninitialized").red().bold()
        }
    );
    println!(
        "│   • Size / Sessions:  {:<34} │",
        style(format!("{:.1} KB ({} sessions indexed)", db_size_bytes as f64 / 1024.0, total_indexed)).cyan()
    );
    println!(
        "{}",
        style("├──────────────────────────────────────────────────────────┤").bold()
    );
    println!("│ Detected Agent Adapters:                                 │");

    for d in &detections {
        let status_str = if d.is_present {
            format!("✓ Detected ({} roots)", d.discovered_roots.len())
        } else {
            "○ Not found".to_string()
        };
        println!(
            "│   • {:<18} {:<33} │",
            style(&d.adapter_name).bold(),
            if d.is_present { style(status_str).green() } else { style(status_str).dim() }
        );
    }

    println!(
        "{}",
        style("└──────────────────────────────────────────────────────────┘").bold()
    );
    println!();

    Ok(())
}

// -----------------------------------------------------------------------------
// Command: Matrix
// -----------------------------------------------------------------------------

fn run_matrix_command(json_output: bool, _db_path: Option<PathBuf>, ui: &crate::ui::Ui) -> Result<()> {
    // Derived from the registry (`agentworth_adapters::all_adapters()`) rather than a
    // hand-copied list here: a newly-registered adapter now shows up in this table without
    // anyone remembering to add it in a second place.
    let adapters: Vec<Box<dyn agentworth_adapter_sdk::AgentAdapter>> = agentworth_adapters::all_adapters();

    let default_roots: std::collections::HashMap<&'static str, &'static str> = [
        ("aider", "~/.aider* / chat history"),
        ("claude_code", "~/.claude/projects/"),
        ("cline", "~/.config/Code/.../cline/"),
        ("codex", "~/.codex/sessions/"),
        ("cursor", "~/.cursor/ / workspaceStorage"),
        ("deepseek", "~/.deepseek/"),
        ("gemini", "~/.gemini/ / antigravity"),
        ("goose", "~/.config/goose/sessions/"),
        ("grok", "~/.grok/ / ~/.xai/"),
        ("herdr", "~/.herdr/"),
        ("hermes", "~/.hermes/"),
        ("kimi", "~/.kimi/"),
        ("manus", "~/.manus/"),
        ("minimax", "~/.minimax/"),
        ("openclaw", "~/.openclaw/"),
        ("opencode", "~/.opencode/"),
        ("pi", "~/.pi/"),
        ("qwen", "~/.qwen/"),
        ("windsurf", "~/.codeium/windsurf/"),
        ("zhipu", "~/.zhipu/ / codegeex/"),
    ]
    .into_iter()
    .collect();

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
        let source_root = default_roots.get(name).copied().unwrap_or("~/.<agent>/");

        rows_data.push((name, source_root, caps, is_detected));
    }

    let real_coverage_rate = format!(
        "{:.1}%",
        (total_supported as f64 / total_possible as f64) * 100.0
    );

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

    print_archie_banner(ui);
    println!(
        "{}",
        style("┌──────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐").bold()
    );
    println!(
        "│ {:<112} │",
        style("AgentWorth Adapter Extraction Coverage Matrix (20 Production Adapters)").bold().cyan()
    );
    println!(
        "{}",
        style("├─────────────────┬────────────────────────────┬─────────┬────────┬───────┬───────┬───────┬───────┬──────────┬─────────────┤").bold()
    );
    println!(
        "│ {:<15} │ {:<26} │ {:<7} │ {:<6} │ {:<5} │ {:<5} │ {:<5} │ {:<5} │ {:<8} │ {:<11} │",
        style("ADAPTER").bold().cyan(),
        style("DEFAULT SOURCE").bold().cyan(),
        style("PROMPT").bold().cyan(),
        style("TOKENS").bold().cyan(),
        style("TOOLS").bold().cyan(),
        style("SHELL").bold().cyan(),
        style("DIFFS").bold().cyan(),
        style("THINK").bold().cyan(),
        style("OUTCOMES").bold().cyan(),
        style("STATUS").bold().cyan(),
    );
    println!(
        "{}",
        style("├─────────────────┼────────────────────────────┼─────────┼────────┼───────┼───────┼───────┼───────┼──────────┼─────────────┤").bold()
    );

    let fmt_cap = |supported: bool| -> String {
        if supported {
            style("✓").bold().green().to_string()
        } else {
            style("-").dim().to_string()
        }
    };

    for (name, source_root, caps, is_detected) in &rows_data {
        let status_str = if *is_detected {
            style("✓ Detected").bold().green()
        } else {
            style("○ Available").dim()
        };

        let src_display = if source_root.len() > 26 {
            format!("{}...", &source_root[..23])
        } else {
            source_root.to_string()
        };

        println!(
            "│ {:<15} │ {:<26} │ {:^7} │ {:^6} │ {:^5} │ {:^5} │ {:^5} │ {:^5} │ {:^8} │ {:<11} │",
            style(name).bold(),
            style(src_display).dim(),
            fmt_cap(caps.prompts),
            fmt_cap(caps.tokens),
            fmt_cap(caps.tools),
            fmt_cap(caps.shell),
            fmt_cap(caps.diffs),
            fmt_cap(caps.thinking),
            fmt_cap(caps.outcomes),
            status_str,
        );
    }

    println!(
        "{}",
        style("└─────────────────┴────────────────────────────┴─────────┴────────┴───────┴───────┴───────┴───────┴──────────┴─────────────┘").bold()
    );
    println!(
        "Showing {} production adapters. Grounded extraction coverage across feature dimensions: {}.",
        rows_data.len(),
        style(&real_coverage_rate).bold().cyan()
    );
    println!();

    Ok(())
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
    by_model: bool,
    json: bool,
    db_path: Option<PathBuf>,
    ui: &'a crate::ui::Ui,
}

fn run_usage_command(args: UsageCommandArgs) -> Result<()> {
    let UsageCommandArgs {
        period,
        pacing,
        hours,
        alert_above,
        limit,
        by_model,
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

    if by_model {
        let records = storage.get_model_usage(period, limit)?;

        if json {
            println!("{}", serde_json::to_string_pretty(&records)?);
            return Ok(());
        }

        if records.is_empty() {
            print!("{}", no_usage_records(ui));
            return Ok(());
        }

        let rows: Vec<crate::ui::views::UsageRow> = records
            .iter()
            .map(|r| crate::ui::views::UsageRow {
                period: r.period.clone(),
                who: r.model.clone(),
                sessions: r.session_count,
                input: r.input_tokens,
                output: r.output_tokens,
                cache_read: r.cache_read_tokens,
                cost_usd: r.estimated_cost_usd,
                measured: r.total_tokens > 0,
            })
            .collect();
        print!(
            "{}",
            crate::ui::views::usage(
                ui,
                &format!("agentworth usage --period {} --by-model", period),
                "MODEL",
                period,
                &rows
            )
        );
        return Ok(());
    }

    let records = match period {
        "week" => storage.get_weekly_usage(Some(limit))?,
        "month" => storage.get_monthly_usage(Some(limit))?,
        _ => storage.get_daily_usage(Some(limit))?,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&records)?);
        return Ok(());
    }

    if records.is_empty() {
        print!("{}", no_usage_records(ui));
        return Ok(());
    }

    let rows: Vec<crate::ui::views::UsageRow> = records
        .iter()
        .map(|r| crate::ui::views::UsageRow {
            period: r.period.clone(),
            who: r.adapter.clone(),
            sessions: r.session_count,
            input: r.input_tokens,
            output: r.output_tokens,
            cache_read: r.cache_read_tokens,
            cost_usd: r.estimated_cost_usd,
            // Nine of today's eleven adapters keep no token counts. Printing 0 and $0.00
            // for them asserts something false, so the row prints dashes instead.
            measured: r.total_tokens > 0,
        })
        .collect();
    print!(
        "{}",
        crate::ui::views::usage(
            ui,
            &format!("agentworth usage --period {}", period),
            "ADAPTER",
            period,
            &rows
        )
    );

    Ok(())
}

fn no_usage_records(ui: &crate::ui::Ui) -> String {
    crate::ui::views::error(
        ui,
        "agentworth usage",
        "No usage records in the index.",
        "",
        &[],
        &[(
            "agentworth scan".to_string(),
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
        &format!("agentworth usage --pacing --hours {}", hours),
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
    out.push_str(&ui.next("agentworth usage --period day", "the same spend, by day"));
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
    // row's trace is scored for its ladder position, the same way `traces` does it.
    let scanner = Scanner::new(storage.clone());
    let rows: Vec<crate::ui::views::BlameRow> = matches
        .iter()
        .map(|m| {
            let rung = scanner
                .load_trace(&m.session_id)
                .ok()
                .and_then(|t| agentworth_outcomes::highest_outcome(&evaluate_trace_outcomes(&t)).map(|o| o.kind))
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

        // Test parsing command using "agentworth" binary name
        let parsed_agentworth = Cli::try_parse_from(["agentworth", "doctor", "--json"]).unwrap();
        match parsed_agentworth.command {
            Commands::Doctor { json } => assert!(json),
            _ => panic!("Expected Doctor command"),
        }

        // Test parsing command using "agwt" binary alias
        let parsed_agwt = Cli::try_parse_from(["agwt", "doctor", "--json"]).unwrap();
        match parsed_agwt.command {
            Commands::Doctor { json } => assert!(json),
            _ => panic!("Expected Doctor command"),
        }

        // Test usage pacing with alert-above
        let parsed_usage = Cli::try_parse_from(["agwt", "usage", "--pacing", "--alert-above", "50.0"]).unwrap();
        match parsed_usage.command {
            Commands::Usage { pacing, alert_above, .. } => {
                assert!(pacing);
                assert_eq!(alert_above, Some(50.0));
            }
            _ => panic!("Expected Usage command"),
        }
    }
}
