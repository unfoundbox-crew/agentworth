use std::fs::{self, File};
use std::io::Write;
use std::sync::Arc;

use agentworth_adapter_sdk::ScanOptions;
use agentworth_core::Scanner;
use agentworth_storage::Storage;
use tempfile::tempdir;

#[test]
fn test_scanner_with_nested_claude_project_directory() {
    let temp = tempdir().unwrap();
    let claude_dir = temp
        .path()
        .join(".claude")
        .join("projects")
        .join("my-project");
    fs::create_dir_all(&claude_dir).unwrap();

    // Session 1: Normal session
    let file1_path = claude_dir.join("session_1.jsonl");
    let mut file1 = File::create(&file1_path).unwrap();
    writeln!(
        file1,
        r#"{{"type":"user","timestamp":"2026-08-29T10:00:00Z","content":"Write a function"}}"#
    )
    .unwrap();
    writeln!(
        file1,
        r#"{{"type":"assistant","timestamp":"2026-08-29T10:00:02Z","model":"claude-3-5-sonnet-20241022","usage":{{"input_tokens":300,"output_tokens":80,"cache_read_input_tokens":100,"cache_creation_input_tokens":20}},"content":[{{"type":"text","text":"Here is the function"}},{{"type":"tool_use","id":"t1","name":"FileEdit","input":{{"file_path":"src/lib.rs","diff":"+fn hello() {{}}"}}}}]}}"#
    )
    .unwrap();
    writeln!(
        file1,
        r#"{{"type":"tool_result","timestamp":"2026-08-29T10:00:03Z","tool_use_id":"t1","content":"File updated successfully","is_error":false}}"#
    )
    .unwrap();

    // Session 2: Session with test outcome evidence
    let file2_path = claude_dir.join("session_2.jsonl");
    let mut file2 = File::create(&file2_path).unwrap();
    writeln!(
        file2,
        r#"{{"type":"user","timestamp":"2026-08-29T11:00:00Z","content":"Run the tests"}}"#
    )
    .unwrap();
    writeln!(
        file2,
        r#"{{"type":"assistant","timestamp":"2026-08-29T11:00:02Z","model":"claude-3-5-haiku-20241022","usage":{{"input_tokens":200,"output_tokens":40,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}},"content":[{{"type":"tool_use","id":"t2","name":"Bash","input":{{"command":"cargo test"}}}}]}}"#
    )
    .unwrap();
    writeln!(
        file2,
        r#"{{"type":"tool_result","timestamp":"2026-08-29T11:00:04Z","tool_use_id":"t2","content":"test result: ok. 10 passed; 0 failed","is_error":false}}"#
    )
    .unwrap();

    let storage_dir = temp.path().join(".agentworth");
    let db_path = storage_dir.join("agentworth.db");
    let storage = Arc::new(Storage::open_path(&db_path).unwrap());
    let scanner = Scanner::new(storage.clone());

    let options = ScanOptions {
        custom_paths: vec![temp.path().to_path_buf()],
        force: false,
    };

    // 1. Initial scan
    let summary1 = scanner.run_scan(&options, |_, _| {}).unwrap();
    assert_eq!(summary1.discovered_sources, 2);
    assert_eq!(summary1.scanned_sessions, 2);
    assert_eq!(summary1.skipped_unchanged, 0);
    assert_eq!(summary1.total_indexed_sessions, 2);
    assert_eq!(
        summary1
            .aggregate_stats
            .sessions_by_adapter
            .get("claude_code"),
        Some(&2)
    );

    // Token totals: (300+80+100+20) + (200+40) = 500 + 240 = 740
    assert_eq!(summary1.aggregate_stats.token_usage.total(), 740);
    assert_eq!(summary1.aggregate_stats.token_usage.input_tokens, 500);
    assert_eq!(summary1.aggregate_stats.token_usage.output_tokens, 120);

    // Tools & Models
    assert_eq!(
        summary1
            .aggregate_stats
            .models_usage_count
            .get("claude-3-5-sonnet-20241022"),
        Some(&1)
    );
    assert_eq!(
        summary1
            .aggregate_stats
            .models_usage_count
            .get("claude-3-5-haiku-20241022"),
        Some(&1)
    );
    assert_eq!(
        summary1.aggregate_stats.tools_usage_count.get("FileEdit"),
        Some(&1)
    );
    assert_eq!(
        summary1.aggregate_stats.tools_usage_count.get("Bash"),
        Some(&1)
    );

    // 2. Incremental rescan -> both should be skipped
    let summary2 = scanner.run_scan(&options, |_, _| {}).unwrap();
    assert_eq!(summary2.discovered_sources, 2);
    assert_eq!(summary2.scanned_sessions, 0);
    assert_eq!(summary2.skipped_unchanged, 2);
    assert_eq!(summary2.total_indexed_sessions, 2);

    // 3. Force rescan -> both should be rescanned
    let force_options = ScanOptions {
        custom_paths: vec![temp.path().to_path_buf()],
        force: true,
    };
    let summary3 = scanner.run_scan(&force_options, |_, _| {}).unwrap();
    assert_eq!(summary3.discovered_sources, 2);
    assert_eq!(summary3.scanned_sessions, 2);
    assert_eq!(summary3.skipped_unchanged, 0);
    assert_eq!(summary3.total_indexed_sessions, 2);
}

#[test]
fn test_scanner_with_all_adapters_end_to_end() {
    let temp = tempdir().unwrap();

    // 1. Claude session
    let claude_dir = temp.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    let claude_file = claude_dir.join("claude_session.jsonl");
    let mut f1 = File::create(&claude_file).unwrap();
    writeln!(f1, r#"{{"type":"user","content":"Build feature"}}"#).unwrap();
    writeln!(
        f1,
        r#"{{"type":"assistant","model":"claude-3-5-sonnet","usage":{{"input_tokens":100,"output_tokens":50}},"content":[{{"type":"text","text":"Done"}}]}}"#
    ).unwrap();

    // 2. Codex session
    let codex_dir = temp.path().join(".codex");
    fs::create_dir_all(&codex_dir).unwrap();
    let codex_file = codex_dir.join("codex_session.jsonl");
    let mut f2 = File::create(&codex_file).unwrap();
    writeln!(f2, r#"{{"role":"user","content":"Fix test"}}"#).unwrap();
    writeln!(
        f2,
        r#"{{"role":"assistant","model":"gpt-4o","usage":{{"prompt_tokens":200,"completion_tokens":80,"prompt_tokens_details":{{"cached_tokens":40}}}},"content":"Fixed"}}"#
    ).unwrap();

    // 3. Gemini session
    let gemini_dir = temp.path().join(".gemini");
    fs::create_dir_all(&gemini_dir).unwrap();
    let gemini_file = gemini_dir.join("gemini_session.jsonl");
    let mut f3 = File::create(&gemini_file).unwrap();
    writeln!(f3, r#"{{"type":"USER_INPUT","content":"Refactor module"}}"#).unwrap();
    writeln!(
        f3,
        r#"{{"type":"PLANNER_RESPONSE","model":"gemini-2.5-pro","usageMetadata":{{"promptTokenCount":300,"candidatesTokenCount":100,"cachedContentTokenCount":50}},"content":"Refactored"}}"#
    ).unwrap();

    // 4. OpenCode session
    let opencode_dir = temp.path().join(".opencode");
    fs::create_dir_all(&opencode_dir).unwrap();
    let opencode_file = opencode_dir.join("opencode_session.jsonl");
    let mut f4 = File::create(&opencode_file).unwrap();
    writeln!(f4, r#"{{"type":"user_message","content":"Review code"}}"#).unwrap();
    writeln!(
        f4,
        r#"{{"type":"assistant_message","model":"deepseek-coder","usage":{{"input_tokens":150,"output_tokens":60}},"content":"Reviewed"}}"#
    ).unwrap();

    let storage_dir = temp.path().join(".agentworth");
    let db_path = storage_dir.join("agentworth.db");
    let storage = Arc::new(Storage::open_path(&db_path).unwrap());
    let scanner = Scanner::new(storage.clone());

    let options = ScanOptions {
        custom_paths: vec![temp.path().to_path_buf()],
        force: false,
    };

    let summary = scanner.run_scan(&options, |_, _| {}).unwrap();
    assert_eq!(summary.discovered_sources, 4);
    assert_eq!(summary.scanned_sessions, 4);
    assert_eq!(summary.total_indexed_sessions, 4);

    // Verify adapter distribution
    let by_adapter = &summary.aggregate_stats.sessions_by_adapter;
    assert_eq!(by_adapter.get("claude_code"), Some(&1));
    assert_eq!(by_adapter.get("codex"), Some(&1));
    assert_eq!(by_adapter.get("antigravity"), Some(&1));
    assert_eq!(by_adapter.get("opencode"), Some(&1));

    // Verify models distribution
    let models = &summary.aggregate_stats.models_usage_count;
    assert_eq!(models.get("claude-3-5-sonnet"), Some(&1));
    assert_eq!(models.get("gpt-4o"), Some(&1));
    assert_eq!(models.get("gemini-2.5-pro"), Some(&1));
    assert_eq!(models.get("deepseek-coder"), Some(&1));

    // Verify aggregate token counts:
    // Claude: 100 in, 50 out = 150
    // Codex: 200 in, 80 out, 40 cache_read = 320
    // Gemini: 300 in, 100 out, 50 cache_read = 450
    // OpenCode: 150 in, 60 out = 210
    // Total = 150 + 320 + 450 + 210 = 1130
    assert_eq!(summary.aggregate_stats.token_usage.total(), 1130);
    assert_eq!(summary.aggregate_stats.token_usage.input_tokens, 750);
    assert_eq!(summary.aggregate_stats.token_usage.output_tokens, 290);
    assert_eq!(summary.aggregate_stats.token_usage.cache_read_tokens, 90);
}
