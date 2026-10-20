use super::*;
use agentworth_schema::work_receipt::example;

/// RFC 8032 §7.1 TEST 1 secret key (public test vector, never a real signer).
pub(crate) const RFC8032_TEST1_SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

/// `ssh-keygen -lf` (OpenSSH 10.0p2) on the RFC 8032 TEST 1 public key line.
const TEST1_FINGERPRINT: &str = "SHA256:bbXpuKG6zhzdmnxq256TlqzFBzRl2f6OOg722cYNbU8";

/// A throwaway public key made with `ssh-keygen -t ed25519 -N '' -C agentworth-fixture`
/// (OpenSSH 10.0p2); its private half was discarded. Fingerprint from `ssh-keygen -lf`.
const KEYGEN_PUB: &str =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJH3WVoww2rVSJDJI4GcCwOa4wv+BajkOb1/lkoO7Dxo agentworth-fixture";
const KEYGEN_FINGERPRINT: &str = "SHA256:yGpkILdVrQ547k7Sb6EzadaR2pqQLc4SnqCUcwB9dPA";

#[test]
fn rfc8032_test1_public_key_derives_from_its_seed() {
    let key = ReceiptSigningKey::from_seed(&RFC8032_TEST1_SEED);
    assert_eq!(key.public_key().raw(), example::SIGNER_PUBKEY);
}

#[test]
fn rfc8032_test1_signature_over_the_empty_message() {
    // RFC 8032 §7.1 TEST 1: MESSAGE (length 0 bytes), SIGNATURE below.
    let sig = ReceiptSigningKey::from_seed(&RFC8032_TEST1_SEED).sign(b"");
    assert_eq!(
        hex_lower(&sig),
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155\
         5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
    );
}

pub(crate) fn hex_lower(b: &[u8]) -> String {
    hex::encode(b)
}

#[test]
fn openssh_line_and_fingerprint_match_ssh_keygen() {
    let key = PublicKey::from_raw(example::SIGNER_PUBKEY).unwrap();
    assert_eq!(key.to_openssh(), example::SIGNER_OPENSSH);
    assert_eq!(key.fingerprint(), TEST1_FINGERPRINT);

    let parsed = PublicKey::parse_openssh(KEYGEN_PUB).unwrap();
    assert_eq!(parsed.fingerprint(), KEYGEN_FINGERPRINT);
    // Round trip drops the comment, which is the form a receipt carries.
    let bare: Vec<&str> = KEYGEN_PUB.split_whitespace().take(2).collect();
    assert_eq!(parsed.to_openssh(), bare.join(" "));
}

#[test]
fn parse_tolerates_comment_and_surrounding_whitespace() {
    let line = format!("  {} dev@example.invalid\n", example::SIGNER_OPENSSH);
    assert_eq!(
        PublicKey::parse_openssh(&line).unwrap().raw(),
        example::SIGNER_PUBKEY
    );
}

#[test]
fn parse_rejects_other_key_types_and_garbage() {
    assert!(matches!(
        PublicKey::parse_openssh("ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQ== x"),
        Err(SshKeyError::WrongKeyType(_))
    ));
    assert!(matches!(
        PublicKey::parse_openssh("sk-ssh-ed25519@openssh.com AAAA"),
        Err(SshKeyError::WrongKeyType(_))
    ));
    assert_eq!(
        PublicKey::parse_openssh(""),
        Err(SshKeyError::NotEd25519Line)
    );
    assert_eq!(
        PublicKey::parse_openssh("ssh-ed25519"),
        Err(SshKeyError::NotEd25519Line)
    );
    assert_eq!(
        PublicKey::parse_openssh("ssh-ed25519 !!!notbase64"),
        Err(SshKeyError::Base64)
    );
    // Blob type field disagrees with the line's type.
    let mut blob = Vec::new();
    put_string(&mut blob, b"ssh-rsa");
    put_string(&mut blob, &example::SIGNER_PUBKEY);
    assert!(matches!(
        PublicKey::parse_openssh(&format!("ssh-ed25519 {}", STANDARD.encode(&blob))),
        Err(SshKeyError::WrongKeyType(_))
    ));
    // Trailing bytes after the key.
    let mut blob = key_blob(&example::SIGNER_PUBKEY);
    blob.push(0);
    assert_eq!(
        PublicKey::parse_openssh(&format!("ssh-ed25519 {}", STANDARD.encode(&blob))),
        Err(SshKeyError::Malformed)
    );
    // Short key.
    let mut blob = Vec::new();
    put_string(&mut blob, b"ssh-ed25519");
    put_string(&mut blob, &[1u8; 31]);
    assert_eq!(
        PublicKey::parse_openssh(&format!("ssh-ed25519 {}", STANDARD.encode(&blob))),
        Err(SshKeyError::Malformed)
    );
}

/// sha256 of `to_openssh_private_with_checkint("agentworth-fixture", 0x0102_0304)` for the
/// RFC 8032 TEST 1 seed. When pinned, `ssh-keygen -y -f` (OpenSSH 10.0p2) on that file printed
/// the TEST 1 public key, and our parser read a fresh `ssh-keygen -t ed25519 -N ''` file and
/// re-encoded it byte for byte (same checkint and comment). The armored text is not committed
/// so no file in the repo looks like a private key.
const TEST1_PRIVATE_SHA256: &str =
    "aad345f5ee57905a16f6f1caefb87be83435b289d97026d98a4b5c0f6dbb6ff9";

#[test]
fn private_key_encoding_is_golden() {
    let key = ReceiptSigningKey::from_seed(&RFC8032_TEST1_SEED);
    let pem = key.to_openssh_private_with_checkint("agentworth-fixture", 0x0102_0304);
    assert!(pem.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----\n"));
    assert!(pem.ends_with("-----END OPENSSH PRIVATE KEY-----\n"));
    assert!(pem.lines().all(|l| l.len() <= 70));
    assert_eq!(
        hex_lower(&Sha256::digest(pem.as_bytes())),
        TEST1_PRIVATE_SHA256
    );
}

#[test]
fn private_key_round_trips() {
    let key = ReceiptSigningKey::generate().unwrap();
    let pem = key.to_openssh_private("dev@example.invalid").unwrap();
    let (back, comment) = ReceiptSigningKey::from_openssh_private(&pem).unwrap();
    assert_eq!(back.public_key(), key.public_key());
    assert_eq!(comment, "dev@example.invalid");
    assert_eq!(back.sign(b"m"), key.sign(b"m"));
    // Debug never prints secret material.
    assert!(format!("{back:?}").starts_with("ReceiptSigningKey(SHA256:"));
}

fn armor(body: &[u8]) -> String {
    let b64 = STANDARD.encode(body);
    format!("{PEM_BEGIN}\n{b64}\n{PEM_END}\n")
}

fn unarmor(pem: &str) -> Vec<u8> {
    let inner: String = pem
        .trim()
        .strip_prefix(PEM_BEGIN)
        .unwrap()
        .strip_suffix(PEM_END)
        .unwrap()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    STANDARD.decode(inner).unwrap()
}

/// Offset of the private section (after its length prefix) in a single-ed25519-key body.
const PRIVATE_START: usize = 15 + (4 + 4) + (4 + 4) + 4 + 4 + (4 + 51) + 4;

#[test]
fn private_key_refuses_encrypted_and_corrupt_files() {
    let key = ReceiptSigningKey::from_seed(&RFC8032_TEST1_SEED);
    let pem = key.to_openssh_private_with_checkint("c", 7);
    let body = unarmor(&pem);
    assert_eq!(AUTH_MAGIC.len(), 15);

    // Encrypted: cipher field says aes256-ctr.
    let mut enc = AUTH_MAGIC.to_vec();
    put_string(&mut enc, b"aes256-ctr");
    put_string(&mut enc, b"bcrypt");
    put_string(&mut enc, b"");
    enc.extend_from_slice(&1u32.to_be_bytes());
    assert!(matches!(
        ReceiptSigningKey::from_openssh_private(&armor(&enc)),
        Err(SshKeyError::Encrypted(c)) if c == "aes256-ctr"
    ));

    // Check integers differ.
    let mut bad = body.clone();
    bad[PRIVATE_START] ^= 0xff;
    assert_eq!(
        ReceiptSigningKey::from_openssh_private(&armor(&bad)).unwrap_err(),
        SshKeyError::CheckIntMismatch
    );

    // A flipped seed byte no longer derives the stated public key.
    let seed_start = PRIVATE_START + 8 + (4 + 11) + (4 + 32) + 4;
    let mut bad = body.clone();
    bad[seed_start] ^= 0x01;
    assert_eq!(
        ReceiptSigningKey::from_openssh_private(&armor(&bad)).unwrap_err(),
        SshKeyError::KeyMismatch
    );

    // Bad magic, and not a PEM at all.
    let mut bad = body;
    bad[0] = b'X';
    assert_eq!(
        ReceiptSigningKey::from_openssh_private(&armor(&bad)).unwrap_err(),
        SshKeyError::Malformed
    );
    assert_eq!(
        ReceiptSigningKey::from_openssh_private("ssh-ed25519 AAAA").unwrap_err(),
        SshKeyError::Malformed
    );
}
