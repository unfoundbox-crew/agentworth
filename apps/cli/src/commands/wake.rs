//! `archie session wake`.
//!
//! The same facts the `session_wake` MCP tool returns, rendered for a person instead of an
//! agent. The document is thirty lines by design (`docs/specs/wake.md`), so it is the terminal
//! output too -- there is no separate human rendering to keep in sync with the markdown one.

use std::path::PathBuf;
use std::sync::Arc;

use agentworth_core::Scanner;
use agentworth_storage::Storage;
use anyhow::Result;

use crate::ui::Ui;
use crate::wake::{load_wake, render_markdown, WakeOptions};

fn open_storage(db_path: Option<PathBuf>) -> Result<Arc<Storage>> {
    if let Some(path) = db_path {
        Ok(Arc::new(Storage::open_path(&path)?))
    } else {
        Ok(Arc::new(Storage::open_default()?))
    }
}

pub fn run_wake_command(
    workspace: Option<PathBuf>,
    repo: Option<String>,
    redact: bool,
    json: bool,
    db_path: Option<PathBuf>,
    ui: &Ui,
) -> Result<()> {
    let storage = open_storage(db_path)?;
    let scanner = Scanner::new(storage.clone());

    let workspace = workspace
        .map(Ok)
        .unwrap_or_else(std::env::current_dir)?;
    let repo = repo.unwrap_or_else(|| {
        agentworth_schema::extract_repository_or_workspace(&workspace.to_string_lossy())
    });
    let options = WakeOptions {
        include_raw: !redact,
    };

    let report = crate::ui::with_status(ui, "reading the checkout and the index", || {
        load_wake(&storage, &scanner, &repo, &workspace, options)
    })?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("{}", render_markdown(&report));
    Ok(())
}
