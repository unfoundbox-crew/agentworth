use super::*;
use crate::handoff::tests::{fixture_summary, fixture_trace};
use agentworth_schema::{FileActionType, Provenance, ShellCommand};
use chrono::Duration;

fn checkout() -> CheckoutProbe {
    CheckoutProbe::Found(Checkout {
        root: "/Users/x/code/unfoundbox/agentworth".to_string(),
        is_worktree: false,
        branch: Some("claude/session-wake".to_string()),
        head_short: Some("a25b9cd".to_string()),
        head_subject: Some("site: the landing page sells the product".to_string()),
        dirty_files: Some(3),
        ahead: Some(2),
        behind: Some(0),
        upstream: Some("origin/main".to_string()),
    })
}

fn scanned_at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-01T18:58:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn report_from(trace: &AgentWorthTrace, prompt_preview: Option<&str>) -> WakeReport {
    let outcomes = OutcomeDetector::new().detect_outcomes(trace);
    build_wake(
        &fixture_summary(prompt_preview),
        trace,
        &outcomes,
        &[],
        checkout(),
        "/Users/x/code/unfoundbox/agentworth",
        Some(scanned_at()),
        Some(false),
    )
}

fn fixture_report() -> WakeReport {
    report_from(
        &fixture_trace(),
        Some("port the loose-ends detector to Rust"),
    )
}

/// A trace with nothing in it but the events a test pushes, sharing the fixture's source path so
/// the repository identity rule fires the same way.
fn empty_trace(session_id: &str) -> AgentWorthTrace {
    let prov = Provenance::new(
        "/Users/x/.claude/projects/-Users-x-code-unfoundbox-agentworth/fixture.jsonl",
        "claude_code",
        4096,
        1_756_000_000,
        "sha256:fixture",
    );
    let start = DateTime::parse_from_rfc3339("2026-09-04T05:32:00Z")
        .unwrap()
        .with_timezone(&Utc);
    AgentWorthTrace::new(session_id, "claude_code", prov, start)
}

fn push(trace: &mut AgentWorthTrace, seq: u64, payload: EventPayload) {
    let at = trace.started_at + Duration::minutes(seq as i64);
    trace.events.push(NormalizedEvent::new(seq, at, payload));
}

fn shell(command: &str, exit_code: Option<i32>) -> EventPayload {
    EventPayload::ShellCommand(ShellCommand {
        command: command.to_string(),
        cwd: None,
        exit_code,
        output: None,
    })
}

fn lines(markdown: &str) -> usize {
    markdown.lines().count()
}

#[test]
fn the_whole_document_fits_the_budget_and_answers_the_four_questions() {
    let report = fixture_report();
    let markdown = render_markdown(&report);

    assert!(
        lines(&markdown) <= MAX_LINES,
        "{} lines:\n{markdown}",
        lines(&markdown)
    );
    assert!(markdown.starts_with("# Wake · unfoundbox/agentworth · "));
    assert!(markdown.contains(
        "Checkout /Users/x/code/unfoundbox/agentworth · branch claude/session-wake · \
         HEAD a25b9cd \"site: the landing page sells the product\" · 3 files dirty · 2 ahead of \
         origin/main"
    ));
    assert!(markdown.contains("source unchanged since scan"));
    assert!(markdown.contains("**Task** port the loose-ends detector to Rust"));
    assert!(markdown.contains("**Last asked** thanks, stop there"));
    assert!(markdown.contains("**Outcome** rung 4, commit_observed"));
    assert!(markdown.contains("**Proof** last passed `cargo test -p agentworth-storage`"));
    assert!(markdown.contains("**Changed** 1 file · lib.rs (2)"));
    assert!(markdown.contains("**Loose ends** (1)"));
    assert!(markdown.contains("delete the stale worktree"));
    assert!(markdown.contains("**Said it decided** \"We decided to keep"));
    assert!(markdown.contains("## Next"));
    assert!(markdown.contains("Blocker none recorded."));
    assert!(markdown.contains("`gh pr list` for the first."));
}

#[test]
fn a_failure_that_was_re_run_and_passed_is_not_a_blocker() {
    let mut trace = empty_trace("rerun-session");
    push(
        &mut trace,
        1,
        shell("cargo test -p agentworth-cli", Some(101)),
    );
    push(&mut trace, 2, shell("ls crates", None));
    push(
        &mut trace,
        3,
        shell("cargo test -p agentworth-cli", Some(0)),
    );
    trace.recalculate_stats();

    let report = report_from(&trace, Some("fix the flaky test"));
    let proof = &report.session.as_ref().expect("a session").proof;
    assert_eq!(proof.failed_was_rerun, Some(true));
    assert_eq!(
        proof.verification_total, 2,
        "`ls` is not verification-shaped"
    );
    assert_eq!(proof.ran_total, 3);
    assert!(report.next.blocker.is_none());
    assert!(render_markdown(&report).contains("re-run and passed"));
}

#[test]
fn a_failure_nothing_re_ran_is_the_blocker() {
    let mut trace = empty_trace("blocked-session");
    push(
        &mut trace,
        1,
        shell("cargo test -p agentworth-cli", Some(0)),
    );
    push(&mut trace, 2, shell("cargo clippy --workspace", Some(101)));
    trace.recalculate_stats();

    let report = report_from(&trace, Some("fix the lints"));
    let proof = &report.session.as_ref().expect("a session").proof;
    assert_eq!(proof.failed_was_rerun, Some(false));
    assert_eq!(
        proof.last_failed.as_ref().map(|c| c.command.as_str()),
        Some("cargo clippy --workspace")
    );
    assert_eq!(
        report.next.blocker.as_ref().map(|c| c.command.as_str()),
        Some("cargo clippy --workspace")
    );

    let markdown = render_markdown(&report);
    assert!(markdown.contains("last failed `cargo clippy --workspace`"));
    assert!(markdown.contains("Blocker `cargo clippy --workspace` failed at"));
}

/// A run whose ending nothing recorded is neither a pass nor a failure. Rounding it up to a pass
/// is the one thing this section must never do.
#[test]
fn a_command_with_no_recorded_ending_is_neither_passed_nor_failed() {
    let mut trace = empty_trace("unknown-session");
    push(&mut trace, 1, shell("cargo test --workspace", None));
    trace.recalculate_stats();

    let proof = report_from(&trace, None).session.expect("a session").proof;
    assert!(proof.last_passed.is_none());
    assert!(proof.last_failed.is_none());
    assert_eq!(proof.failed_was_rerun, None);
    assert_eq!(proof.verification_total, 1);
}

#[test]
fn a_compaction_summary_is_not_what_the_user_last_asked() {
    let mut trace = empty_trace("compacted-session");
    push(
        &mut trace,
        1,
        EventPayload::UserMessage {
            content: "Rebuild the landing page terminals at 2x and push".to_string(),
        },
    );
    push(
        &mut trace,
        2,
        EventPayload::UserMessage {
            content: "This session is being continued from a previous conversation that ran out \
                      of context.\n\nSummary: we rebuilt the terminals."
                .to_string(),
        },
    );
    trace.recalculate_stats();

    let session = report_from(&trace, None).session.expect("a session");
    assert_eq!(
        session.last_asked.as_deref(),
        Some("Rebuild the landing page terminals at 2x and push")
    );
}

#[test]
fn no_user_message_at_all_is_a_gap_and_the_line_is_omitted() {
    let mut trace = empty_trace("silent-session");
    push(&mut trace, 1, shell("ls", None));
    trace.recalculate_stats();

    let report = report_from(&trace, None);
    assert!(report
        .session
        .as_ref()
        .expect("a session")
        .last_asked
        .is_none());
    assert!(report.gaps.iter().any(|g| g == gap::NO_USER_MESSAGE));
    assert!(!render_markdown(&report).contains("**Last asked**"));
}

#[test]
fn a_repo_with_no_indexed_session_still_gets_the_checkout_block() {
    let report = build_wake_without_session(
        "unfoundbox/agentworth",
        checkout(),
        "/Users/x/code/unfoundbox/agentworth",
        Some(scanned_at()),
    );
    assert!(report.session.is_none());
    assert!(report.gaps.iter().any(|g| g == gap::NO_SESSION_FOR_REPO));

    let markdown = render_markdown(&report);
    assert!(lines(&markdown) <= MAX_LINES, "{markdown}");
    assert!(markdown.contains("Checkout /Users/x/code/unfoundbox/agentworth · branch"));
    assert!(markdown.contains("No session for this repo in the index"));
    assert!(markdown.contains("archie scan"));
    assert!(
        !markdown.contains("## Next"),
        "nothing to say, so nothing is said"
    );
}

#[test]
fn a_checkout_that_could_not_be_read_says_which_way_it_failed() {
    for (probe, expected, expected_gap) in [
        (
            CheckoutProbe::NotACheckout,
            "Checkout: not a git checkout",
            gap::NOT_A_GIT_CHECKOUT,
        ),
        (
            CheckoutProbe::GitUnavailable,
            "Checkout: git unavailable",
            gap::GIT_UNAVAILABLE,
        ),
        (
            CheckoutProbe::Unreadable,
            "Checkout: git did not answer in time",
            gap::GIT_TIMED_OUT,
        ),
    ] {
        let report = build_wake_without_session("unfoundbox/agentworth", probe, "/tmp/x", None);
        assert!(report.checkout.is_none());
        assert_ne!(report.checkout_state, checkout_state::FOUND);
        assert!(report.gaps.iter().any(|g| g == expected_gap));
        assert!(render_markdown(&report).contains(expected));
    }
}

#[test]
fn fifty_loose_ends_forty_files_and_two_hundred_commands_still_fit_in_thirty_lines() {
    let mut trace = empty_trace("stress-session");
    let mut seq = 0u64;
    for i in 0..50 {
        seq += 1;
        push(
            &mut trace,
            seq,
            EventPayload::AssistantMessage {
                content: format!("I'll delete the stale worktree number {i} before the next scan."),
                thinking: None,
            },
        );
    }
    for i in 0..40 {
        seq += 1;
        push(
            &mut trace,
            seq,
            EventPayload::FileAction {
                path: format!("crates/very/long/path/to/src/module_{i:03}.rs"),
                action: FileActionType::Edit,
                diff: None,
                lines_changed: Some(3),
            },
        );
    }
    for i in 0..200 {
        seq += 1;
        push(
            &mut trace,
            seq,
            shell(&format!("cargo test -p crate_{i:03}"), Some(0)),
        );
    }
    trace.recalculate_stats();

    let mut report = report_from(&trace, Some("a very long day"));
    report.before = (0..20)
        .map(|i| PriorSession {
            session_id: format!("older-{i}"),
            started_at: trace.started_at,
            outcome_rung: Some(3),
            outcome_kind: Some("test_or_build_passed".to_string()),
            task: Some(format!(
                "session number {i}, which also had a long prompt behind it"
            )),
        })
        .collect();
    let session = report.session.as_ref().expect("a session");
    assert_eq!(session.files.len(), 3, "the section carries three rows");
    assert_eq!(session.files_total, 40, "and states the true total");
    assert!(session.loose_ends.len() <= 3);
    assert_eq!(session.proof.ran_total, 200);

    let markdown = render_markdown(&report);
    assert!(
        lines(&markdown) <= MAX_LINES,
        "{} lines:\n{markdown}",
        lines(&markdown)
    );
}

#[test]
fn ran_in_is_present_only_when_the_adapter_recorded_a_workspace() {
    let mut trace = fixture_trace();
    trace.metadata = serde_json::json!({
        "workspace": {
            "cwd": "/Users/x/code/unfoundbox/agentworth/.claude/worktrees/plugin",
            "git_branch": "claude/landing-product-page"
        }
    });
    let report = report_from(&trace, None);
    let ran_in = report
        .session
        .as_ref()
        .expect("a session")
        .ran_in
        .as_ref()
        .expect("workspace metadata is present");
    assert_eq!(
        ran_in.cwd.as_deref(),
        Some("/Users/x/code/unfoundbox/agentworth/.claude/worktrees/plugin")
    );
    assert_eq!(
        ran_in.git_branch.as_deref(),
        Some("claude/landing-product-page")
    );
    let markdown = render_markdown(&report);
    assert!(
        markdown.contains(".claude/worktrees/plugin on claude/landing-product-page"),
        "{markdown}"
    );

    let bare = report_from(&fixture_trace(), None);
    assert!(
        bare.session.expect("a session").ran_in.is_none(),
        "an adapter that records no workspace gets no line, not a borrowed one"
    );
}

#[test]
fn redaction_masks_the_repository_identity_in_every_field() {
    let trace = fixture_trace();
    let mut report = fixture_report();
    let repo = "unfoundbox/agentworth";

    {
        let session = report.session.as_mut().expect("a session");
        session.task = Some(format!("work on {repo}"));
        session.last_asked = Some(format!("push {repo} please"));
        session.ran_in = Some(RanIn {
            cwd: Some(format!("/Users/x/code/{repo}")),
            git_branch: Some(format!("{repo}-branch")),
        });
        if let Some(outcome) = session.outcome.as_mut() {
            outcome.summary = format!("commit in {repo}");
        }
        session.proof.last_passed = Some(RanCommand {
            command: format!("cargo test -p {repo}"),
            exit_code: Some(0),
            failed: Some(false),
            at: trace.started_at,
            sequence: 1,
            verification: true,
        });
        session.files[0].path = format!("/Users/x/code/{repo}/src/lib.rs");
        session.loose_ends[0].text = format!("I'll re-scan {repo} later.");
        if let Some(decided) = session.decided.as_mut() {
            decided.text = format!("We decided to keep {repo} as one crate.");
        }
    }
    report.before = vec![PriorSession {
        session_id: "older".to_string(),
        started_at: trace.started_at,
        outcome_rung: Some(3),
        outcome_kind: Some("test_or_build_passed".to_string()),
        task: Some(format!("scan {repo}")),
    }];
    report.next = Next {
        blocker: Some(RanCommand {
            command: format!("cargo build -p {repo}"),
            exit_code: Some(1),
            failed: Some(true),
            at: trace.started_at,
            sequence: 2,
            verification: true,
        }),
        step: Some(report.session.as_ref().unwrap().loose_ends[0].clone()),
    };

    let redacted = report.redacted(&Redactor::new().for_trace(&trace));
    assert!(redacted.receipt.redacted);

    let json = serde_json::to_string(&redacted).expect("serialises");
    assert!(
        !json.contains(repo),
        "the repository identity survived redaction somewhere in the report: {json}"
    );
    let markdown = render_markdown(&redacted);
    assert!(!markdown.contains(repo), "{markdown}");
    assert!(markdown.contains("· redacted"));

    // A copy, not a mutation -- the same guarantee `HandoffReport::redacted` gives.
    assert!(report.receipt.repo.contains(repo));
}

#[test]
fn every_gap_name_is_unique_and_snake_case() {
    let mut seen = std::collections::HashSet::new();
    for name in gap::ALL {
        assert!(seen.insert(*name), "duplicate gap name: {name}");
        assert!(
            name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "gap names are snake_case: {name}"
        );
        assert!(!name.starts_with('_') && !name.ends_with('_'), "{name}");
    }
    assert_eq!(seen.len(), gap::ALL.len());
}

/// The rendered fixture, printed so the PR can quote it and count its tokens.
#[test]
fn print_the_fixture_document() {
    let markdown = render_markdown(&fixture_report());
    println!("----- BEGIN WAKE MARKDOWN -----");
    println!("{markdown}");
    println!("----- END WAKE MARKDOWN -----");
    println!("lines: {}", lines(&markdown));
    println!("chars: {}", markdown.chars().count());
}

#[test]
fn a_path_under_the_home_directory_is_shortened_to_a_tilde() {
    let Ok(home) = std::env::var("HOME") else {
        eprintln!("skipped: no HOME on this host");
        return;
    };
    let mut probe = checkout();
    if let CheckoutProbe::Found(c) = &mut probe {
        c.root = format!("{home}/code/unfoundbox/agentworth");
    }
    let report = build_wake_without_session("unfoundbox/agentworth", probe, &home, None);
    assert!(
        render_markdown(&report).contains("Checkout ~/code/unfoundbox/agentworth"),
        "the home directory is shortened, never lengthened"
    );
}

/// A bounded scan that ran out of budget found no session *yet*. Reporting that as "no session
/// for this repo" is a partial answer wearing a complete one's clothes.
#[test]
fn a_scan_that_ran_out_of_budget_says_so_rather_than_claiming_no_session() {
    let mut report = build_wake_without_session(
        "unfoundbox/agentworth",
        checkout(),
        "/Users/x/code/unfoundbox/agentworth",
        Some(scanned_at()),
    );
    assert!(!report.scan_exhausted);
    report.mark_scan_exhausted(true);

    assert!(report.scan_exhausted);
    assert!(report.gaps.iter().any(|g| g == gap::SCAN_BUDGET_EXHAUSTED));
    report.mark_scan_exhausted(true);
    assert_eq!(
        report
            .gaps
            .iter()
            .filter(|g| *g == gap::SCAN_BUDGET_EXHAUSTED)
            .count(),
        1,
        "the gap is named once, however often it is marked"
    );

    let markdown = render_markdown(&report);
    assert!(lines(&markdown) <= MAX_LINES, "{markdown}");
    assert!(
        markdown.contains("The newest 5,000 sessions held none for this repo"),
        "{markdown}"
    );

    // Unexhausted, the sentence is absent rather than softened.
    report.mark_scan_exhausted(false);
    assert!(!render_markdown(&report).contains("older ones may exist"));
}

/// The early return for a repo with no sessions has no trace to build a repository rule from,
/// which is exactly why it was the path that forgot to redact at all.
#[test]
fn the_no_session_document_is_redacted_unless_raw_was_asked_for() {
    use std::sync::Arc;

    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/x".to_string());
    let workspace = std::path::PathBuf::from(format!("{home}/code/unfoundbox/agentworth"));
    let storage = Arc::new(agentworth_storage::Storage::open_in_memory().expect("open storage"));
    let scanner = Scanner::new(Arc::clone(&storage));

    let redacted = load_wake(
        &storage,
        &scanner,
        "unfoundbox/agentworth",
        &workspace,
        WakeOptions { include_raw: false },
    )
    .expect("wake with no session");
    assert!(redacted.session.is_none());
    assert!(redacted.receipt.redacted);
    assert!(
        !redacted.workspace.contains(&home),
        "the home directory must not survive redaction: {}",
        redacted.workspace
    );

    let raw = load_wake(
        &storage,
        &scanner,
        "unfoundbox/agentworth",
        &workspace,
        WakeOptions { include_raw: true },
    )
    .expect("wake with no session");
    assert!(!raw.receipt.redacted);
    assert!(
        raw.workspace.contains(&home),
        "include_raw is the opt-in, and it opts in"
    );
}
