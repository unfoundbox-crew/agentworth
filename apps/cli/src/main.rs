use std::path::PathBuf;
use std::sync::Arc;

use agentworth_adapter_sdk::ScanOptions;
use agentworth_core::{ScanSummary, Scanner};
use agentworth_outcomes::evaluate_trace_outcomes;
use agentworth_storage::{SessionFilter, SessionOrderBy, Storage};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
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
#[path = "commands/autopsy.rs"]
mod autopsy;
#[path = "commands/recall.rs"]
mod recall;
#[path = "commands/bisect.rs"]
mod bisect;

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
        /// Maximum number of traces to display
        #[arg(short, long, default_value_t = 20)]
        limit: usize,

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

        /// Export format: json (default) or atif
        #[arg(short, long, default_value = "json", value_parser = ["json", "atif"])]
        format: String,

        /// Optional file path to write export output to (defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Semantic vector search across indexed trajectory turns with ASCII thermal receipts
    Search {
        /// Search query (natural language or code snippet)
        query: String,

        /// Maximum number of results to return
        #[arg(short, long, default_value_t = 10)]
        limit: usize,

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
        #[arg(short, long, default_value_t = agentworth_cli::DEFAULT_PORT)]
        port: u16,

        /// Automatically open the Web UI in the default browser
        #[arg(long)]
        open: bool,

        /// Optional path to custom web frontend dist directory
        #[arg(long)]
        dist: Option<PathBuf>,
    },

    /// View deep usage, pacing, and token expenditure rollups
    Usage {
        /// Rollup period: day, week, or month
        #[arg(short, long, default_value = "day", value_parser = ["day", "week", "month"])]
        period: String,

        /// Show 5-hour rolling pacing window (burn rate, active models, quota headroom)
        #[arg(long)]
        pacing: bool,

        /// Pacing window duration in hours
        #[arg(long, default_value_t = 5)]
        hours: i64,

        /// Alert and highlight if window spend exceeds this threshold in USD
        #[arg(long)]
        alert_above: Option<f64>,

        /// Maximum number of rows to display
        #[arg(short, long, default_value_t = 20)]
        limit: usize,

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
        /// Maximum number of sessions to list (default: 20)
        #[arg(short, long, default_value_t = 20)]
        limit: usize,

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

        /// Maximum number of results to return (default: 5)
        #[arg(short, long, default_value_t = 5)]
        limit: usize,

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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("warn")
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    match cli.command {
        Commands::Scan { paths, force, json } => {
            run_scan_command(paths, force, json, cli.db_path)?;
        }
        Commands::Stats { json } => {
            run_stats_command(json, cli.db_path)?;
        }
        Commands::Doctor { json } => {
            run_doctor_command(json, cli.db_path)?;
        }
        Commands::Matrix { json } => {
            run_matrix_command(json, cli.db_path)?;
        }
        Commands::Traces {
            limit,
            adapter,
            model,
            all_stubs,
            json,
        } => {
            run_traces_command(limit, adapter, model, all_stubs, json, cli.db_path)?;
        }
        Commands::Inspect { session_id, json } => {
            run_inspect_command(&session_id, json, cli.db_path)?;
        }
        Commands::Export {
            session_id,
            redact,
            format,
            output,
        } => {
            run_export_command(&session_id, redact, &format, output.as_deref(), cli.db_path)?;
        }
        Commands::Search {
            query,
            limit,
            min_score,
            kind,
            json,
        } => {
            agentworth_cli::run_search_command(&query, limit, min_score, kind, json, cli.db_path)?;
        }
        Commands::Audit { safety, json } => {
            agentworth_cli::run_audit_command(safety, json, cli.db_path)?;
        }
        Commands::Blunder { top, submit, json } => {
            agentworth_cli::run_blunder_command(top, submit, json, cli.db_path)?;
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
            run_usage_command(
                &period,
                pacing,
                hours,
                alert_above,
                limit,
                by_model,
                json,
                cli.db_path,
            )?;
        }
        Commands::Blame { file_path, json } => {
            run_blame_command(&file_path, json, cli.db_path)?;
        }
        Commands::Serve { port, open, dist } => {
            let storage = open_storage(cli.db_path)?;
            let dist_path = dist.or_else(|| {
                let default_dist = PathBuf::from("apps/web/dist");
                if default_dist.exists() {
                    Some(default_dist)
                } else {
                    None
                }
            });
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(agentworth_cli::start_server(storage, port, open, dist_path))?;
        }
        Commands::Merge { source_db, json } => {
            merge::run_merge_command(source_db, json, cli.db_path)?;
        }
        Commands::Watch {
            interval_secs,
            poll_once,
            json,
            paths,
        } => {
            watch::run_watch_command(interval_secs, poll_once, json, paths)?;
        }
        Commands::CacheDoctor { session_id, json } => {
            cache_doctor::run_cache_doctor_command(&session_id, json, cli.db_path)?;
        }
        Commands::BlindSpots { limit, json } => {
            blind_spots::run_blind_spots_command(limit, json, cli.db_path)?;
        }
        Commands::Autopsy {
            min_occurrences,
            json,
        } => {
            autopsy::run_autopsy_command(min_occurrences, json, cli.db_path)?;
        }
        Commands::Recall {
            query,
            limit,
            min_score,
            json,
        } => {
            recall::run_recall_command(&query, limit, min_score, json, cli.db_path)?;
        }
        Commands::Bisect { session_id, json } => {
            bisect::run_bisect_command(&session_id, json, cli.db_path)?;
        }
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
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let scanner = Scanner::new(storage.clone());
    let options = ScanOptions {
        custom_paths: paths,
        force,
    };

    let pb = if !json {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                .template("{spinner:.cyan.bold} {msg}")
                .unwrap(),
        );
        pb.set_message("Discovering agent history sources...");
        pb.enable_steady_tick(std::time::Duration::from_millis(80));
        Some(pb)
    } else {
        None
    };

    let mut configured_bar = false;

    let summary = scanner.run_scan(&options, |current, total| {
        if let Some(ref pb) = pb {
            if !configured_bar && total > 0 {
                pb.set_length(total as u64);
                pb.set_style(
                    ProgressStyle::default_bar()
                        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                        .template(
                            "{spinner:.cyan.bold} Scanning agent histories ▕{bar:30.cyan/238}▏ {percent:>3}% [ETA {eta}]",
                        )
                        .unwrap()
                        .progress_chars("█▉▊▋▌▍▎▏ "),
                );
                configured_bar = true;
            }
            pb.set_position(current as u64);
        }
    })?;

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        print_scan_summary(&summary, storage.db_path());
    }

    Ok(())
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

fn run_stats_command(json: bool, db_path: Option<PathBuf>) -> Result<()> {
    let storage = open_storage(db_path)?;
    let stats = storage.get_aggregate_stats()?;
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
        print_stats_view(&stats, &top_repos, &verdict, storage.db_path());
    }

    Ok(())
}

fn print_archie_ascii_banner() {
    println!();
    println!("       {}", style("┌───────────┐").dim());
    println!("       {}   {}", style("│ ( • _ • ) │").bold().cyan(), style("\"Your agents left receipts.\"").italic());
    println!("       {}    {}", style("│  /| 🔎 |\\ │").bold(), style("────────────────────────────").dim());
    println!("       {}    {}", style("│  / |  | \\ │").dim(), style("• Digging through dotfiles").dim());
    println!("       {}    {}", style("│   /    \\  │").dim(), style("• Auditing token burn pacing").dim());
    println!("       {}    {}", style("└───┴────┴──┘").dim(), style("• Tracing line-by-line lineage").dim());
    println!();
}

fn print_stats_view(
    stats: &agentworth_storage::AggregateStats,
    top_repos: &[(String, usize)],
    verdict: &VerdictBreakdown,
    db_path: Option<&std::path::Path>,
) {
    print_archie_ascii_banner();
    println!(
        "{}",
        style("┌──────────────────────────────────────────────────────────┐").bold()
    );
    println!(
        "│ {:<56} │",
        style("AgentWorth Machine-Wide Experience Summary").bold().cyan()
    );
    println!(
        "{}",
        style("├──────────────────────────────────────────────────────────┤").bold()
    );
    println!(
        "│ Total Sessions: {:>8}                                 │",
        style(stats.total_sessions).bold().yellow()
    );
    println!(
        "│ Total Events:   {:>8}                                 │",
        style(stats.total_events).bold()
    );

    if let (Some(first), Some(last)) = (stats.first_session_at, stats.last_session_at) {
        println!(
            "│ Date Range:     {:<41}│",
            style(format!(
                "{} to {}",
                first.format("%Y-%m-%d"),
                last.format("%Y-%m-%d")
            ))
            .dim()
        );
    }

    if let Some(path) = db_path {
        let path_str = path.to_string_lossy();
        let display_path = if path_str.len() > 40 {
            format!("...{}", &path_str[path_str.len() - 37..])
        } else {
            path_str.to_string()
        };
        println!("│ Database Index: {:<41}│", style(display_path).dim());
    }

    println!(
        "{}",
        style("├──────────────────────────────────────────────────────────┤").bold()
    );
    println!("│ Verdict Breakdown:                                       │");
    let total = stats.total_sessions;
    let pct = |count: usize| -> f64 {
        if total > 0 {
            (count as f64 / total as f64) * 100.0
        } else {
            0.0
        }
    };
    println!(
        "│   • CI or Deployment Verified (Rung 5): {:>4} ({:>5.1}%) │",
        style(verdict.ci_or_deployment_verified).bold().green(),
        pct(verdict.ci_or_deployment_verified)
    );
    println!(
        "│   • Commit Observed (Rung 4):           {:>4} ({:>5.1}%) │",
        style(verdict.commit_observed).bold().green(),
        pct(verdict.commit_observed)
    );
    println!(
        "│   • Test or Build Passed (Rung 3):      {:>4} ({:>5.1}%) │",
        style(verdict.test_or_build_passed).bold().cyan(),
        pct(verdict.test_or_build_passed)
    );
    println!(
        "│   • Artifact Changed (Rung 2):          {:>4} ({:>5.1}%) │",
        style(verdict.artifact_changed).bold().yellow(),
        pct(verdict.artifact_changed)
    );
    println!(
        "│   • Done Claimed (Rung 1):              {:>4} ({:>5.1}%) │",
        style(verdict.done_claimed).dim(),
        pct(verdict.done_claimed)
    );
    if verdict.unverified > 0 {
        println!(
            "│   • Unverified / In-Progress:           {:>4} ({:>5.1}%) │",
            style(verdict.unverified).dim(),
            pct(verdict.unverified)
        );
    }
    println!("│                                                          │");
    println!(
        "│ Real Verified Tasks: {:>5} / {:<5} ({:>5.1}%)             │",
        style(verdict.real_verified_tasks).bold().green(),
        total,
        verdict.real_verified_rate
    );

    println!(
        "{}",
        style("├──────────────────────────────────────────────────────────┤").bold()
    );

    let tokens = &stats.token_usage;
    println!(
        "│ Total Tokens:   {:>8} ({})                     │",
        style(format_number(tokens.total())).bold().magenta(),
        tokens.total()
    );
    println!(
        "│   • Input:      {:>8}                                │",
        style(format_number(tokens.input_tokens)).dim()
    );
    println!(
        "│   • Output:     {:>8}                                │",
        style(format_number(tokens.output_tokens)).dim()
    );
    println!(
        "│   • Cache Read: {:>8}                                │",
        style(format_number(tokens.cache_read_tokens)).dim()
    );
    println!(
        "│   • Cache Write:{:>8}                                │",
        style(format_number(tokens.cache_creation_tokens)).dim()
    );

    if !stats.sessions_by_adapter.is_empty() {
        println!(
            "{}",
            style("├──────────────────────────────────────────────────────────┤").bold()
        );
        println!("│ Adapters:                                                │");
        let mut sorted_adapters: Vec<_> = stats.sessions_by_adapter.iter().collect();
        sorted_adapters.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        for (adapter, count) in sorted_adapters {
            let pct_val = if stats.total_sessions > 0 {
                (*count as f64 / stats.total_sessions as f64) * 100.0
            } else {
                0.0
            };
            println!(
                "│   • {:<20} {:>6} sessions ({:>4.1}%)        │",
                style(adapter).cyan(),
                style(count).bold(),
                pct_val
            );
        }
    }

    if !stats.models_usage_count.is_empty() {
        println!(
            "{}",
            style("├──────────────────────────────────────────────────────────┤").bold()
        );
        println!("│ Top Models:                                              │");
        let mut models: Vec<_> = stats.models_usage_count.iter().collect();
        models.sort_by(|a, b| b.1.cmp(a.1));
        for (model, count) in models.iter().take(5) {
            let model_display = if model.len() > 28 {
                format!("{}...", &model[..25])
            } else {
                model.to_string()
            };
            println!(
                "│   • {:<28} {:>6} sessions         │",
                style(model_display).green(),
                style(count).bold()
            );
        }
    }

    if !stats.tools_usage_count.is_empty() {
        println!(
            "{}",
            style("├──────────────────────────────────────────────────────────┤").bold()
        );
        println!("│ Top Tools Used:                                          │");
        let mut tools: Vec<_> = stats.tools_usage_count.iter().collect();
        tools.sort_by(|a, b| b.1.cmp(a.1));
        for (tool, count) in tools.iter().take(5) {
            let tool_display = if tool.len() > 28 {
                format!("{}...", &tool[..25])
            } else {
                tool.to_string()
            };
            println!(
                "│   • {:<28} {:>6} calls            │",
                style(tool_display).yellow(),
                style(count).bold()
            );
        }
    }

    if !top_repos.is_empty() {
        println!(
            "{}",
            style("├──────────────────────────────────────────────────────────┤").bold()
        );
        println!("│ Top Repositories / Workspaces:                           │");
        for (repo, count) in top_repos.iter().take(5) {
            let repo_display = if repo.len() > 28 {
                format!("{}...", &repo[..25])
            } else {
                repo.to_string()
            };
            println!(
                "│   • {:<28} {:>6} sessions         │",
                style(repo_display).blue(),
                style(count).bold()
            );
        }
    }

    println!(
        "{}",
        style("└──────────────────────────────────────────────────────────┘").bold()
    );
    println!();
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
                    "primary_outcome": kind.map(|k| format!("{:?}", k)),
                    "score": score,
                    "model_filter_tokens": model_filter_tokens,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_rows)?);
    } else if rows.is_empty() {
        println!("{}", style("No sessions found in index.").yellow());
        println!(
            "Run {} to discover and index traces.",
            style("agentworth scan").cyan().bold()
        );
    } else {
        print_traces_table(&rows);
    }

    Ok(())
}

fn print_traces_table(rows: &[(String, f64, Option<agentworth_schema::OutcomeKind>, agentworth_storage::SessionSummary)]) {
    println!();
    println!(
        "{}",
        style("┌───────────────┬───────┬──────────────────────────────────────┬─────────────┬──────────────────┬──────────┬──────────┬────────┬──────────────────────┐").bold()
    );
    println!(
        "│ {:<13} │ {:<5} │ {:<36} │ {:<11} │ {:<16} │ {:<8} │ {:<8} │ {:<6} │ {:<20} │",
        style("VERDICT").bold().cyan(),
        style("SCORE").bold().cyan(),
        style("SESSION ID").bold().cyan(),
        style("ADAPTER").bold().cyan(),
        style("STARTED").bold().cyan(),
        style("DURATION").bold().cyan(),
        style("TOKENS").bold().cyan(),
        style("EVENTS").bold().cyan(),
        style("MODELS").bold().cyan(),
    );
    println!(
        "{}",
        style("├───────────────┼───────┼──────────────────────────────────────┼─────────────┼──────────────────┼──────────┼──────────┼────────┼──────────────────────┤").bold()
    );

    for (badge, score, kind, s) in rows {
        let id_display = if s.session_id.len() > 36 {
            format!("{}...", &s.session_id[..33])
        } else {
            s.session_id.clone()
        };

        let started_display = s.started_at.format("%Y-%m-%d %H:%M").to_string();

        let duration_display = if let Some(d) = s.duration_seconds {
            format_duration(d)
        } else {
            "-".to_string()
        };

        let models_display = if s.models_used.is_empty() {
            "-".to_string()
        } else {
            let joined = s.models_used.join(", ");
            if joined.len() > 20 {
                format!("{}...", &joined[..17])
            } else {
                joined
            }
        };

        let styled_badge = match kind {
            Some(agentworth_schema::OutcomeKind::CiOrDeploymentVerified) => style(badge).bold().green(),
            Some(agentworth_schema::OutcomeKind::CommitObserved) => style(badge).green(),
            Some(agentworth_schema::OutcomeKind::TestOrBuildPassed) => style(badge).bold().cyan(),
            Some(agentworth_schema::OutcomeKind::ArtifactChanged) => style(badge).yellow(),
            Some(agentworth_schema::OutcomeKind::DoneClaimed) => style(badge).dim(),
            None => style(badge).dim(),
        };

        let styled_score = if *score >= 70.0 {
            style(format!("{:>5.0}", score)).bold().green()
        } else if *score >= 40.0 {
            style(format!("{:>5.0}", score)).bold().yellow()
        } else {
            style(format!("{:>5.0}", score)).dim()
        };

        println!(
            "│ {:<13} │ {:>5} │ {:<36} │ {:<11} │ {:<16} │ {:>8} │ {:>8} │ {:>6} │ {:<20} │",
            styled_badge,
            styled_score,
            style(id_display).bold(),
            style(&s.adapter).green(),
            style(started_display).dim(),
            duration_display,
            style(format_number(s.total_tokens)).magenta(),
            s.total_events,
            style(models_display).dim(),
        );
    }

    println!(
        "{}",
        style("└───────────────┴───────┴──────────────────────────────────────┴─────────────┴──────────────────┴──────────┴──────────┴────────┴──────────────────────┘").bold()
    );
    println!(
        "Showing {} traces. Use {} for details.",
        rows.len(),
        style("agentworth inspect <id>").cyan()
    );
    println!();
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

    let output_content = match format {
        "atif" => agentworth_export_atif::export_to_atif(&trace, true)?,
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

fn print_scan_summary(summary: &ScanSummary, db_path: Option<&std::path::Path>) {
    println!();
    println!(
        "{}",
        style("┌──────────────────────────────────────────────────────────┐").bold()
    );
    println!(
        "{}",
        style("│ AgentWorth Scan Summary                                  │")
            .bold()
            .cyan()
    );
    println!(
        "{}",
        style("├──────────────────────────────────────────────────────────┤").bold()
    );
    println!(
        "│ Discovered:     {:>8} session files                   │",
        style(summary.discovered_sources).bold()
    );
    println!(
        "│ Scanned / Sync: {:>8} ({} skipped, {} errors)   │",
        style(summary.scanned_sessions).green().bold(),
        style(summary.skipped_unchanged).dim(),
        if summary.errors_encountered > 0 {
            style(summary.errors_encountered).red().bold().to_string()
        } else {
            style(0).dim().to_string()
        }
    );
    println!(
        "│ Total Indexed:  {:>8} sessions in SQLite index        │",
        style(summary.total_indexed_sessions).bold().yellow()
    );
    if let Some(path) = db_path {
        let path_str = path.to_string_lossy();
        let display_path = if path_str.len() > 40 {
            format!("...{}", &path_str[path_str.len() - 37..])
        } else {
            path_str.to_string()
        };
        println!("│ Index Path:     {:<41}│", style(display_path).dim());
    }
    println!(
        "{}",
        style("├──────────────────────────────────────────────────────────┤").bold()
    );

    let tokens = &summary.aggregate_stats.token_usage;
    println!(
        "│ Total Tokens:   {:>8} ({})                     │",
        style(format_number(tokens.total())).bold().magenta(),
        tokens.total()
    );
    println!(
        "│   • Input:      {:>8}                                │",
        style(format_number(tokens.input_tokens)).dim()
    );
    println!(
        "│   • Output:     {:>8}                                │",
        style(format_number(tokens.output_tokens)).dim()
    );
    println!(
        "│   • Cache Read: {:>8}                                │",
        style(format_number(tokens.cache_read_tokens)).dim()
    );
    println!(
        "│   • Cache Write:{:>8}                                │",
        style(format_number(tokens.cache_creation_tokens)).dim()
    );

    if !summary.aggregate_stats.sessions_by_adapter.is_empty() {
        println!(
            "{}",
            style("├──────────────────────────────────────────────────────────┤").bold()
        );
        println!("│ Adapters:                                                │");
        let mut sorted_adapters: Vec<_> = summary.aggregate_stats.sessions_by_adapter.iter().collect();
        sorted_adapters.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        for (adapter, count) in sorted_adapters {
            println!(
                "│   • {:<20} {:>6} sessions               │",
                style(adapter).cyan(),
                style(count).bold()
            );
        }
    }

    if !summary.aggregate_stats.models_usage_count.is_empty() {
        println!(
            "{}",
            style("├──────────────────────────────────────────────────────────┤").bold()
        );
        println!("│ Top Models:                                              │");
        let mut models: Vec<_> = summary.aggregate_stats.models_usage_count.iter().collect();
        models.sort_by(|a, b| b.1.cmp(a.1));
        for (model, count) in models.iter().take(5) {
            let model_display = if model.len() > 28 {
                format!("{}...", &model[..25])
            } else {
                model.to_string()
            };
            println!(
                "│   • {:<28} {:>6} sessions         │",
                style(model_display).green(),
                style(count).bold()
            );
        }
    }

    if !summary.aggregate_stats.tools_usage_count.is_empty() {
        println!(
            "{}",
            style("├──────────────────────────────────────────────────────────┤").bold()
        );
        println!("│ Top Tools Used:                                          │");
        let mut tools: Vec<_> = summary.aggregate_stats.tools_usage_count.iter().collect();
        tools.sort_by(|a, b| b.1.cmp(a.1));
        for (tool, count) in tools.iter().take(5) {
            let tool_display = if tool.len() > 28 {
                format!("{}...", &tool[..25])
            } else {
                tool.to_string()
            };
            println!(
                "│   • {:<28} {:>6} calls            │",
                style(tool_display).yellow(),
                style(count).bold()
            );
        }
    }

    println!(
        "{}",
        style("└──────────────────────────────────────────────────────────┘").bold()
    );
    println!();
}

fn run_doctor_command(json_output: bool, custom_db_path: Option<PathBuf>) -> Result<()> {
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
        if let Ok(stats) = st.get_aggregate_stats() {
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

    print_archie_ascii_banner();
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

fn run_matrix_command(json_output: bool, _db_path: Option<PathBuf>) -> Result<()> {
    let adapters: Vec<Box<dyn agentworth_adapter_sdk::AgentAdapter>> = vec![
        Box::new(agentworth_adapters::AiderAdapter::new()),
        Box::new(agentworth_adapters::ClaudeCodeAdapter::new()),
        Box::new(agentworth_adapters::ClineAdapter::new()),
        Box::new(agentworth_adapters::CodexAdapter::new()),
        Box::new(agentworth_adapters::CursorAdapter::new()),
        Box::new(agentworth_adapters::DeepSeekAdapter::new()),
        Box::new(agentworth_adapters::GeminiAdapter::new()),
        Box::new(agentworth_adapters::GooseAdapter::new()),
        Box::new(agentworth_adapters::GrokAdapter::new()),
        Box::new(agentworth_adapters::HerdrAdapter::new()),
        Box::new(agentworth_adapters::HermesAdapter::new()),
        Box::new(agentworth_adapters::KimiAdapter::new()),
        Box::new(agentworth_adapters::ManusAdapter::new()),
        Box::new(agentworth_adapters::MiniMaxAdapter::new()),
        Box::new(agentworth_adapters::OpenClawAdapter::new()),
        Box::new(agentworth_adapters::OpenCodeAdapter::new()),
        Box::new(agentworth_adapters::PiAdapter::new()),
        Box::new(agentworth_adapters::QwenAdapter::new()),
        Box::new(agentworth_adapters::WindsurfAdapter::new()),
        Box::new(agentworth_adapters::ZhipuAdapter::new()),
    ];

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

    print_archie_ascii_banner();
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

fn run_usage_command(
    period: &str,
    pacing: bool,
    hours: i64,
    alert_above: Option<f64>,
    limit: usize,
    by_model: bool,
    json: bool,
    db_path: Option<PathBuf>,
) -> Result<()> {
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

        println!();
        if let Some(threshold) = alert_above {
            if alert_triggered {
                println!(
                    "{}",
                    style(format!(
                        "🚨 BURN ALARM TRIGGERED: Window spend (${:.2}) exceeds alert threshold (${:.2})!",
                        p.estimated_cost_usd, threshold
                    ))
                    .bold()
                    .red()
                );
            } else {
                println!(
                    "{}",
                    style(format!(
                        "🛡️  Burn Alarm Safe: Window spend (${:.2}) is below threshold (${:.2}).",
                        p.estimated_cost_usd, threshold
                    ))
                    .bold()
                    .green()
                );
            }
            println!();
        }

        println!(
            "{}",
            style("┌──────────────────────────────────────────────────────────┐").bold()
        );
        println!(
            "│ {}                   │",
            style(format!("⏱️  {}-Hour Rolling Pacing Window", hours)).bold()
        );
        println!(
            "{}",
            style("├──────────────────────────────────────────────────────────┤").bold()
        );
        println!(
            "│ Window:          {} -> {} │",
            style(p.started_at.format("%Y-%m-%d %H:%M").to_string()).cyan(),
            style(p.ended_at.format("%H:%M").to_string()).cyan(),
        );
        println!(
            "│ Sessions Active: {:<39} │",
            style(p.session_count).bold()
        );
        println!(
            "│ Total Events:    {:<39} │",
            style(p.total_events).bold()
        );
        println!(
            "│ Tokens Consumed: {:<39} │",
            style(format!(
                "{} ({:.2}M)",
                format_number(p.total_tokens),
                p.total_tokens as f64 / 1_000_000.0
            ))
            .green()
            .bold()
        );
        println!(
            "│ Burn Velocity:   {:<39} │",
            style(format!(
                "{:.1}M tokens / hour",
                p.burn_rate_tokens_per_hour / 1_000_000.0
            ))
            .yellow()
        );
        println!(
            "│ Prompt Caching:  {:<39} │",
            style(format!("{:.1}% cache hit ratio", p.cache_hit_ratio)).cyan()
        );
        println!(
            "│ Estimated Cost:  {:<39} │",
            style(format!("${:.2} USD", p.estimated_cost_usd))
                .magenta()
                .bold()
        );

        if !p.active_adapters.is_empty() {
            println!(
                "{}",
                style("├──────────────────────────────────────────────────────────┤").bold()
            );
            println!("│ Active Adapters in Window:                               │");
            for a in &p.active_adapters {
                println!("│   • {:<52} │", style(a).cyan());
            }
        }

        if !p.active_models.is_empty() {
            println!(
                "{}",
                style("├──────────────────────────────────────────────────────────┤").bold()
            );
            println!("│ Active Models in Window:                                 │");
            for m in &p.active_models {
                println!("│   • {:<52} │", style(m).green());
            }
        }

        println!(
            "{}",
            style("└──────────────────────────────────────────────────────────┘").bold()
        );
        println!();
        return Ok(());
    }

    if by_model {
        let records = storage.get_model_usage(period, limit)?;

        if json {
            println!("{}", serde_json::to_string_pretty(&records)?);
            return Ok(());
        }

        if records.is_empty() {
            println!("No usage records found. Run `agwt scan` to index local sessions.");
            return Ok(());
        }

        let title = match period {
            "week" => "📅 AgentWorth Weekly Usage Rollup (by model)",
            "month" => "🗓️  AgentWorth Monthly Usage Rollup (by model)",
            _ => "📊 AgentWorth Daily Usage Ledger (by model)",
        };

        println!();
        println!("{}", style(format!("┌─ {} ───────────────────────────────────────────┐", title)).bold());
        println!(
            "{}",
            style("├────────────┬─────────────┬───────────┬────────────┬────────────┬────────────┬───────────┤").bold()
        );
        println!(
            "│ {:<10} │ {:<11} │ {:<9} │ {:<10} │ {:<10} │ {:<10} │ {:<9} │",
            style("PERIOD").bold(),
            style("MODEL").bold(),
            style("SESSIONS").bold(),
            style("INPUT").bold(),
            style("OUTPUT").bold(),
            style("CACHE READ").bold(),
            style("EST. COST").bold()
        );
        println!(
            "{}",
            style("├────────────┼─────────────┼───────────┼────────────┼────────────┼────────────┼───────────┤").bold()
        );

        let mut total_sessions = 0;
        let mut total_tokens = 0;
        let mut total_cost = 0.0;

        for r in &records {
            total_sessions += r.session_count;
            total_tokens += r.total_tokens;
            total_cost += r.estimated_cost_usd;

            let model_disp = if r.model.len() > 11 {
                format!("{}...", &r.model[..8])
            } else {
                r.model.clone()
            };
            let input_disp = format_number(r.input_tokens);
            let output_disp = format_number(r.output_tokens);
            let cache_disp = format_number(r.cache_read_tokens);
            let cost_disp = format!("${:.2}", r.estimated_cost_usd);

            println!(
                "│ {:<10} │ {:<11} │ {:>9} │ {:>10} │ {:>10} │ {:>10} │ {:>9} │",
                style(&r.period).cyan(),
                style(model_disp).green(),
                r.session_count,
                input_disp,
                output_disp,
                cache_disp,
                style(cost_disp).magenta()
            );
        }

        println!(
            "{}",
            style("├────────────┴─────────────┼───────────┼────────────┴────────────┼────────────┼───────────┤").bold()
        );
        println!(
            "│ TOTAL (Displayed)        │ {:>9} │ {:<23} │ {:>10} │ {:>9} │",
            style(total_sessions).bold(),
            "",
            style(format_number(total_tokens)).bold(),
            style(format!("${:.2}", total_cost)).bold().magenta()
        );
        println!(
            "{}",
            style("└──────────────────────────┴───────────┴─────────────────────────┴────────────┴───────────┘").bold()
        );
        println!();

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
        println!("No usage records found. Run `agentworth scan` (or `agwt scan`) to index local sessions.");
        return Ok(());
    }

    let title = match period {
        "week" => "📅 AgentWorth Weekly Usage Rollup",
        "month" => "🗓️  AgentWorth Monthly Usage Rollup",
        _ => "📊 AgentWorth Daily Usage Ledger",
    };

    println!();
    println!("{}", style(format!("┌─ {} ───────────────────────────────────────────┐", title)).bold());
    println!(
        "{}",
        style("├────────────┬─────────────┬───────────┬────────────┬────────────┬────────────┬───────────┤").bold()
    );
    println!(
        "│ {:<10} │ {:<11} │ {:<9} │ {:<10} │ {:<10} │ {:<10} │ {:<9} │",
        style("PERIOD").bold(),
        style("ADAPTER").bold(),
        style("SESSIONS").bold(),
        style("INPUT").bold(),
        style("OUTPUT").bold(),
        style("CACHE READ").bold(),
        style("EST. COST").bold()
    );
    println!(
        "{}",
        style("├────────────┼─────────────┼───────────┼────────────┼────────────┼────────────┼───────────┤").bold()
    );

    let mut total_sessions = 0;
    let mut total_tokens = 0;
    let mut total_cost = 0.0;

    for r in &records {
        total_sessions += r.session_count;
        total_tokens += r.total_tokens;
        total_cost += r.estimated_cost_usd;

        let input_disp = format_number(r.input_tokens);
        let output_disp = format_number(r.output_tokens);
        let cache_disp = format_number(r.cache_read_tokens);
        let cost_disp = format!("${:.2}", r.estimated_cost_usd);

        println!(
            "│ {:<10} │ {:<11} │ {:>9} │ {:>10} │ {:>10} │ {:>10} │ {:>9} │",
            style(&r.period).cyan(),
            style(&r.adapter).green(),
            r.session_count,
            input_disp,
            output_disp,
            cache_disp,
            style(cost_disp).magenta()
        );
    }

    println!(
        "{}",
        style("├────────────┴─────────────┼───────────┼────────────┴────────────┼────────────┼───────────┤").bold()
    );
    println!(
        "│ TOTAL (Displayed)        │ {:>9} │ {:<23} │ {:>10} │ {:>9} │",
        style(total_sessions).bold(),
        "",
        style(format_number(total_tokens)).bold(),
        style(format!("${:.2}", total_cost)).bold().magenta()
    );
    println!(
        "{}",
        style("└──────────────────────────┴───────────┴─────────────────────────┴────────────┴───────────┘").bold()
    );
    println!();

    Ok(())
}

// -----------------------------------------------------------------------------
// Command: Blame
// -----------------------------------------------------------------------------

fn run_blame_command(
    file_path: &str,
    json: bool,
    db_path: Option<PathBuf>,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let matches = storage.find_sessions_for_blame(file_path)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&matches)?);
        return Ok(());
    }

    println!();
    println!(
        "{}",
        style("┌──────────────────────────────────────────────────────────┐").bold()
    );
    println!(
        "│ {}             │",
        style(format!("🔍 AI Code Blame: {}", if file_path.len() > 38 { format!("...{}", &file_path[file_path.len()-35..]) } else { file_path.to_string() })).bold()
    );
    println!(
        "{}",
        style("├──────────────────────────────────────────────────────────┤").bold()
    );

    if matches.is_empty() {
        println!("│ No indexed sessions modified files matching pattern.     │");
    } else {
        println!("│ Found {} session(s) touching this path:                  │", matches.len());
        println!(
            "{}",
            style("├──────────────────────────────────────────────────────────┤").bold()
        );
        for (i, m) in matches.iter().enumerate() {
            println!(
                "│ [{}] Session: {:<43} │",
                i + 1,
                style(&m.session_id).cyan().bold()
            );
            println!(
                "│     Adapter:   {:<41} │",
                style(&m.adapter).green()
            );
            println!(
                "│     Touched:   {:<41} │",
                format!(
                    "{} ({})",
                    m.modified_at.format("%Y-%m-%d %H:%M:%S UTC"),
                    m.action
                )
            );
            println!(
                "│     Started:   {:<41} │",
                m.started_at.format("%Y-%m-%d %H:%M:%S UTC").to_string()
            );
            if !m.models_used.is_empty() {
                println!(
                    "│     Models:    {:<41} │",
                    style(m.models_used.join(", ")).yellow()
                );
            }
            println!(
                "│     Tokens:    {:<41} │",
                format!("{} tokens ({} tool calls)", format_number(m.total_tokens), m.tool_calls_count)
            );
            if i + 1 < matches.len() {
                println!(
                    "│                                                          │"
                );
            }
        }
    }

    println!(
        "{}",
        style("└──────────────────────────────────────────────────────────┘").bold()
    );
    println!();

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
