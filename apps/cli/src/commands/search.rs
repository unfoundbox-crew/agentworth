//! Semantic latent vector search command for AgentWorth.
//!
//! Subcommand: `agwt search "<query>" [--limit N] [--min-score F] [--kind KIND] [--json]`
//! Generates query embedding vector, queries the VectorStore, and renders ASCII thermal receipt cards.

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use agentworth_core::Scanner;
use agentworth_schema::vector::{ChunkKind, VectorSearchResult};
use agentworth_storage::vector::VectorStore;
use agentworth_storage::{
    extract_repository_or_workspace, LocalEmbedder, SessionFilter, Storage, TrajectoryChunker,
};
use anyhow::Result;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};

/// Execute the `agwt search` subcommand.
pub fn run_search_command(
    query: &str,
    limit: usize,
    min_score: f32,
    kind: Option<String>,
    json_output: bool,
    db_path: Option<PathBuf>,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let vector_store = storage.vector_store()?;
    let embedder = LocalEmbedder::new();

    // 1. Check if vector store needs auto-indexing
    let mut stats = vector_store.stats()?;
    if stats.total_chunks == 0 {
        let all_sessions = storage.list_sessions_filtered(&SessionFilter {
            limit: Some(10000),
            ..Default::default()
        })?;

        if !all_sessions.is_empty() {
            if !json_output {
                println!(
                    "{}",
                    style(format!(
                        "⚡ Indexing {} sessions into vector store for latent semantic retrieval...",
                        all_sessions.len()
                    ))
                    .dim()
                );
            }

            let scanner = Scanner::new(storage.clone());
            let mut total_indexed_chunks = 0;

            let pb = if !json_output {
                let pb = ProgressBar::new(all_sessions.len() as u64);
                pb.set_style(
                    ProgressStyle::default_bar()
                        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                        .template(
                            "{spinner:.cyan.bold} Embedding trajectories ▕{bar:30.cyan/238}▏ {pos}/{len} sessions",
                        )
                        .unwrap()
                        .progress_chars("█▉▊▋▌▍▎▏ "),
                );
                Some(pb)
            } else {
                None
            };

            for sess in &all_sessions {
                if let Ok(trace) = scanner.load_trace(&sess.session_id) {
                    let chunks = TrajectoryChunker::extract_chunks(&trace);
                    if !chunks.is_empty() {
                        let texts: Vec<String> =
                            chunks.iter().map(|c| c.text_content.clone()).collect();
                        if let Ok(embeddings) = embedder.embed_batch(&texts) {
                            let _ = vector_store.insert_embeddings(&chunks, &embeddings);
                            total_indexed_chunks += chunks.len();
                        }
                    }
                }
                if let Some(ref pb) = pb {
                    pb.inc(1);
                }
            }

            if let Some(pb) = pb {
                pb.finish_and_clear();
            }

            if !json_output && total_indexed_chunks > 0 {
                println!(
                    "{}",
                    style(format!(
                        "✓ Indexed {} trajectory chunks ({} dimensions)",
                        total_indexed_chunks,
                        embedder.dimension()
                    ))
                    .green()
                );
            }

            stats = vector_store.stats()?;
        }
    }

    // 2. Generate embedding for search query
    let query_vector = embedder.embed_text(query)?;

    // 3. Parse optional kind filter
    let kind_filter = kind.as_deref().and_then(|k| ChunkKind::from_str(k).ok());

    // 4. Query Vector Store with cosine similarity ranking
    let results =
        vector_store.search_filtered(&query_vector, limit, min_score, None, kind_filter)?;

    // 5. Output rendering
    if json_output {
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(());
    }

    if results.is_empty() {
        println!();
        println!(
            "{}",
            style(format!(
                "No semantic matches found for query '{}' (min_score: {:.2}).",
                query, min_score
            ))
            .dim()
        );
        if stats.total_chunks == 0 {
            println!(
                "{}",
                style("Tip: Run `agwt scan` first to index local agent histories.").yellow()
            );
        }
        println!();
        return Ok(());
    }

    render_ascii_thermal_receipts(query, &results, &embedder, &storage);

    Ok(())
}

fn open_storage(db_path: Option<PathBuf>) -> Result<Arc<Storage>> {
    if let Some(path) = db_path {
        Ok(Arc::new(Storage::open_path(&path)?))
    } else {
        Ok(Arc::new(Storage::open_default()?))
    }
}

/// Render beautiful ASCII thermal receipt cards with score badges, model, tokens, and turn snippets.
fn render_ascii_thermal_receipts(
    query: &str,
    results: &[VectorSearchResult],
    embedder: &LocalEmbedder,
    storage: &Storage,
) {
    let engine_tag = if embedder.is_onnx() {
        "ONNX (BGE-Small-EN-v1.5)"
    } else {
        "Offline Semantic Vector Engine (384-dim)"
    };

    println!();
    println!(
        "{}",
        style("┌─ ⚡ AgentWorth Semantic Latent Vector Search ──────────────────────────┐")
            .bold()
            .cyan()
    );
    println!(
        "│ Query:   {:<59} │",
        style(format!("\"{}\"", query)).bold().yellow()
    );
    println!(
        "│ Engine:  {:<59} │",
        style(engine_tag).dim()
    );
    println!(
        "│ Matches: {:<59} │",
        style(format!("{} trajectory turn(s) retrieved", results.len())).green()
    );
    println!(
        "{}",
        style("├────────────────────────────────────────────────────────────────────────┤")
            .bold()
    );

    for (i, res) in results.iter().enumerate() {
        let (_badge, badge_styled) = format_thermal_score_badge(res.score);
        let kind_badge = format!("[{}]", res.kind.as_str().to_uppercase());

        println!(
            "│ {}  {:<24} {:>32} │",
            style(format!("#{:02}", i + 1)).bold().dim(),
            badge_styled,
            style(&kind_badge).bold().magenta()
        );

        println!(
            "│ Session:   {:<58} │",
            style(&res.session_id).cyan().bold()
        );

        let model_disp = res.model.as_deref().unwrap_or("unknown");
        println!(
            "│ Adapter:   {:<24} Model:   {:<25} │",
            style(&res.adapter).green(),
            style(model_disp).yellow()
        );

        let started_disp = res
            .started_at
            .as_deref()
            .unwrap_or("Unknown timestamp");
        let token_burn_disp = if res.total_tokens > 1_000_000 {
            format!("{:.2}M tokens", res.total_tokens as f64 / 1_000_000.0)
        } else if res.total_tokens > 0 {
            format!("{} tokens", format_number(res.total_tokens))
        } else {
            "0 tokens".to_string()
        };

        println!(
            "│ Timestamp: {:<24} Tokens:  {:<25} │",
            style(started_disp).dim(),
            style(token_burn_disp).bold()
        );

        // Fetch session path to display repo / project
        let project_disp = storage
            .get_session_by_id(&res.session_id)
            .ok()
            .flatten()
            .map(|s| extract_repository_or_workspace(&s.source_path))
            .unwrap_or_else(|| "workspace".to_string());

        let turn_disp = if res.turn_index > 0 {
            format!("#{}", res.turn_index)
        } else {
            "Summary".to_string()
        };

        println!(
            "│ Project:   {:<24} Turn:    {:<25} │",
            style(project_disp).cyan(),
            style(turn_disp).yellow()
        );

        println!(
            "│ {}",
            style("──────────────────────────────────────────────────────────────────────").dim()
        );

        // Clean & wrap snippet text
        let snippet_lines = clean_and_wrap_text(&res.text_content, 68);
        for line in snippet_lines.iter().take(6) {
            println!("│   {:<66} │", style(line).italic());
        }
        if snippet_lines.len() > 6 {
            println!(
                "│   {:<66} │",
                style(format!("... [{} more lines]", snippet_lines.len() - 6)).dim()
            );
        }

        if i + 1 < results.len() {
            println!(
                "{}",
                style("├────────────────────────────────────────────────────────────────────────┤")
                    .bold()
            );
        }
    }

    println!(
        "{}",
        style("└────────────────────────────────────────────────────────────────────────┘")
            .bold()
    );
    println!();
}

/// Format thermal match score badge with emoji and color.
fn format_thermal_score_badge(score: f32) -> (String, console::StyledObject<String>) {
    let pct = (score * 100.0).clamp(0.0, 100.0);
    if pct >= 85.0 {
        let txt = format!("[MATCH: {:.1}% 🔥]", pct);
        (txt.clone(), style(txt).bold().red())
    } else if pct >= 70.0 {
        let txt = format!("[MATCH: {:.1}% ⚡]", pct);
        (txt.clone(), style(txt).bold().yellow())
    } else if pct >= 50.0 {
        let txt = format!("[MATCH: {:.1}% 💡]", pct);
        (txt.clone(), style(txt).bold().cyan())
    } else {
        let txt = format!("[MATCH: {:.1}% 🔎]", pct);
        (txt.clone(), style(txt).dim())
    }
}

/// Wrap text to a maximum line width.
fn clean_and_wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let mut current = String::new();
        for word in trimmed.split_whitespace() {
            if current.len() + word.len() + 1 > max_width {
                if !current.is_empty() {
                    lines.push(current);
                    current = String::new();
                }
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    if lines.is_empty() {
        lines.push("No text content recorded.".to_string());
    }
    lines
}

fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let mut count = 0;
    for c in s.chars().rev() {
        if count > 0 && count % 3 == 0 {
            result.push(',');
        }
        result.push(c);
        count += 1;
    }
    result.chars().rev().collect()
}
