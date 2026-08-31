use assert_cmd::Command;
use predicates::prelude::*;
use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;

fn setup_sample_claude_session(dir: &std::path::Path) -> (std::path::PathBuf, String) {
    let claude_dir = dir.join(".claude").join("projects").join("my-project");
    fs::create_dir_all(&claude_dir).unwrap();

    let session_file = claude_dir.join("sample_session_123.jsonl");
    let mut file = File::create(&session_file).unwrap();

    writeln!(
        file,
        r#"{{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"Fix the database bug and export OPENAI_API_KEY=sk-testsecretkey12345678901234567890"}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"assistant","timestamp":"2026-08-29T10:00:02Z","model":"claude-3-5-sonnet-20241022","usage":{{"input_tokens":500,"output_tokens":120,"cache_read_input_tokens":50,"cache_creation_input_tokens":10}},"content":[{{"type":"thinking","thinking":"I should check src/db.rs and run the test suite."}},{{"type":"text","text":"I will inspect the database module and run tests."}},{{"type":"tool_use","id":"t1","name":"FileEdit","input":{{"file_path":"src/db.rs","diff":"+ fn fix_connection() {{}}"}}}}]}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"tool_result","timestamp":"2026-08-29T10:00:04Z","tool_use_id":"t1","content":"File modified successfully","is_error":false}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"assistant","timestamp":"2026-08-29T10:00:05Z","content":[{{"type":"tool_use","id":"t2","name":"Bash","input":{{"command":"cargo test"}}}}],"usage":{{"input_tokens":200,"output_tokens":30,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"tool_result","timestamp":"2026-08-29T10:00:07Z","tool_use_id":"t2","content":"test result: ok. 8 passed; 0 failed","is_error":false}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"assistant","timestamp":"2026-08-29T10:00:08Z","content":[{{"type":"text","text":"All 8 tests are now passing!"}}]}}"#
    )
    .unwrap();

    let session_id = "sample_session_123".to_string();
    (session_file, session_id)
}

#[test]
fn test_cli_scan_and_stats_commands() {
    let temp = tempdir().unwrap();
    let (_session_file, _session_id) = setup_sample_claude_session(temp.path());

    let db_path = temp.path().join("test_agentworth.db");

    // 1. Scan the directory
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    cmd.arg("--db-path")
        .arg(&db_path)
        .arg("scan")
        .arg(temp.path())
        .arg("--json");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("\"scanned_sessions\": 1"))
        .stdout(predicate::str::contains("\"total_indexed_sessions\": 1"));

    // 2. Run stats (formatted text)
    let mut stats_cmd = Command::cargo_bin("agentworth").unwrap();
    stats_cmd.arg("--db-path").arg(&db_path).arg("stats");

    stats_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("Total Sessions:"))
        .stdout(predicate::str::contains("Verdict Breakdown:"))
        .stdout(predicate::str::contains("Test or Build Passed (Rung 3)"))
        .stdout(predicate::str::contains("Real Verified Tasks:"))
        .stdout(predicate::str::contains("claude_code"))
        .stdout(predicate::str::contains("claude-3-5-sonnet-20241022"))
        .stdout(predicate::str::contains("FileEdit"))
        .stdout(predicate::str::contains("Bash"));

    // 3. Run stats (--json)
    let mut stats_json_cmd = Command::cargo_bin("agentworth").unwrap();
    stats_json_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("stats")
        .arg("--json");

    stats_json_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("\"total_sessions\": 1"))
        .stdout(predicate::str::contains("\"verdict_breakdown\":"))
        .stdout(predicate::str::contains("\"test_or_build_passed\": 1"))
        .stdout(predicate::str::contains("\"real_verified_tasks\": 1"))
        .stdout(predicate::str::contains("\"claude_code\": 1"));
}

#[test]
fn test_cli_matrix_command_table_and_json() {
    // 1. agwt matrix (table view)
    let mut matrix_cmd = Command::cargo_bin("agwt").unwrap();
    matrix_cmd.arg("matrix");

    matrix_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("AgentWorth Adapter Extraction Coverage Matrix (20 Production Adapters)"))
        .stdout(predicate::str::contains("claude_code"))
        .stdout(predicate::str::contains("codex"))
        .stdout(predicate::str::contains("gemini"))
        .stdout(predicate::str::contains("opencode"))
        .stdout(predicate::str::contains("cursor"))
        .stdout(predicate::str::contains("windsurf"))
        .stdout(predicate::str::contains("deepseek"))
        .stdout(predicate::str::contains("qwen"))
        .stdout(predicate::str::contains("zhipu"))
        .stdout(predicate::str::contains("manus"))
        .stdout(predicate::str::contains("100% extraction parity"));

    // 2. agwt matrix (--json)
    let mut matrix_json_cmd = Command::cargo_bin("agwt").unwrap();
    matrix_json_cmd.arg("matrix").arg("--json");

    matrix_json_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("\"total_adapters\": 20"))
        .stdout(predicate::str::contains("\"coverage_rate\": \"100%\""))
        .stdout(predicate::str::contains("\"adapter\": \"claude_code\""))
        .stdout(predicate::str::contains("\"prompts\": true"))
        .stdout(predicate::str::contains("\"tokens\": true"))
        .stdout(predicate::str::contains("\"tools\": true"))
        .stdout(predicate::str::contains("\"shell\": true"))
        .stdout(predicate::str::contains("\"diffs\": true"))
        .stdout(predicate::str::contains("\"outcomes\": true"));
}

#[test]
fn test_cli_traces_list_and_filters() {
    let temp = tempdir().unwrap();
    let (_session_file, session_id) = setup_sample_claude_session(temp.path());
    let db_path = temp.path().join("test_agentworth.db");

    // Initial scan
    let mut scan_cmd = Command::cargo_bin("agentworth").unwrap();
    scan_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("scan")
        .arg(temp.path())
        .assert()
        .success();

    // 1. List traces in table view (with verdict badges and score)
    let mut traces_cmd = Command::cargo_bin("agentworth").unwrap();
    traces_cmd.arg("--db-path").arg(&db_path).arg("traces");

    traces_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("VERDICT"))
        .stdout(predicate::str::contains("SCORE"))
        .stdout(predicate::str::contains("[TEST_PASSED]"))
        .stdout(predicate::str::contains("SESSION ID"))
        .stdout(predicate::str::contains(&session_id))
        .stdout(predicate::str::contains("claude_code"));

    // 2. List traces with --json
    let mut traces_json_cmd = Command::cargo_bin("agentworth").unwrap();
    traces_json_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("traces")
        .arg("--json");

    traces_json_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains(&session_id))
        .stdout(predicate::str::contains("\"verdict_badge\": \"[TEST_PASSED]\""))
        .stdout(predicate::str::contains("\"adapter\": \"claude_code\""));

    // 3. List traces with matching filter
    let mut filtered_cmd = Command::cargo_bin("agentworth").unwrap();
    filtered_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("traces")
        .arg("--adapter")
        .arg("claude_code")
        .arg("--model")
        .arg("sonnet");

    filtered_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains(&session_id));

    // 4. List traces with non-matching filter
    let mut non_matching_cmd = Command::cargo_bin("agentworth").unwrap();
    non_matching_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("traces")
        .arg("--adapter")
        .arg("codex");

    non_matching_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("No sessions found in index"));
}

#[test]
fn test_cli_inspect_command() {
    let temp = tempdir().unwrap();
    let (_session_file, session_id) = setup_sample_claude_session(temp.path());
    let db_path = temp.path().join("test_agentworth.db");

    // Scan
    Command::cargo_bin("agentworth")
        .unwrap()
        .arg("--db-path")
        .arg(&db_path)
        .arg("scan")
        .arg(temp.path())
        .assert()
        .success();

    // 1. Inspect timeline view with grouped highest outcome
    let mut inspect_cmd = Command::cargo_bin("agentworth").unwrap();
    inspect_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("inspect")
        .arg(&session_id);

    inspect_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("AgentWorth Session Trace:"))
        .stdout(predicate::str::contains(&session_id))
        .stdout(predicate::str::contains("Highest Outcome Reached:"))
        .stdout(predicate::str::contains("Rung 3 - Test or Build Passed"))
        .stdout(predicate::str::contains("Supporting Evidence:"))
        .stdout(predicate::str::contains("USER PROMPT"))
        .stdout(predicate::str::contains("ASSISTANT THINKING"))
        .stdout(predicate::str::contains("TOOL CALL: FileEdit"))
        .stdout(predicate::str::contains("TOOL CALL: Bash"));

    // 2. Inspect with --json
    let mut inspect_json_cmd = Command::cargo_bin("agentworth").unwrap();
    inspect_json_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("inspect")
        .arg(&session_id)
        .arg("--json");

    inspect_json_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"session_id\": \"sample_session_123\"",
        ))
        .stdout(predicate::str::contains("\"events\": ["));
}

#[test]
fn test_cli_export_command_json_and_atif_and_redaction() {
    let temp = tempdir().unwrap();
    let (_session_file, session_id) = setup_sample_claude_session(temp.path());
    let db_path = temp.path().join("test_agentworth.db");

    // Scan
    Command::cargo_bin("agentworth")
        .unwrap()
        .arg("--db-path")
        .arg(&db_path)
        .arg("scan")
        .arg(temp.path())
        .assert()
        .success();

    // 1. Export raw JSON to stdout without redaction (should contain secret)
    let mut export_raw_cmd = Command::cargo_bin("agentworth").unwrap();
    export_raw_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("export")
        .arg(&session_id)
        .arg("--format")
        .arg("json");

    export_raw_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "sk-testsecretkey12345678901234567890",
        ));

    // 2. Export with --redact (should mask the secret API key)
    let mut export_redacted_cmd = Command::cargo_bin("agentworth").unwrap();
    export_redacted_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("export")
        .arg(&session_id)
        .arg("--redact")
        .arg("--format")
        .arg("json");

    export_redacted_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("sk-testsecretkey12345678901234567890").not())
        .stdout(predicate::str::contains("[REDACTED_API_KEY]"));

    // 3. Export ATIF format to a file
    let atif_out_file = temp.path().join("exports").join("trace.atif.json");
    let mut export_atif_cmd = Command::cargo_bin("agentworth").unwrap();
    export_atif_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("export")
        .arg(&session_id)
        .arg("--redact")
        .arg("--format")
        .arg("atif")
        .arg("--output")
        .arg(&atif_out_file);

    export_atif_cmd.assert().success();

    assert!(atif_out_file.exists());
    let atif_content = fs::read_to_string(&atif_out_file).unwrap();
    assert!(atif_content.contains("atif-v1.0"));
    assert!(atif_content.contains("sample_session_123"));
    assert!(!atif_content.contains("sk-testsecretkey12345678901234567890"));
}

#[test]
fn test_cli_serve_command_help_and_flags() {
    let mut cmd = Command::cargo_bin("agentworth").unwrap();
    cmd.arg("serve").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("--port"))
        .stdout(predicate::str::contains("--open"))
        .stdout(predicate::str::contains("--dist"));
}

#[test]
fn test_cli_search_command_ascii_and_json() {
    let temp = tempdir().unwrap();
    let (_session_file, session_id) = setup_sample_claude_session(temp.path());
    let db_path = temp.path().join("test_agentworth.db");

    // Scan
    Command::cargo_bin("agentworth")
        .unwrap()
        .arg("--db-path")
        .arg(&db_path)
        .arg("scan")
        .arg(temp.path())
        .assert()
        .success();

    // 1. Search (ASCII Thermal Receipt card view)
    let mut search_cmd = Command::cargo_bin("agwt").unwrap();
    search_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("search")
        .arg("database bug fix");

    search_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("Semantic Latent Vector Search"))
        .stdout(predicate::str::contains("MATCH:"))
        .stdout(predicate::str::contains(&session_id))
        .stdout(predicate::str::contains("claude_code"));

    // 2. Search with --json
    let mut search_json_cmd = Command::cargo_bin("agwt").unwrap();
    search_json_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("search")
        .arg("database bug fix")
        .arg("--json");

    search_json_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("\"chunk_id\":"))
        .stdout(predicate::str::contains("\"session_id\": \"sample_session_123\""))
        .stdout(predicate::str::contains("\"score\":"));
}

#[test]
fn test_cli_audit_command_safety_detection() {
    let temp = tempdir().unwrap();
    let claude_dir = temp.path().join(".claude").join("projects").join("katana");
    fs::create_dir_all(&claude_dir).unwrap();

    let session_file = claude_dir.join("katana_catastrophe.jsonl");
    let mut file = File::create(&session_file).unwrap();

    // Dangerous session with rm -rf $d and credential leak
    writeln!(
        file,
        r#"{{"type":"user","timestamp":"2026-08-31T10:00:00Z","content":"Clean worktrees and use GITHUB_TOKEN=ghp_123456789012345678901234567890123456"}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"assistant","timestamp":"2026-08-31T10:00:02Z","model":"claude-opus-5","content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{"command":"rm -rf $d"}}}}]}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"tool_result","timestamp":"2026-08-31T10:00:04Z","tool_use_id":"t1","content":"Deleted directory","is_error":false}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"assistant","timestamp":"2026-08-31T10:00:05Z","content":[{{"type":"text","text":"STOP. That was my mistake — I accidentally deleted the katana directory. A missing local turned my safety mechanism into a weapon."}}]}}"#
    )
    .unwrap();

    let db_path = temp.path().join("test_agentworth.db");

    // Scan
    Command::cargo_bin("agentworth")
        .unwrap()
        .arg("--db-path")
        .arg(&db_path)
        .arg("scan")
        .arg(temp.path())
        .assert()
        .success();

    // 1. Run audit (formatted text view)
    let mut audit_cmd = Command::cargo_bin("agwt").unwrap();
    audit_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("audit")
        .arg("--safety");

    audit_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("Agent Safety & Forensic Threat Audit"))
        .stdout(predicate::str::contains("CRITICAL"))
        .stdout(predicate::str::contains("LEAKED_SHELL_VARIABLE"))
        .stdout(predicate::str::contains("rm -rf $d"))
        .stdout(predicate::str::contains("CREDENTIAL_LEAK"));

    // 2. Run audit with --json
    let mut audit_json_cmd = Command::cargo_bin("agwt").unwrap();
    audit_json_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("audit")
        .arg("--json");

    audit_json_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("\"critical_count\": 1"))
        .stdout(predicate::str::contains("\"rule_id\": \"LEAKED_SHELL_VARIABLE\""));
}

#[test]
fn test_cli_blunder_command() {
    let temp = tempdir().unwrap();
    let katana_dir = temp.path().join(".claude").join("projects").join("katana");
    fs::create_dir_all(&katana_dir).unwrap();

    let session_file = katana_dir.join("katana_blunder_session.jsonl");
    let mut file = File::create(&session_file).unwrap();

    writeln!(
        file,
        r#"{{"type":"user","timestamp":"2026-08-31T10:00:00Z","content":"Clean worktrees and delete old directories"}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"assistant","timestamp":"2026-08-31T10:00:02Z","model":"Claude Opus 5 (Extended Thinking)","usage":{{"input_tokens":1200000,"output_tokens":450000,"cache_read_input_tokens":3000000,"cache_creation_input_tokens":150000}},"content":[{{"type":"tool_use","id":"t1","name":"Bash","input":{{"command":"for d in \"${{PROTECTED_PATHS[@]}}\"; do rm -rf \"$d\"; done"}}}}]}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"tool_result","timestamp":"2026-08-31T10:00:04Z","tool_use_id":"t1","content":"Deleted 27.1 GB","is_error":false}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"type":"assistant","timestamp":"2026-08-31T10:00:05Z","content":[{{"type":"text","text":"STOP. That was my mistake — the path was deleted precisely because it was on the protect list. The guard became the target. Tell Sam today."}}]}}"#
    )
    .unwrap();

    let db_path = temp.path().join("test_agentworth.db");

    // Scan
    Command::cargo_bin("agentworth")
        .unwrap()
        .arg("--db-path")
        .arg(&db_path)
        .arg("scan")
        .arg(temp.path())
        .assert()
        .success();

    // 1. Run blunder (JSON mode)
    let mut blunder_json_cmd = Command::cargo_bin("agwt").unwrap();
    blunder_json_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("blunder")
        .arg("--top")
        .arg("3")
        .arg("--json");

    blunder_json_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("\"rule_id\": \"LEAKED_SHELL_VARIABLE\""))
        .stdout(predicate::str::contains("\"title\": \"The Missing `local` Weapon (The Katana Incident)\""))
        .stdout(predicate::str::contains("\"severity\": \"CRITICAL\""))
        .stdout(predicate::str::contains("\"session_hash\""));

    // 2. Run blunder (ASCII slip text output) with stdin EOF
    let mut blunder_cmd = Command::cargo_bin("agwt").unwrap();
    blunder_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("blunder")
        .write_stdin("n\n");

    blunder_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("AGENTWORTH HALL OF BLUNDERS"))
        .stdout(predicate::str::contains("The Missing `local` Weapon (The Katana Incident)"))
        .stdout(predicate::str::contains("LEAKED_SHELL_VARIABLE"))
        .stdout(predicate::str::contains("CRITICAL"))
        .stdout(predicate::str::contains("AGENT REMORSE QUOTE"))
        .stdout(predicate::str::contains("FATAL MONOSPACE SNIPPET"))
        .stdout(predicate::str::contains("for d in \"${PROTECTED_PATHS[@]}\"; do rm -rf \"$d\"; done"))
        .stdout(predicate::str::contains("[ VERIFIED BY AGENTWORTH ]"));

    // 3. Run blunder with --submit --json (offline fallback mode)
    let mut blunder_submit_cmd = Command::cargo_bin("agwt").unwrap();
    blunder_submit_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("blunder")
        .arg("--submit")
        .arg("--json");

    blunder_submit_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("\"submission\""))
        .stdout(predicate::str::contains("\"status\""));
}

#[test]
fn test_cli_receipt_command_ansi_svg_and_export() {
    let temp = tempdir().unwrap();
    let (_session_file, session_id) = setup_sample_claude_session(temp.path());

    let db_path = temp.path().join("test_agentworth.db");

    // Index the sample session
    let mut scan_cmd = Command::cargo_bin("agwt").unwrap();
    scan_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("scan")
        .arg(temp.path())
        .assert()
        .success();

    // 1. Run receipt command with default format (Terminal ANSI)
    let mut receipt_term_cmd = Command::cargo_bin("agwt").unwrap();
    receipt_term_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("receipt")
        .arg(&session_id);

    receipt_term_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("AGENTWORTH FLIGHT RECEIPT"))
        .stdout(predicate::str::contains("TYPED PROVENANCE:"))
        .stdout(predicate::str::contains("FLOWN"))
        .stdout(predicate::str::contains("COMPOSITE SCORE:"))
        .stdout(predicate::str::contains("TOKEN USAGE & FINANCIALS:"))
        .stdout(predicate::str::contains("APOLOGY TAX & REMORSE AUDIT:"))
        .stdout(predicate::str::contains("AUTONOMOUS RESILIENCE & RECOVERY:"))
        .stdout(predicate::str::contains("VERIFIED BY AGENTWORTH"));

    // 2. Run receipt command with format svg
    let mut receipt_svg_cmd = Command::cargo_bin("agwt").unwrap();
    receipt_svg_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("receipt")
        .arg(&session_id)
        .arg("--format")
        .arg("svg");

    receipt_svg_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"))
        .stdout(predicate::str::contains("<svg width=\"1200\" height=\"630\""))
        .stdout(predicate::str::contains("AGENTWORTH"))
        .stdout(predicate::str::contains("FLIGHT RECEIPT"))
        .stdout(predicate::str::contains("FLOWN"))
        .stdout(predicate::str::contains("TOKEN USAGE &amp; SPEND"))
        .stdout(predicate::str::contains("AGENTWORTH VERIFIED"));

    // 3. Run receipt command with format json
    let mut receipt_json_cmd = Command::cargo_bin("agwt").unwrap();
    receipt_json_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("receipt")
        .arg(&session_id)
        .arg("--format")
        .arg("json");

    receipt_json_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("\"session_id\":"))
        .stdout(predicate::str::contains("\"provenance_status\": \"Flown\""))
        .stdout(predicate::str::contains("\"composite_score\":"));

    // 4. Output SVG to a file using --output
    let svg_out_path = temp.path().join("exports").join("flight_receipt.svg");
    let mut receipt_out_cmd = Command::cargo_bin("agwt").unwrap();
    receipt_out_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("receipt")
        .arg(&session_id)
        .arg("--format")
        .arg("svg")
        .arg("--output")
        .arg(&svg_out_path);

    receipt_out_cmd.assert().success();
    assert!(svg_out_path.exists());
    let svg_content = fs::read_to_string(&svg_out_path).unwrap();
    assert!(svg_content.contains("<svg width=\"1200\" height=\"630\""));

    // 5. Test export command with --format receipt
    let mut export_receipt_cmd = Command::cargo_bin("agwt").unwrap();
    export_receipt_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("export")
        .arg(&session_id)
        .arg("--format")
        .arg("receipt");

    export_receipt_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("AGENTWORTH FLIGHT RECEIPT"))
        .stdout(predicate::str::contains("TYPED PROVENANCE:"));

    // 6. Test export command with --format svg
    let mut export_svg_cmd = Command::cargo_bin("agwt").unwrap();
    export_svg_cmd
        .arg("--db-path")
        .arg(&db_path)
        .arg("export")
        .arg(&session_id)
        .arg("--format")
        .arg("svg");

    export_svg_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("<svg width=\"1200\" height=\"630\""));
}

