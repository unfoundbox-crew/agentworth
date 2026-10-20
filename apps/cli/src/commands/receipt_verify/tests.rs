//! Spec §6.2 for PR 3: golden render, tamper and wrong-key states, the rung matrix (pure and
//! against real temp git repos), multi-harness, conclusion policy and input parsing.

use std::path::{Path, PathBuf};
use std::process::Command;

use agentworth_attest::{sign_payload, sign_statement, ReceiptSigningKey};
use agentworth_schema::work_receipt::{
    example, session_ref, CommitAttribution, Signer, Subject, WorkReceiptStatement,
    IN_TOTO_PAYLOAD_TYPE,
};
use base64::engine::general_purpose::STANDARD;

use super::*;

/// RFC 8032 §7.1 TEST 1 secret key: public test vector, never a real signer. Its public key is
/// `example::SIGNER_PUBKEY`.
const TEST1_SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

/// Uncovered fourth commit of the golden PR.
const EXTRA: &str = "fedcba9876543210fedcba9876543210fedcba98";

fn test1() -> ReceiptSigningKey {
    ReceiptSigningKey::from_seed(&TEST1_SEED)
}

fn other_key() -> ReceiptSigningKey {
    ReceiptSigningKey::from_seed(&[7u8; 32])
}

fn link(login: &str, key: &ReceiptSigningKey) -> KeyLink {
    KeyLink {
        login: login.into(),
        key: key.public_key(),
    }
}

/// The example statement re-targeted at `key` (new signer, re-derived session refs).
fn statement_for(key: &ReceiptSigningKey) -> WorkReceiptStatement {
    let mut s = example::statement();
    let pk = key.public_key();
    s.predicate.signer = Signer {
        public_key: pk.to_openssh(),
    };
    let old: Vec<String> = s
        .predicate
        .sessions
        .iter()
        .map(|x| x.session_ref.clone())
        .collect();
    let new: Vec<String> = example::SESSION_IDS
        .iter()
        .map(|id| session_ref(&pk.raw(), id))
        .collect();
    for (sess, r) in s.predicate.sessions.iter_mut().zip(&new) {
        sess.session_ref.clone_from(r);
    }
    for c in &mut s.predicate.commits {
        for r in &mut c.sessions {
            if let Some(i) = old.iter().position(|o| o == r) {
                r.clone_from(&new[i]);
            }
        }
    }
    s
}

/// A statement over arbitrary commits: `(sha, files, session indexes into the example's two
/// sessions: 0 = claude_code, 1 = codex)`. Newest commit first.
fn statement_over(
    key: &ReceiptSigningKey,
    base: &str,
    commits: &[(&str, &[&str], &[usize])],
) -> WorkReceiptStatement {
    let mut s = statement_for(key);
    let refs: Vec<String> = s
        .predicate
        .sessions
        .iter()
        .map(|x| x.session_ref.clone())
        .collect();
    s.subject = commits
        .iter()
        .map(|(sha, _, _)| Subject::commit(*sha))
        .collect();
    s.predicate.commits = commits
        .iter()
        .map(|(sha, files, sessions)| CommitAttribution {
            sha: (*sha).into(),
            sessions: sessions.iter().map(|i| refs[*i].clone()).collect(),
            files: files.iter().map(|f| (*f).into()).collect(),
            anchor: None,
        })
        .collect();
    s.predicate.range.base = Some(base.into());
    s.predicate.range.head = commits[0].0.into();
    s.predicate.range.commits = commits.len() as u32;
    s
}

fn envelope_bytes(st: &WorkReceiptStatement, key: &ReceiptSigningKey) -> Vec<u8> {
    sign_statement(st, key).expect("sign").envelope.to_json()
}

fn note(bytes: Vec<u8>) -> ReceiptSource {
    ReceiptSource {
        transport: Transport::Note,
        bytes,
    }
}

fn pr_commit(sha: &str, subject: &str, files: &[&str]) -> PrCommit {
    PrCommit {
        sha: sha.into(),
        subject: subject.into(),
        files: files.iter().map(|f| (*f).into()).collect(),
    }
}

/// The golden PR: the example receipt's three commits plus one no receipt covers.
fn golden_commits() -> Vec<PrCommit> {
    vec![
        pr_commit(
            example::HEAD,
            "Fix scan path isolation",
            &["crates/core/src/turns.rs", "crates/core/src/lib.rs"],
        ),
        pr_commit(
            example::MIDDLE,
            "Add receipt schema",
            &["crates/schema/src/work_receipt.rs"],
        ),
        pr_commit(example::FIRST, "Bump deps", &["Cargo.lock"]),
        pr_commit(EXTRA, "Tweak | the *docs* <br>", &["README.md"]),
    ]
}

fn green(n: usize) -> CiInput {
    CiInput {
        head_sha: None,
        entries: vec![CiEntryState::Passed; n],
    }
}

fn golden_input() -> VerifyInput {
    VerifyInput {
        base: example::BASE.into(),
        head: example::HEAD.into(),
        commits: golden_commits(),
        receipts: vec![note(envelope_bytes(&example::statement(), &test1()))],
        ci: Some(green(3)),
        keys: vec![link("example-dev", &test1())],
        fail_on: vec![],
    }
}

fn row<'a>(r: &'a VerifyReport, sha: &str) -> &'a CommitReport {
    r.commits.iter().find(|c| c.sha == sha).expect("row")
}

fn assert_glyphs(s: &str) {
    for c in s.chars() {
        let ok = c.is_ascii()
            || matches!(c as u32, 0x2500..=0x259F)
            || matches!(c, '●' | '○' | '·' | '—' | '→');
        assert!(
            ok,
            "glyph U+{:04X} ({c}) is outside the allowed set",
            c as u32
        );
    }
}

/// Compare with a checked-in golden file; `AGENTWORTH_BLESS=1` rewrites it.
fn golden(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/commands/receipt_verify/testdata")
        .join(name);
    if std::env::var_os("AGENTWORTH_BLESS").is_some() {
        std::fs::write(&path, actual).expect("bless golden");
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "missing golden {}; run with AGENTWORTH_BLESS=1",
            path.display()
        )
    });
    assert_eq!(
        actual, expected,
        "golden {name} changed; re-bless if intended"
    );
}

// ---------------------------------------------------------------------------------------------
// Golden
// ---------------------------------------------------------------------------------------------

#[test]
fn golden_markdown_and_check_json() {
    let r = verify(&golden_input());
    assert_eq!(r.highest_verified(), Some(5));
    assert_eq!(r.highest_attested(), Some(3));
    assert_eq!(r.signed_commits(), 3);
    assert_eq!(
        r.conclusion,
        Conclusion::Neutral,
        "one commit is unattributed"
    );
    assert_eq!(
        render_title(&r),
        "Rung 5 · CI verified · 3 of 4 commits carry a signed receipt"
    );
    let md = render_markdown(&r);
    let json = render_check_json(&r);
    assert!(md.contains("2 of 3 claimed files appear in their diffs"));
    assert_glyphs(&md);
    assert_glyphs(&json);
    golden("golden_main.md", &md);
    golden("golden_main.check.json", &json);
}

#[test]
fn golden_receipt_id_is_the_pr1_fixture() {
    let r = verify(&golden_input());
    assert_eq!(
        r.receipts[0].receipt_id.as_deref(),
        Some("e69b9f1b456cc59f28235be6a1de6b17b4cb614b3a36302abd73837ce4dee560"),
        "PR 1's pinned receipt_id for the main fixture"
    );
}

#[test]
fn golden_rows_rederive_each_rung() {
    let r = verify(&golden_input());
    let head = row(&r, example::HEAD);
    assert_eq!(
        head.verified,
        vec![
            VerifiedFact::Ci {
                passed: 3,
                total: 3
            },
            VerifiedFact::CommitInPr,
            VerifiedFact::FilesMatch {
                matched: 1,
                claimed: 1
            },
        ]
    );
    assert_eq!(head.attested.len(), 1);
    assert_eq!(head.claimed, vec![Claim::CompletionClaims(1)]);

    let middle = row(&r, example::MIDDLE);
    assert_eq!(middle.highest_verified(), Some(4));
    assert!(middle.verified.contains(&VerifiedFact::FilesMatch {
        matched: 1,
        claimed: 2
    }));
    assert!(middle.claimed.contains(&Claim::FilesNotInDiff(1)));
    assert!(
        middle.attested.is_empty(),
        "codex rung 2 is re-derived, not attested"
    );

    let first = row(&r, example::FIRST);
    assert!(first.sessions.is_empty());
    assert_eq!(first.highest_verified(), Some(4));

    assert_eq!(row(&r, EXTRA).coverage, Coverage::None);
}

// ---------------------------------------------------------------------------------------------
// Signature states: tamper and wrong key
// ---------------------------------------------------------------------------------------------

fn only_receipt(input: &VerifyInput) -> (VerifyReport, SignatureState) {
    let r = verify(input);
    assert_eq!(r.receipts.len(), 1, "{:?}", r.receipts);
    let sig = r.receipts[0].signature.clone();
    (r, sig)
}

#[test]
fn tamper_flip_one_payload_byte_drops_every_rung_to_claimed() {
    let mut env = sign_statement(&example::statement(), &test1())
        .unwrap()
        .envelope;
    let mut payload = STANDARD.decode(&env.payload).unwrap();
    // Flip one digit inside "tokens_total":1840000 so the JSON still parses.
    let pos = payload
        .windows(7)
        .position(|w| w == b"1840000")
        .expect("token count in payload");
    payload[pos] = b'2';
    env.payload = STANDARD.encode(&payload);
    let mut input = golden_input();
    input.receipts = vec![note(env.to_json())];
    let (r, sig) = only_receipt(&input);
    assert_eq!(sig, SignatureState::Invalid(InvalidKind::SignatureMismatch));
    for sha in [example::HEAD, example::MIDDLE, example::FIRST] {
        let c = row(&r, sha);
        assert_eq!(c.coverage, Coverage::InvalidOnly);
        assert!(c.verified.is_empty() && c.attested.is_empty());
        assert!(matches!(c.claimed[0], Claim::InvalidSignature { .. }));
    }
    assert_eq!(
        row(&r, example::HEAD).claimed,
        vec![Claim::InvalidSignature {
            rungs: 2,
            highest: Some(3)
        }]
    );
    assert_eq!(r.highest_verified(), None);
    assert_eq!(r.conclusion, Conclusion::Neutral);
    assert!(render_title(&r).starts_with("Signature invalid · "));
    input.fail_on = vec![FailOn::InvalidSignature];
    assert_eq!(verify(&input).conclusion, Conclusion::Failure);
}

#[test]
fn tamper_swap_a_subject_sha_is_invalid() {
    let env = sign_statement(&example::statement(), &test1())
        .unwrap()
        .envelope;
    let payload = String::from_utf8(STANDARD.decode(&env.payload).unwrap()).unwrap();
    let swapped = payload.replace(example::FIRST, EXTRA);
    let mut env2 = env.clone();
    env2.payload = STANDARD.encode(swapped.as_bytes());
    let mut input = golden_input();
    input.receipts = vec![note(env2.to_json())];
    let (r, sig) = only_receipt(&input);
    assert_eq!(sig, SignatureState::Invalid(InvalidKind::SignatureMismatch));
    // The swapped-in commit is only claimed, never verified.
    assert_eq!(row(&r, EXTRA).coverage, Coverage::InvalidOnly);
    assert_eq!(row(&r, example::FIRST).coverage, Coverage::None);
}

#[test]
fn resigned_with_another_key_is_valid_but_not_linked() {
    let st = statement_for(&other_key());
    let mut input = golden_input();
    input.receipts = vec![note(envelope_bytes(&st, &other_key()))];
    let (r, sig) = only_receipt(&input);
    assert!(matches!(sig, SignatureState::KeyNotLinked { .. }));
    assert_eq!(
        row(&r, example::HEAD).coverage,
        Coverage::Signed { linked: false }
    );
    assert_eq!(
        r.highest_verified(),
        Some(5),
        "integrity holds, rungs still re-derived"
    );
    let md = render_markdown(&r);
    assert!(md.contains("signed, key not linked"));
}

#[test]
fn wrong_key_signature_over_a_test1_payload_is_invalid() {
    // The payload names TEST 1 as signer, but the signature is by another key.
    let payload = example::statement().canonical_bytes().unwrap();
    let env = sign_payload(IN_TOTO_PAYLOAD_TYPE, &payload, &other_key());
    let mut input = golden_input();
    input.receipts = vec![note(env.to_json())];
    let (_, sig) = only_receipt(&input);
    assert_eq!(sig, SignatureState::Invalid(InvalidKind::SignatureMismatch));
}

#[test]
fn unreadable_note_is_invalid_and_never_covers() {
    let mut input = golden_input();
    input.receipts = vec![note(b"not an envelope".to_vec())];
    let (r, sig) = only_receipt(&input);
    assert_eq!(sig, SignatureState::Invalid(InvalidKind::Unreadable));
    assert!(r.commits.iter().all(|c| c.coverage == Coverage::None));
    assert!(render_markdown(&r).contains("Unreadable receipt data"));
}

#[test]
fn a_login_that_is_not_a_github_login_is_never_displayed() {
    let mut input = golden_input();
    input.keys = vec![link("<script>", &test1())];
    let (_, sig) = only_receipt(&input);
    assert!(matches!(sig, SignatureState::KeyNotLinked { .. }));
}

// ---------------------------------------------------------------------------------------------
// Rung matrix (pure)
// ---------------------------------------------------------------------------------------------

#[test]
fn no_receipt_reports_unattributed_and_ci() {
    let mut input = golden_input();
    input.receipts.clear();
    let r = verify(&input);
    assert!(r.commits.iter().all(|c| c.coverage == Coverage::None));
    assert_eq!(r.conclusion, Conclusion::Neutral);
    assert_eq!(
        render_title(&r),
        "0 of 4 commits carry a signed receipt · head CI 3/3 green"
    );
    let md = render_markdown(&r);
    assert!(md.contains("unattributed: no receipt covers this commit"));
    assert!(md.contains("npx -y agentworth@latest repo receipt init"));
}

#[test]
fn full_linked_coverage_and_green_ci_is_success() {
    let mut input = golden_input();
    input.commits.retain(|c| c.sha != EXTRA);
    let r = verify(&input);
    assert_eq!(r.conclusion, Conclusion::Success);
}

#[test]
fn ci_pending_red_and_unknown_never_reach_rung_5() {
    let cases = [
        (
            CiInput {
                head_sha: None,
                entries: vec![CiEntryState::Passed, CiEntryState::Pending],
            },
            "CI pending 1/2",
        ),
        (
            CiInput {
                head_sha: None,
                entries: vec![
                    CiEntryState::Passed,
                    CiEntryState::Failed,
                    CiEntryState::Pending,
                ],
            },
            "CI red 1/3 failed",
        ),
        (
            CiInput {
                head_sha: Some(example::MIDDLE.into()),
                entries: vec![CiEntryState::Passed],
            },
            "CI unknown",
        ),
        (CiInput::default(), "no CI checks"),
    ];
    for (ci, title) in cases {
        let mut input = golden_input();
        input.commits.retain(|c| c.sha != EXTRA);
        input.ci = Some(ci);
        let r = verify(&input);
        assert_eq!(r.highest_verified(), Some(4), "{title}");
        assert_eq!(r.conclusion, Conclusion::Neutral, "{title}");
        assert!(render_title(&r).ends_with(title), "{}", render_title(&r));
        assert!(!row(&r, example::MIDDLE)
            .verified
            .contains(&VerifiedFact::HeadCiCoversFinalTree));
    }
}

#[test]
fn fork_pr_without_ci_or_keys_still_renders_neutral() {
    // On a fork PR the Action has a read-only token: no check-run post, and it may not have
    // collected keys or CI. Verify must still produce a summary for the step summary.
    let mut input = golden_input();
    input.ci = None;
    input.keys.clear();
    let r = verify(&input);
    assert_eq!(r.conclusion, Conclusion::Neutral);
    assert!(matches!(
        r.receipts[0].signature,
        SignatureState::KeyNotLinked { .. }
    ));
    assert!(render_markdown(&r).contains("CI status not provided"));
}

#[test]
fn rebased_shas_leave_the_receipt_stale_and_commits_unattributed() {
    let mut input = golden_input();
    for (c, sha) in input.commits.iter_mut().zip([
        "1111111111111111111111111111111111111111",
        "2222222222222222222222222222222222222222",
        "3333333333333333333333333333333333333333",
        "4444444444444444444444444444444444444444",
    ]) {
        c.sha = sha.into();
    }
    input.head = "1111111111111111111111111111111111111111".into();
    let r = verify(&input);
    assert_eq!(r.stale_receipts, 1);
    assert!(r.receipts.is_empty());
    assert!(r.commits.iter().all(|c| c.coverage == Coverage::None));
    assert!(render_markdown(&r).contains("1 signed receipt names no commit in this PR"));
}

#[test]
fn files_mismatch_demotes_to_claimed() {
    let mut input = golden_input();
    input.commits[0].files = ["src/other.rs".to_string()].into_iter().collect();
    let r = verify(&input);
    let head = row(&r, example::HEAD);
    assert!(!head
        .verified
        .iter()
        .any(|f| matches!(f, VerifiedFact::FilesMatch { .. })));
    assert!(head.claimed.contains(&Claim::FilesNotInDiff(1)));
}

#[test]
fn truncated_receipt_without_files_makes_no_file_claim() {
    let mut st = statement_for(&test1());
    for c in &mut st.predicate.commits {
        c.files.clear();
    }
    st.predicate.truncated = true;
    let mut input = golden_input();
    input.receipts = vec![note(envelope_bytes(&st, &test1()))];
    let r = verify(&input);
    let head = row(&r, example::HEAD);
    assert!(!head
        .claimed
        .iter()
        .any(|c| matches!(c, Claim::FilesNotInDiff(_))));
    assert_eq!(head.highest_verified(), Some(5));
}

#[test]
fn multi_harness_commit_shows_both_sessions_and_highest_rung_wins() {
    let st = statement_over(
        &test1(),
        example::BASE,
        &[(example::HEAD, &["crates/core/src/turns.rs"], &[0, 1])],
    );
    let input = VerifyInput {
        base: example::BASE.into(),
        head: example::HEAD.into(),
        commits: vec![golden_commits().remove(0)],
        receipts: vec![note(envelope_bytes(&st, &test1()))],
        ci: Some(green(2)),
        keys: vec![link("example-dev", &test1())],
        fail_on: vec![],
    };
    let r = verify(&input);
    let head = row(&r, example::HEAD);
    let adapters: Vec<&str> = head.sessions.iter().map(|t| t.adapter.as_str()).collect();
    assert_eq!(adapters, vec!["claude_code", "codex"]);
    assert_eq!(head.highest_attested(), Some(3), "claude's test run");
    assert_eq!(head.highest_verified(), Some(5));
    assert_eq!(r.risk.sessions, 2);
    assert_eq!(r.risk.unknown_sessions, 1);
    assert_eq!(r.conclusion, Conclusion::Success);
}

#[test]
fn rungs_not_verified_locally_are_claims() {
    let mut st = statement_for(&test1());
    st.predicate.sessions[0].rungs[0].verified_locally = false;
    let mut input = golden_input();
    input.receipts = vec![note(envelope_bytes(&st, &test1()))];
    let r = verify(&input);
    let head = row(&r, example::HEAD);
    assert!(head.attested.is_empty());
    assert!(head
        .claimed
        .contains(&Claim::NotVerifiedLocally { rung: 3, count: 1 }));
    assert_eq!(r.highest_attested(), None);
    assert!(render_markdown(&r).contains("○ 3 test or build pass not re-checked locally"));
}

#[test]
fn unknown_adapters_render_as_other() {
    let mut st = statement_for(&test1());
    st.predicate.sessions[0].adapter = "evil | <b>adapter</b>".into();
    let mut input = golden_input();
    input.receipts = vec![note(envelope_bytes(&st, &test1()))];
    let md = render_markdown(&verify(&input));
    assert!(!md.contains("evil"));
    assert!(md.contains("other · "));
}

#[test]
fn note_with_merged_envelopes_and_duplicates() {
    let a = envelope_bytes(&example::statement(), &test1());
    let b = envelope_bytes(&statement_for(&other_key()), &other_key());
    let mut merged = a.clone();
    merged.push(b'\n');
    merged.extend_from_slice(&b);
    merged.push(b'\n');
    let mut input = golden_input();
    input.receipts = vec![
        note(merged),
        note(a.clone()),
        ReceiptSource {
            transport: Transport::File,
            bytes: a,
        },
    ];
    let r = verify(&input);
    assert_eq!(r.receipts.len(), 2, "duplicates collapse across transports");
    assert_eq!(
        row(&r, example::HEAD).coverage,
        Coverage::Signed { linked: true }
    );
}

#[test]
fn fail_on_policies() {
    assert_eq!(
        FailOn::parse_list("invalid-signature, below-rung:3 unattributed").unwrap(),
        vec![
            FailOn::InvalidSignature,
            FailOn::BelowRung(3),
            FailOn::Unattributed
        ]
    );
    assert!(FailOn::parse_list("below-rung:9").is_err());
    assert!(FailOn::parse_list("sometimes").is_err());
    assert!(FailOn::parse_list("").unwrap().is_empty());

    let mut input = golden_input();
    input.fail_on = vec![FailOn::Unattributed];
    let r = verify(&input);
    assert_eq!(r.conclusion, Conclusion::Failure);
    assert!(render_markdown(&r).contains("`fail-on: unattributed`"));

    input.fail_on = vec![FailOn::BelowRung(5)];
    assert_eq!(
        verify(&input).conclusion,
        Conclusion::Neutral,
        "rung 5 reached"
    );
    input.ci = None;
    assert_eq!(
        verify(&input).conclusion,
        Conclusion::Failure,
        "best is rung 4"
    );
    input.receipts.clear();
    input.fail_on = vec![FailOn::BelowRung(1)];
    assert_eq!(
        verify(&input).conclusion,
        Conclusion::Failure,
        "no rung at all"
    );
}

#[test]
fn empty_pr_is_neutral() {
    let mut input = golden_input();
    input.commits.clear();
    let r = verify(&input);
    assert_eq!(r.conclusion, Conclusion::Neutral);
    assert!(render_markdown(&r).contains("no commits"));
}

#[test]
fn summary_stays_under_the_github_limit() {
    let mut input = golden_input();
    input.receipts.clear();
    input.commits = (0..MAX_PR_COMMITS)
        .map(|i| pr_commit(&format!("{i:040x}"), &"x".repeat(500), &[]))
        .collect();
    let md = render_markdown(&verify(&input));
    assert!(md.chars().count() <= render::MAX_SUMMARY_CHARS);
}

#[test]
fn escape_cell_neutralizes_markdown() {
    assert_eq!(
        render::escape_cell("a|b *c* <d> @e `f`"),
        "a\\|b \\*c\\* \\<d\\> \\@e \\`f\\`"
    );
    assert_eq!(render::escape_cell(&"y".repeat(70)).chars().count(), 63);
}

// ---------------------------------------------------------------------------------------------
// Input parsing
// ---------------------------------------------------------------------------------------------

#[test]
fn ci_json_accepts_gh_shapes_and_ignores_itself() {
    let raw = br#"{
      "head_sha": "ABCDEF",
      "check_runs": {"total_count": 3, "check_runs": [
        {"name": "test", "status": "completed", "conclusion": "success"},
        {"name": "lint", "status": "completed", "conclusion": "skipped"},
        {"name": "AgentWorth / Evidence", "status": "in_progress", "conclusion": null}
      ]},
      "statuses": {"state": "pending", "statuses": [{"context": "deploy", "state": "pending"}]}
    }"#;
    let ci = CiInput::from_json(raw).unwrap();
    assert_eq!(ci.head_sha.as_deref(), Some("abcdef"));
    assert_eq!(
        CiState::of(Some(&ci), "abcdef"),
        CiState::Pending { done: 2, total: 3 }
    );
    let flat = br#"{"check_runs":[{"name":"t","status":"completed","conclusion":"failure"}],"statuses":[]}"#;
    assert_eq!(
        CiState::of(Some(&CiInput::from_json(flat).unwrap()), "x"),
        CiState::Red {
            failed: 1,
            total: 1
        }
    );
    assert!(CiInput::from_json(b"[]").is_err());
}

#[test]
fn keys_json_shapes_logins_and_cap() {
    let pk = test1().public_key().to_openssh();
    let raw = format!(
        r#"[{{"login":"example-dev","keys":["{pk} laptop",{{"id":1,"key":"{pk}","title":"x"}},"ssh-rsa AAAAB3Nza"]}},
            {{"login":"bad login","keys":["{pk}"]}}]"#
    );
    let links = parse_keys_json(raw.as_bytes()).unwrap();
    assert_eq!(links.len(), 2);
    assert!(links.iter().all(|l| l.login == "example-dev"));

    let map = format!(r#"{{"octo-cat":["{pk}"]}}"#);
    assert_eq!(
        parse_keys_json(map.as_bytes()).unwrap()[0].login,
        "octo-cat"
    );

    let many: Vec<String> = (0..12)
        .map(|i| format!(r#"{{"login":"u{i}","keys":["{pk}"]}}"#))
        .collect();
    let links = parse_keys_json(format!("[{}]", many.join(",")).as_bytes()).unwrap();
    assert_eq!(links.len(), MAX_KEY_LOGINS);

    assert!(is_github_login("a-b"));
    for bad in ["", "-a", "a-", "a--b", "a_b", &"a".repeat(40)] {
        assert!(!is_github_login(bad), "{bad}");
    }
}

// ---------------------------------------------------------------------------------------------
// Real git repositories
// ---------------------------------------------------------------------------------------------

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
        self.dir.path().to_path_buf()
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
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn commit_file(&self, rel: &str, contents: &str, message: &str) -> String {
        let full = self.dir.path().join(rel);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&full, contents).expect("write");
        self.git(&["add", rel]);
        self.git(&["commit", "-q", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn write(&self, name: &str, contents: &[u8]) -> PathBuf {
        let p = self.dir.path().join(".git").join(name);
        std::fs::write(&p, contents).expect("write input");
        p
    }
}

/// base, then two PR commits. Returns (repo, base, first, head).
fn pr_repo() -> (TempRepo, String, String, String) {
    let repo = TempRepo::new();
    let base = repo.commit_file("README.md", "hi\n", "Initial");
    let first = repo.commit_file("src/a.rs", "fn a() {}\n", "Add a");
    let head = repo.commit_file("src/b.rs", "fn b() {}\n", "Add b");
    (repo, base, first, head)
}

fn args_for(repo: &TempRepo, base: &str, head: &str) -> VerifyArgs {
    let pk = test1().public_key().to_openssh();
    let keys = repo.write(
        "keys.json",
        format!(r#"[{{"login":"example-dev","keys":["{pk}"]}}]"#).as_bytes(),
    );
    let ci = repo.write(
        "ci.json",
        format!(
            r#"{{"head_sha":"{head}","check_runs":[{{"name":"test","status":"completed","conclusion":"success"}}]}}"#
        )
        .as_bytes(),
    );
    VerifyArgs {
        repo: repo.path(),
        base: base.into(),
        head: head.into(),
        ci: Some(ci),
        keys: Some(keys),
        ..VerifyArgs::default()
    }
}

#[test]
fn git_notes_transport_end_to_end() {
    let (repo, base, first, head) = pr_repo();
    let st = statement_over(
        &test1(),
        &base,
        &[(&head, &["src/b.rs"], &[0]), (&first, &["src/a.rs"], &[1])],
    );
    let env = String::from_utf8(envelope_bytes(&st, &test1())).unwrap();
    repo.git(&["notes", "--ref=agentworth", "add", "-m", &env, &head]);

    // `--head` may be any revision; ci.json names the resolved SHA.
    let mut args = args_for(&repo, &base, &head);
    args.head = "HEAD".into();
    let r = verify_repo(&args).unwrap();
    assert_eq!(r.commits.len(), 2);
    assert_eq!(r.commits[0].sha, head, "newest first");
    assert_eq!(r.conclusion, Conclusion::Success);
    assert_eq!(r.highest_verified(), Some(5));
    assert!(row(&r, &first)
        .verified
        .contains(&VerifiedFact::FilesMatch {
            matched: 1,
            claimed: 1
        }));

    let mut json_args = args.clone();
    json_args.format = VerifyFormat::CheckJson;
    let out = run_receipt_verify_command(&json_args).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["name"], CHECK_NAME);
    assert_eq!(v["head_sha"], head.as_str());
    assert_eq!(v["conclusion"], "success");
    assert_glyphs(&out);
}

#[test]
fn git_file_transport_covers_the_commits_before_it() {
    let (repo, base, first, head) = pr_repo();
    let st = statement_over(
        &test1(),
        &base,
        &[(&head, &["src/b.rs"], &[0]), (&first, &["src/a.rs"], &[])],
    );
    let env = String::from_utf8(envelope_bytes(&st, &test1())).unwrap();
    let carrier = repo.commit_file(".agentworth/receipts/r1.dsse.json", &env, "Add receipt");
    let r = verify_repo(&args_for(&repo, &base, &carrier)).unwrap();
    assert_eq!(r.receipts[0].transport, Transport::File);
    assert_eq!(row(&r, &carrier).coverage, Coverage::None);
    assert_eq!(row(&r, &head).coverage, Coverage::Signed { linked: true });
    assert_eq!(
        r.conclusion,
        Conclusion::Neutral,
        "the carrier commit is unattributed"
    );
    // CI is green on the carrier (head), which no receipt covers: no rung 5 for anyone.
    assert_eq!(r.highest_verified(), Some(4));
}

#[test]
fn git_rebased_commit_loses_its_note() {
    let (repo, base, first, head) = pr_repo();
    let st = statement_over(
        &test1(),
        &base,
        &[(&head, &["src/b.rs"], &[0]), (&first, &["src/a.rs"], &[])],
    );
    let env = String::from_utf8(envelope_bytes(&st, &test1())).unwrap();
    repo.git(&["notes", "--ref=agentworth", "add", "-m", &env, &head]);
    repo.git(&["commit", "-q", "--amend", "-m", "Add b (amended)"]);
    let new_head = repo.git(&["rev-parse", "HEAD"]);
    assert_ne!(new_head, head);
    let r = verify_repo(&args_for(&repo, &base, &new_head)).unwrap();
    assert!(r.receipts.is_empty(), "the note hangs off the old SHA");
    assert!(r.commits.iter().all(|c| c.coverage == Coverage::None));
}

#[test]
fn git_merge_commit_lists_first_parent_diff() {
    let (repo, base, _first, _head) = pr_repo();
    repo.git(&["checkout", "-q", "-b", "side", &base]);
    repo.commit_file("side.txt", "s\n", "Side");
    repo.git(&["checkout", "-q", "main"]);
    repo.git(&["merge", "-q", "--no-ff", "-m", "Merge side", "side"]);
    let merge = repo.git(&["rev-parse", "HEAD"]);
    let commits = collect_pr_commits(&repo.path(), &base, &merge).unwrap();
    let m = commits.iter().find(|c| c.sha == merge).unwrap();
    assert_eq!(m.files.iter().collect::<Vec<_>>(), vec!["side.txt"]);
}

#[test]
fn git_rejects_option_like_revisions() {
    let (repo, base, _, _) = pr_repo();
    let mut args = args_for(&repo, &base, "HEAD");
    args.base = "--output=/tmp/x".into();
    assert!(verify_repo(&args).is_err());
    args.base = "does-not-exist".into();
    assert!(verify_repo(&args).is_err());
}

#[test]
fn explicit_receipt_file_argument() {
    let (repo, base, first, head) = pr_repo();
    let st = statement_over(
        &test1(),
        &base,
        &[(&head, &["src/b.rs"], &[0]), (&first, &["src/a.rs"], &[])],
    );
    let path = repo.write("r.dsse.json", &envelope_bytes(&st, &test1()));
    let mut args = args_for(&repo, &base, &head);
    args.receipts = vec![path];
    let r = verify_repo(&args).unwrap();
    assert_eq!(r.receipts[0].transport, Transport::Explicit);
    assert_eq!(r.conclusion, Conclusion::Success);
}
