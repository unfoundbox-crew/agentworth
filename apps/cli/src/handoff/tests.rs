use super::*;
use agentworth_schema::{
    FileActionType, NormalizedEvent, Provenance, ShellCommand, TokenUsage, ToolCall,
};
use chrono::Duration;

/// The fixture every test in this file reads: one session that promised something and dropped
/// it, stated one decision, ran a test and a commit, and touched two files. It is deliberately
/// the shape a real working session has, so a change to any of the four sections shows up here
/// rather than in a test that only exercises one of them.
pub(super) fn fixture_trace() -> AgentWorthTrace {
    let prov = Provenance::new(
        "/Users/x/.claude/projects/-Users-x-code-unfoundbox-agentworth/fixture.jsonl",
        "claude_code",
        4096,
        1_756_000_000,
        "sha256:fixture",
    );
    let start = DateTime::parse_from_rfc3339("2026-09-01T14:02:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let mut trace = AgentWorthTrace::new("452c23fd-6e9b-4948-8e8f-6a31f1c3f7dd", "claude_code", prov, start);

    let mut push = |seq: u64, minutes: i64, payload: EventPayload| {
        trace
            .events
            .push(NormalizedEvent::new(seq, start + Duration::minutes(minutes), payload));
    };

    push(
        1,
        0,
        EventPayload::ModelInvocation {
            model: "claude-opus-4".to_string(),
            token_usage: TokenUsage::new(1000, 400, 200, 100),
            cost_usd: None,
            latency_ms: None,
        },
    );
    push(
        2,
        1,
        EventPayload::AssistantMessage {
            content: "We decided to keep the exit-code index out of SQLite for now.".to_string(),
            thinking: None,
        },
    );
    push(
        3,
        2,
        EventPayload::FileAction {
            path: "crates/storage/src/lib.rs".to_string(),
            action: FileActionType::Edit,
            diff: None,
            lines_changed: Some(40),
        },
    );
    push(
        4,
        3,
        EventPayload::FileAction {
            path: "crates/storage/src/lib.rs".to_string(),
            action: FileActionType::Edit,
            diff: None,
            lines_changed: Some(12),
        },
    );
    push(
        5,
        4,
        EventPayload::FileAction {
            path: "README.md".to_string(),
            action: FileActionType::Read,
            diff: None,
            lines_changed: None,
        },
    );
    push(
        6,
        5,
        EventPayload::ShellCommand(ShellCommand {
            command: "cargo test -p agentworth-storage".to_string(),
            cwd: Some("/repo".to_string()),
            exit_code: Some(0),
            output: Some("test result: ok. 12 passed; 0 failed".to_string()),
        }),
    );
    push(
        7,
        6,
        EventPayload::ShellCommand(ShellCommand {
            command: "git commit -m 'feat: repo-scoped lookup'".to_string(),
            cwd: Some("/repo".to_string()),
            exit_code: Some(0),
            output: Some("[main 9f3e1a2] feat: repo-scoped lookup\n 1 file changed".to_string()),
        }),
    );
    // No exit code recorded: dropped from "Ran" rather than listed as unknown.
    push(
        8,
        7,
        EventPayload::ShellCommand(ShellCommand {
            command: "ls crates".to_string(),
            cwd: None,
            exit_code: None,
            output: None,
        }),
    );
    push(
        9,
        8,
        EventPayload::AssistantMessage {
            content: "I'll re-run the export once the schema lands.".to_string(),
            thinking: None,
        },
    );
    push(
        10,
        9,
        EventPayload::AssistantMessage {
            content: "I'll delete the stale worktree before the next scan.".to_string(),
            thinking: None,
        },
    );
    push(
        11,
        10,
        EventPayload::UserMessage {
            content: "thanks, stop there".to_string(),
        },
    );

    trace.recalculate_stats();
    trace
}

pub(super) fn fixture_summary(prompt_preview: Option<&str>) -> SessionSummary {
    SessionSummary {
        session_id: "452c23fd-6e9b-4948-8e8f-6a31f1c3f7dd".to_string(),
        adapter: "claude_code".to_string(),
        source_path: "/Users/x/.claude/projects/-Users-x-code-unfoundbox-agentworth/fixture.jsonl"
            .to_string(),
        started_at: Utc::now(),
        duration_seconds: Some(600.0),
        total_tokens: 1700,
        total_events: 11,
        tool_calls_count: 0,
        models_used: vec!["claude-opus-4".to_string()],
        primary_outcome: Some("commit_observed".to_string()),
        composite_score: Some(0.8),
        prompt_preview: prompt_preview.map(str::to_string),
        source_mtime_epoch_secs: Some(1_756_000_000),
        compaction_count: 0,
        compaction_tokens_dropped: 0,
    }
}

pub(super) fn fixture_report(prompt_preview: Option<&str>) -> HandoffReport {
    let trace = fixture_trace();
    let outcomes = OutcomeDetector::new().detect_outcomes(&trace);
    build_handoff(
        &fixture_summary(prompt_preview),
        &trace,
        &outcomes,
        Some(
            DateTime::parse_from_rfc3339("2026-09-01T18:58:00Z")
                .unwrap()
                .with_timezone(&Utc),
        ),
        HandoffOptions::default(),
    )
}

#[test]
fn assembles_every_section_from_the_fixture() {
    let report = fixture_report(Some("port the loose-ends detector to Rust"));

    assert_eq!(report.task.as_deref(), Some("port the loose-ends detector to Rust"));
    assert_eq!(report.receipt.repo, "unfoundbox/agentworth");

    // "I'll re-run the export once the schema lands" carries `once`, so it is gated, not
    // dropped. The other one is a genuine loose end.
    assert_eq!(report.loose_ends.len(), 1, "{:?}", report.loose_ends);
    assert!(report.loose_ends[0].text.contains("delete the stale worktree"));

    assert_eq!(report.decided.len(), 1);
    assert!(report.decided[0].text.contains("out of SQLite"));

    // The read of README.md is not a change, so it is not in "files touched".
    assert_eq!(report.files_total, 1);
    assert_eq!(report.files[0].path, "crates/storage/src/lib.rs");
    assert_eq!(report.files[0].edits, 2, "two edits to the same file collapse to one row");

    // `ls crates` ran too and is listed -- naming what ran is most of the point -- but it is
    // not verification-shaped, so it sorts below the test and the commit and is the first
    // thing a tight line budget drops.
    assert_eq!(report.ran_total, 3);
    assert!(report.ran[0].verification && report.ran[1].verification);
    assert_eq!(report.ran[2].command, "ls crates");
    assert!(report.ran.iter().any(|c| c.command.starts_with("cargo test")));
    assert_eq!(report.ran[2].ending(), "exit not recorded");
    assert_eq!(report.ran[0].ending(), "exit 0");
}

/// Claude Code records the command a `Bash` tool call asked for and never its exit status, so
/// the only thing known about the ending is the harness's own error flag on the correlated
/// tool result. That is a weaker receipt than an exit code and has to read as one.
#[test]
fn a_command_with_no_exit_code_falls_back_to_the_tool_result_and_says_so() {
    use agentworth_schema::ToolResult;

    let prov = Provenance::new(
        "/Users/x/.claude/projects/-Users-x-code-unfoundbox-agentworth/bash.jsonl",
        "claude_code",
        10,
        1,
        "fp",
    );
    let start = Utc::now();
    let mut trace = AgentWorthTrace::new("bash-session", "claude_code", prov, start);
    trace.events.push(NormalizedEvent::new(
        1,
        start,
        EventPayload::ToolCall(ToolCall {
            id: Some("toolu_1".to_string()),
            name: "Bash".to_string(),
            arguments: serde_json::json!({"command": "cargo test --workspace"}),
        }),
    ));
    trace.events.push(NormalizedEvent::new(
        2,
        start,
        EventPayload::ShellCommand(ShellCommand {
            command: "cargo test --workspace".to_string(),
            cwd: None,
            exit_code: None,
            output: None,
        }),
    ));
    trace.events.push(NormalizedEvent::new(
        3,
        start,
        EventPayload::ToolResult(ToolResult {
            call_id: Some("toolu_1".to_string()),
            name: Some("Bash".to_string()),
            output: serde_json::json!("error: could not compile"),
            is_error: true,
        }),
    ));
    trace.recalculate_stats();

    let report = build_handoff(&fixture_summary(None), &trace, &[], None, HandoffOptions::default());
    assert_eq!(report.ran.len(), 1);
    assert_eq!(report.ran[0].exit_code, None);
    assert_eq!(report.ran[0].failed, Some(true));
    assert_eq!(
        report.ran[0].ending(),
        "reported an error, no exit code recorded",
        "an is_error flag is not an exit code and must not be printed as one"
    );
}

#[test]
fn outcome_reports_the_highest_rung_reached() {
    let report = fixture_report(None);
    let outcome = report.outcome.expect("a commit ran, so there is an outcome");
    assert_eq!(outcome.rung, 4);
    assert_eq!(outcome.kind, "commit_observed");
}

#[test]
fn an_unindexed_prompt_preview_is_a_gap_not_a_guess() {
    let report = fixture_report(None);
    assert!(report.task.is_none());
    assert!(report.gaps.iter().any(|g| g == gap::PROMPT_PREVIEW_EMPTY));

    let markdown = render_markdown(&report, DEFAULT_MAX_LINES);
    assert!(
        markdown.contains("**Task** _first prompt not indexed yet_"),
        "an empty prompt_preview must be stated, never filled in from somewhere else"
    );
}

#[test]
fn a_blank_prompt_preview_counts_as_empty() {
    let report = fixture_report(Some("   "));
    assert!(report.task.is_none());
    assert!(report.gaps.iter().any(|g| g == gap::PROMPT_PREVIEW_EMPTY));
}

#[test]
fn loose_ends_can_be_switched_off_and_the_gap_says_so() {
    let trace = fixture_trace();
    let outcomes = OutcomeDetector::new().detect_outcomes(&trace);
    let report = build_handoff(
        &fixture_summary(None),
        &trace,
        &outcomes,
        None,
        HandoffOptions {
            include_loose_ends: false,
        },
    );
    assert!(report.loose_ends.is_empty());
    assert!(report.gaps.iter().any(|g| g == gap::LOOSE_ENDS_NOT_REQUESTED));
}

#[test]
fn a_session_with_nothing_in_it_gets_a_receipt_and_no_body() {
    let prov = Provenance::new(
        "/Users/x/.claude/projects/-Users-x-code-unfoundbox-agentworth/empty.jsonl",
        "claude_code",
        10,
        1,
        "fp",
    );
    let mut trace = AgentWorthTrace::new("empty-session", "claude_code", prov, Utc::now());
    trace.events.push(NormalizedEvent::new(
        1,
        Utc::now(),
        EventPayload::ToolCall(ToolCall {
            id: None,
            name: "Read".to_string(),
            arguments: serde_json::json!({}),
        }),
    ));
    trace.recalculate_stats();

    let report = build_handoff(&fixture_summary(None), &trace, &[], None, HandoffOptions::default());
    assert!(report.body_is_empty());

    let markdown = render_markdown(&report, DEFAULT_MAX_LINES);
    assert!(markdown.contains("nothing to hand over but the receipt"));
    assert!(!markdown.contains("## Files touched"));
    assert!(markdown.contains("session empty-session"));
    assert!(markdown.contains("index last updated unknown"));
}

#[test]
fn markdown_carries_a_receipt_on_the_document_and_on_every_line() {
    let report = fixture_report(Some("port the loose-ends detector to Rust"));
    let markdown = render_markdown(&report, DEFAULT_MAX_LINES);

    assert!(markdown.starts_with("# Session 452c23fd · unfoundbox/agentworth · 2026-09-01 14:02"));
    assert!(markdown.contains("**Outcome** rung 4, commit_observed"));
    assert!(markdown.contains("session 452c23fd-6e9b-4948-8e8f-6a31f1c3f7dd · claude_code · generated"));
    assert!(markdown.contains("index last updated 2026-09-01T18:58Z"));
    assert!(markdown.contains("## Not in this handoff"));

    // Every quoted sentence names the sequence it came from; every command and file names a
    // time. Without those a line cannot be checked against the transcript.
    assert!(markdown.contains("[seq 10]"));
    assert!(markdown.contains("[seq 2]"));
    assert!(markdown.contains("`cargo test -p agentworth-storage` — exit 0, 14:07"));
    assert!(markdown.contains("crates/storage/src/lib.rs — 2 edits, last 14:05"));
}

#[test]
fn the_line_budget_is_a_ceiling_and_truncation_is_stated() {
    let mut report = fixture_report(None);
    // 60 file rows against a 20-line budget: most of them have to go.
    report.files = (0..60)
        .map(|i| FileTouch {
            path: format!("crates/thing/src/file_{i:02}.rs"),
            edits: 1,
            last_at: report.receipt.started_at,
            last_sequence: 100 + i,
        })
        .collect();
    report.files_total = 60;

    for budget in [14usize, 20, 40, DEFAULT_MAX_LINES] {
        let markdown = render_markdown(&report, budget);
        let lines = markdown.lines().count();
        assert!(
            lines <= budget,
            "budget {budget} exceeded: {lines} lines\n{markdown}"
        );
        assert!(markdown.contains("## Not in this handoff"), "the note is never truncated");
        assert!(markdown.contains("index last updated"), "the receipt is never truncated");
        // Nothing goes missing quietly: either a section is truncated and says so, or it is
        // dropped whole and gets named.
        assert!(
            markdown.contains("more, not shown") || markdown.contains("Dropped whole, for room"),
            "at budget {budget} something was cut without saying so:\n{markdown}"
        );
    }

    // At 40 lines there is room for the file list, truncated; at 20 there is not, and the
    // section has to be named as dropped rather than vanishing.
    assert!(render_markdown(&report, 40).contains("more, not shown"));
    let tight = render_markdown(&report, 20);
    assert!(tight.contains("Dropped whole, for room: Files touched (60)"), "{tight}");

    // Given room for everything, nothing is cut and nothing apologises for being cut.
    let roomy = render_markdown(&report, MAX_LINES_CEILING);
    assert!(!roomy.contains("more, not shown"), "{roomy}");
    assert!(!roomy.contains("Dropped whole"), "{roomy}");
    assert!(roomy.contains("crates/thing/src/file_59.rs"), "all 60 rows are present");
}

#[test]
fn a_budget_below_the_floor_still_gets_the_head_and_the_receipt() {
    let report = fixture_report(Some("do the thing"));
    let markdown = render_markdown(&report, 1);
    assert!(markdown.contains("**Outcome** rung 4, commit_observed"));
    assert!(markdown.contains("## Not in this handoff"));
    assert!(markdown.contains("index last updated"));
}

#[test]
fn the_budget_is_clamped_to_the_ceiling() {
    let report = fixture_report(None);
    let huge = render_markdown(&report, 10_000);
    assert!(huge.lines().count() <= MAX_LINES_CEILING);
}

#[test]
fn every_non_empty_section_gets_a_row_before_any_section_gets_a_second() {
    let mut report = fixture_report(Some("do the thing"));
    report.files = (0..40)
        .map(|i| FileTouch {
            path: format!("src/file_{i}.rs"),
            edits: 1,
            last_at: report.receipt.started_at,
            last_sequence: i,
        })
        .collect();
    report.files_total = 40;

    let markdown = render_markdown(&report, DEFAULT_MAX_LINES);
    for heading in [
        "## Said it would, no evidence it did",
        "## Ran",
        "## Files touched",
        "## Said it decided",
    ] {
        assert!(
            markdown.contains(heading),
            "a long file list must not starve {heading}:\n{markdown}"
        );
    }
    assert!(
        markdown.contains("more, not shown"),
        "the file list is the section that pays for the others, and it says so"
    );
}

#[test]
fn redaction_masks_paths_commands_and_quoted_text_through_one_instance() {
    let trace = fixture_trace();
    let mut report = fixture_report(Some("work in /Users/x/code/unfoundbox/agentworth"));
    report.ran.push(RanCommand {
        command: "curl -H 'Authorization: Bearer sk-ant-abcdefghijklmnopqrstuvwxyz012345'".to_string(),
        exit_code: Some(0),
        failed: Some(false),
        at: report.receipt.started_at,
        sequence: 99,
        verification: false,
    });
    report.ran_total += 1;

    let redactor = Redactor::new().for_trace(&trace);
    let redacted = report.redacted(&redactor);

    assert!(redacted.receipt.redacted);
    let markdown = render_markdown(&redacted, MAX_LINES_CEILING);
    assert!(
        !markdown.contains("sk-ant-abcdefghijklmnopqrstuvwxyz012345"),
        "an API key in a command must not survive redaction:\n{markdown}"
    );
    assert!(
        !markdown.contains("agentworth/fixture.jsonl"),
        "the session's own source path must be masked"
    );
    assert!(markdown.contains("· redacted"), "the receipt says the copy is redacted");

    // The unredacted report is untouched -- `redacted` returns a copy, same guarantee
    // `Redactor::redact_trace` already gives.
    assert!(report.ran.iter().any(|c| c.command.contains("sk-ant-")));
}
