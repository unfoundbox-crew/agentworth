//! Filesystem-event-driven live tail: watches the directories where adapters write session
//! files with `notify` and fans changes out to a broadcast channel for SSE clients.
//!
//! This is deliberately independent of `archie session watch` (`commands/watch.rs`), which polls
//! parsed traces on an interval to detect doom loops. That command answers "is this session
//! going wrong"; this module answers "a session file just changed, right now" — real
//! filesystem events, not a diff between two polls. They share no code on purpose.

use std::path::{Path, PathBuf};

use agentworth_adapter_sdk::ScanOptions;
use agentworth_core::Scanner;
use chrono::{DateTime, Utc};
use notify::{Event as NotifyEvent, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// Capacity of the live-tail broadcast channel. A subscriber that falls this far behind
/// gets a `Lagged` notification on its next receive rather than the channel growing
/// unbounded — see `get_live_tail_handler` in `routes.rs` for how that surfaces over SSE.
pub const LIVE_TAIL_CHANNEL_CAPACITY: usize = 256;

/// A directory watched for session-file changes, and the adapter that claims it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchRoot {
    pub path: PathBuf,
    pub adapter: String,
}

/// The kind of filesystem change observed on a watched session directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveTailChangeKind {
    Created,
    Modified,
    Removed,
    Other,
}

/// A single filesystem change broadcast to connected live-tail SSE clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveTailEvent {
    pub path: PathBuf,
    pub kind: LiveTailChangeKind,
    /// Name of the adapter whose session root contains `path`, when it could be attributed.
    pub adapter: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// Maps a raw `notify` event kind to our change kind, or `None` to drop the event entirely.
/// Pure reads (`EventKind::Access`) aren't a session-file change and would just be noise on
/// a "something changed" feed, so they never reach the broadcast channel.
fn classify(kind: &EventKind) -> Option<LiveTailChangeKind> {
    match kind {
        EventKind::Create(_) => Some(LiveTailChangeKind::Created),
        EventKind::Modify(_) => Some(LiveTailChangeKind::Modified),
        EventKind::Remove(_) => Some(LiveTailChangeKind::Removed),
        EventKind::Access(_) => None,
        EventKind::Any | EventKind::Other => Some(LiveTailChangeKind::Other),
    }
}

/// Picks the most specific (deepest) watch root that contains `changed`, if any, and
/// returns its adapter name. Roots are not assumed to be pre-collapsed: a nested pair
/// like `~/.claude` and `~/.claude/projects` both attributing to `claude_code` resolves
/// to the deeper one, which is what you'd want even if that stops being true.
///
/// Both `roots` and `changed` must already be in the same (canonical or raw) form for the
/// `starts_with` comparison to mean anything — see `canonicalize_or_self` and its callers
/// in `spawn_live_tail_watcher`, which is why this stays a plain, uncanonicalizing helper.
fn attribute_adapter(roots: &[WatchRoot], changed: &Path) -> Option<String> {
    roots
        .iter()
        .filter(|root| changed.starts_with(&root.path))
        .max_by_key(|root| root.path.components().count())
        .map(|root| root.adapter.clone())
}

/// Resolves `path` to its canonical form (symlinks and relative components resolved), or
/// returns it unchanged if that fails — e.g. the path doesn't exist yet, or was just removed.
///
/// `notify`'s FSEvents backend on macOS always reports canonicalized paths (`/private/var/...`
/// for `/var/...`, and resolved symlinks under `$HOME` in some setups), so both a watch root
/// and an incoming event path need this before `attribute_adapter` can compare them
/// meaningfully.
fn canonicalize_or_self(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Collapses a set of paths down to the top-level ones, dropping any path that is a
/// descendant of another path already in the set.
///
/// Several adapters report overlapping roots (e.g. Claude Code detects both `~/.claude`
/// and `~/.claude/projects`). Watching both recursively would double- or triple-broadcast
/// the same underlying change once per overlapping watch — this collapses them to one
/// watch per independent subtree first.
pub fn collapse_nested_roots(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort();
    paths.dedup();
    // Ancestors always have fewer path components than their descendants, so sorting by
    // component count guarantees every ancestor is already in `kept` by the time we reach
    // any of its descendants.
    paths.sort_by_key(|p| p.components().count());

    let mut kept: Vec<PathBuf> = Vec::new();
    'paths: for path in paths {
        for existing in &kept {
            if path.starts_with(existing) {
                continue 'paths;
            }
        }
        kept.push(path);
    }
    kept
}

/// Discovers the on-disk session directories for every registered adapter that is
/// currently present on this machine, by running each adapter's own `detect()` — the
/// same detection logic the scanner and `/api/matrix` already rely on, so the watch set
/// never drifts from what actually gets scanned.
pub fn discover_session_roots(scanner: &Scanner) -> Vec<WatchRoot> {
    let options = ScanOptions::default();
    let mut roots = Vec::new();

    for adapter in scanner.adapters() {
        match adapter.detect(&options) {
            Ok(detection) if detection.is_present => {
                for path in detection.discovered_roots {
                    roots.push(WatchRoot {
                        path,
                        adapter: adapter.name().to_string(),
                    });
                }
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(
                    "live-tail: adapter '{}' detection failed: {}",
                    adapter.name(),
                    e
                );
            }
        }
    }

    roots
}

/// Starts a filesystem watcher over `roots` that publishes a `LiveTailEvent` to `tx` for
/// every relevant change. Returns the live `RecommendedWatcher` — dropping it stops the
/// watch, so the caller must keep it alive for as long as events should keep flowing (see
/// `start_server`, which holds it for the lifetime of the server).
///
/// A `send` failure just means there are no subscribers connected right now (no SSE
/// client attached), which is normal and not a failure worth logging.
pub fn spawn_live_tail_watcher(
    roots: &[WatchRoot],
    tx: broadcast::Sender<LiveTailEvent>,
) -> notify::Result<RecommendedWatcher> {
    // `attribution` holds the canonicalized form of each root, used only for matching
    // incoming (canonicalized) event paths. `raw_roots` keeps the original, uncanonicalized
    // paths alongside it so a root whose directory didn't exist yet at watcher-start (and so
    // fell back to its raw path here) can be retried the next time an event comes in, once
    // the directory has plausibly been created. The roots actually passed to `watcher.watch`
    // below stay in their original raw form throughout — canonicalizing is purely an
    // attribution concern, not a watching one.
    let mut attribution: Vec<WatchRoot> = roots
        .iter()
        .map(|root| WatchRoot {
            path: canonicalize_or_self(&root.path),
            adapter: root.adapter.clone(),
        })
        .collect();
    let raw_roots: Vec<PathBuf> = roots.iter().map(|root| root.path.clone()).collect();

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<NotifyEvent>| {
        let event = match res {
            Ok(event) => event,
            Err(e) => {
                tracing::warn!("live-tail watcher error: {}", e);
                return;
            }
        };

        let Some(kind) = classify(&event.kind) else {
            return;
        };
        let timestamp = Utc::now();

        for (root, raw) in attribution.iter_mut().zip(raw_roots.iter()) {
            if root.path == *raw {
                if let Ok(canonical) = std::fs::canonicalize(raw) {
                    root.path = canonical;
                }
            }
        }

        for path in &event.paths {
            let canonical_path = canonicalize_or_self(path);
            let live_event = LiveTailEvent {
                path: path.clone(),
                kind,
                adapter: attribute_adapter(&attribution, &canonical_path),
                timestamp,
            };
            let _ = tx.send(live_event);
        }
    })?;

    for path in collapse_nested_roots(roots.iter().map(|r| r.path.clone()).collect()) {
        if let Err(e) = watcher.watch(&path, RecursiveMode::Recursive) {
            tracing::warn!("live-tail: failed to watch {:?}: {}", path, e);
        }
    }

    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentworth_adapter_sdk::{AgentAdapter, DetectionResult, ParseResult, SessionSource};
    use agentworth_storage::Storage;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn test_collapse_nested_roots_drops_descendants() {
        let roots = vec![
            PathBuf::from("/home/dev/.claude/projects"),
            PathBuf::from("/home/dev/.claude"),
            PathBuf::from("/home/dev/.codex"),
            PathBuf::from("/home/dev/.claude/sessions"),
        ];

        let mut collapsed = collapse_nested_roots(roots);
        collapsed.sort();

        assert_eq!(
            collapsed,
            vec![
                PathBuf::from("/home/dev/.claude"),
                PathBuf::from("/home/dev/.codex"),
            ]
        );
    }

    #[test]
    fn test_collapse_nested_roots_keeps_unrelated_paths() {
        let roots = vec![
            PathBuf::from("/home/dev/.claude"),
            PathBuf::from("/home/dev/.config/claude"),
        ];

        let mut collapsed = collapse_nested_roots(roots.clone());
        collapsed.sort();
        let mut expected = roots;
        expected.sort();

        assert_eq!(collapsed, expected);
    }

    #[test]
    fn test_attribute_adapter_picks_most_specific_root() {
        let roots = vec![
            WatchRoot {
                path: PathBuf::from("/home/dev/.claude"),
                adapter: "claude_code_outer".to_string(),
            },
            WatchRoot {
                path: PathBuf::from("/home/dev/.claude/projects"),
                adapter: "claude_code_inner".to_string(),
            },
            WatchRoot {
                path: PathBuf::from("/home/dev/.codex"),
                adapter: "codex".to_string(),
            },
        ];

        assert_eq!(
            attribute_adapter(&roots, Path::new("/home/dev/.claude/projects/sess-1.jsonl")),
            Some("claude_code_inner".to_string())
        );
        assert_eq!(
            attribute_adapter(&roots, Path::new("/home/dev/.claude/settings.json")),
            Some("claude_code_outer".to_string())
        );
        assert_eq!(
            attribute_adapter(&roots, Path::new("/home/dev/.codex/log.jsonl")),
            Some("codex".to_string())
        );
        assert_eq!(
            attribute_adapter(&roots, Path::new("/home/dev/unrelated/file.txt")),
            None
        );
    }

    // Reproduces the macOS FSEvents mismatch (see the real-filesystem test below) without
    // needing a platform-specific `/private/var` path: a symlinked root, declared in its raw
    // (un-resolved) form, mirrors a watch root that gets canonicalized by the OS the same way
    // FSEvents canonicalizes reported event paths. Uses a real tempdir + symlink so it runs
    // identically on macOS and Linux CI.
    #[test]
    fn test_attribute_adapter_requires_canonicalized_inputs_to_match_symlinked_root() {
        let temp_dir = tempfile::tempdir().expect("create tempdir");
        let real_dir = temp_dir.path().join("real");
        std::fs::create_dir(&real_dir).expect("create real dir");
        let symlinked_root = temp_dir.path().join("symlinked-root");
        std::os::unix::fs::symlink(&real_dir, &symlinked_root).expect("create symlink");

        let event_path = real_dir.join("session.jsonl");
        std::fs::write(&event_path, b"{}").expect("write file");

        let roots = vec![WatchRoot {
            path: symlinked_root,
            adapter: "test_adapter".to_string(),
        }];

        // Raw comparison fails: the event is reported (or, here, constructed) against the
        // resolved directory, not the symlink the root was declared as.
        assert_eq!(attribute_adapter(&roots, &event_path), None);

        // Canonicalizing both sides the way `spawn_live_tail_watcher` does resolves the
        // symlink on both the root and the event path, and the match succeeds.
        let canonical_roots: Vec<WatchRoot> = roots
            .iter()
            .map(|root| WatchRoot {
                path: canonicalize_or_self(&root.path),
                adapter: root.adapter.clone(),
            })
            .collect();
        let canonical_event_path = canonicalize_or_self(&event_path);
        assert_eq!(
            attribute_adapter(&canonical_roots, &canonical_event_path),
            Some("test_adapter".to_string())
        );
    }

    /// A minimal fake adapter for exercising `discover_session_roots` without touching the
    /// real home directory or any of the 20 real adapters.
    struct FakeAdapter {
        adapter_name: &'static str,
        roots: Vec<PathBuf>,
    }

    impl AgentAdapter for FakeAdapter {
        fn name(&self) -> &'static str {
            self.adapter_name
        }

        fn detect(&self, _options: &ScanOptions) -> anyhow::Result<DetectionResult> {
            Ok(DetectionResult {
                adapter_name: self.adapter_name,
                is_present: !self.roots.is_empty(),
                discovered_roots: self.roots.clone(),
                confidence: if self.roots.is_empty() { 0.0 } else { 1.0 },
            })
        }

        fn enumerate(&self, _options: &ScanOptions) -> anyhow::Result<Vec<SessionSource>> {
            Ok(Vec::new())
        }

        fn parse(&self, _source: &SessionSource) -> anyhow::Result<ParseResult> {
            unimplemented!("fake adapter: parse is not exercised by discover_session_roots")
        }
    }

    #[test]
    fn test_discover_session_roots_uses_each_adapters_detect() {
        let storage = Arc::new(Storage::open_in_memory().expect("open in-memory storage"));
        let scanner = Scanner::with_adapters(
            vec![
                Box::new(FakeAdapter {
                    adapter_name: "present_adapter",
                    roots: vec![PathBuf::from("/fake/home/.present")],
                }),
                Box::new(FakeAdapter {
                    adapter_name: "absent_adapter",
                    roots: vec![],
                }),
            ],
            storage,
        );

        let mut roots = discover_session_roots(&scanner);
        roots.sort_by(|a, b| a.adapter.cmp(&b.adapter));

        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].adapter, "present_adapter");
        assert_eq!(roots[0].path, PathBuf::from("/fake/home/.present"));
    }

    /// Exercises the real bug from PR #79: notify's FSEvents backend always reports
    /// canonicalized paths on macOS (confirmed in notify's own RawEvent docs), so a temp dir
    /// path like `/var/folders/...` (via the `/var` -> `/private/var` symlink) never matched
    /// what `attribute_adapter`'s plain `starts_with` comparison expected, and adapter
    /// attribution silently came back `None` for every live-tail event on macOS.
    /// `spawn_live_tail_watcher` now canonicalizes both the watch roots and each incoming
    /// event path before attributing, so this runs unconditionally on every OS.
    #[tokio::test]
    async fn test_watcher_broadcasts_real_filesystem_event() {
        let temp_dir = tempfile::tempdir().expect("create tempdir");
        let root = WatchRoot {
            path: temp_dir.path().to_path_buf(),
            adapter: "test_adapter".to_string(),
        };

        let (tx, mut rx) = broadcast::channel(LIVE_TAIL_CHANNEL_CAPACITY);
        let _watcher = spawn_live_tail_watcher(&[root], tx).expect("spawn watcher");

        // Give the watcher a moment to finish registering with the OS backend before we
        // write — inotify/FSEvents registration is not instantaneous.
        tokio::time::sleep(Duration::from_millis(200)).await;

        let file_path = temp_dir.path().join("session.jsonl");
        std::fs::write(&file_path, b"{\"type\":\"user\"}\n").expect("write session file");

        // macOS FSEvents can report activity for unrelated temp files that other tests
        // create concurrently in the same OS temp root (e.g. NamedTempFile in adapter
        // tests) before it reports the one this test cares about -- so scan the stream
        // for our file rather than assuming the first broadcast event is it.
        let event = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = rx
                    .recv()
                    .await
                    .expect("broadcast channel closed unexpectedly");
                if event.path.file_name() == file_path.file_name() {
                    return event;
                }
            }
        })
        .await
        .expect("timed out waiting for a filesystem event for session.jsonl");

        assert_eq!(event.path.file_name(), file_path.file_name());
        assert_eq!(event.adapter, Some("test_adapter".to_string()));
        assert!(
            matches!(
                event.kind,
                LiveTailChangeKind::Created | LiveTailChangeKind::Modified
            ),
            "expected Created or Modified, got {:?}",
            event.kind
        );
    }
}
