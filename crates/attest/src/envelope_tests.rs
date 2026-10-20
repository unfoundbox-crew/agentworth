use super::*;
use crate::ssh::tests::RFC8032_TEST1_SEED;
use agentworth_schema::work_receipt::{example, session_ref, Signer};

/// Envelope of the main fixture signed with the RFC 8032 TEST 1 key (ed25519 is deterministic).
/// Cross-checked when pinned with securesystemslib 1.5.1 (the in-toto project's DSSE reference
/// implementation): `Envelope.from_dict(...).verify([SSlibKey ed25519 TEST 1 pubkey], 1)`
/// accepted it (so its own `Envelope.pae()` produced the bytes [`pae`] signed), and rejected the
/// same one-digit payload flip `tamper_flip_one_payload_byte_is_invalid` makes.
const MAIN_ENVELOPE: &str = include_str!("testdata/work_receipt_v1_main.dsse.json");
const MAIN_RECEIPT_ID: &str = "e69b9f1b456cc59f28235be6a1de6b17b4cb614b3a36302abd73837ce4dee560";

fn test1_key() -> ReceiptSigningKey {
    ReceiptSigningKey::from_seed(&RFC8032_TEST1_SEED)
}

fn other_key() -> ReceiptSigningKey {
    ReceiptSigningKey::from_seed(&[7u8; 32])
}

fn signed_main() -> SignedReceipt {
    sign_statement(&example::statement(), &test1_key()).unwrap()
}

fn link(login: &str, key: &ReceiptSigningKey) -> KeyLink {
    KeyLink {
        login: login.to_string(),
        key: key.public_key(),
    }
}

/// Re-target a statement at another signer: new signer field and re-derived session refs.
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

fn payload_of(env: &Envelope) -> Vec<u8> {
    STANDARD.decode(&env.payload).unwrap()
}

fn with_payload(env: &Envelope, payload: &[u8]) -> Envelope {
    Envelope {
        payload: STANDARD.encode(payload),
        ..env.clone()
    }
}

#[test]
fn main_envelope_is_golden() {
    let signed = signed_main();
    assert_eq!(signed.receipt_id, MAIN_RECEIPT_ID);
    assert_eq!(
        String::from_utf8(signed.envelope.to_json()).unwrap(),
        MAIN_ENVELOPE
    );
    assert_eq!(
        signed.envelope.signatures[0].keyid,
        "SHA256:bbXpuKG6zhzdmnxq256TlqzFBzRl2f6OOg722cYNbU8"
    );
    assert_eq!(
        payload_of(&signed.envelope),
        example::statement().canonical_bytes().unwrap()
    );
}

#[test]
fn golden_envelope_parses_and_verifies() {
    let env = Envelope::from_json(MAIN_ENVELOPE.as_bytes()).unwrap();
    let v = verify_envelope(&env, &[link("dev", &test1_key())]);
    assert_eq!(
        v.verdict,
        SignatureVerdict::SignedBy {
            login: "dev".into(),
            keyid: test1_key().public_key().fingerprint(),
        }
    );
    let receipt = v.receipt.unwrap();
    assert_eq!(receipt.receipt_id, MAIN_RECEIPT_ID);
    assert_eq!(receipt.statement, example::statement());
}

#[test]
fn valid_signature_from_an_unlinked_key_is_key_not_linked() {
    let env = signed_main().envelope;
    let v = verify_envelope(&env, &[]);
    assert!(matches!(v.verdict, SignatureVerdict::KeyNotLinked { .. }));
    assert!(v.verdict.is_valid());
    assert!(v.receipt.is_some());
    // A link for a different key does not count.
    let v = verify_envelope(&env, &[link("someone-else", &other_key())]);
    assert!(matches!(v.verdict, SignatureVerdict::KeyNotLinked { .. }));
}

// --- Tamper matrix (spec §6.2) ----------------------------------------------------------------

#[test]
fn tamper_flip_one_payload_byte_is_invalid() {
    let env = signed_main().envelope;
    let mut payload = payload_of(&env);
    // Flip one byte of a digit so the JSON stays valid and the Statement still validates:
    // only the signature can catch it.
    let at = find(&payload, b"1840000").unwrap() + 6;
    payload[at] = b'1';
    let v = verify_envelope(&with_payload(&env, &payload), &[link("dev", &test1_key())]);
    assert_eq!(
        v.verdict,
        SignatureVerdict::Invalid(InvalidReason::SignatureMismatch)
    );
    assert!(v.verdict.rungs_claimed_only());
    assert!(v.receipt.is_none());

    // Any other single-byte flip is invalid too, whatever stage catches it.
    let original = payload_of(&env);
    for i in (0..original.len()).step_by(97) {
        let mut p = original.clone();
        p[i] ^= 0x01;
        let v = verify_envelope(&with_payload(&env, &p), &[link("dev", &test1_key())]);
        assert!(v.verdict.rungs_claimed_only(), "flip at {i} accepted");
    }
}

#[test]
fn tamper_flip_one_signature_byte_is_invalid() {
    let mut env = signed_main().envelope;
    let mut sig = STANDARD.decode(&env.signatures[0].sig).unwrap();
    sig[10] ^= 0x80;
    env.signatures[0].sig = STANDARD.encode(sig);
    let v = verify_envelope(&env, &[link("dev", &test1_key())]);
    assert_eq!(
        v.verdict,
        SignatureVerdict::Invalid(InvalidReason::SignatureMismatch)
    );
}

#[test]
fn tamper_swap_a_subject_sha_is_invalid_and_the_real_commit_is_not_covered() {
    let env = signed_main().envelope;
    let swapped = "ffffffffffffffffffffffffffffffffffffffff";
    let text = String::from_utf8(payload_of(&env)).unwrap();
    // Swap both the subject and the matching commits[].sha so the Statement stays consistent.
    let tampered = text.replace(example::HEAD, swapped);
    let v = verify_envelope(
        &with_payload(&env, tampered.as_bytes()),
        &[link("dev", &test1_key())],
    );
    assert_eq!(
        v.verdict,
        SignatureVerdict::Invalid(InvalidReason::SignatureMismatch)
    );
    assert!(v.verdict.rungs_claimed_only());

    // The honest receipt does not cover the swapped-in commit: "no receipt covers this commit".
    let honest = verify_envelope(&env, &[]).receipt.unwrap().statement;
    assert!(honest.covers_commit(example::HEAD));
    assert!(!honest.covers_commit(swapped));
}

#[test]
fn tamper_resign_with_another_key_is_key_not_linked() {
    // An attacker edits the receipt and re-signs it with their own key, honestly naming it.
    let attacker = other_key();
    let mut s = statement_for(&attacker);
    s.predicate.sessions[0].highest_rung = Some(3);
    let env = sign_statement(&s, &attacker).unwrap().envelope;
    let v = verify_envelope(&env, &[link("dev", &test1_key())]);
    assert_eq!(
        v.verdict,
        SignatureVerdict::KeyNotLinked {
            keyid: attacker.public_key().fingerprint()
        }
    );
    // Integrity holds, identity does not: the receipt is readable but not "Signed by @dev".
    assert_eq!(v.receipt.unwrap().signer, attacker.public_key());
}

#[test]
fn tamper_resign_with_another_key_but_keep_the_victims_signer_is_invalid() {
    let original = signed_main().envelope;
    let payload = payload_of(&original);
    let forged = sign_payload(IN_TOTO_PAYLOAD_TYPE, &payload, &other_key());
    let v = verify_envelope(&forged, &[link("dev", &test1_key())]);
    assert_eq!(
        v.verdict,
        SignatureVerdict::Invalid(InvalidReason::SignatureMismatch)
    );

    // Same, with the keyid hint forged to the victim's: keyid is never trusted.
    let mut forged = forged;
    forged.signatures[0].keyid = test1_key().public_key().fingerprint();
    let v = verify_envelope(&forged, &[link("dev", &test1_key())]);
    assert_eq!(
        v.verdict,
        SignatureVerdict::Invalid(InvalidReason::SignatureMismatch)
    );
}

#[test]
fn signing_refuses_a_statement_that_names_another_signer() {
    assert_eq!(
        sign_statement(&example::statement(), &other_key()).unwrap_err(),
        SignError::SignerMismatch
    );
}

#[test]
fn wrong_payload_type_is_invalid() {
    let mut env = signed_main().envelope;
    env.payload_type = "application/json".into();
    assert_eq!(
        verify_envelope(&env, &[]).verdict,
        SignatureVerdict::Invalid(InvalidReason::UnsupportedPayloadType)
    );
}

#[test]
fn non_canonical_payload_is_invalid_even_when_signed() {
    let value: serde_json::Value =
        serde_json::from_slice(&example::statement().canonical_bytes().unwrap()).unwrap();
    let pretty = serde_json::to_vec_pretty(&value).unwrap();
    let env = sign_payload(IN_TOTO_PAYLOAD_TYPE, &pretty, &test1_key());
    assert_eq!(
        verify_envelope(&env, &[]).verdict,
        SignatureVerdict::Invalid(InvalidReason::NonCanonicalPayload)
    );
}

#[test]
fn malformed_and_oversized_payloads_are_invalid() {
    let env = sign_payload(IN_TOTO_PAYLOAD_TYPE, b"{\"_type\":1}", &test1_key());
    assert!(matches!(
        verify_envelope(&env, &[]).verdict,
        SignatureVerdict::Invalid(InvalidReason::MalformedStatement(_))
    ));
    let mut env = signed_main().envelope;
    env.payload = "%%%".into();
    assert_eq!(
        verify_envelope(&env, &[]).verdict,
        SignatureVerdict::Invalid(InvalidReason::BadBase64)
    );
    let big = vec![b' '; MAX_RECEIPT_BYTES + 1];
    let env = sign_payload(IN_TOTO_PAYLOAD_TYPE, &big, &test1_key());
    assert_eq!(
        verify_envelope(&env, &[]).verdict,
        SignatureVerdict::Invalid(InvalidReason::TooLarge)
    );
    let mut env = signed_main().envelope;
    env.signatures.clear();
    assert_eq!(
        verify_envelope(&env, &[]).verdict,
        SignatureVerdict::Invalid(InvalidReason::SignatureMismatch)
    );
}

#[test]
fn url_safe_unpadded_base64_is_accepted() {
    let mut env = signed_main().envelope;
    env.payload = URL_SAFE_NO_PAD.encode(payload_of(&env));
    let sig = STANDARD.decode(&env.signatures[0].sig).unwrap();
    env.signatures[0].sig = URL_SAFE_NO_PAD.encode(sig);
    assert!(verify_envelope(&env, &[]).verdict.is_valid());
}

#[test]
fn signing_applies_the_size_cap_and_still_verifies() {
    let mut s = example::statement();
    let files: Vec<String> = (0..1000)
        .map(|i| format!("crates/generated/src/module_{i:05}/mod.rs"))
        .collect();
    for c in &mut s.predicate.commits {
        c.files.clone_from(&files);
    }
    let signed = sign_statement(&s, &test1_key()).unwrap();
    assert!(signed.statement.predicate.truncated);
    let v = verify_envelope(&signed.envelope, &[]);
    let receipt = v.receipt.unwrap();
    assert!(receipt.statement.predicate.truncated);
    assert!(receipt
        .statement
        .predicate
        .commits
        .iter()
        .all(|c| c.files.is_empty()));
    assert_eq!(receipt.receipt_id, signed.receipt_id);
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}
