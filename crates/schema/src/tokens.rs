use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign};

/// Token accounting metrics for model invocations and sessions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Number of input / prompt tokens.
    pub input_tokens: u64,
    /// Number of output / completion tokens.
    pub output_tokens: u64,
    /// Number of tokens read from prompt cache.
    pub cache_read_tokens: u64,
    /// Number of tokens written into prompt cache creation.
    pub cache_creation_tokens: u64,
}

impl TokenUsage {
    /// Create a new TokenUsage instance.
    pub fn new(
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_creation_tokens: u64,
    ) -> Self {
        Self {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
        }
    }

    /// Total tokens across all categories.
    pub fn total(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.cache_read_tokens)
            .saturating_add(self.cache_creation_tokens)
    }

    /// Net billable tokens or standard input+output.
    pub fn standard_total(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
}

impl Add for TokenUsage {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            input_tokens: self.input_tokens.saturating_add(rhs.input_tokens),
            output_tokens: self.output_tokens.saturating_add(rhs.output_tokens),
            cache_read_tokens: self.cache_read_tokens.saturating_add(rhs.cache_read_tokens),
            cache_creation_tokens: self
                .cache_creation_tokens
                .saturating_add(rhs.cache_creation_tokens),
        }
    }
}

impl AddAssign for TokenUsage {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_usage_addition() {
        let mut a = TokenUsage::new(100, 50, 20, 10);
        let b = TokenUsage::new(200, 150, 0, 30);

        let c = a + b;
        assert_eq!(c.input_tokens, 300);
        assert_eq!(c.output_tokens, 200);
        assert_eq!(c.cache_read_tokens, 20);
        assert_eq!(c.cache_creation_tokens, 40);
        assert_eq!(c.total(), 560);
        assert_eq!(c.standard_total(), 500);

        a += b;
        assert_eq!(a, c);
    }

    #[test]
    fn test_serde_token_usage() {
        let usage = TokenUsage::new(1000, 200, 50, 10);
        let json = serde_json::to_string(&usage).expect("serialize");
        let parsed: TokenUsage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(usage, parsed);
    }
}
