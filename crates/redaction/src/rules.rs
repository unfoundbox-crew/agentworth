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

/// Builds a rule that masks literal occurrences of a session's own repository/workspace
/// identity (e.g. "unfoundbox/agentworth", from [`agentworth_schema::extract_repository_or_workspace`]).
/// A project name isn't shaped like a secret, so none of the rules in [`default_rules`] catch
/// it -- but AGENTS.md's redaction policy says exports must never leak repository or project
/// names, and this is the one identifier that's the same every time a given session gets
/// redacted, so it's worth masking on top of the generic rules rather than folding it into them.
///
/// Returns `None` for the "unknown"/"plugins/cache" sentinel values `extract_repository_or_workspace`
/// returns when it can't derive real identity -- those aren't project names to protect, and a
/// literal-match rule on "unknown" would over-redact any session whose content happens to
/// contain that common word.
pub fn repository_identity_rule(repo_or_workspace: &str) -> Option<RedactionRule> {
    if repo_or_workspace.is_empty()
        || repo_or_workspace == "unknown"
        || repo_or_workspace == "plugins/cache"
    {
        return None;
    }
    let pattern = Regex::new(&format!(r"(?i){}", regex::escape(repo_or_workspace))).ok()?;
    Some(RedactionRule::new(
        "repository_or_workspace_identity",
        RedactionCategory::Custom,
        pattern,
        "[REDACTED_REPOSITORY]",
    ))
}

/// Builds the default suite of redaction rules covering API keys, env vars, paths, emails, etc.
/// The rules `default_rules()` deliberately does NOT apply, added on top of it by
/// [`crate::Redactor::for_publication`] wherever redacted text leaves the machine.
///
/// The default profile's path rule strips a home directory's USERNAME and stops there
/// (`/Users/alice/code/acme/api` -> `~/code/acme/api`). That is the right answer for every
/// local surface: `repo_blame` and `session_show` exist to tell an operator which repository a
/// session touched, and a profile that blanked the repo name would make them useless. It is the
/// wrong answer the moment the same string is posted publicly -- measured 2026-09-10, a
/// `blunder --submit` payload captured against a local listener still carried repository and
/// organization names after full default redaction.
///
/// So this is additive and narrow: it removes what follows `~`, which is where the org and repo
/// names sit once the username is gone. It runs AFTER the home-directory rules, on their output,
/// which is why `Redactor::for_publication` appends rather than replaces.
///
/// What it does not claim to catch: an organization or repository named in free prose rather
/// than in a path ("we pushed to acme/api"). There is no general rule for that -- a name is only
/// identifiable as a name in context. Publication is an operator decision, and this raises the
/// floor rather than making the payload provably clean.
pub fn publication_rules() -> Vec<RedactionRule> {
    vec![
        // Everything after `~`, which by this point is what the home-directory rules left
        // behind. Anchored on `~/` so a bare tilde in prose ("~5 minutes") is untouched, and
        // stopping at whitespace or a quote so it takes one path and not the rest of the line.
        RedactionRule::new(
            "publication_unix_path_tail",
            RedactionCategory::FilePath,
            Regex::new(r#"~/[^\s"'`;|&)\]]*"#).expect("valid regex"),
            "~/[PATH]",
        ),
        RedactionRule::new(
            "publication_windows_path_tail",
            RedactionCategory::FilePath,
            Regex::new(r#"~\\[^\s"'`;|&)\]]*"#).expect("valid regex"),
            r"~\[PATH]",
        ),
        // A repository named by its forge URL or `owner/repo` shorthand next to a forge host.
        RedactionRule::new(
            "publication_forge_repo",
            RedactionCategory::FilePath,
            Regex::new(r"(?i)\b((?:github|gitlab|bitbucket)\.com)[:/][A-Za-z0-9._-]+/[A-Za-z0-9._-]+")
                .expect("valid regex"),
            "${1}/[REPO]",
        ),
    ]
}

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
