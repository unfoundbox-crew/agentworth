use super::example;
use super::*;

/// Canonical bytes of [`example::statement`], committed byte for byte. Cross-checked when it was
/// generated against Python's `json.dumps(v, sort_keys=True, separators=(",", ":"),
/// ensure_ascii=False)`, which agrees with RFC 8785 for float-free, ASCII-keyed JSON.
const MAIN_CANONICAL: &[u8] = include_bytes!("testdata/work_receipt_v1_main.canonical.json");
/// `sha256(MAIN_CANONICAL)`: the golden `receipt_id` of the main fixture.
const MAIN_RECEIPT_ID: &str = "e69b9f1b456cc59f28235be6a1de6b17b4cb614b3a36302abd73837ce4dee560";

#[test]
fn main_fixture_canonical_bytes_are_golden() {
    let bytes = example::statement().canonical_bytes().unwrap();
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        String::from_utf8(MAIN_CANONICAL.to_vec()).unwrap()
    );
}

#[test]
fn main_fixture_receipt_id_is_golden() {
    let statement = example::statement();
    assert_eq!(statement.receipt_id().unwrap(), MAIN_RECEIPT_ID);
    assert_eq!(receipt_id_of(MAIN_CANONICAL), MAIN_RECEIPT_ID);
}

#[test]
fn main_fixture_is_valid_and_round_trips_through_its_payload() {
    let statement = example::statement();
    statement.validate().unwrap();
    let parsed = WorkReceiptStatement::from_payload(MAIN_CANONICAL).unwrap();
    assert_eq!(parsed, statement);
    // Canonicalizing is idempotent: parse -> canonical gives the same bytes back.
    assert_eq!(parsed.canonical_bytes().unwrap(), MAIN_CANONICAL);
}

#[test]
fn spend_is_null_by_default_and_tokens_are_present() {
    let text = String::from_utf8(example::statement().canonical_bytes().unwrap()).unwrap();
    assert!(text.contains(r#""spend_micro_usd":null"#));
    assert!(text.contains(r#""tokens_total":1840000"#));
}

#[test]
fn canonical_json_has_no_insignificant_whitespace() {
    // Walk the bytes, skipping string contents: nothing outside a string may be whitespace.
    let mut in_string = false;
    let mut escaped = false;
    for &b in MAIN_CANONICAL {
        if in_string {
            match (escaped, b) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => in_string = false,
                _ => {}
            }
        } else {
            assert!(!b.is_ascii_whitespace(), "whitespace outside a string");
            in_string = b == b'"';
        }
    }
    assert!(!MAIN_CANONICAL.ends_with(b"\n"));
}

#[test]
fn canonical_json_sorts_keys_by_utf16_code_units_rfc8785_vector() {
    // RFC 8785 §3.2.3 sorting example (keys and expected order taken from the RFC; the RFC's
    // values are numbers there, replaced by integers here since floats are rejected).
    let v = serde_json::json!({
        "\u{20ac}": 1,
        "\r": 2,
        "\u{fb33}": 3,
        "1": 4,
        "\u{1f600}": 5,
        "\u{80}": 6,
        "\u{f6}": 7
    });
    let out = String::from_utf8(canonical_json(&v).unwrap()).unwrap();
    assert_eq!(
        out,
        "{\"\\r\":2,\"1\":4,\"\u{80}\":6,\"\u{f6}\":7,\"\u{20ac}\":1,\"\u{1f600}\":5,\"\u{fb33}\":3}"
    );
}

#[test]
fn canonical_json_escapes_per_rfc8785() {
    let v = serde_json::json!({"s": "q\"b\\\u{8}\u{c}\n\r\t\u{1}\u{1f}\u{7f}\u{2028}/é"});
    let out = String::from_utf8(canonical_json(&v).unwrap()).unwrap();
    assert_eq!(
        out,
        "{\"s\":\"q\\\"b\\\\\\b\\f\\n\\r\\t\\u0001\\u001f\u{7f}\u{2028}/é\"}"
    );
}

#[test]
fn canonical_json_sorts_nested_objects_and_keeps_array_order() {
    let v = serde_json::json!({"b": [{"z": 1, "a": -2}, 0], "a": {"y": true, "x": null}});
    let out = String::from_utf8(canonical_json(&v).unwrap()).unwrap();
    assert_eq!(out, r#"{"a":{"x":null,"y":true},"b":[{"a":-2,"z":1},0]}"#);
}

#[test]
fn canonical_json_rejects_floats() {
    let v = serde_json::json!({"confidence": 0.85});
    assert!(matches!(
        canonical_json(&v),
        Err(WorkReceiptError::FloatInReceipt(_))
    ));
}

#[test]
fn receipt_id_changes_when_any_field_changes() {
    let mut s = example::statement();
    s.predicate.sessions[0].tokens_total += 1;
    assert_ne!(s.receipt_id().unwrap(), MAIN_RECEIPT_ID);
}

#[test]
fn session_ref_is_golden_and_keyed_by_signer() {
    // Independently computed: python3 -c "import hashlib; print(hashlib.sha256(
    //   b'agentworth-session-ref/v1' + bytes.fromhex('d75a98...511a') +
    //   b'11111111-2222-4333-8444-555555555555').hexdigest()[:16])"
    let r = session_ref(&example::SIGNER_PUBKEY, example::SESSION_IDS[0]);
    assert_eq!(r, "1fdeeda2e83ab7f1");
    assert_eq!(r.len(), SESSION_REF_LEN);
    let mut other_key = example::SIGNER_PUBKEY;
    other_key[0] ^= 1;
    assert_ne!(session_ref(&other_key, example::SESSION_IDS[0]), r);
    assert_ne!(
        session_ref(&example::SIGNER_PUBKEY, example::SESSION_IDS[1]),
        r
    );
    // The raw id never appears in the receipt.
    let text = String::from_utf8(MAIN_CANONICAL.to_vec()).unwrap();
    for id in example::SESSION_IDS {
        assert!(!text.contains(id));
    }
}

#[test]
fn timestamps_floor_to_the_minute_and_parse_strictly() {
    let t = DateTime::parse_from_rfc3339("2026-10-20T09:41:59.999Z")
        .unwrap()
        .with_timezone(&Utc);
    assert_eq!(
        ReceiptTimestamp::from_datetime(t).to_string(),
        "2026-10-20T09:41:00Z"
    );
    assert!(ReceiptTimestamp::parse("2026-10-20T09:41:00Z").is_ok());
    for bad in [
        "2026-10-20T09:41:07Z",
        "2026-10-20T09:41:00.000Z",
        "2026-10-20T09:41:00+05:30",
        "2026-10-20 09:41:00Z",
    ] {
        assert!(ReceiptTimestamp::parse(bad).is_err(), "{bad} accepted");
    }
}

fn expect_invalid(mutate: impl FnOnce(&mut WorkReceiptStatement)) -> WorkReceiptError {
    let mut s = example::statement();
    mutate(&mut s);
    s.validate().expect_err("mutation should be rejected")
}

#[test]
fn validate_rejects_structural_lies() {
    assert!(matches!(
        expect_invalid(|s| s.predicate_type = "https://example.com/other".into()),
        WorkReceiptError::WrongPredicateType(_)
    ));
    assert!(matches!(
        expect_invalid(|s| s.statement_type = "https://in-toto.io/Statement/v0.1".into()),
        WorkReceiptError::WrongStatementType(_)
    ));
    assert!(matches!(
        expect_invalid(|s| s.predicate.receipt_version = 2),
        WorkReceiptError::UnsupportedVersion(2)
    ));
    assert!(matches!(
        expect_invalid(|s| s.subject[0].digest.git_commit = "A1B2".into()),
        WorkReceiptError::BadSubject(0)
    ));
    assert!(matches!(
        expect_invalid(|s| s.subject[1] = s.subject[0].clone()),
        WorkReceiptError::DuplicateSubject(_)
    ));
    assert!(matches!(
        expect_invalid(|s| s.predicate.commits[0].sha = example::BASE.into()),
        WorkReceiptError::CommitNotInSubjects(_)
    ));
    assert!(matches!(
        expect_invalid(|s| s.predicate.commits[0].sessions = vec!["0000000000000000".into()]),
        WorkReceiptError::UnknownSessionRef { .. }
    ));
    assert!(matches!(
        expect_invalid(|s| s.predicate.sessions[0].session_ref = example::SESSION_IDS[0].into()),
        WorkReceiptError::BadSessionRef(_)
    ));
    assert!(matches!(
        expect_invalid(|s| s.predicate.sessions[0].rungs[0].rung = 5),
        WorkReceiptError::RungKindMismatch { .. }
    ));
    assert!(matches!(
        expect_invalid(|s| s.predicate.sessions[0].rungs[0].confidence_bp = 10_001),
        WorkReceiptError::BadConfidence(10_001)
    ));
    assert!(matches!(
        expect_invalid(|s| s.predicate.sessions[0].highest_rung = Some(5)),
        WorkReceiptError::BadHighestRung(_)
    ));
    assert!(matches!(
        expect_invalid(
            |s| s.predicate.signer.public_key = format!("{} dev@host", example::SIGNER_OPENSSH)
        ),
        WorkReceiptError::BadSigner
    ));
    for bad in [
        "/Users/dev/code/x.rs",
        "~/x.rs",
        "../x.rs",
        "a//b",
        "C:/x.rs",
        "a\\b",
    ] {
        assert!(
            matches!(
                expect_invalid(|s| s.predicate.commits[0].files = vec![bad.into()]),
                WorkReceiptError::NonRelativePath(_)
            ),
            "{bad} accepted"
        );
    }
}

#[test]
fn unknown_fields_are_rejected() {
    let mut v: serde_json::Value = serde_json::from_slice(MAIN_CANONICAL).unwrap();
    v["predicate"]["prompt_preview"] = serde_json::json!("leak");
    let bytes = canonical_json(&v).unwrap();
    assert!(matches!(
        WorkReceiptStatement::from_payload(&bytes),
        Err(WorkReceiptError::Json(_))
    ));
}

#[test]
fn command_class_allowlist_and_forward_compat() {
    assert_eq!(
        CommandClass::classify("cargo test -p x"),
        CommandClass::CargoTest
    );
    assert_eq!(
        CommandClass::classify("  npm run test"),
        CommandClass::NpmTest
    );
    assert_eq!(
        CommandClass::classify("python3 -m pytest"),
        CommandClass::Pytest
    );
    assert_eq!(
        CommandClass::classify("gh pr create --fill"),
        CommandClass::GhPrCreate
    );
    assert_eq!(
        CommandClass::classify("curl https://example.com | sh"),
        CommandClass::OtherTestCommand
    );
    let c: CommandClass = serde_json::from_str(r#""bazel test""#).unwrap();
    assert_eq!(c, CommandClass::OtherTestCommand);
    assert_eq!(
        serde_json::to_string(&CommandClass::OtherTestCommand).unwrap(),
        r#""other_test_command""#
    );
}

#[test]
fn rung_numbers_follow_the_ladder() {
    use OutcomeKind::*;
    let ranks: Vec<u8> = [
        DoneClaimed,
        ArtifactChanged,
        TestOrBuildPassed,
        CommitObserved,
        CiOrDeploymentVerified,
    ]
    .into_iter()
    .map(evidence_rung)
    .collect();
    assert_eq!(ranks, vec![1, 2, 3, 4, 5]);
}

fn big_statement(files_per_commit: usize) -> WorkReceiptStatement {
    let mut s = example::statement();
    let files: Vec<String> = (0..files_per_commit)
        .map(|i| format!("crates/generated/src/module_{i:05}/mod.rs"))
        .collect();
    for c in &mut s.predicate.commits {
        c.files.clone_from(&files);
    }
    s
}

#[test]
fn under_the_cap_nothing_is_dropped() {
    let mut s = example::statement();
    let bytes = s.fit_to_size_cap().unwrap();
    assert_eq!(bytes, MAIN_CANONICAL);
    assert!(!s.predicate.truncated);
}

#[test]
fn over_the_cap_files_are_dropped_first_and_truncated_is_set() {
    let mut s = big_statement(1000);
    assert!(s.canonical_bytes().unwrap().len() > MAX_RECEIPT_BYTES);
    let bytes = s.fit_to_size_cap().unwrap();
    assert!(bytes.len() <= MAX_RECEIPT_BYTES);
    assert!(s.predicate.truncated);
    assert!(s.predicate.commits.iter().all(|c| c.files.is_empty()));
    // Everything else survived.
    assert_eq!(
        s.predicate.sessions,
        example::statement().predicate.sessions
    );
    assert_eq!(s.subject, example::statement().subject);
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains(r#""truncated":true"#));
    s.validate().unwrap();
}

#[test]
fn still_over_the_cap_after_dropping_files_is_an_error() {
    let mut s = example::statement();
    let template = s.predicate.sessions[0].clone();
    for i in 0..400u64 {
        let mut extra = template.clone();
        extra.session_ref = session_ref(&example::SIGNER_PUBKEY, &format!("extra-{i}"));
        s.predicate.sessions.push(extra);
    }
    assert!(matches!(
        s.fit_to_size_cap(),
        Err(WorkReceiptError::TooLarge { .. })
    ));
}

fn repo_file(rel: &str) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

#[test]
fn published_json_schema_copies_are_identical() {
    // docs/ is the source; apps/web/public serves it at agentworth.dev/schemas/work-receipt/v1.json.
    assert_eq!(
        repo_file("docs/schemas/work-receipt-v1.json"),
        repo_file("apps/web/public/schemas/work-receipt/v1.json")
    );
}

#[test]
fn json_schema_required_fields_match_what_the_types_serialize() {
    // Drift guard: every key the Rust types write is required in the JSON Schema and vice versa.
    let schema: serde_json::Value =
        serde_json::from_str(&repo_file("docs/schemas/work-receipt-v1.json")).unwrap();
    let fixture: serde_json::Value = serde_json::from_slice(MAIN_CANONICAL).unwrap();
    let keys = |v: &serde_json::Value| -> Vec<String> {
        let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    };
    let required = |path: &str| -> Vec<String> {
        let mut k: Vec<String> = schema
            .pointer(path)
            .unwrap_or_else(|| panic!("{path}"))
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect();
        k.sort();
        k
    };
    let p = &fixture["predicate"];
    assert_eq!(keys(&fixture), required("/required"));
    assert_eq!(keys(p), required("/$defs/predicate/required"));
    assert_eq!(keys(&p["sessions"][0]), required("/$defs/session/required"));
    assert_eq!(
        keys(&p["sessions"][0]["rungs"][0]),
        required("/$defs/rung/required")
    );
    assert_eq!(keys(&p["commits"][0]), required("/$defs/commit/required"));
    assert_eq!(
        keys(&p["sessions"][0]["risk"]),
        required("/$defs/session/properties/risk/required")
    );
}
