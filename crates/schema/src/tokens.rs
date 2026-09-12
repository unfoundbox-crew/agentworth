use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign};

/// Anthropic's published price multiplier for a 5-minute prompt-cache write, relative to one
/// base input token. See `TokenUsage::cost_weighted_total`.
pub const CACHE_WRITE_5M_MULTIPLIER: f64 = 1.25;

/// Anthropic's published price multiplier for a prompt-cache read, relative to one base input
/// token. See `TokenUsage::cost_weighted_total`.
pub const CACHE_READ_MULTIPLIER: f64 = 0.1;

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

    /// Tokens weighted by what they actually cost, relative to one base input token.
    ///
    /// `total()` adds `cache_read_tokens` at face value, so on a long session it tracks how
    /// big the context got, not what the session spent -- a real subagent transcript measured
    /// 2026-09-10 held 19,042,532 cache-read tokens out of a 19,294,496 total, so the headline
    /// was 98.7% re-reads of an already-paid-for prompt.
    ///
    /// The multipliers are Anthropic's published prompt-caching ratios, relative to the base
    /// input price: a 5-minute cache write costs 1.25x and a cache read costs 0.1x. Verified
    /// 2026-09-10 against
    /// <https://platform.claude.com/docs/en/docs/build-with-claude/prompt-caching> ("Pricing").
    ///
    /// Two things this is deliberately NOT: it is not dollars (a token's base price varies by
    /// model, and this crate does not know the model here), and it is not exact for every
    /// cache entry -- a 1-hour cache write is 2x, not 1.25x, and Fable 5.1 / Mythos 5.1 read
    /// at 0.025x rather than 0.1x. `cache_creation_tokens` does not record which TTL was
    /// bought, so the common 5-minute case is assumed. Treat this as a comparable weight for
    /// ranking sessions against each other, not as a bill.
    pub fn cost_weighted_total(&self) -> u64 {
        let weighted = self.input_tokens as f64
            + self.output_tokens as f64
            + CACHE_WRITE_5M_MULTIPLIER * self.cache_creation_tokens as f64
            + CACHE_READ_MULTIPLIER * self.cache_read_tokens as f64;
        weighted as u64
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
