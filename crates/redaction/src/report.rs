use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// High-level categories for redacted sensitive items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactionCategory {
    ApiKey,
    EnvVar,
    FilePath,
    Email,
    Credential,
    JwtToken,
    IpAddress,
    PrivateKey,
    HighEntropySecret,
    Custom,
}

impl std::fmt::Display for RedactionCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApiKey => write!(f, "API Key"),
            Self::EnvVar => write!(f, "Environment Variable"),
            Self::FilePath => write!(f, "File Path (Home / Username)"),
            Self::Email => write!(f, "Email Address"),
            Self::Credential => write!(f, "URL / Database Credentials"),
            Self::JwtToken => write!(f, "JWT Token"),
            Self::IpAddress => write!(f, "Private IP Address"),
            Self::PrivateKey => write!(f, "Private Key"),
            Self::HighEntropySecret => write!(f, "High-Entropy Secret"),
            Self::Custom => write!(f, "Custom Rule"),
        }
    }
}

/// Detailed report on sensitive items detected and redacted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactionReport {
    pub total_redactions: usize,
    pub api_keys_count: usize,
    pub env_vars_count: usize,
    pub paths_count: usize,
    pub emails_count: usize,
    pub credentials_count: usize,
    pub jwt_tokens_count: usize,
    pub ip_addresses_count: usize,
    pub private_keys_count: usize,
    #[serde(default)]
    pub high_entropy_secrets_count: usize,
    pub custom_count: usize,
    #[serde(default)]
    pub breakdown_by_category: BTreeMap<String, usize>,
}

impl RedactionReport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Total number of redacted elements.
    pub fn total(&self) -> usize {
        self.total_redactions
    }

    /// Returns true if no sensitive items were detected.
    pub fn is_clean(&self) -> bool {
        self.total_redactions == 0
    }

    /// Add a redaction count for a given category.
    pub fn add(&mut self, category: RedactionCategory, count: usize) {
        if count == 0 {
            return;
        }
        self.total_redactions += count;
        match category {
            RedactionCategory::ApiKey => self.api_keys_count += count,
            RedactionCategory::EnvVar => self.env_vars_count += count,
            RedactionCategory::FilePath => self.paths_count += count,
            RedactionCategory::Email => self.emails_count += count,
            RedactionCategory::Credential => self.credentials_count += count,
            RedactionCategory::JwtToken => self.jwt_tokens_count += count,
            RedactionCategory::IpAddress => self.ip_addresses_count += count,
            RedactionCategory::PrivateKey => self.private_keys_count += count,
            RedactionCategory::HighEntropySecret => self.high_entropy_secrets_count += count,
            RedactionCategory::Custom => self.custom_count += count,
        }
        *self
            .breakdown_by_category
            .entry(category.to_string())
            .or_insert(0) += count;
    }

    /// Merge another report into this one.
    pub fn merge(&mut self, other: &RedactionReport) {
        self.total_redactions += other.total_redactions;
        self.api_keys_count += other.api_keys_count;
        self.env_vars_count += other.env_vars_count;
        self.paths_count += other.paths_count;
        self.emails_count += other.emails_count;
        self.credentials_count += other.credentials_count;
        self.jwt_tokens_count += other.jwt_tokens_count;
        self.ip_addresses_count += other.ip_addresses_count;
        self.private_keys_count += other.private_keys_count;
        self.high_entropy_secrets_count += other.high_entropy_secrets_count;
        self.custom_count += other.custom_count;

        for (k, v) in &other.breakdown_by_category {
            *self.breakdown_by_category.entry(k.clone()).or_insert(0) += v;
        }
    }
}
