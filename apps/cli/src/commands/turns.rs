//! The `archie turns` verb: ingest the human-turn lane on its own, without a full scan.
//! A taxonomy-version bump wipes and re-parses every turn source (see
//! `agentworth_adapters::human_turns::INGESTION_VERSION`); this is the one command that
//! pays that bump's cost without rescanning sessions. Also the repair turn for the
//! turn↔session link: tier-2 (exact source path) and tier-3 (display repo) need each
//! turn row's `source_path`, which ingester v2+ writes — legacy rows carry empty strings.

use anyhow::Result;

pub fn run_turns_command(
    force: bool,
    json: bool,
    db_path: Option<std::path::PathBuf>,
    _ui: &crate::ui::Ui,
) -> Result<()> {
    let storage = crate::app::open_storage(db_path)?;
    let ingestor = agentworth_adapters::human_turns::HumanTurnIngestor::from_system();
    let summary =
        agentworth_core::turns::ingest_human_turns(&ingestor, &storage, force)?;

    if json {
        let value = serde_json::json!({
            "ingestion_version": agentworth_adapters::human_turns::INGESTION_VERSION,
            "sources_found": summary.sources_found,
            "sources_skipped_unchanged": summary.sources_skipped_unchanged,
            "turns_inserted": summary.turns_inserted,
            "turns_deduplicated": summary.turns_deduplicated,
            "turns_degraded": summary.turns_degraded,
            "errors": summary.errors,
            "total_turns": summary.total_turns(),
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }

    if summary.sources_found == 0 {
        println!("no turn sources found on this system; nothing to ingest");
        return Ok(());
    }

    println!(
        "Turn ingestion v{}: {} source(s), {} unchanged, {} turn(s) added, {} deduplicated, {} degraded, {} error(s)",
        agentworth_adapters::human_turns::INGESTION_VERSION,
        summary.sources_found,
        summary.sources_skipped_unchanged,
        summary.turns_inserted,
        summary.turns_deduplicated,
        summary.turns_degraded,
        summary.errors
    );
    let insights = storage.get_insights()?;
    let tl = insights.turn_link;
    println!(
        "Turn↔session link: {} of {} linked ({} by id, {} by path, {} by repo) · {} unmatched",
        tl.linked, tl.total, tl.linked_by_id, tl.linked_by_path, tl.linked_by_repo, tl.unmatched
    );
    Ok(())
}
