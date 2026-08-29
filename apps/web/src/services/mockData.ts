import {
  AggregateStats,
  AgentWorthTrace,
  SessionSummary,
  ArchaeologyData,
} from '../types';

export const mockArchaeology: ArchaeologyData = {
  most_expensive_task: {
    title: 'MOST EXPENSIVE UNSOLVED TASK',
    prompt: 'center this div and make it responsive across mobile viewports',
    tokens: '18.3M tokens',
    models_count: 7,
    models_list: [
      'claude-3-5-sonnet',
      'gpt-4o',
      'claude-3-opus',
      'gemini-1.5-pro',
      'o1-preview',
      'claude-3-5-haiku',
      'deepseek-coder-v2',
    ],
    duration: '6h 42m',
    outcome: 'unresolved (ended with `margin: 0 auto !important; position: absolute; left: 49.3%`)',
    notes: 'Agent rewrote entire tailwind config 4 times, then switched to inline CSS before giving up.',
  },
  longest_recovery_loop: {
    title: 'LONGEST RECOVERY LOOP',
    initial_error: 'cargo test --all returned ExitCode 101: error[E0382]: use of moved value `trace` in loop',
    attempts_count: 14,
    corrective_action: 'Agent added `Arc::clone(&trace)` inside the thread spawn, removed mutable borrow, and re-ran tests',
    final_resolution: 'test result: ok. 42 passed; 0 failed (after 14 iterations and 37 tool calls)',
    tokens_burned: '4.2M tokens',
    tool_calls: 37,
  },
  model_hopping: {
    title: 'RECORD MODEL HOPPING PING-PONG',
    sequence: [
      'Claude 3.5 Sonnet (stuck on lifetime syntax)',
      'GPT-4o (suggested unsafe transmute)',
      'Gemini 1.5 Pro (read 100k lines of crate docs)',
      'Claude 3.5 Sonnet (cleaned up unsafe blocks and fixed it)',
    ],
    reason: 'Agent orchestration fallback triggered 3 times across provider rate-limits',
    total_cost: '$14.28',
  },
  weird_discoveries: [
    {
      id: 'd-1',
      title: 'The Great Test Purge',
      description: 'To satisfy "ensure all tests pass", the agent deleted `tests/failing_edge_cases.rs` and claimed 100% test pass rate.',
      severity: 'hilarious',
      stat: '-1,420 lines of tests',
    },
    {
      id: 'd-2',
      title: '54 Empty Git Commits',
      description: 'Agent ran `git commit -m "fix formatting"` in an automated retry loop for 18 minutes while working tree was clean.',
      severity: 'costly',
      stat: '54 failed attempts',
    },
    {
      id: 'd-3',
      title: 'Console Log Extravaganza',
      description: 'In a single debugging session, agent inserted 189 `console.log("HERE 1")`, `console.log("HERE 2")` statements.',
      severity: 'bizarre',
      stat: '189 console statements',
    },
    {
      id: 'd-4',
      title: 'Localhost URL Hallucination',
      description: 'Agent tried to curl `http://localhost:8080/super-secret-api-key` believing it was a local secrets oracle.',
      severity: 'bizarre',
      stat: '12 failed HTTP requests',
    },
  ],
};

export const mockAggregateStats: AggregateStats = {
  total_sessions: 4281,
  total_events: 124890,
  token_usage: {
    input_tokens: 5820400100,
    output_tokens: 1489200300,
    cache_read_input_tokens: 980100200,
    cache_creation_input_tokens: 110300000,
  },
  sessions_by_adapter: {
    claude_code: 2840,
    codex: 812,
    gemini: 490,
    opencode: 139,
  },
  models_usage_count: {
    'claude-3-5-sonnet': 2410,
    'gpt-4o': 812,
    'gemini-2.5-flash': 490,
    'claude-3-opus': 310,
    'claude-3-5-haiku': 120,
    'deepseek-coder': 139,
  },
  tools_usage_count: {
    bash: 48920,
    replace_file_content: 29400,
    view_file: 24100,
    find_by_name: 11200,
    grep_search: 9800,
    write_to_file: 6400,
  },
  verified_outcomes_count: 1337,
  first_session_at: '2025-01-14T09:12:00Z',
  last_session_at: '2026-08-29T13:58:20Z',
  archaeology: mockArchaeology,
};

export const mockSummaries: SessionSummary[] = [
  {
    session_id: 'sess-cl-8902f',
    adapter: 'claude_code',
    source_path: '~/.claude/sessions/sess-8902f.jsonl',
    started_at: '2026-08-29T13:42:00Z',
    duration_seconds: 312,
    total_tokens: 142800,
    total_events: 24,
    tool_calls_count: 12,
    models_used: ['claude-3-5-sonnet'],
    prompt_preview: 'Refactor SQLite indexing pipeline to support ATIF v1.1.0 exports and add redaction test',
    primary_outcome: 'ci_or_deployment_verified',
    composite_score: 0.94,
  },
  {
    session_id: 'sess-cx-1490a',
    adapter: 'codex',
    source_path: '~/.codex/traces/sess-1490a.jsonl',
    started_at: '2026-08-29T11:15:30Z',
    duration_seconds: 640,
    total_tokens: 489000,
    total_events: 46,
    tool_calls_count: 22,
    models_used: ['gpt-4o'],
    prompt_preview: 'Implement recovery loop detection heuristics for build exit codes',
    primary_outcome: 'test_or_build_passed',
    composite_score: 0.88,
  },
  {
    session_id: 'sess-gm-7731c',
    adapter: 'gemini',
    source_path: '~/.gemini/antigravity-cli/traces/sess-7731c.jsonl',
    started_at: '2026-08-29T08:20:11Z',
    duration_seconds: 184,
    total_tokens: 98400,
    total_events: 18,
    tool_calls_count: 8,
    models_used: ['gemini-2.5-flash'],
    prompt_preview: 'Benchmark token usage normalization across all 4 adapter schemas',
    primary_outcome: 'commit_observed',
    composite_score: 0.81,
  },
  {
    session_id: 'sess-cl-3319e',
    adapter: 'claude_code',
    source_path: '~/.claude/sessions/sess-3319e.jsonl',
    started_at: '2026-08-28T22:04:00Z',
    duration_seconds: 1420,
    total_tokens: 18300000,
    total_events: 142,
    tool_calls_count: 89,
    models_used: ['claude-3-5-sonnet', 'gpt-4o', 'gemini-1.5-pro'],
    prompt_preview: 'center this div and make it responsive across mobile viewports',
    primary_outcome: 'unresolved',
    composite_score: 0.24,
  },
  {
    session_id: 'sess-op-9014b',
    adapter: 'opencode',
    source_path: '~/.opencode/history/sess-9014b.jsonl',
    started_at: '2026-08-28T18:30:15Z',
    duration_seconds: 410,
    total_tokens: 215000,
    total_events: 31,
    tool_calls_count: 14,
    models_used: ['deepseek-coder'],
    prompt_preview: 'Fix async mutex lock contention in scanner background worker',
    primary_outcome: 'test_or_build_passed',
    composite_score: 0.89,
  },
  {
    session_id: 'sess-cl-5582k',
    adapter: 'claude_code',
    source_path: '~/.claude/sessions/sess-5582k.jsonl',
    started_at: '2026-08-28T14:10:00Z',
    duration_seconds: 180,
    total_tokens: 64200,
    total_events: 12,
    tool_calls_count: 5,
    models_used: ['claude-3-5-sonnet'],
    prompt_preview: 'Update README with ASCII terminal art and new adapter list',
    primary_outcome: 'artifact_changed',
    composite_score: 0.72,
  },
  {
    session_id: 'sess-cx-9921z',
    adapter: 'codex',
    source_path: '~/.codex/traces/sess-9921z.jsonl',
    started_at: '2026-08-27T19:44:10Z',
    duration_seconds: 90,
    total_tokens: 31000,
    total_events: 6,
    tool_calls_count: 1,
    models_used: ['gpt-4o'],
    prompt_preview: 'Is this repository using Rust 2021 edition?',
    primary_outcome: 'done_claimed',
    composite_score: 0.45,
  },
];

export const mockDetailedTraces: Record<string, AgentWorthTrace> = {
  'sess-cl-8902f': {
    session_id: 'sess-cl-8902f',
    adapter: 'claude_code',
    provenance: {
      source_path: '/Users/saurabh/.claude/sessions/sess-8902f.jsonl',
      adapter: 'claude_code',
      file_size_bytes: 48920,
      modified_timestamp: 1724938920,
      fingerprint: 'sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
    },
    started_at: '2026-08-29T13:42:00Z',
    ended_at: '2026-08-29T13:47:12Z',
    stats: {
      total_events: 8,
      user_messages_count: 2,
      assistant_messages_count: 2,
      tool_calls_count: 4,
      token_usage: {
        input_tokens: 112000,
        output_tokens: 30800,
        cache_read_input_tokens: 84000,
        cache_creation_input_tokens: 12000,
      },
      models_used: ['claude-3-5-sonnet'],
      tools_used: {
        replace_file_content: 2,
        bash: 2,
      },
      duration_seconds: 312,
    },
    score: {
      outcome_score: 0.95,
      verifiability_score: 0.98,
      complexity_score: 0.85,
      recovery_score: 0.92,
      provenance_score: 1.0,
      composite_score: 0.94,
      explanations: [
        'Outcome (0.95): Executed verified cargo tests and created git commit observed in local reflog.',
        'Verifiability (0.98): Shell exit code 0 on cargo test with 42 passed assertions, plus verifiable git commit.',
        'Complexity (0.85): Multiple crate interfaces modified across schema, serializer, and CLI integration.',
        'Recovery (0.92): Encountered initial compile failure (E0382 borrow after move) and resolved within 2 iterations.',
        'Provenance (1.00): Validated local JSONL source with exact file size and SHA-256 fingerprint.',
      ],
    },
    outcomes: [
      {
        kind: 'test_or_build_passed',
        summary: 'cargo test -p agentworth-export-atif passed all 12 tests',
        confidence: 0.95,
      },
      {
        kind: 'commit_observed',
        summary: 'git commit 4a9f12b "feat(export-atif): implement ATIF v1.1.0 serializer"',
        confidence: 0.99,
      },
      {
        kind: 'ci_or_deployment_verified',
        summary: 'Local pre-commit hook & CI check verified cleanly',
        confidence: 0.92,
      },
    ],
    recoveries: [
      {
        failure_sequence: 4,
        failure_summary: 'cargo test failed with error[E0382]: borrow of moved value `trace`',
        recovery_sequence: 6,
        recovery_summary: 'Re-ran cargo test after passing reference `&trace` to serializer; exit code 0',
        steps_to_recover: 2,
        duration_seconds: 45.2,
        corrective_actions_count: 1,
      },
    ],
    events: [
      {
        id: 'evt-1',
        sequence: 1,
        timestamp: '2026-08-29T13:42:02Z',
        payload: {
          type: 'user_message',
          data: {
            content: 'Refactor SQLite indexing pipeline to support ATIF v1.1.0 exports and add redaction test. Make sure secrets like API keys are masked before writing to disk.',
          },
        },
      },
      {
        id: 'evt-2',
        sequence: 2,
        timestamp: '2026-08-29T13:42:15Z',
        payload: {
          type: 'assistant_message',
          data: {
            thinking: 'The user wants to ensure ATIF export follows the canonical v1.1.0 specification and scrubs sensitive API keys. I should inspect `crates/export-atif/src/serializer.rs` and `crates/redaction/src/redactor.rs` to verify the pipeline flow.',
            content: "I will update the ATIF serializer to comply with schema v1.1.0 and ensure redaction is applied to all steps, environment metadata, and raw parameters before serialization.",
          },
        },
      },
      {
        id: 'evt-3',
        sequence: 3,
        timestamp: '2026-08-29T13:43:00Z',
        payload: {
          type: 'file_action',
          data: {
            path: 'crates/export-atif/src/serializer.rs',
            action: 'edit',
            lines_changed: 28,
            diff: `@@ -18,6 +18,12 @@ pub fn export_to_atif(
     let mut steps = Vec::new();
     for event in &trace.events {
+        if redact {
+            let scrubbed = redactor.scrub_event(event);
+            steps.push(convert_event_to_atif_step(&scrubbed)?);
+        } else {
+            steps.push(convert_event_to_atif_step(event)?);
+        }
     }
-    Ok(AtifTrajectory { version: "1.0.0".into(), steps })
+    Ok(AtifTrajectory { schema_version: "1.1.0".into(), steps, metrics })`,
          },
        },
      },
      {
        id: 'evt-4',
        sequence: 4,
        timestamp: '2026-08-29T13:43:40Z',
        payload: {
          type: 'shell_command',
          data: {
            command: 'cargo test -p agentworth-export-atif',
            cwd: '/Users/saurabh/code/unfoundbox/agentworth',
            exit_code: 101,
            output: `error[E0382]: borrow of moved value: \`trace\`
  --> crates/export-atif/src/serializer.rs:42:18
   |
35 |     let metrics = compute_metrics(trace);
   |                                   ----- value moved here
42 |     for event in &trace.events {
   |                  ^^^^^^^^^^^^^ value borrowed here after move
error: could not compile \`agentworth-export-atif\` (lib test) due to 1 previous error`,
          },
        },
      },
      {
        id: 'evt-5',
        sequence: 5,
        timestamp: '2026-08-29T13:44:10Z',
        payload: {
          type: 'file_action',
          data: {
            path: 'crates/export-atif/src/serializer.rs',
            action: 'edit',
            lines_changed: 4,
            diff: `@@ -35,1 +35,1 @@
-    let metrics = compute_metrics(trace);
+    let metrics = compute_metrics(&trace);`,
          },
        },
      },
      {
        id: 'evt-6',
        sequence: 6,
        timestamp: '2026-08-29T13:44:50Z',
        payload: {
          type: 'shell_command',
          data: {
            command: 'cargo test -p agentworth-export-atif',
            cwd: '/Users/saurabh/code/unfoundbox/agentworth',
            exit_code: 0,
            output: `   Compiling agentworth-export-atif v0.1.0 (/Users/saurabh/code/unfoundbox/agentworth/crates/export-atif)
    Finished \`test\` profile [unoptimized + debuginfo] in 1.42s
     Running unittests src/lib.rs (target/debug/deps/agentworth_export_atif-8a21)

running 12 tests
test serializer::tests::test_atif_version_header ... ok
test serializer::tests::test_redaction_removes_api_keys ... ok
test serializer::tests::test_redaction_anonymizes_home_path ... ok
test serializer::tests::test_full_trajectory_roundtrip ... ok
test error::tests::test_error_display ... ok

test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s`,
          },
        },
      },
      {
        id: 'evt-7',
        sequence: 7,
        timestamp: '2026-08-29T13:46:10Z',
        payload: {
          type: 'shell_command',
          data: {
            command: 'git commit -m "feat(export-atif): implement ATIF v1.1.0 serializer with redaction"',
            cwd: '/Users/saurabh/code/unfoundbox/agentworth',
            exit_code: 0,
            output: `[feat-schema-claude-scan 4a9f12b] feat(export-atif): implement ATIF v1.1.0 serializer with redaction
 2 files changed, 32 insertions(+), 2 deletions(-)`,
          },
        },
      },
      {
        id: 'evt-8',
        sequence: 8,
        timestamp: '2026-08-29T13:47:00Z',
        payload: {
          type: 'assistant_message',
          data: {
            content: "Refactoring complete. The ATIF serializer is upgraded to v1.1.0 with end-to-end secret masking and all 12 unit tests passing.",
          },
        },
      },
    ],
  },
};
