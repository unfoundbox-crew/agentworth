use crate::entropy::{self, EntropyConfig};
use crate::report::{RedactionCategory, RedactionReport};
use regex::{Captures, Regex};
use std::borrow::Cow;

/// Single redaction rule defining a regular expression and replacement template.
///
/// Most rules match unconditionally: any regex hit is a redaction. The high-entropy
/// detector is the exception — its regex only finds coarse *candidates* (long
/// alphanumeric-ish runs), and `entropy_filter`, when set, decides which of those
/// candidates actually look like random secrets rather than routine hex/identifier
/// text. See [`RedactionRule::with_entropy_filter`].
#[derive(Debug, Clone)]
pub struct RedactionRule {
    pub name: String,
    pub category: RedactionCategory,
    pub pattern: Regex,
    pub replacement: String,
    entropy_filter: Option<EntropyConfig>,
}

impl RedactionRule {
    pub fn new(
        name: impl Into<String>,
        category: RedactionCategory,
        pattern: Regex,
        replacement: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            category,
            pattern,
            replacement: replacement.into(),
            entropy_filter: None,
        }
    }

    /// Turns this rule into a high-entropy detector: `pattern` still finds coarse
    /// candidate substrings, but each match is only treated as a redaction when it
    /// also passes the Shannon-entropy "plausible random secret" shape check
    /// described in [`crate::entropy`].
    pub fn with_entropy_filter(mut self, config: EntropyConfig) -> Self {
        self.entropy_filter = Some(config);
        self
    }

    /// Apply this rule to the given text, optionally recording counts to the report.
    pub fn apply<'a>(
        &self,
        text: &'a str,
        report: &mut Option<&mut RedactionReport>,
    ) -> Cow<'a, str> {
        match self.entropy_filter {
            Some(config) => {
                let mut matched = 0usize;
                let replacement = self.replacement.as_str();
                let result = self.pattern.replace_all(text, |caps: &Captures<'_>| {
                    let candidate = &caps[0];
                    if entropy::is_probable_secret(candidate, &config) {
                        matched += 1;
                        replacement.to_string()
                    } else {
                        candidate.to_string()
                    }
                });
                if matched > 0 {
                    if let Some(r) = report {
                        r.add(self.category, matched);
                    }
                }
                result
            }
            None => {
                let count = self.pattern.find_iter(text).count();
                if count > 0 {
                    if let Some(r) = report {
                        r.add(self.category, count);
                    }
                    self.pattern.replace_all(text, self.replacement.as_str())
                } else {
                    Cow::Borrowed(text)
                }
            }
        }
    }
}

/// Builds the default suite of redaction rules covering API keys, env vars, paths, emails, etc.
pub fn default_rules() -> Vec<RedactionRule> {
    vec![
        // 1. PEM Private Keys
        RedactionRule::new(
            "pem_private_key",
            RedactionCategory::PrivateKey,
            Regex::new(r"(?s)-----BEGIN\s+[A-Z\s]+PRIVATE\s+KEY-----.*?-----END\s+[A-Z\s]+PRIVATE\s+KEY-----")
                .expect("valid regex"),
            "[REDACTED_PRIVATE_KEY]",
        ),
        // 2. JWT Tokens
        RedactionRule::new(
            "jwt_token",
            RedactionCategory::JwtToken,
            Regex::new(r"\beyJ[a-zA-Z0-9_\-]{10,}\.eyJ[a-zA-Z0-9_\-]{10,}\.[a-zA-Z0-9_\-]{10,}\b")
                .expect("valid regex"),
            "[REDACTED_JWT]",
        ),
        // 3. Anthropic API Keys (checked before generic OpenAI)
        RedactionRule::new(
            "anthropic_api_key",
            RedactionCategory::ApiKey,
            Regex::new(r"\bsk-ant-[a-zA-Z0-9_\-]{20,}\b").expect("valid regex"),
            "[REDACTED_API_KEY]",
        ),
        // 4. OpenAI API Keys (sk-..., sk-proj-..., sk-admin-..., etc.)
        RedactionRule::new(
            "openai_api_key",
            RedactionCategory::ApiKey,
            Regex::new(r"\bsk-(?:proj-|admin-|org-)?[a-zA-Z0-9_\-]{20,}\b").expect("valid regex"),
            "[REDACTED_API_KEY]",
        ),
        // 5. Google API Keys
        RedactionRule::new(
            "google_api_key",
            RedactionCategory::ApiKey,
            Regex::new(r"\bAIza[0-9A-Za-z\-_]{30,45}\b").expect("valid regex"),
            "[REDACTED_API_KEY]",
        ),
        // 6. GitHub Tokens (personal, classic, fine-grained, app, OAuth)
        RedactionRule::new(
            "github_token",
            RedactionCategory::ApiKey,
            Regex::new(r"\b(?:ghp|gho|ghu|ghs|ghr)_[a-zA-Z0-9]{30,50}\b|\bgithub_pat_[a-zA-Z0-9_]{60,100}\b")
                .expect("valid regex"),
            "[REDACTED_GITHUB_TOKEN]",
        ),
        // 7. AWS Access Keys
        RedactionRule::new(
            "aws_access_key",
            RedactionCategory::ApiKey,
            Regex::new(r"\bAKIA[0-9A-Z]{16}\b").expect("valid regex"),
            "[REDACTED_AWS_ACCESS_KEY]",
        ),
        // 8. Bearer Authorization Tokens
        RedactionRule::new(
            "bearer_token",
            RedactionCategory::ApiKey,
            Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9_\-\.]{20,}\b").expect("valid regex"),
            "Bearer [REDACTED_TOKEN]",
        ),
        // 9. Sensitive Env Vars
        RedactionRule::new(
            "sensitive_env_vars",
            RedactionCategory::EnvVar,
            Regex::new(r#"(?i)\b([a-zA-Z0-9_]*(?:PASSWORD|PASSWD|SECRET(?:_KEY)?|AUTH_TOKEN|ACCESS_TOKEN|API_KEY|API_SECRET|PRIVATE_KEY|DATABASE_URL|DB_PASSWORD|REDIS_URL|MONGODB_URI|AWS_SECRET_ACCESS_KEY|CLIENT_SECRET|SESSION_SECRET|ENCRYPTION_KEY))\s*[:=]\s*(?:"([^"]*)"|'([^']*)'|([^\s,;'"\n\r]+))"#)
                .expect("valid regex"),
            "$1=[REDACTED_ENV_VAR]",
        ),
        // 10. URLs with Embedded Credentials
        RedactionRule::new(
            "credential_url",
            RedactionCategory::Credential,
            Regex::new(r"(?i)\b([a-zA-Z][a-zA-Z0-9+.-]*://)([^:\s/@]+):([^@\s/]+)@").expect("valid regex"),
            "${1}[REDACTED_CREDENTIALS]@",
        ),
        // 11. User Home Directories (macOS, Linux, Windows)
        RedactionRule::new(
            "macos_home_path",
            RedactionCategory::FilePath,
            Regex::new(r"(?i)/Users/[a-zA-Z0-9._-]+").expect("valid regex"),
            "~",
        ),
        RedactionRule::new(
            "linux_home_path",
            RedactionCategory::FilePath,
            Regex::new(r"(?i)/home/[a-zA-Z0-9._-]+").expect("valid regex"),
            "~",
        ),
        RedactionRule::new(
            "windows_home_path",
            RedactionCategory::FilePath,
            Regex::new(r"(?i)[a-zA-Z]:\\Users\\[a-zA-Z0-9._-]+").expect("valid regex"),
            "~",
        ),
        // 12. Email Addresses
        RedactionRule::new(
            "email_address",
            RedactionCategory::Email,
            Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b").expect("valid regex"),
            "[REDACTED_EMAIL]",
        ),
        // 13. Private IPv4 Addresses
        RedactionRule::new(
            "private_ip_address",
            RedactionCategory::IpAddress,
            Regex::new(r"\b(?:10\.\d{1,3}\.\d{1,3}\.\d{1,3}|172\.(?:1[6-9]|2[0-9]|3[0-1])\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3})\b")
                .expect("valid regex"),
            "[REDACTED_IP]",
        ),
        // 14. High-Entropy Secrets — fallback net for anything the named patterns
        // above miss. Runs last, deliberately: by the time text reaches this rule,
        // every known-format secret has already been redacted, so this only ever
        // scores what survived. See `crate::entropy` for the false-positive
        // avoidance (git SHAs, UUIDs, content hashes must not be flagged here).
        RedactionRule::new(
            "high_entropy_secret",
            RedactionCategory::HighEntropySecret,
            Regex::new(entropy::CANDIDATE_PATTERN).expect("valid regex"),
            "[REDACTED_HIGH_ENTROPY_SECRET]",
        )
        .with_entropy_filter(EntropyConfig::default()),
    ]
}
