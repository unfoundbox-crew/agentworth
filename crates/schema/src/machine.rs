//! Box fingerprint, ported from SpacePilot so the two products assign the same
//! machine the same id. Mirrors `spacepilot/spacepilot/measurements.py:325-360`
//! (`_fingerprint`) and `spacepilot/spacepilot/paths.py:59-73` (`env_value`)
//! exactly: same env names, same precedence, same default salt.
//!
//! SpacePilot's `System.id` (`system_id_for`, measurements.py:325-329) is a
//! *configuration* id -- chip plus memory -- not a machine id. This is the
//! box: one hostname, one id, regardless of what is running on it.

use sha2::{Digest, Sha256};

/// Default salt if neither env var is set. Permanent: changing it would
/// reassign every existing machine a new identity.
pub const FINGERPRINT_SALT_DEFAULT: &str = "pluto-measurements-v1";

/// Canonical env var, checked first.
pub const FINGERPRINT_SALT_ENV: &str = "SPACEPILOT_FINGERPRINT_SALT";

/// Legacy env var name, checked if the canonical one is unset. Permanent
/// compatibility input, same as the canonical name.
pub const FINGERPRINT_SALT_ENV_LEGACY: &str = "PLUTO_FINGERPRINT_SALT";

fn fingerprint_salt() -> String {
    std::env::var(FINGERPRINT_SALT_ENV)
        .or_else(|_| std::env::var(FINGERPRINT_SALT_ENV_LEGACY))
        .unwrap_or_else(|_| FINGERPRINT_SALT_DEFAULT.to_string())
}

/// Truncated salted digest of this box's hostname. `None` when the hostname
/// cannot be read (matches Python's `_fingerprint` returning `None` on any
/// failure or an empty hostname). Shells out to `hostname(1)` rather than
/// pulling in a hostname crate: same source `platform.node()` reads on
/// Unix (a `uname`/`gethostname` call), no new dependency.
pub fn host_fingerprint() -> Option<String> {
    let output = std::process::Command::new("hostname").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let host = String::from_utf8(output.stdout).ok()?;
    let host = host.trim();
    if host.is_empty() {
        return None;
    }
    Some(host_fingerprint_for(host, &fingerprint_salt()))
}

/// Same digest, for a caller that already has `host` and `salt` (tests, or a
/// caller that read the salt itself).
pub fn host_fingerprint_for(host: &str, salt: &str) -> String {
    let digest = Sha256::digest(format!("{salt}:{host}").as_bytes());
    hex::encode(digest).chars().take(12).collect()
}

/// This machine: box fingerprint plus OS/arch, so a session or measurement
/// can be joined to "which machine" without either product learning about
/// the other's schema.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MachineInfo {
    pub host_fingerprint: Option<String>,
    pub os: String,
    pub arch: String,
}

impl MachineInfo {
    pub fn probe() -> Self {
        Self {
            host_fingerprint: host_fingerprint(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expected value computed on this machine via:
    /// `python3 -c "import hashlib; print(hashlib.sha256(f'pluto-measurements-v1:test-host.example'.encode()).hexdigest()[:12])"`
    /// -> `dcebd5876c09`. Same salt, same host, same algorithm as
    /// `spacepilot/spacepilot/measurements.py:_fingerprint`.
    #[test]
    fn matches_python_fingerprint() {
        assert_eq!(
            host_fingerprint_for("test-host.example", FINGERPRINT_SALT_DEFAULT),
            "dcebd5876c09"
        );
    }

    #[test]
    fn twelve_hex_chars() {
        let fp = host_fingerprint_for("any-host", "any-salt");
        assert_eq!(fp.len(), 12);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn probe_returns_os_and_arch() {
        let info = MachineInfo::probe();
        assert_eq!(info.os, std::env::consts::OS);
        assert_eq!(info.arch, std::env::consts::ARCH);
    }
}
