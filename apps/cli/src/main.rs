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

        /// Output traces as formatted JSON
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
        Commands::Traces {
            limit,
            adapter,
            model,
            json,
        } => {
            run_traces_command(limit, adapter, model, json, cli.db_path)?;
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

fn run_stats_command(json: bool, db_path: Option<PathBuf>) -> Result<()> {
    let storage = open_storage(db_path)?;
    let stats = storage.get_aggregate_stats()?;
    let top_repos = storage.get_top_repositories()?;

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
        print_stats_view(&stats, &top_repos, storage.db_path());
    }

    Ok(())
}

fn print_stats_view(
    stats: &agentworth_storage::AggregateStats,
    top_repos: &[(String, usize)],
    db_path: Option<&std::path::Path>,
) {
    println!();
    println!(
        "{}",
        style("┌──────────────────────────────────────────────────────────┐").bold()
    );
    println!(
        "{}",
        style("│ AgentWorth Machine-Wide Experience Summary               │")
            .bold()
            .cyan()
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
        for (adapter, count) in &stats.sessions_by_adapter {
            let pct = if stats.total_sessions > 0 {
                (*count as f64 / stats.total_sessions as f64) * 100.0
            } else {
                0.0
            };
            println!(
                "│   • {:<20} {:>6} sessions ({:>4.1}%)        │",
                style(adapter).cyan(),
                style(count).bold(),
                pct
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
    json: bool,
    db_path: Option<PathBuf>,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let filter = SessionFilter {
        adapter,
        model,
        limit: Some(limit),
        order_by: Some(SessionOrderBy::StartedAtDesc),
        ..Default::default()
    };

    let sessions = storage.list_sessions_filtered(&filter)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&sessions)?);
    } else if sessions.is_empty() {
        println!("{}", style("No sessions found in index.").yellow());
        println!(
            "Run {} to discover and index traces.",
            style("agentworth scan").cyan().bold()
        );
    } else {
        print_traces_table(&sessions);
    }

    Ok(())
}

fn print_traces_table(sessions: &[agentworth_storage::SessionSummary]) {
    println!();
    println!(
        "{}",
        style("┌──────────────────────────────────────┬─────────────┬──────────────────┬──────────┬──────────┬────────┬────────────────────────┐").bold()
    );
    println!(
        "│ {:<36} │ {:<11} │ {:<16} │ {:<8} │ {:<8} │ {:<6} │ {:<22} │",
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
        style("├──────────────────────────────────────┼─────────────┼──────────────────┼──────────┼──────────┼────────┼────────────────────────┤").bold()
    );

    for s in sessions {
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
            if joined.len() > 22 {
                format!("{}...", &joined[..19])
            } else {
                joined
            }
        };

        println!(
            "│ {:<36} │ {:<11} │ {:<16} │ {:>8} │ {:>8} │ {:>6} │ {:<22} │",
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
        style("└──────────────────────────────────────┴─────────────┴──────────────────┴──────────┴──────────┴────────┴────────────────────────┘").bold()
    );
    println!(
        "Showing {} traces. Use {} for details.",
        sessions.len(),
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

    if !outcomes.is_empty() {
        println!(
            "{}",
            style("╠════════════════════════════════════════════════════════════════════════════════╣").bold()
        );
        println!(
            "║ Detected Outcomes:                                                             ║"
        );
        for o in &outcomes {
            println!(
                "║   🏆 {:<24} (confidence: {:>3.0}%) - {:<28} ║",
                style(format!("{:?}", o.kind)).bold().green(),
                o.confidence * 100.0,
                if o.summary.len() > 28 {
                    format!("{}...", &o.summary[..25])
                } else {
                    o.summary.clone()
                }
            );
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
        for (adapter, count) in &summary.aggregate_stats.sessions_by_adapter {
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
