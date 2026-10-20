//! Signing and verification for AgentWorth Work Receipts.
//!
//! A Work Receipt (`agentworth_schema::work_receipt`) is an in-toto Statement v1 carried in a
//! [DSSE] envelope and signed with an ed25519 key stored in OpenSSH format. This crate holds
//! every piece of that crypto so `ed25519-dalek` never reaches the scanner or adapter crates:
//!
//! - [`pae`]: DSSE pre-authentication encoding, the exact bytes that are signed.
//! - [`ssh`]: hand-rolled OpenSSH `ssh-ed25519` public keys, `SHA256:` fingerprints (the DSSE
//!   `keyid`), and unencrypted `openssh-key-v1` private keys. No `ssh-keygen` subprocess.
//! - [`envelope`]: sign a canonical Statement, and verify an envelope into one of the three
//!   signature states the evidence check reports (spec §2.4).
//!
//! No CLI surface lives here; `archie repo receipt …` (a later PR) is the only caller.
//!
//! [DSSE]: https://github.com/secure-systems-lab/dsse/blob/master/protocol.md

pub mod envelope;
pub mod pae;
pub mod ssh;

pub use envelope::{
    sign_payload, sign_statement, verify_envelope, Envelope, EnvelopeSignature, InvalidReason,
    KeyLink, SignError, SignatureVerdict, SignedReceipt, Verification, VerifiedReceipt,
};
pub use pae::pae;
pub use ssh::{PublicKey, ReceiptSigningKey, SshKeyError};
