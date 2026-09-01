//! Fallback high-entropy secret detector.
//!
//! Complements the named regex rules in [`crate::rules`]: those catch secrets with
//! a known shape (a recognizable prefix, a fixed length). This catches the rest —
//! a novel API key format, a random token, anything that's clearly not natural
//! text but doesn't match any known vendor pattern.
//!
//! The hard part isn't finding high-entropy substrings; it's *not* flagging the
//! high-entropy substrings that are already everywhere in this exact domain: git
//! SHAs, UUIDs (session IDs, event IDs), and SHA-256 content fingerprints. All
//! three are, by construction, close to uniformly random over their alphabet —
//! which is indistinguishable from "random secret" by entropy alone. See
//! [`is_probable_secret`] for how that's handled.

/// Tuning knobs for [`is_probable_secret`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntropyConfig {
    /// Minimum candidate length in bytes before it's even considered. Candidates
    /// shorter than this are where a random token's entropy and a natural-language
    /// identifier's entropy overlap the most (verified empirically, not guessed) —
    /// below this length we simply don't attempt a call.
    pub min_length: usize,
    /// Minimum Shannon entropy, in bits per character, required to flag a candidate.
    pub min_bits_per_char: f64,
}

impl Default for EntropyConfig {
    /// `min_length: 24`, `min_bits_per_char: 4.5`.
    ///
    /// Calibrated against a 400k-sample Monte Carlo of realistic 2-4-word
    /// camelCase/PascalCase/snake_case identifiers built from this codebase's own
    /// vocabulary (session, fingerprint, derive, threshold, handler, ...): the
    /// worst-case identifier found was `fieldStorageChunkEmpty0` at 4.350 bits/char.
    /// 4.5 leaves real margin above that ceiling. Real mixed-case random secrets of
    /// 32+ characters clear it well over 85% of the time; shorter or
    /// lowercase-plus-digit-only secrets are caught less reliably, which is an
    /// accepted trade-off — those are exactly the shapes the 13 named regex rules
    /// already cover well (`sk-...`, `ghp_...`, `AKIA...`, `AIza...`, etc).
    fn default() -> Self {
        Self {
            min_length: 24,
            min_bits_per_char: 4.5,
        }
    }
}

/// Regex source for finding raw candidate substrings worth scoring.
///
/// Deliberately excludes `_` and `-` from the alphabet: those are the near-universal
/// word separators in this domain's own routine identifiers (snake_case functions
/// like `find_sessions_for_blame`, kebab-case CLI flags, session slugs like
/// `hermes-session-007`). Excluding them from the match means those split into
/// short, harmless fragments instead of one long low-information "word soup" that
/// can read as high-entropy purely from concatenating several distinct words.
/// Known formats that use `_`/`-` only as a prefix separator (`ghp_...`,
/// `sk-ant-...`) still get their random body scored as its own, now-undiluted,
/// candidate — splitting only ever helps isolate the random part.
///
/// The trade-off: a novel secret format that uses `_`/`-` *inside* its random body
/// (e.g. base64url, which substitutes `-`/`_` for `+`/`/`) can get fragmented into
/// pieces short enough to fall under `min_length` and be missed. Accepted, given
/// this is a best-effort fallback net behind 13 named regex rules, not the only
/// line of defense.
pub const CANDIDATE_PATTERN: &str = r"[A-Za-z0-9+/=]{24,}";

/// Shannon entropy of `s`, in bits per byte. `s` is assumed ASCII (true of every
/// candidate this module scores, since [`CANDIDATE_PATTERN`] only matches ASCII).
pub fn shannon_entropy_bits_per_char(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut counts = [0u32; 256];
    for b in s.bytes() {
        counts[b as usize] += 1;
    }
    let len = s.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .fold(0.0, |acc, &c| {
            let p = c as f64 / len;
            acc - p * p.log2()
        })
}

/// True if every byte of `s` is an ASCII hex digit (either case).
///
/// A cryptographic hash's hex encoding is, by design, close to uniformly random
/// over the hex alphabet — that's what makes it a good hash. Shannon entropy over
/// `[0-9a-f]` cannot tell "git SHA" or "SHA-256 content fingerprint" apart from
/// "hex-encoded secret". Rather than chase that with a threshold, hex candidates
/// are excluded from this detector entirely; known hex-shaped secret formats (AWS
/// access keys, etc.) already have their own regex rule earlier in the pipeline.
fn is_pure_hex(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Number of distinct character classes among {lowercase, uppercase, digit} present
/// in `s`. Used as a cheap, independent second gate alongside entropy: natural-text
/// identifiers are overwhelmingly single-case (snake_case, kebab-case, CONSTANT_CASE)
/// or two-case-no-digit (camelCase), while real secrets usually mix at least two.
fn char_class_count(s: &str) -> u8 {
    let has_lower = s.bytes().any(|b| b.is_ascii_lowercase());
    let has_upper = s.bytes().any(|b| b.is_ascii_uppercase());
    let has_digit = s.bytes().any(|b| b.is_ascii_digit());
    has_lower as u8 + has_upper as u8 + has_digit as u8
}

/// Decides whether `candidate` (already matched by [`CANDIDATE_PATTERN`]) looks like
/// a plausible random secret rather than a routine high-entropy-*looking* identifier
/// (git SHA, UUID, content hash, or a chained natural-language identifier).
pub fn is_probable_secret(candidate: &str, config: &EntropyConfig) -> bool {
    candidate.len() >= config.min_length
        && !is_pure_hex(candidate)
        && char_class_count(candidate) >= 2
        && shannon_entropy_bits_per_char(candidate) >= config.min_bits_per_char
}
