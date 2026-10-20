use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use agentworth_attest::{verify_envelope, ReceiptSigningKey, SignatureVerdict};
use agentworth_schema::work_receipt::{CommandClass, EvidenceSource, RangeRule};
use agentworth_schema::{
    AgentWorthTrace, EventPayload, FileActionType, NormalizedEvent, OutcomeKind, Provenance,
    ShellCommand, TokenUsage,
};
use agentworth_storage::{SessionRisk, Storage};
use chrono::{Duration, Utc};

use super::*;

/// A real git repository in a temp dir (the `suspect` tests' pattern): the range, the notes
/// and the push are all things git does, so a mock would test nothing.
struct TempRepo {
    dir: tempfile::TempDir,
}

impl TempRepo {
    fn new() -> Self {
        let repo = Self {
            dir: tempfile::tempdir().expect("tempdir"),
        };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.email", "test@example.invalid"]);
        repo.git(&["config", "user.name", "Test"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo
    }

    fn path(&self) -> PathBuf {
        std::fs::canonicalize(self.dir.path()).expect("canonicalize temp dir")
    }

    fn git(&self, args: &[&str]) -> String {
        git_in(self.dir.path(), args)
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

fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
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

/// Every script the multilingual corpus (`apps/cli/tests/fixtures/gen_multilingual_corpus.py`)
/// packs into its text fields, plus the secrets and paths the privacy test hunts for.
const MULTISCRIPT: &str = "ךخطأ في تشغيل الاختبار 测试失败了请修复这个问题🎉🚀🔥éf́ǵ";
const SECRETS: &[&str] = &[
    "sk-proj-abcdefghijklmnopqrstuvwxyz0123456789",
    "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123",
    "alice@example.com",
    "/Users/alice/code/secret-client",
    "/home/bob/work/acme-internal",
];

struct MapLoader(HashMap<String, AgentWorthTrace>);

impl TraceLoader for MapLoader {
    fn load(&self, session_id: &str) -> Option<AgentWorthTrace> {
        self.0.get(session_id).cloned()
    }
}

/// A trace that edited `abs_path` at `at` and, when `test_passed`, ran `cargo test` to exit 0.
/// Its text fields are full of things that must never reach a receipt.
fn make_trace(
    session_id: &str,
    adapter: &str,
    model: &str,
    repo_root: &Path,
    abs_path: &str,
    at: chrono::DateTime<Utc>,
    test_passed: bool,
) -> AgentWorthTrace {
    let slug = repo_root.to_string_lossy().replace('/', "-");
    let source_path = format!("/tmp/{adapter}/projects/{slug}/{session_id}.jsonl");
    let prov = Provenance::new(source_path, adapter, 100, 100, format!("fp_{session_id}"));
    let mut trace = AgentWorthTrace::new(session_id, adapter, prov, at - Duration::minutes(30));
    trace.ended_at = Some(at + Duration::minutes(1));
    trace.stats.models_used = vec![model.to_string()];
    trace.stats.token_usage = TokenUsage {
        input_tokens: 1_000,
        output_tokens: 500,
        cache_read_tokens: 10_000,
        cache_creation_tokens: 0,
    };
    let leaky = format!("{MULTISCRIPT} {}", SECRETS.join(" "));
    trace.events.push(NormalizedEvent::new(
        1,
        at - Duration::minutes(20),
        EventPayload::UserMessage {
            content: format!("please fix {leaky}"),
        },
    ));
    trace.events.push(NormalizedEvent::new(
        2,
        at,
        EventPayload::FileAction {
            path: abs_path.to_string(),
            action: FileActionType::Edit,
            diff: Some(format!("+ {leaky}")),
            lines_changed: Some(1),
        },
    ));
    if test_passed {
        trace.events.push(NormalizedEvent::new(
            3,
            at,
            EventPayload::ShellCommand(ShellCommand {
                command: format!("cargo test -p secret --features {}", SECRETS[0]),
                cwd: Some(SECRETS[3].to_string()),
                exit_code: Some(0),
                output: Some(format!("test result: ok. {leaky}")),
            }),
        ));
    }
    trace.events.push(NormalizedEvent::new(
        4,
        at,
        EventPayload::AssistantMessage {
            content: format!("I have completed the task. {leaky}"),
            thinking: Some(leaky.clone()),
        },
    ));
    trace
}

fn seed(storage: &Storage, trace: &AgentWorthTrace, outcome: &str) {
    storage
        .upsert_session(trace, Some(outcome), Some(0.5), 7)
        .expect("seed session");
}

fn opts(root: &Path) -> BuildOptions {
    BuildOptions {
        query: SuspectQuery {
            repo: root.to_path_buf(),
            max_commits: 50,
            ..SuspectQuery::default()
        },
        include_spend: false,
        now: Utc::now(),
    }
}

fn test_key() -> ReceiptSigningKey {
    ReceiptSigningKey::from_seed(&[7u8; 32])
}

/// Two commits, one by a Claude session that passed tests and one by a Codex session that
/// only edited. Returns (repo, storage, loader, [claude_commit, codex_commit]).
fn two_harness_fixture() -> (TempRepo, Storage, MapLoader, [String; 2]) {
    let repo = TempRepo::new();
    let storage = Storage::open_in_memory().expect("storage");
    let root = repo.path();
    let at = Utc::now() - Duration::minutes(5);

    let claude = make_trace(
        "11111111-aaaa-4bbb-8ccc-000000000001",
        "claude_code",
        "claude-opus-4-5",
        &root,
        &format!("{}/src/lib.rs", root.display()),
        at,
        true,
    );
    let codex = make_trace(
        "22222222-aaaa-4bbb-8ccc-000000000002",
        "codex",
        "gpt-5-codex",
        &root,
        &format!("{}/src/app.rs", root.display()),
        at,
        false,
    );
    seed(&storage, &claude, "test_or_build_passed");
    seed(&storage, &codex, "artifact_changed");
    let c1 = repo.commit_file("src/lib.rs", "fn a() {}\n", "feat: a");
    let c2 = repo.commit_file("src/app.rs", "fn b() {}\n", "feat: b");
    let loader = MapLoader(
        [claude, codex]
            .into_iter()
            .map(|t| (t.session_id.clone(), t))
            .collect(),
    );
    (repo, storage, loader, [c1, c2])
}

#[test]
fn builds_a_valid_receipt_over_the_range_with_refs_not_ids() {
    let (repo, storage, loader, [c1, c2]) = two_harness_fixture();
    let key = test_key();
    let built = build_receipt(&storage, &loader, &key.public_key(), &opts(&repo.path()))
        .expect("build")
        .expect("non-empty range");
    let st = &built.statement;
    st.validate().expect("valid statement");

    // No upstream, no origin/main: the last-N fallback, named by the enum.
    assert_eq!(st.predicate.range.rule, RangeRule::LastN);
    assert_eq!(st.predicate.range.head, c2);
    assert_eq!(st.predicate.range.base, None);
    assert_eq!(st.predicate.range.commits, 2);
    assert!(st.covers_commit(&c1) && st.covers_commit(&c2));
    assert_eq!(built.head_sha, c2);

    assert_eq!(st.predicate.sessions.len(), 2);
    let json = String::from_utf8(st.canonical_bytes().unwrap()).unwrap();
    for id in built.session_ids_by_ref.values() {
        assert!(!json.contains(id.as_str()), "raw session id leaked: {id}");
    }
    let claude = st
        .predicate
        .sessions
        .iter()
        .find(|s| s.adapter == "claude_code")
        .expect("claude session");
    assert_eq!(claude.highest_rung, Some(3));
    let test_rung = claude
        .rungs
        .iter()
        .find(|r| r.kind == OutcomeKind::TestOrBuildPassed)
        .expect("rung 3");
    assert_eq!(test_rung.source, EvidenceSource::ShellExit0);
    assert_eq!(test_rung.command_class, Some(CommandClass::CargoTest));
    assert!(
        test_rung.verified_locally,
        "an exit code captured as 0 is ground truth"
    );
    assert_eq!(test_rung.event_seq, 3);
    let claim = claude
        .rungs
        .iter()
        .find(|r| r.kind == OutcomeKind::DoneClaimed)
        .expect("rung 1");
    assert!(
        !claim.verified_locally,
        "the assistant's word never verifies"
    );
    assert_eq!(claude.spend_micro_usd, None, "tokens only by default");
    assert_eq!(claude.tokens_total, 11_500);
    assert!(
        !claude.risk.known,
        "no session_risk row: unknown, not clean"
    );
    assert_eq!(st.predicate.index.parser_version_min, 7);

    let commit = st.predicate.commits.iter().find(|c| c.sha == c1).unwrap();
    assert_eq!(commit.files, vec!["src/lib.rs".to_string()]);
    assert_eq!(commit.anchor, Some(CommitAnchor::Absolute));
    assert_eq!(commit.sessions, vec![claude.session_ref.clone()]);
}

#[test]
fn privacy_no_prompt_text_secret_email_or_home_path_reaches_the_receipt() {
    let (repo, storage, loader, _) = two_harness_fixture();
    let key = test_key();
    let mut o = opts(&repo.path());
    o.include_spend = true;
    let built = build_receipt(&storage, &loader, &key.public_key(), &o)
        .unwrap()
        .unwrap();
    let signed = sign_statement(&built.statement, &key).unwrap();
    let payload = String::from_utf8(built.statement.canonical_bytes().unwrap()).unwrap();
    let envelope = String::from_utf8(signed.envelope.to_json()).unwrap();
    for needle in SECRETS
        .iter()
        .copied()
        .chain([
            "测试失败",
            "خطأ",
            "🎉",
            "please fix",
            "I have completed",
            "cargo test -p",
        ])
        .chain([repo.path().to_string_lossy().as_ref()])
    {
        assert!(
            !payload.contains(needle),
            "{needle:?} leaked into the payload"
        );
        assert!(
            !envelope.contains(needle),
            "{needle:?} leaked into the envelope"
        );
    }
    // The command survived only as its class.
    assert!(payload.contains("\"command_class\":\"cargo test\""));
}

#[test]
fn spend_is_only_present_with_include_spend() {
    let (repo, storage, loader, _) = two_harness_fixture();
    let key = test_key();
    let mut o = opts(&repo.path());
    let without = build_receipt(&storage, &loader, &key.public_key(), &o)
        .unwrap()
        .unwrap();
    assert!(without
        .statement
        .predicate
        .sessions
        .iter()
        .all(|s| s.spend_micro_usd.is_none()));
    o.include_spend = true;
    let with = build_receipt(&storage, &loader, &key.public_key(), &o)
        .unwrap()
        .unwrap();
    assert!(with
        .statement
        .predicate
        .sessions
        .iter()
        .all(|s| s.spend_micro_usd.is_some()));
    assert!(reveals(&with.statement, &key.public_key())
        .iter()
        .any(|l| l.contains("--include-spend")));
}

#[test]
fn fail_closed_a_free_text_leak_into_a_structured_field_aborts_with_its_field() {
    let (repo, storage, loader, _) = two_harness_fixture();
    let key = test_key();
    let mut built = build_receipt(&storage, &loader, &key.public_key(), &opts(&repo.path()))
        .unwrap()
        .unwrap();
    // Inject free text the projection would never produce.
    built.statement.predicate.sessions[0].models[0] =
        "claude reading /Users/alice/code/secret-client/main.rs".to_string();
    let err = check_publication_safe(&built.statement).expect_err("must trip");
    assert_eq!(err.field, "/predicate/sessions/0/models/0");
}

#[test]
fn fail_closed_a_secret_shaped_file_name_in_the_diff_refuses_to_build() {
    let repo = TempRepo::new();
    let storage = Storage::open_in_memory().unwrap();
    let root = repo.path();
    let rel = "notes/sk-proj-abcdefghijklmnopqrstuvwxyz0123456789.txt";
    let at = Utc::now() - Duration::minutes(5);
    let t = make_trace(
        "33333333-aaaa-4bbb-8ccc-000000000003",
        "claude_code",
        "claude-opus-4-5",
        &root,
        &format!("{}/{rel}", root.display()),
        at,
        false,
    );
    seed(&storage, &t, "artifact_changed");
    repo.commit_file(rel, "x\n", "docs: note");
    let loader = MapLoader(HashMap::new());
    let err = build_receipt(&storage, &loader, &test_key().public_key(), &opts(&root))
        .expect_err("a path the redactor rewrites must not be signed");
    let leak = err.downcast_ref::<LeakError>().expect("LeakError");
    assert_eq!(leak.field, "/predicate/commits/0/files/0");
}

#[test]
fn multi_harness_one_commit_carries_both_refs_and_the_highest_rung_wins() {
    let repo = TempRepo::new();
    let storage = Storage::open_in_memory().unwrap();
    let root = repo.path();
    let at = Utc::now() - Duration::minutes(5);
    let path = format!("{}/src/shared.rs", root.display());
    let claude = make_trace(
        "aaaa0000-0000-4000-8000-00000000000a",
        "claude_code",
        "claude-opus-4-5",
        &root,
        &path,
        at,
        false,
    );
    let codex = make_trace(
        "bbbb0000-0000-4000-8000-00000000000b",
        "codex",
        "gpt-5-codex",
        &root,
        &path,
        at,
        true,
    );
    seed(&storage, &claude, "artifact_changed");
    seed(&storage, &codex, "test_or_build_passed");
    let sha = repo.commit_file("src/shared.rs", "fn s() {}\n", "feat: shared");
    let loader = MapLoader(
        [claude, codex]
            .into_iter()
            .map(|t| (t.session_id.clone(), t))
            .collect(),
    );
    let built = build_receipt(&storage, &loader, &test_key().public_key(), &opts(&root))
        .unwrap()
        .unwrap();
    let commit = &built.statement.predicate.commits[0];
    assert_eq!(commit.sha, sha);
    assert_eq!(commit.sessions.len(), 2, "both harnesses are named");
    let predicted = predict(&built.statement);
    assert_eq!(predicted[0].highest_attested_rung, Some(3));
    assert!(
        predicted[0].claimed_only_rungs.is_empty(),
        "a claim below the attested rung adds nothing: {:?}",
        predicted[0].claimed_only_rungs
    );
}

#[test]
fn an_empty_range_builds_nothing() {
    let repo = TempRepo::new();
    let storage = Storage::open_in_memory().unwrap();
    let sha = repo.commit_file("a.txt", "a\n", "init");
    let mut o = opts(&repo.path());
    o.query.base = Some(sha);
    let built = build_receipt(&storage, &NoTraces, &test_key().public_key(), &o).unwrap();
    assert!(built.is_none());
}

#[test]
fn an_explicit_base_names_the_rule_and_the_base_sha() {
    let repo = TempRepo::new();
    let storage = Storage::open_in_memory().unwrap();
    let base = repo.commit_file("a.txt", "a\n", "init");
    let head = repo.commit_file("b.txt", "b\n", "next");
    let mut o = opts(&repo.path());
    o.query.base = Some(base.clone());
    let built = build_receipt(&storage, &NoTraces, &test_key().public_key(), &o)
        .unwrap()
        .unwrap();
    let r = &built.statement.predicate.range;
    assert_eq!(r.rule, RangeRule::ExplicitBase);
    assert_eq!(r.base.as_deref(), Some(base.as_str()));
    assert_eq!(r.head, head);
    // An unattributed commit is still a subject, with no sessions and no anchor.
    let c = &built.statement.predicate.commits[0];
    assert!(c.sessions.is_empty() && c.files.is_empty() && c.anchor.is_none());
}

#[test]
fn rungs_are_capped_per_kind_so_a_long_session_stays_under_the_size_cap() {
    let repo = TempRepo::new();
    let root = repo.path();
    let at = Utc::now();
    let mut t = make_trace(
        "cccc0000-0000-4000-8000-00000000000c",
        "claude_code",
        "m",
        &root,
        "/x",
        at,
        false,
    );
    for i in 0..500u64 {
        t.events.push(NormalizedEvent::new(
            100 + i,
            at,
            EventPayload::FileAction {
                path: format!("{}/f{i}.rs", root.display()),
                action: FileActionType::Edit,
                diff: None,
                lines_changed: None,
            },
        ));
    }
    let rungs = crate::commands::receipt::work_rungs(&t);
    let edits = rungs
        .iter()
        .filter(|r| r.kind == OutcomeKind::ArtifactChanged)
        .count();
    assert_eq!(edits, crate::commands::receipt::WORK_RUNGS_PER_KIND);
}

#[test]
fn risk_counters_and_governor_halts_are_carried_as_integers() {
    let (repo, storage, loader, _) = two_harness_fixture();
    let id = "11111111-aaaa-4bbb-8ccc-000000000001";
    storage
        .upsert_session_risk(&SessionRisk {
            session_id: id.to_string(),
            loop_alerts: 2,
            recoveries: 1,
            ..Default::default()
        })
        .unwrap();
    storage
        .insert_governor_event(&agentworth_storage::GovernorEventRow {
            id: None,
            session_id: id.to_string(),
            at: Utc::now(),
            seq: Some(3),
            rule: "thrash".to_string(),
            action: "halt".to_string(),
            reason: format!("free text {}", SECRETS[3]),
            evidence: Some(SECRETS[2].to_string()),
        })
        .unwrap();
    let built = build_receipt(
        &storage,
        &loader,
        &test_key().public_key(),
        &opts(&repo.path()),
    )
    .unwrap()
    .unwrap();
    let s = built
        .statement
        .predicate
        .sessions
        .iter()
        .find(|s| s.adapter == "claude_code")
        .unwrap();
    assert!(s.risk.known);
    assert_eq!(s.risk.loop_alerts, 2);
    assert_eq!(s.risk.recoveries, 1);
    assert_eq!(s.risk.governor_halts, 1);
    let json = String::from_utf8(built.statement.canonical_bytes().unwrap()).unwrap();
    assert!(
        !json.contains("free text"),
        "only the count of a halt, never its reason"
    );
}

#[test]
fn notes_transport_round_trips_and_verifies_offline() {
    let (repo, storage, loader, [_, head]) = two_harness_fixture();
    let key = test_key();
    let built = build_receipt(&storage, &loader, &key.public_key(), &opts(&repo.path()))
        .unwrap()
        .unwrap();
    let signed = sign_statement(&built.statement, &key).unwrap();
    write_note(&repo.path(), &built.head_sha, &signed.envelope).unwrap();
    let envelopes = read_note(&repo.path(), &head).unwrap();
    assert_eq!(envelopes.len(), 1);
    let v = verify_envelope(&envelopes[0], &[]);
    assert!(matches!(v.verdict, SignatureVerdict::KeyNotLinked { .. }));
    assert_eq!(v.receipt.unwrap().receipt_id, signed.receipt_id);

    // Re-signing the same tip replaces this machine's note rather than stacking it.
    write_note(&repo.path(), &built.head_sha, &signed.envelope).unwrap();
    assert_eq!(read_note(&repo.path(), &head).unwrap().len(), 1);

    // `show` by id prefix and by commit both find it, and map refs back to local sessions.
    let public = key.public_key();
    for target in [signed.receipt_id.get(..12).unwrap(), head.as_str()] {
        let shown = show_receipts(&storage, &repo.path(), target, Some(&public)).unwrap();
        assert_eq!(shown.len(), 1, "target {target}");
        let r = &shown[0];
        assert_eq!(r.signature, "key_not_linked");
        assert!(r.signed_by_this_machine);
        assert_eq!(r.commits_present, 2);
        assert!(r.sessions.values().all(Option::is_some), "{:?}", r.sessions);
    }
    // Another machine's key cannot map refs back.
    let other = ReceiptSigningKey::from_seed(&[9u8; 32]).public_key();
    let shown = show_receipts(&storage, &repo.path(), &head, Some(&other)).unwrap();
    assert!(!shown[0].signed_by_this_machine);
    assert!(shown[0].sessions.values().all(Option::is_none));
}

#[test]
fn file_transport_writes_one_envelope_named_by_receipt_id() {
    let (repo, storage, loader, _) = two_harness_fixture();
    let key = test_key();
    let built = build_receipt(&storage, &loader, &key.public_key(), &opts(&repo.path()))
        .unwrap()
        .unwrap();
    let signed = sign_statement(&built.statement, &key).unwrap();
    let path = write_receipt_file(&repo.path(), &signed.receipt_id, &signed.envelope).unwrap();
    assert_eq!(
        path,
        repo.path()
            .join(RECEIPT_DIR)
            .join(format!("{}.dsse.json", signed.receipt_id))
    );
    let shown = show_receipts(&storage, &repo.path(), signed.receipt_id.get(..8).unwrap(), None).unwrap();
    assert_eq!(shown.len(), 1);
    assert!(shown[0].source.starts_with(RECEIPT_DIR));
}

#[test]
fn a_tampered_note_shows_as_invalid() {
    let (repo, storage, loader, [_, head]) = two_harness_fixture();
    let key = test_key();
    let built = build_receipt(&storage, &loader, &key.public_key(), &opts(&repo.path()))
        .unwrap()
        .unwrap();
    let mut env = sign_statement(&built.statement, &key).unwrap().envelope;
    // Flip one payload byte: the signature no longer covers it.
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut payload = b64.decode(&env.payload).unwrap();
    payload[10] ^= 0x01;
    env.payload = b64.encode(payload);
    write_note(&repo.path(), &head, &env).unwrap();
    let shown = show_receipts(&storage, &repo.path(), &head, None).unwrap();
    assert!(
        shown[0].signature.starts_with("invalid"),
        "{}",
        shown[0].signature
    );
    assert!(shown[0].statement.is_none());
}

/// A bare remote, a clone, and a second clone that pushes notes first.
#[test]
fn push_notes_lands_on_the_remote_and_merges_a_concurrent_writer_once() {
    let remote = tempfile::tempdir().unwrap();
    git_in(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
    let a = TempRepo::new();
    a.commit_file("a.txt", "a\n", "init");
    let remote_url = remote.path().to_string_lossy().to_string();
    a.git(&["remote", "add", "origin", &remote_url]);
    a.git(&["push", "-q", "origin", "main"]);

    let b_dir = tempfile::tempdir().unwrap();
    git_in(b_dir.path(), &["clone", "-q", &remote_url, "."]);
    git_in(b_dir.path(), &["config", "user.email", "b@example.invalid"]);
    git_in(b_dir.path(), &["config", "user.name", "B"]);
    let b_tip = git_in(b_dir.path(), &["rev-parse", "HEAD"]);

    let key = test_key();
    let env_for = |text: &str| agentworth_attest::sign_payload("text/plain", text.as_bytes(), &key);

    // B writes and pushes a note first.
    write_note(b_dir.path(), &b_tip, &env_for("from-b")).unwrap();
    let pb = push_notes(b_dir.path(), "origin").unwrap();
    assert!(pb.pushed && !pb.merged, "{pb:?}");

    // A writes its own note on another commit; its push is rejected, then merged, then lands.
    let a_tip = a.commit_file("b.txt", "b\n", "next");
    a.git(&["push", "-q", "origin", "main"]);
    write_note(&a.path(), &a_tip, &env_for("from-a")).unwrap();
    let pa = push_notes(&a.path(), "origin").unwrap();
    assert!(pa.pushed && pa.merged, "{pa:?}");

    let notes = git_in(remote.path(), &["notes", "--ref", NOTES_REF_NAME, "list"]);
    assert_eq!(
        notes.lines().count(),
        2,
        "both writers' notes survive: {notes}"
    );
    // The temporary remote-tracking ref is cleaned up.
    assert!(Command::new("git")
        .arg("-C")
        .arg(a.path())
        .args(["rev-parse", "--verify", "--quiet", NOTES_REMOTE_TMP])
        .output()
        .map(|o| !o.status.success())
        .unwrap());
}

#[test]
fn a_failing_notes_push_is_reported_not_raised() {
    let a = TempRepo::new();
    let tip = a.commit_file("a.txt", "a\n", "init");
    a.git(&[
        "remote",
        "add",
        "origin",
        "/nonexistent/agentworth-remote.git",
    ]);
    let key = test_key();
    write_note(
        &a.path(),
        &tip,
        &agentworth_attest::sign_payload("text/plain", b"x", &key),
    )
    .unwrap();
    let p = push_notes(&a.path(), "origin").expect("a failed push is an outcome, not an error");
    assert!(!p.pushed);
    assert!(p.error.is_some());
}

#[test]
fn a_remote_that_looks_like_a_flag_is_rejected() {
    let a = TempRepo::new();
    a.commit_file("a.txt", "a\n", "init");
    assert!(push_notes(&a.path(), "--receive-pack=evil").is_err());
}

#[test]
fn key_is_created_once_with_private_permissions_and_a_matching_pub_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keys").join("receipt_ed25519");
    let (k1, created) = load_or_create_key(&path).unwrap();
    assert!(created);
    let (k2, created_again) = load_or_create_key(&path).unwrap();
    assert!(!created_again);
    assert_eq!(k1.public_key(), k2.public_key());
    assert_eq!(read_public_key(&path).unwrap(), Some(k1.public_key()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dir_mode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
    }
    let gh = github_signing_key_command(&path);
    assert!(
        gh.starts_with("gh ssh-key add ") && gh.contains("--type signing"),
        "{gh}"
    );
    assert!(gh.contains("receipt_ed25519.pub"));
}

#[test]
fn consent_is_per_repository_and_recorded_once() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("config.toml");
    let a = Path::new("/tmp/repo-a");
    let b = Path::new("/tmp/repo-b");
    record_consent(&cfg_path, a).unwrap();
    record_consent(&cfg_path, a).unwrap();
    let cfg = crate::app::config::load_config_from(&cfg_path).unwrap();
    assert!(has_consent(&cfg, a));
    assert!(!has_consent(&cfg, b));
    assert_eq!(cfg.receipts.unwrap().consented_repos.len(), 1);
}

#[test]
fn init_write_never_overwrites_an_existing_hook_or_workflow() {
    let repo = TempRepo::new();
    repo.commit_file("a.txt", "a\n", "init");
    let root = repo.path();
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("receipt_ed25519");
    let cfg = dir.path().join("config.toml");

    let hook = root.join(".git/hooks/pre-push");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, "#!/bin/sh\necho mine\n").unwrap();

    let report = init_write(&root, &key, &cfg).unwrap();
    assert_eq!(
        std::fs::read_to_string(&hook).unwrap(),
        "#!/bin/sh\necho mine\n"
    );
    assert!(report.hook.contains("left as is"), "{}", report.hook);
    assert!(report.workflow.starts_with("wrote"));
    assert_eq!(
        std::fs::read_to_string(root.join(WORKFLOW_PATH)).unwrap(),
        WORKFLOW_YAML
    );
    assert!(report.key_created && report.consent_recorded);

    // A second run changes nothing and creates no second key.
    std::fs::remove_file(&hook).unwrap();
    let again = init_write(&root, &key, &cfg).unwrap();
    assert!(!again.key_created);
    assert!(again.workflow.contains("already exists"));
    assert_eq!(again.hook, "wrote the pre-push hook");
    let written = std::fs::read_to_string(&hook).unwrap();
    assert_eq!(written, suspect::PRE_PUSH_HOOK);
    let cfg = crate::app::config::load_config_from(&cfg).unwrap();
    assert!(has_consent(&cfg, &root));
}

#[test]
fn the_workflow_matches_the_spec_and_the_hook_carries_the_receipt_line() {
    assert!(WORKFLOW_YAML.contains("uses: unfoundbox-crew/agentworth@v0"));
    assert!(WORKFLOW_YAML.contains("checks: write"));
    assert!(!WORKFLOW_YAML.contains("pull_request_target"));
    let hook = suspect::PRE_PUSH_HOOK;
    assert!(hook.contains("archie repo receipt create --push --quiet"));
    assert!(hook.contains("|| true"));
    assert!(hook.contains("AGENTWORTH_RECEIPT_PUSH"));
    assert!(hook.trim_end().ends_with("exit 0"));
}

/// The hook script itself, run by `sh` with nothing on PATH but a fake `agentworth` and a
/// fake `archie` that both fail: it still exits 0. Then with no archie at all.
#[cfg(unix)]
#[test]
fn hook_exits_zero_when_archie_fails_or_is_missing() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let hook = dir.path().join("pre-push");
    std::fs::write(&hook, suspect::PRE_PUSH_HOOK).unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for name in ["agentworth", "archie"] {
        let p = bin.join(name);
        std::fs::write(&p, "#!/bin/sh\necho boom >&2\nexit 3\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let run = |path: &str, guard: bool| {
        let mut c = Command::new("/bin/sh");
        c.arg(&hook)
            .arg("origin")
            .arg("/dev/null")
            .env("PATH", path);
        if guard {
            c.env(PUSH_GUARD_ENV, "1");
        } else {
            c.env_remove(PUSH_GUARD_ENV);
        }
        c.output().unwrap()
    };
    let failing = run(&bin.to_string_lossy(), false);
    assert_eq!(failing.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&failing.stderr).contains("boom"),
        "archie did run"
    );

    let empty = dir.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert_eq!(run(&empty.to_string_lossy(), false).status.code(), Some(0));

    // The nested-push guard short-circuits before archie is even looked up.
    let guarded = run(&bin.to_string_lossy(), true);
    assert_eq!(guarded.status.code(), Some(0));
    assert!(guarded.stderr.is_empty(), "guard must skip everything");
}

#[test]
fn preview_without_a_key_uses_a_placeholder_and_says_so() {
    let (repo, storage, loader, _) = two_harness_fixture();
    let p = preview_receipt(&storage, &loader, None, &|_| false, &opts(&repo.path())).unwrap();
    assert!(!p.key_present && !p.consent_recorded);
    assert_eq!(p.commits_in_range, 2);
    assert!(p.summary.contains("rung 3 attested"), "{}", p.summary);
    assert!(p.summary.contains("rung 1 claimed only") || p.summary.contains("claimed only"));
    assert_eq!(p.sessions_local.len(), 2);
    assert_eq!(p.predicted.len(), 2);
    let codex = p
        .predicted
        .iter()
        .find(|c| c.highest_attested_rung != Some(3))
        .expect("the codex commit");
    assert!(codex.highest_attested_rung.unwrap_or(0) < 3);
}

#[test]
fn summary_line_matches_the_demo_wording() {
    let (repo, storage, loader, _) = two_harness_fixture();
    let built = build_receipt(
        &storage,
        &loader,
        &test_key().public_key(),
        &opts(&repo.path()),
    )
    .unwrap()
    .unwrap();
    let line = summary_line("7c41e0a9ffffffff", &built);
    assert_eq!(line, "receipt 7c41e0a9 signed, 2 sessions, rung 3 attested");
}
