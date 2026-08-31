//! End-to-end vector indexing and semantic retrieval integration tests for AgentWorth.

use agentworth_schema::vector::ChunkKind;
use agentworth_schema::{
    AgentWorthTrace, EventPayload, FileActionType, NormalizedEvent, OutcomeEvidence, OutcomeKind,
    Provenance, ShellCommand,
};
use agentworth_storage::vector::VectorStore;
use agentworth_storage::{LocalEmbedder, Storage, TrajectoryChunker};
use chrono::Utc;

#[test]
fn test_vector_engine_end_to_end_indexing_and_semantic_retrieval() {
    // 1. Create in-memory storage and vector store
    let storage = Storage::open_in_memory().expect("open in-memory storage");
    let vector_store = storage.vector_store().expect("vector store");
    let embedder = LocalEmbedder::new_deterministic();

    // 2. Build a realistic catastrophic session trace (Claude Opus accidentally deleted Katana repo)
    let start_time = Utc::now();
    let prov1 = Provenance::new(
        "/Users/saurabh/.claude/projects/-Users-saurabh-code-katana/trace.jsonl",
        "claude_code",
        2048,
        1700000000,
        "fp_katana",
    );
    let mut trace1 = AgentWorthTrace::new("sess-opus-catastrophe", "claude_code", prov1, start_time);
    trace1.stats.models_used = vec!["claude-opus-5".to_string()];
    trace1.stats.token_usage.input_tokens = 180_000_000;
    trace1.stats.token_usage.output_tokens = 7_100_000;

    trace1.events.push(NormalizedEvent::new(
        1,
        start_time,
        EventPayload::UserMessage {
            content: "Run the cleanup script to remove stale worktree branches across repositories.".to_string(),
        },
    ));
    trace1.events.push(NormalizedEvent::new(
        2,
        start_time,
        EventPayload::ShellCommand(ShellCommand {
            command: "rm -rf /Users/saurabh/code/katana".to_string(),
            cwd: Some("/Users/saurabh/code".to_string()),
            exit_code: Some(0),
            output: None,
        }),
    ));
    trace1.events.push(NormalizedEvent::new(
        3,
        start_time,
        EventPayload::AssistantMessage {
            content: "STOP. The trace shows rm -rf /Users/saurabh/code/katana — my own code, executing against the path that was supposed to be protected. Killing it now. Katana was 30 GB; it is now 2.9 GB. A missing local turned my safety mechanism into a weapon.".to_string(),
            thinking: Some("Emergency stop: I deleted the katana repository by mistake.".to_string()),
        },
    ));

    storage.upsert_trace(&trace1).expect("upsert trace1 in SQLite");

    // 3. Build a second session (Codex building frontend components cleanly)
    let prov2 = Provenance::new(
        "/Users/saurabh/code/vibelaunch/session.jsonl",
        "codex",
        1024,
        1700005000,
        "fp_vibe",
    );
    let mut trace2 = AgentWorthTrace::new("sess-codex-frontend", "codex", prov2, start_time);
    trace2.stats.models_used = vec!["gpt-5".to_string()];
    trace2.stats.token_usage.input_tokens = 50_000;
    trace2.stats.token_usage.output_tokens = 10_000;

    trace2.events.push(NormalizedEvent::new(
        1,
        start_time,
        EventPayload::UserMessage {
            content: "Build an accessible modal dialog in React with Tailwind CSS glassmorphism styles.".to_string(),
        },
    ));
    trace2.events.push(NormalizedEvent::new(
        2,
        start_time,
        EventPayload::FileAction {
            path: "src/components/Modal.tsx".to_string(),
            action: FileActionType::Write,
            diff: Some("+ export function Modal() { return <div className='glass backdrop-blur'>Modal</div>; }".to_string()),
            lines_changed: Some(15),
        },
    ));
    trace2.events.push(NormalizedEvent::new(
        3,
        start_time,
        EventPayload::OutcomeEvidence(OutcomeEvidence {
            kind: OutcomeKind::TestOrBuildPassed,
            summary: "npm test passed with 8 component assertions".to_string(),
            confidence: 1.0,
        }),
    ));

    storage.upsert_trace(&trace2).expect("upsert trace2 in SQLite");

    // 4. Extract semantic chunks from both sessions
    let chunks1 = TrajectoryChunker::extract_chunks(&trace1);
    let chunks2 = TrajectoryChunker::extract_chunks(&trace2);

    assert!(chunks1.len() >= 3, "Expected summary, tool invocation, and panic chunks");
    assert!(chunks2.len() >= 2, "Expected summary and code lineage chunks");

    // 5. Generate embeddings
    let texts1: Vec<String> = chunks1.iter().map(|c| c.text_content.clone()).collect();
    let embeddings1 = embedder.embed_batch(&texts1).expect("embed texts1");
    vector_store
        .insert_embeddings(&chunks1, &embeddings1)
        .expect("insert chunks1");

    let texts2: Vec<String> = chunks2.iter().map(|c| c.text_content.clone()).collect();
    let embeddings2 = embedder.embed_batch(&texts2).expect("embed texts2");
    vector_store
        .insert_embeddings(&chunks2, &embeddings2)
        .expect("insert chunks2");

    // 6. Verify vector stats
    let stats = vector_store.stats().expect("vector stats");
    assert_eq!(stats.total_sessions, 2);
    assert_eq!(stats.total_chunks, chunks1.len() + chunks2.len());
    assert_eq!(stats.dimensions, 384);

    // 7. Query: "opus worktree cleanup deleted the repos by accident"
    let query_danger = embedder
        .embed_text("opus worktree cleanup deleted the repos by accident")
        .expect("embed query");
    let danger_results = vector_store
        .search(&query_danger, 5, 0.0)
        .expect("search danger query");

    assert!(!danger_results.is_empty());
    // Top hit must be from the catastrophic session
    assert_eq!(danger_results[0].session_id, "sess-opus-catastrophe");
    assert_eq!(danger_results[0].adapter, "claude_code");
    assert_eq!(danger_results[0].total_tokens, 187_100_000);
    assert_eq!(danger_results[0].model.as_deref(), Some("claude-opus-5"));

    // 8. Query: "React Tailwind modal glassmorphism"
    let query_frontend = embedder
        .embed_text("React Tailwind modal component styling")
        .expect("embed frontend query");
    let frontend_results = vector_store
        .search(&query_frontend, 5, 0.0)
        .expect("search frontend query");

    assert!(!frontend_results.is_empty());
    assert_eq!(frontend_results[0].session_id, "sess-codex-frontend");
    assert_eq!(frontend_results[0].adapter, "codex");

    // 9. Filtered query by ChunkKind
    let tool_only_results = vector_store
        .search_filtered(&query_danger, 5, 0.0, None, Some(ChunkKind::ToolInvocation))
        .expect("search tool only");
    assert!(!tool_only_results.is_empty());
    for r in &tool_only_results {
        assert_eq!(r.kind, ChunkKind::ToolInvocation);
    }

    // 10. Rescan / delete session
    vector_store
        .delete_session("sess-opus-catastrophe")
        .expect("delete session vectors");
    let stats_after = vector_store.stats().expect("stats after delete");
    assert_eq!(stats_after.total_sessions, 1);
    assert_eq!(stats_after.total_chunks, chunks2.len());
}
