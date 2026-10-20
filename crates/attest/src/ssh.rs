//! OpenSSH encodings for ed25519 keys, written by hand (no `ssh-keygen` subprocess).
//!
//! - Public key line: `ssh-ed25519 <base64(blob)> [comment]`, where the blob is
//!   `string "ssh-ed25519" || string <32-byte key>` (RFC 4253 §6.6, RFC 8709 §4).
//! - Fingerprint: `SHA256:` + unpadded standard base64 of `sha256(blob)`, the form
//!   `ssh-keygen -lf` and GitHub print. Used as the DSSE `keyid`.
//! - Private key: unencrypted `openssh-key-v1` (OpenSSH `PROTOCOL.key`), the format
//!   `ssh-keygen -t ed25519 -N ''` writes. Encrypted keys are refused, not decrypted.

use std::fmt;

use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use base64::Engine as _;
use ed25519_dalek::{Signer as _, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// The only key type a Work Receipt accepts.
pub const KEY_TYPE: &str = "ssh-ed25519";
const AUTH_MAGIC: &[u8] = b"openssh-key-v1\0";
const PEM_BEGIN: &str = "-----BEGIN OPENSSH PRIVATE KEY-----";
const PEM_END: &str = "-----END OPENSSH PRIVATE KEY-----";
/// Cipher block size for `none`; the private section is padded to a multiple of it.
const NONE_BLOCK: usize = 8;

/// Everything that can go wrong reading an OpenSSH key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SshKeyError {
    #[error("not an `ssh-ed25519 <base64>` public key line")]
    NotEd25519Line,
    #[error("key type {0} is not ssh-ed25519")]
    WrongKeyType(String),
    #[error("invalid base64 in key")]
    Base64,
    #[error("truncated or malformed key encoding")]
    Malformed,
    #[error("not a valid ed25519 public key")]
    InvalidPoint,
    #[error("private key is encrypted (cipher {0}); receipt keys are stored unencrypted at 0600")]
    Encrypted(String),
    #[error("private key file holds {0} keys, expected 1")]
    KeyCount(u32),
    #[error("private key check integers differ (corrupt file)")]
    CheckIntMismatch,
    #[error("private key does not match its public key")]
    KeyMismatch,
    #[error("could not read randomness from the OS: {0}")]
    Rng(String),
}

// ---------------------------------------------------------------------------------------------
// SSH wire encoding helpers
// ---------------------------------------------------------------------------------------------

fn put_string(out: &mut Vec<u8>, bytes: &[u8]) {
    let len = u32::try_from(bytes.len()).expect("ssh string under 4 GiB");
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
}

struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], SshKeyError> {
        if self.buf.len() < n {
            return Err(SshKeyError::Malformed);
        }
        let (head, rest) = self.buf.split_at(n);
        self.buf = rest;
        Ok(head)
    }

    fn u32(&mut self) -> Result<u32, SshKeyError> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn string(&mut self) -> Result<&'a [u8], SshKeyError> {
        let len = self.u32()? as usize;
        self.take(len)
    }

    fn finish(&self) -> Result<(), SshKeyError> {
        if self.buf.is_empty() {
            Ok(())
        } else {
            Err(SshKeyError::Malformed)
        }
    }

    fn rest(&self) -> &'a [u8] {
        self.buf
    }
}

fn key_blob(raw: &[u8; 32]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(51);
    put_string(&mut blob, KEY_TYPE.as_bytes());
    put_string(&mut blob, raw);
    blob
}

fn parse_key_blob(blob: &[u8]) -> Result<[u8; 32], SshKeyError> {
    let mut r = Reader::new(blob);
    let ty = r.string()?;
    if ty != KEY_TYPE.as_bytes() {
        return Err(SshKeyError::WrongKeyType(
            String::from_utf8_lossy(ty).into_owned(),
        ));
    }
    let key = r.string()?;
    r.finish()?;
    key.try_into().map_err(|_| SshKeyError::Malformed)
}

// ---------------------------------------------------------------------------------------------
// Public key
// ---------------------------------------------------------------------------------------------

/// An ed25519 public key that is known to be a valid curve point.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PublicKey {
    raw: [u8; 32],
}

impl PublicKey {
    /// From the raw 32-byte encoding.
    pub fn from_raw(raw: [u8; 32]) -> Result<Self, SshKeyError> {
        VerifyingKey::from_bytes(&raw).map_err(|_| SshKeyError::InvalidPoint)?;
        Ok(Self { raw })
    }

    /// Parse `ssh-ed25519 <base64> [comment]` (one line, as in `~/.ssh/*.pub`, an
    /// `authorized_keys` entry without options, or GitHub's `ssh_signing_keys[].key`).
    pub fn parse_openssh(line: &str) -> Result<Self, SshKeyError> {
        let mut parts = line.split_whitespace();
        let ty = parts.next().ok_or(SshKeyError::NotEd25519Line)?;
        if ty != KEY_TYPE {
            return Err(SshKeyError::WrongKeyType(ty.to_string()));
        }
        let b64 = parts.next().ok_or(SshKeyError::NotEd25519Line)?;
        let blob = STANDARD.decode(b64).map_err(|_| SshKeyError::Base64)?;
        Self::from_raw(parse_key_blob(&blob)?)
    }

    /// The raw 32 bytes (the `signer_pubkey` input of a session ref).
    pub fn raw(&self) -> [u8; 32] {
        self.raw
    }

    /// The SSH wire blob `string "ssh-ed25519" || string key`.
    pub fn blob(&self) -> Vec<u8> {
        key_blob(&self.raw)
    }

    /// `ssh-ed25519 <base64>`, no comment: the form a receipt's `signer.public_key` carries.
    pub fn to_openssh(&self) -> String {
        format!("{KEY_TYPE} {}", STANDARD.encode(self.blob()))
    }

    /// `SHA256:<unpadded base64>`, identical to `ssh-keygen -lf` and GitHub's display.
    pub fn fingerprint(&self) -> String {
        format!(
            "SHA256:{}",
            STANDARD_NO_PAD.encode(Sha256::digest(self.blob()))
        )
    }

    pub(crate) fn verifying_key(&self) -> VerifyingKey {
        VerifyingKey::from_bytes(&self.raw).expect("validated at construction")
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicKey({})", self.fingerprint())
    }
}

// ---------------------------------------------------------------------------------------------
// Signing key
// ---------------------------------------------------------------------------------------------

/// The receipt signing key. Secret material is zeroized on drop (ed25519-dalek's `zeroize`).
pub struct ReceiptSigningKey {
    key: SigningKey,
}

impl fmt::Debug for ReceiptSigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ReceiptSigningKey({})", self.public_key().fingerprint())
    }
}

impl ReceiptSigningKey {
    /// From a 32-byte ed25519 seed.
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        Self {
            key: SigningKey::from_bytes(seed),
        }
    }

    /// A fresh key from the OS RNG.
    pub fn generate() -> Result<Self, SshKeyError> {
        let mut seed = Zeroizing::new([0u8; 32]);
        getrandom::getrandom(seed.as_mut()).map_err(|e| SshKeyError::Rng(e.to_string()))?;
        Ok(Self::from_seed(&seed))
    }

    pub fn public_key(&self) -> PublicKey {
        PublicKey {
            raw: self.key.verifying_key().to_bytes(),
        }
    }

    /// Raw ed25519 signature over `message`.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.key.sign(message).to_bytes()
    }

    /// Encode as an unencrypted `openssh-key-v1` PEM file with a random check integer.
    pub fn to_openssh_private(&self, comment: &str) -> Result<Zeroizing<String>, SshKeyError> {
        let mut c = [0u8; 4];
        getrandom::getrandom(&mut c).map_err(|e| SshKeyError::Rng(e.to_string()))?;
        Ok(self.to_openssh_private_with_checkint(comment, u32::from_be_bytes(c)))
    }

    /// Deterministic form of [`Self::to_openssh_private`], for golden tests.
    pub fn to_openssh_private_with_checkint(
        &self,
        comment: &str,
        checkint: u32,
    ) -> Zeroizing<String> {
        let public = self.public_key();
        let blob = public.blob();

        let mut private = Zeroizing::new(Vec::new());
        private.extend_from_slice(&checkint.to_be_bytes());
        private.extend_from_slice(&checkint.to_be_bytes());
        put_string(&mut private, KEY_TYPE.as_bytes());
        put_string(&mut private, &public.raw);
        let mut secret = Zeroizing::new([0u8; 64]);
        secret[..32].copy_from_slice(self.key.as_bytes());
        secret[32..].copy_from_slice(&public.raw);
        put_string(&mut private, secret.as_ref());
        put_string(&mut private, comment.as_bytes());
        let mut pad = 1u8;
        while private.len() % NONE_BLOCK != 0 {
            private.push(pad);
            pad += 1;
        }

        let mut body = Zeroizing::new(Vec::new());
        body.extend_from_slice(AUTH_MAGIC);
        put_string(&mut body, b"none");
        put_string(&mut body, b"none");
        put_string(&mut body, b"");
        body.extend_from_slice(&1u32.to_be_bytes());
        put_string(&mut body, &blob);
        put_string(&mut body, &private);

        let b64 = Zeroizing::new(STANDARD.encode(body.as_slice()));
        let mut out = Zeroizing::new(String::new());
        out.push_str(PEM_BEGIN);
        out.push('\n');
        // OpenSSH wraps at 70 columns. base64 is ASCII, so byte chunks are char chunks.
        for line in b64.as_bytes().chunks(70) {
            out.push_str(std::str::from_utf8(line).expect("base64 is ASCII"));
            out.push('\n');
        }
        out.push_str(PEM_END);
        out.push('\n');
        out
    }

    /// Parse an unencrypted `openssh-key-v1` file. Returns the key and its comment.
    pub fn from_openssh_private(text: &str) -> Result<(Self, String), SshKeyError> {
        let text = text.trim();
        let inner = text
            .strip_prefix(PEM_BEGIN)
            .and_then(|t| t.strip_suffix(PEM_END))
            .ok_or(SshKeyError::Malformed)?;
        let b64: Zeroizing<String> =
            Zeroizing::new(inner.chars().filter(|c| !c.is_whitespace()).collect());
        let body = Zeroizing::new(
            STANDARD
                .decode(b64.as_bytes())
                .map_err(|_| SshKeyError::Base64)?,
        );

        let mut r = Reader::new(&body);
        if r.take(AUTH_MAGIC.len())? != AUTH_MAGIC {
            return Err(SshKeyError::Malformed);
        }
        let cipher = r.string()?;
        let kdf = r.string()?;
        let _kdf_options = r.string()?;
        if cipher != b"none" || kdf != b"none" {
            return Err(SshKeyError::Encrypted(
                String::from_utf8_lossy(cipher).into_owned(),
            ));
        }
        let n = r.u32()?;
        if n != 1 {
            return Err(SshKeyError::KeyCount(n));
        }
        let public = PublicKey::from_raw(parse_key_blob(r.string()?)?)?;
        let private = r.string()?;
        r.finish()?;
        if private.len() % NONE_BLOCK != 0 {
            return Err(SshKeyError::Malformed);
        }

        let mut p = Reader::new(private);
        if p.u32()? != p.u32()? {
            return Err(SshKeyError::CheckIntMismatch);
        }
        let ty = p.string()?;
        if ty != KEY_TYPE.as_bytes() {
            return Err(SshKeyError::WrongKeyType(
                String::from_utf8_lossy(ty).into_owned(),
            ));
        }
        if p.string()? != public.raw.as_slice() {
            return Err(SshKeyError::KeyMismatch);
        }
        let secret = p.string()?;
        if secret.len() != 64 {
            return Err(SshKeyError::Malformed);
        }
        let (seed, embedded_pub) = secret.split_at(32);
        if embedded_pub != public.raw.as_slice() {
            return Err(SshKeyError::KeyMismatch);
        }
        let comment =
            String::from_utf8(p.string()?.to_vec()).map_err(|_| SshKeyError::Malformed)?;
        for (i, b) in p.rest().iter().enumerate() {
            if usize::from(*b) != i + 1 {
                return Err(SshKeyError::Malformed);
            }
        }
        let seed: Zeroizing<[u8; 32]> =
            Zeroizing::new(seed.try_into().map_err(|_| SshKeyError::Malformed)?);
        let key = Self::from_seed(&seed);
        if key.public_key() != public {
            return Err(SshKeyError::KeyMismatch);
        }
        Ok((key, comment))
    }
}

#[cfg(test)]
#[path = "ssh_tests.rs"]
pub(crate) mod tests;
