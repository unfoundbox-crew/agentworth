use std::path::{Path, PathBuf};
use std::process::Command;

use agentworth_schema::{
    AgentWorthTrace, EventPayload, FileActionType, NormalizedEvent, Provenance,
};
use agentworth_storage::{DemotedClaim, SessionRisk, Storage};
use chrono::{DateTime, Duration, Utc};

use super::*;

/// A real git repository in a temp dir with `git log` history, because the whole point of this
/// command is what git says — a mocked `git log` would test the parser and nothing else.
struct TempRepo {
    dir: tempfile::TempDir,
}

impl TempRepo {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = Self { dir };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.email", "test@example.invalid"]);
        repo.git(&["config", "user.name", "Test"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo
    }

    fn path(&self) -> PathBuf {
        // macOS puts temp dirs under /var, a symlink to /private/var. `git rev-parse
        // --show-toplevel` resolves it, and anchoring compares strings — so the fixture has to
        // use the resolved form too or nothing ever anchors.
        std::fs::canonicalize(self.dir.path()).expect("canonicalize temp dir")
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(self.dir.path())
            .args(args)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn commit_file(&self, rel: &str, contents: &str, message: &str) -> String {
        let full = self.dir.path().join(rel);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&full, contents).expect("write file");
        self.git(&["add", rel]);
        self.git(&["commit", "-q", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }
}

/// Seed a session that edited `abs_path` at `at`, with the given stored outcome.
fn seed_session(
    storage: &Storage,
    session_id: &str,
    repo_root: &Path,
    abs_path: &str,
    at: DateTime<Utc>,
    outcome: Option<&str>,
) {
    // A Claude Code transcript path whose project slug decodes to this repo, so a relative
    // blame row from it would anchor here too.
    let slug = repo_root.to_string_lossy().replace('/', "-");
    let source_path = format!("/tmp/claude/projects/{slug}/{session_id}.jsonl");

    let prov = Provenance::new(source_path, "claude_code", 100, 100, format!("fp_{session_id}"));
    let mut trace = AgentWorthTrace::new(session_id, "claude_code", prov, at);
    trace.stats.models_used = vec!["claude-opus-5".to_string()];
    trace.events.push(NormalizedEvent::new(
        1,
        at,
        EventPayload::FileAction {
            path: abs_path.to_string(),
            action: FileActionType::Edit,
            diff: None,
            lines_changed: None,
        },
    ));
    storage
        .upsert_session(&trace, outcome, Some(0.5))
        .expect("seed session");
}

fn query(repo: &Path) -> SuspectQuery {
    SuspectQuery {
        repo: repo.to_path_buf(),
        base: None,
        branch: None,
        since: None,
        window_hours: DEFAULT_WINDOW_HOURS,
        max_commits: 50,
    }
}

/// The headline case: two commits, one written by a session that never got past
/// `artifact_changed`. Exactly one is flagged, and the other is reported as unattributed rather
/// than quietly counted as clean.
#[test]
fn flags_the_commit_whose_session_never_proved_anything() {
    let repo = TempRepo::new();
    let storage = Storage::open_in_memory().expect("storage");
    let root = repo.path();

    repo.commit_file("src/lib.rs", "fn a() {}\n", "feat: add a");
    repo.commit_file("src/app.rs", "fn b() {}\n", "feat: add b");

    // Only the second commit's file has an authoring session.
    seed_session(
        &storage,
        "sess_unproven",
        &root,
        &format!("{}/src/app.rs", root.display()),
        Utc::now() - Duration::minutes(1),
        Some("artifact_changed"),
    );

    let report = compute_suspect_commits(&storage, &query(&root)).expect("report");

    assert_eq!(report.commits_scanned, 2);
    assert_eq!(report.attributed, 1);
    assert_eq!(report.unattributed, 1, "an untouched commit is unknown, not clean");
    assert_eq!(report.suspect.len(), 1);

    let flagged = &report.suspect[0];
    assert_eq!(flagged.subject, "feat: add b");
    assert_eq!(flagged.sessions.len(), 1);
    let session = &flagged.sessions[0];
    assert_eq!(session.session_id, "sess_unproven");
    assert_eq!(session.outcome.as_deref(), Some("artifact_changed"));
    assert!(session.reasons.iter().any(|r| r.code == "no_test_run"));
    assert!(session.risk_unknown, "no scan ran, so risk was never evaluated");
    assert!(session.evidence_path.ends_with("src/app.rs"));

    assert!(report.prompt.contains("feat: add b"), "prompt: {}", report.prompt);
    assert!(
        report.prompt.contains(&session.session_id[..8]),
        "the prompt hands over a session id to query, not a fix: {}",
        report.prompt
    );
}

/// A session that passed a test is not suspect, and neither is the commit it wrote.
#[test]
fn a_session_that_proved_something_is_not_flagged() {
    let repo = TempRepo::new();
    let storage = Storage::open_in_memory().expect("storage");
    let root = repo.path();

    repo.commit_file("src/lib.rs", "fn a() {}\n", "feat: add a");
    seed_session(
        &storage,
        "sess_proven",
        &root,
        &format!("{}/src/lib.rs", root.display()),
        Utc::now() - Duration::minutes(1),
        Some("test_or_build_passed"),
    );

    let report = compute_suspect_commits(&storage, &query(&root)).expect("report");
    assert_eq!(report.attributed, 1);
    assert!(report.suspect.is_empty());
    assert!(
        report.prompt.contains("risk signal"),
        "the honest empty case still says what was checked: {}",
        report.prompt
    );
}

/// A demoted claim flags a commit whose session otherwise reached a proving rung, and the
/// reason carries the event sequence behind it.
#[test]
fn a_demoted_claim_flags_a_session_that_otherwise_looked_proven() {
    let repo = TempRepo::new();
    let storage = Storage::open_in_memory().expect("storage");
    let root = repo.path();

    repo.commit_file("src/lib.rs", "fn a() {}\n", "feat: add a");
    seed_session(
        &storage,
        "sess_demoted",
        &root,
        &format!("{}/src/lib.rs", root.display()),
        Utc::now() - Duration::minutes(1),
        Some("commit_observed"),
    );
    storage
        .upsert_session_risk(&SessionRisk {
            session_id: "sess_demoted".to_string(),
            demoted_claims: 1,
            demoted_evidence: vec![DemotedClaim {
                event_sequence: 42,
                original_kind: "test_or_build_passed".to_string(),
                final_kind: "done_claimed".to_string(),
                original_confidence: 0.85,
                final_confidence: 0.20,
                reason: "only ever requested".to_string(),
            }],
            ..Default::default()
        })
        .expect("seed risk");

    let report = compute_suspect_commits(&storage, &query(&root)).expect("report");
    assert_eq!(report.suspect.len(), 1);
    let session = &report.suspect[0].sessions[0];
    assert!(!session.risk_unknown);
    let reason = session
        .reasons
        .iter()
        .find(|r| r.code == "demoted_claim")
        .expect("demoted_claim reason");
    assert_eq!(reason.event_sequence, Some(42));
    assert_eq!(report.sessions_with_unknown_risk, 0);
}

/// A file touch outside the attribution window does not author the commit.
#[test]
fn a_touch_outside_the_window_does_not_attribute() {
    let repo = TempRepo::new();
    let storage = Storage::open_in_memory().expect("storage");
    let root = repo.path();

    repo.commit_file("src/lib.rs", "fn a() {}\n", "feat: add a");
    seed_session(
        &storage,
        "sess_ancient",
        &root,
        &format!("{}/src/lib.rs", root.display()),
        Utc::now() - Duration::hours(72),
        Some("artifact_changed"),
    );

    let report = compute_suspect_commits(&storage, &query(&root)).expect("report");
    assert_eq!(report.attributed, 0);
    assert_eq!(report.unattributed, 1);
    assert!(report.suspect.is_empty());
    assert!(
        report.prompt.contains("not a clean bill of health"),
        "an unmatched range must not read as clean: {}",
        report.prompt
    );
}

/// The measured trap, at the level a user meets it: a session that worked in a different
/// repository and recorded a bare relative `Cargo.lock` must not attribute this repo's
/// `Cargo.lock` commit — and the dropped row must be counted.
#[test]
fn the_cargo_lock_collision_does_not_attribute_and_is_counted() {
    let repo = TempRepo::new();
    let storage = Storage::open_in_memory().expect("storage");
    let root = repo.path();

    repo.commit_file("Cargo.lock", "# lockfile\n", "chore: bump deps");

    let elsewhere = "/tmp/claude/projects/-Users-dev-code-someone-else/otherrepo.jsonl";
    let prov = Provenance::new(elsewhere, "opencode", 100, 100, "fp_other");
    let at = Utc::now() - Duration::minutes(2);
    let mut trace = AgentWorthTrace::new("sess_other_repo", "opencode", prov, at);
    trace.events.push(NormalizedEvent::new(
        1,
        at,
        EventPayload::FileAction {
            path: "Cargo.lock".to_string(),
            action: FileActionType::Edit,
            diff: None,
            lines_changed: None,
        },
    ));
    storage
        .upsert_session(&trace, Some("done_claimed"), Some(0.2))
        .expect("seed other-repo session");

    let report = compute_suspect_commits(&storage, &query(&root)).expect("report");
    assert_eq!(report.attributed, 0, "a bare Cargo.lock from elsewhere is not evidence here");
    assert!(report.suspect.is_empty());
    assert_eq!(
        report.unanchored_blame_rows, 1,
        "the dropped row is reported, not silently discarded"
    );
}

#[test]
fn a_ref_that_looks_like_a_flag_is_rejected() {
    let repo = TempRepo::new();
    let storage = Storage::open_in_memory().expect("storage");
    let root = repo.path();
    repo.commit_file("a.txt", "a\n", "init");

    let mut q = query(&root);
    q.base = Some("--output=/tmp/pwned".to_string());
    let err = compute_suspect_commits(&storage, &q).expect_err("must reject");
    assert!(err.to_string().contains("option"), "got: {err}");
}

#[test]
fn a_path_outside_a_git_repo_is_a_clear_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let storage = Storage::open_in_memory().expect("storage");
    let err = compute_suspect_commits(&storage, &query(dir.path())).expect_err("must fail");
    assert!(
        err.to_string().contains("git repository"),
        "the error should name the missing noun: {err}"
    );
}
