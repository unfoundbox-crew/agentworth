//! DSSE envelopes for Work Receipts: sign a Statement, verify an envelope.
//!
//! Verification ends in exactly one of the three signature states the evidence check reports
//! (spec §2.4):
//!
//! | state | meaning | effect on rungs |
//! | :-- | :-- | :-- |
//! | [`SignatureVerdict::SignedBy`] | valid, and the key is one of `@login`'s GitHub signing keys | may be "attested locally" |
//! | [`SignatureVerdict::KeyNotLinked`] | valid, key unknown to the caller's key set | integrity holds, identity does not |
//! | [`SignatureVerdict::Invalid`] | anything else | every rung drops to "claimed" |
//!
//! The signature is checked over the exact payload bytes (DSSE never re-canonicalizes), and the
//! Statement handed back is parsed from those same bytes.

use agentworth_schema::work_receipt::{
    canonical_json, receipt_id_of, WorkReceiptError, WorkReceiptStatement, IN_TOTO_PAYLOAD_TYPE,
    MAX_RECEIPT_BYTES,
};
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine as _;
use ed25519_dalek::Signature;
use serde::{Deserialize, Serialize};

use crate::pae::pae;
use crate::ssh::{PublicKey, ReceiptSigningKey};

/// A DSSE envelope (`envelope.md`). This is what is stored in the git note or receipt file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    #[serde(rename = "payloadType")]
    pub payload_type: String,
    /// Base64 of the canonical Statement bytes.
    pub payload: String,
    pub signatures: Vec<EnvelopeSignature>,
}

/// One DSSE signature. `keyid` is an unauthenticated hint (`SHA256:<openssh fingerprint>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvelopeSignature {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub keyid: String,
    pub sig: String,
}

impl Envelope {
    /// Canonical JSON of the envelope (stable bytes for a note or file).
    pub fn to_json(&self) -> Vec<u8> {
        let value = serde_json::to_value(self).expect("envelope serializes");
        canonical_json(&value).expect("envelope holds no floats")
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

/// A `(login, key)` pair from GitHub's public `GET /users/{login}/ssh_signing_keys`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyLink {
    pub login: String,
    pub key: PublicKey,
}

/// Why signing refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SignError {
    #[error(transparent)]
    Receipt(#[from] WorkReceiptError),
    #[error("predicate.signer.public_key is not the signing key's public key")]
    SignerMismatch,
}

/// The output of [`sign_statement`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedReceipt {
    pub envelope: Envelope,
    /// `hex(sha256(payload))`.
    pub receipt_id: String,
    /// The Statement exactly as signed (after the size cap, so `truncated` may be set).
    pub statement: WorkReceiptStatement,
}

/// Validate, apply the 64 KiB cap, canonicalize and sign. The Statement must already name
/// `key` as its signer (it is also the key every session ref was derived from).
pub fn sign_statement(
    statement: &WorkReceiptStatement,
    key: &ReceiptSigningKey,
) -> Result<SignedReceipt, SignError> {
    let public = key.public_key();
    if statement.predicate.signer.public_key != public.to_openssh() {
        return Err(SignError::SignerMismatch);
    }
    statement.validate()?;
    let mut statement = statement.clone();
    let payload = statement.fit_to_size_cap()?;
    let envelope = sign_payload(IN_TOTO_PAYLOAD_TYPE, &payload, key);
    Ok(SignedReceipt {
        envelope,
        receipt_id: receipt_id_of(&payload),
        statement,
    })
}

/// Low-level DSSE sign of arbitrary bytes. Prefer [`sign_statement`].
pub fn sign_payload(payload_type: &str, payload: &[u8], key: &ReceiptSigningKey) -> Envelope {
    let sig = key.sign(&pae(payload_type, payload));
    Envelope {
        payload_type: payload_type.to_string(),
        payload: STANDARD.encode(payload),
        signatures: vec![EnvelopeSignature {
            keyid: key.public_key().fingerprint(),
            sig: STANDARD.encode(sig),
        }],
    }
}

/// Why an envelope is [`SignatureVerdict::Invalid`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidReason {
    /// `payloadType` is not `application/vnd.in-toto+json`.
    UnsupportedPayloadType,
    /// `payload` or a `sig` is not base64.
    BadBase64,
    /// Payload over the 64 KiB cap.
    TooLarge,
    /// Payload is not a valid Work Receipt v1 Statement.
    MalformedStatement(String),
    /// `predicate.signer.public_key` is not a valid `ssh-ed25519` key.
    BadSignerKey,
    /// No signature verifies under the receipt's signer key.
    SignatureMismatch,
    /// The signature verifies but the payload is not in canonical form, so its `receipt_id`
    /// would not be stable.
    NonCanonicalPayload,
}

/// The three states of spec §2.4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureVerdict {
    /// "Signed by @login": valid, and the key is one of that user's GitHub signing keys.
    SignedBy { login: String, keyid: String },
    /// "Signed, key not linked": valid, key unknown. Integrity holds, identity does not.
    KeyNotLinked { keyid: String },
    /// "Signature invalid": every rung in the receipt drops to "claimed".
    Invalid(InvalidReason),
}

impl SignatureVerdict {
    pub fn is_valid(&self) -> bool {
        !matches!(self, Self::Invalid(_))
    }

    /// True when nothing in the receipt may rise above "claimed only".
    pub fn rungs_claimed_only(&self) -> bool {
        !self.is_valid()
    }
}

/// A receipt whose signature verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedReceipt {
    pub statement: WorkReceiptStatement,
    pub receipt_id: String,
    pub signer: PublicKey,
}

/// Result of [`verify_envelope`]. `receipt` is `Some` exactly when the verdict is valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verification {
    pub verdict: SignatureVerdict,
    pub receipt: Option<VerifiedReceipt>,
}

impl Verification {
    fn invalid(reason: InvalidReason) -> Self {
        Self {
            verdict: SignatureVerdict::Invalid(reason),
            receipt: None,
        }
    }
}

/// DSSE allows standard or URL-safe base64, padded or not; verifiers must accept either.
fn decode_b64(s: &str) -> Option<Vec<u8>> {
    [STANDARD, URL_SAFE, STANDARD_NO_PAD, URL_SAFE_NO_PAD]
        .iter()
        .find_map(|e| e.decode(s).ok())
}

/// Verify `envelope` and classify it against `links` (the GitHub signing keys of the logins
/// the caller cares about). Never panics, never touches the network.
pub fn verify_envelope(envelope: &Envelope, links: &[KeyLink]) -> Verification {
    if envelope.payload_type != IN_TOTO_PAYLOAD_TYPE {
        return Verification::invalid(InvalidReason::UnsupportedPayloadType);
    }
    let Some(payload) = decode_b64(&envelope.payload) else {
        return Verification::invalid(InvalidReason::BadBase64);
    };
    if payload.len() > MAX_RECEIPT_BYTES {
        return Verification::invalid(InvalidReason::TooLarge);
    }
    let statement = match WorkReceiptStatement::from_payload(&payload) {
        Ok(s) => s,
        Err(e) => return Verification::invalid(InvalidReason::MalformedStatement(e.to_string())),
    };
    let Ok(signer) = PublicKey::parse_openssh(&statement.predicate.signer.public_key) else {
        return Verification::invalid(InvalidReason::BadSignerKey);
    };
    let keyid = signer.fingerprint();
    let message = pae(&envelope.payload_type, &payload);
    let vk = signer.verifying_key();
    let verified = envelope.signatures.iter().any(|s| {
        if !s.keyid.is_empty() && s.keyid != keyid {
            return false;
        }
        let Some(sig) = decode_b64(&s.sig) else {
            return false;
        };
        let Ok(sig) = Signature::from_slice(&sig) else {
            return false;
        };
        vk.verify_strict(&message, &sig).is_ok()
    });
    if !verified {
        return Verification::invalid(InvalidReason::SignatureMismatch);
    }
    match statement.canonical_bytes() {
        Ok(c) if c == payload => {}
        _ => return Verification::invalid(InvalidReason::NonCanonicalPayload),
    }
    let verdict = match links.iter().find(|l| l.key == signer) {
        Some(l) => SignatureVerdict::SignedBy {
            login: l.login.clone(),
            keyid,
        },
        None => SignatureVerdict::KeyNotLinked { keyid },
    };
    Verification {
        verdict,
        receipt: Some(VerifiedReceipt {
            statement,
            receipt_id: receipt_id_of(&payload),
            signer,
        }),
    }
}

#[cfg(test)]
#[path = "envelope_tests.rs"]
mod tests;
