//! Comprehensive per-model token pricing table and cost calculator.
//!
//! Provides granular per-model pricing rates ($ / 1M tokens) for all major model families
//! (Claude, GPT, Gemini, DeepSeek, Grok, Qwen, GLM, MiniMax) with support for prompt caching rates.

use serde::{Deserialize, Serialize};

/// Pricing rates in USD per 1 Million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ModelRates {
    pub model_pattern: &'static str,
    pub input_per_m: f64,
    pub output_per_m: f64,
    pub cache_read_per_m: f64,
    pub cache_write_per_m: f64,
}

impl Default for ModelRates {
    fn default() -> Self {
        // Default fallback to Claude 3.5 Sonnet standard pricing
        Self {
            model_pattern: "default",
            input_per_m: 3.00,
            output_per_m: 15.00,
            cache_read_per_m: 0.30,
            cache_write_per_m: 3.75,
        }
    }
}

/// Known model pricing table cited from official public API pricing documents.
///
/// Current-generation entries (read 2026-09-02, `input_per_m`/`output_per_m`/`cache_read_per_m`
/// use the provider's standard base rate; `cache_write_per_m` uses the 5-minute cache write
/// rate where the provider distinguishes one, else the base input rate):
/// - Anthropic: https://platform.claude.com/docs/en/about-claude/pricing
/// - Google Gemini: https://ai.google.dev/gemini-api/docs/pricing
/// - DeepSeek: https://api-docs.deepseek.com/quick_start/pricing (off-peak rate; off-peak
///   covers all but a 7-hour/week UTC window, so it is the rate that applies almost always)
/// - Zhipu/Z.ai GLM: https://docs.z.ai/guides/overview/pricing (current promotional rate for
///   GLM-5.3-Flash; the pre-promo rate is $0.15/$0.03/$0.50, see PR discussion if it lapses)
pub const MODEL_PRICING_TABLE: &[ModelRates] = &[
    // --- Anthropic Claude Family (current generation; more specific patterns first) ---
    ModelRates {
        model_pattern: "claude-fable-5-1",
        input_per_m: 10.00,
        output_per_m: 50.00,
        cache_read_per_m: 0.25,
        cache_write_per_m: 12.50,
    },
    ModelRates {
        model_pattern: "claude-fable-5",
        input_per_m: 10.00,
        output_per_m: 50.00,
        cache_read_per_m: 1.00,
        cache_write_per_m: 12.50,
    },
    ModelRates {
        model_pattern: "claude-opus-5",
        input_per_m: 5.00,
        output_per_m: 25.00,
        cache_read_per_m: 0.50,
        cache_write_per_m: 6.25,
    },
    ModelRates {
        model_pattern: "claude-opus-4-8",
        input_per_m: 5.00,
        output_per_m: 25.00,
        cache_read_per_m: 0.50,
        cache_write_per_m: 6.25,
    },
    ModelRates {
        model_pattern: "claude-opus-4-6",
        input_per_m: 5.00,
        output_per_m: 25.00,
        cache_read_per_m: 0.50,
        cache_write_per_m: 6.25,
    },
    ModelRates {
        model_pattern: "claude-sonnet-5",
        input_per_m: 2.00,
        output_per_m: 10.00,
        cache_read_per_m: 0.20,
        cache_write_per_m: 2.50,
    },
    ModelRates {
        model_pattern: "claude-sonnet-4-6",
        input_per_m: 3.00,
        output_per_m: 15.00,
        cache_read_per_m: 0.30,
        cache_write_per_m: 3.75,
    },
    ModelRates {
        model_pattern: "claude-sonnet-4-5",
        input_per_m: 3.00,
        output_per_m: 15.00,
        cache_read_per_m: 0.30,
        cache_write_per_m: 3.75,
    },
    ModelRates {
        model_pattern: "claude-haiku-4-5",
        input_per_m: 1.00,
        output_per_m: 5.00,
        cache_read_per_m: 0.10,
        cache_write_per_m: 1.25,
    },

    // --- Anthropic Claude Family (legacy) ---
    ModelRates {
        model_pattern: "claude-3-7-sonnet",
        input_per_m: 3.00,
        output_per_m: 15.00,
        cache_read_per_m: 0.30,
        cache_write_per_m: 3.75,
    },
    ModelRates {
        model_pattern: "claude-3-5-sonnet",
        input_per_m: 3.00,
        output_per_m: 15.00,
        cache_read_per_m: 0.30,
        cache_write_per_m: 3.75,
    },
    ModelRates {
        model_pattern: "claude-3-sonnet",
        input_per_m: 3.00,
        output_per_m: 15.00,
        cache_read_per_m: 0.30,
        cache_write_per_m: 3.75,
    },
    ModelRates {
        model_pattern: "claude-3-opus",
        input_per_m: 15.00,
        output_per_m: 75.00,
        cache_read_per_m: 1.50,
        cache_write_per_m: 18.75,
    },
    ModelRates {
        model_pattern: "claude-3-5-haiku",
        input_per_m: 0.80,
        output_per_m: 4.00,
        cache_read_per_m: 0.08,
        cache_write_per_m: 1.00,
    },
    ModelRates {
        model_pattern: "claude-3-haiku",
        input_per_m: 0.25,
        output_per_m: 1.25,
        cache_read_per_m: 0.03,
        cache_write_per_m: 0.30,
    },

    // --- OpenAI GPT & Reasoning Family ---
    ModelRates {
        model_pattern: "o1-preview",
        input_per_m: 15.00,
        output_per_m: 60.00,
        cache_read_per_m: 7.50,
        cache_write_per_m: 15.00,
    },
    ModelRates {
        model_pattern: "o1-mini",
        input_per_m: 3.00,
        output_per_m: 12.00,
        cache_read_per_m: 1.50,
        cache_write_per_m: 3.00,
    },
    ModelRates {
        model_pattern: "o1",
        input_per_m: 15.00,
        output_per_m: 60.00,
        cache_read_per_m: 7.50,
        cache_write_per_m: 15.00,
    },
    ModelRates {
        model_pattern: "o3-mini",
        input_per_m: 1.10,
        output_per_m: 4.40,
        cache_read_per_m: 0.55,
        cache_write_per_m: 1.10,
    },
    ModelRates {
        model_pattern: "gpt-4o-mini",
        input_per_m: 0.15,
        output_per_m: 0.60,
        cache_read_per_m: 0.075,
        cache_write_per_m: 0.15,
    },
    ModelRates {
        model_pattern: "gpt-4o",
        input_per_m: 2.50,
        output_per_m: 10.00,
        cache_read_per_m: 1.25,
        cache_write_per_m: 2.50,
    },
    ModelRates {
        model_pattern: "gpt-4-turbo",
        input_per_m: 10.00,
        output_per_m: 30.00,
        cache_read_per_m: 5.00,
        cache_write_per_m: 10.00,
    },
    ModelRates {
        model_pattern: "gpt-4",
        input_per_m: 30.00,
        output_per_m: 60.00,
        cache_read_per_m: 15.00,
        cache_write_per_m: 30.00,
    },
    ModelRates {
        model_pattern: "gpt-3.5",
        input_per_m: 0.50,
        output_per_m: 1.50,
        cache_read_per_m: 0.25,
        cache_write_per_m: 0.50,
    },

    // --- Google Gemini Family (current generation) ---
    ModelRates {
        model_pattern: "gemini-3.7-flash",
        input_per_m: 0.75,
        output_per_m: 3.75,
        cache_read_per_m: 0.075,
        cache_write_per_m: 0.75,
    },
    ModelRates {
        model_pattern: "gemini-3.5-flash",
        input_per_m: 1.50,
        output_per_m: 9.00,
        cache_read_per_m: 0.15,
        cache_write_per_m: 1.50,
    },
    ModelRates {
        model_pattern: "gemini-3.1-pro",
        input_per_m: 2.00,
        output_per_m: 12.00,
        cache_read_per_m: 0.20,
        cache_write_per_m: 2.00,
    },

    // --- Google Gemini Family (legacy) ---
    ModelRates {
        model_pattern: "gemini-2.0-flash",
        input_per_m: 0.10,
        output_per_m: 0.40,
        cache_read_per_m: 0.025,
        cache_write_per_m: 0.10,
    },
    ModelRates {
        model_pattern: "gemini-2.0-pro",
        input_per_m: 1.25,
        output_per_m: 5.00,
        cache_read_per_m: 0.3125,
        cache_write_per_m: 1.25,
    },
    ModelRates {
        model_pattern: "gemini-1.5-pro",
        input_per_m: 1.25,
        output_per_m: 5.00,
        cache_read_per_m: 0.3125,
        cache_write_per_m: 1.25,
    },
    ModelRates {
        model_pattern: "gemini-1.5-flash",
        input_per_m: 0.075,
        output_per_m: 0.30,
        cache_read_per_m: 0.01875,
        cache_write_per_m: 0.075,
    },

    // --- DeepSeek Family (current generation) ---
    ModelRates {
        model_pattern: "deepseek-v4-flash",
        input_per_m: 0.22,
        output_per_m: 0.66,
        cache_read_per_m: 0.007,
        cache_write_per_m: 0.22,
    },
    ModelRates {
        model_pattern: "deepseek-v4-pro",
        input_per_m: 0.66,
        output_per_m: 1.98,
        cache_read_per_m: 0.022,
        cache_write_per_m: 0.66,
    },

    // --- DeepSeek Family (legacy) ---
    ModelRates {
        model_pattern: "deepseek-reasoner",
        input_per_m: 0.55,
        output_per_m: 2.19,
        cache_read_per_m: 0.14,
        cache_write_per_m: 0.55,
    },
    ModelRates {
        model_pattern: "deepseek-r1",
        input_per_m: 0.55,
        output_per_m: 2.19,
        cache_read_per_m: 0.14,
        cache_write_per_m: 0.55,
    },
    ModelRates {
        model_pattern: "deepseek-chat",
        input_per_m: 0.14,
        output_per_m: 0.28,
        cache_read_per_m: 0.014,
        cache_write_per_m: 0.14,
    },
    ModelRates {
        model_pattern: "deepseek-v3",
        input_per_m: 0.14,
        output_per_m: 0.28,
        cache_read_per_m: 0.014,
        cache_write_per_m: 0.14,
    },

    // --- xAI Grok Family ---
    ModelRates {
        model_pattern: "grok-3",
        input_per_m: 4.00,
        output_per_m: 16.00,
        cache_read_per_m: 1.00,
        cache_write_per_m: 4.00,
    },
    ModelRates {
        model_pattern: "grok-2",
        input_per_m: 2.00,
        output_per_m: 10.00,
        cache_read_per_m: 0.50,
        cache_write_per_m: 2.00,
    },
    ModelRates {
        model_pattern: "grok",
        input_per_m: 2.00,
        output_per_m: 10.00,
        cache_read_per_m: 0.50,
        cache_write_per_m: 2.00,
    },

    // --- Alibaba Qwen Family ---
    ModelRates {
        model_pattern: "qwen-2.5-coder-32b",
        input_per_m: 0.40,
        output_per_m: 1.20,
        cache_read_per_m: 0.10,
        cache_write_per_m: 0.40,
    },
    ModelRates {
        model_pattern: "qwen-2.5-coder",
        input_per_m: 0.40,
        output_per_m: 1.20,
        cache_read_per_m: 0.10,
        cache_write_per_m: 0.40,
    },
    ModelRates {
        model_pattern: "qwen-2.5-72b",
        input_per_m: 0.40,
        output_per_m: 1.20,
        cache_read_per_m: 0.10,
        cache_write_per_m: 0.40,
    },
    ModelRates {
        model_pattern: "qwen-max",
        input_per_m: 1.60,
        output_per_m: 6.40,
        cache_read_per_m: 0.40,
        cache_write_per_m: 1.60,
    },
    ModelRates {
        model_pattern: "qwen-plus",
        input_per_m: 0.40,
        output_per_m: 1.20,
        cache_read_per_m: 0.10,
        cache_write_per_m: 0.40,
    },
    ModelRates {
        model_pattern: "qwen-turbo",
        input_per_m: 0.05,
        output_per_m: 0.20,
        cache_read_per_m: 0.01,
        cache_write_per_m: 0.05,
    },

    // --- Zhipu AI GLM Family (current generation; "glm-5.3-flash" must precede "glm-5.3" and
    // "glm-4.7" must precede the legacy bare "glm-4" below, since the matcher is first-match-wins
    // on substring containment) ---
    ModelRates {
        model_pattern: "glm-5.3-flash",
        input_per_m: 0.075,
        output_per_m: 0.25,
        cache_read_per_m: 0.015,
        cache_write_per_m: 0.075,
    },
    ModelRates {
        model_pattern: "glm-5.3",
        input_per_m: 1.40,
        output_per_m: 4.40,
        cache_read_per_m: 0.26,
        cache_write_per_m: 1.40,
    },
    ModelRates {
        model_pattern: "glm-5.2",
        input_per_m: 1.40,
        output_per_m: 4.40,
        cache_read_per_m: 0.26,
        cache_write_per_m: 1.40,
    },
    ModelRates {
        model_pattern: "glm-4.7",
        input_per_m: 0.60,
        output_per_m: 2.20,
        cache_read_per_m: 0.11,
        cache_write_per_m: 0.60,
    },

    // --- Zhipu AI GLM Family (legacy) ---
    ModelRates {
        model_pattern: "glm-4-plus",
        input_per_m: 1.40,
        output_per_m: 1.40,
        cache_read_per_m: 0.35,
        cache_write_per_m: 1.40,
    },
    ModelRates {
        model_pattern: "glm-4",
        input_per_m: 1.40,
        output_per_m: 1.40,
        cache_read_per_m: 0.35,
        cache_write_per_m: 1.40,
    },

    // --- Moonshot Kimi Family ---
    ModelRates {
        model_pattern: "moonshot-v1",
        input_per_m: 1.60,
        output_per_m: 1.60,
        cache_read_per_m: 0.40,
        cache_write_per_m: 1.60,
    },
    ModelRates {
        model_pattern: "kimi",
        input_per_m: 1.60,
        output_per_m: 1.60,
        cache_read_per_m: 0.40,
        cache_write_per_m: 1.60,
    },

    // --- MiniMax Family ---
    ModelRates {
        model_pattern: "abab6.5",
        input_per_m: 0.20,
        output_per_m: 1.00,
        cache_read_per_m: 0.05,
        cache_write_per_m: 0.20,
    },
    ModelRates {
        model_pattern: "minimax",
        input_per_m: 0.20,
        output_per_m: 1.00,
        cache_read_per_m: 0.05,
        cache_write_per_m: 0.20,
    },
];

/// Match a raw model identifier string to the most specific known model pricing rates.
pub fn get_model_rates(model_id: &str) -> ModelRates {
    let lower = model_id.to_lowercase();
    for entry in MODEL_PRICING_TABLE {
        if lower.contains(entry.model_pattern) {
            return *entry;
        }
    }
    ModelRates::default()
}

/// Calculate estimated token cost for a specific model name.
pub fn estimate_model_tokens_cost_usd(
    model_id: Option<&str>,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_creation: u64,
) -> f64 {
    let rates = match model_id {
        Some(m) if !m.is_empty() => get_model_rates(m),
        _ => ModelRates::default(),
    };

    (input as f64 * rates.input_per_m / 1_000_000.0)
        + (output as f64 * rates.output_per_m / 1_000_000.0)
        + (cache_read as f64 * rates.cache_read_per_m / 1_000_000.0)
        + (cache_creation as f64 * rates.cache_write_per_m / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pricing_lookup_known_models() {
        let sonnet = get_model_rates("claude-3-7-sonnet-20250219");
        assert_eq!(sonnet.input_per_m, 3.00);
        assert_eq!(sonnet.output_per_m, 15.00);

        let gpt4o_mini = get_model_rates("gpt-4o-mini-2024-07-18");
        assert_eq!(gpt4o_mini.input_per_m, 0.15);
        assert_eq!(gpt4o_mini.output_per_m, 0.60);

        let deepseek_r1 = get_model_rates("deepseek-reasoner");
        assert_eq!(deepseek_r1.input_per_m, 0.55);
        assert_eq!(deepseek_r1.output_per_m, 2.19);

        let gemini_flash = get_model_rates("gemini-2.0-flash");
        assert_eq!(gemini_flash.input_per_m, 0.10);

        let unknown = get_model_rates("custom-fine-tuned-model-v1");
        assert_eq!(unknown.input_per_m, 3.00); // default
    }

    #[test]
    fn test_cost_calculation_math() {
        // 1M input on DeepSeek V3 ($0.14) + 1M output ($0.28)
        let cost = estimate_model_tokens_cost_usd(Some("deepseek-chat"), 1_000_000, 1_000_000, 0, 0);
        assert!((cost - 0.42).abs() < 1e-6);
    }

    /// Literal model strings pulled from a real `~/.agentworth/agentworth.db`
    /// (`session_model_usage.model`, measured 2026-09-02) that used to fall through to
    /// `ModelRates::default()` because no pattern in the table matched them. Each must now
    /// resolve to its own real entry, not the default Claude 3.5 Sonnet fallback rate.
    #[test]
    fn test_current_generation_models_resolve_to_named_entries() {
        let current_gen_models = [
            "claude-sonnet-5",
            "claude-opus-5",
            "claude-opus-4-8",
            "claude-opus-4-6",
            "claude-fable-5",
            "claude-fable-5-1",
            "claude-haiku-4-5-20251001",
            "claude-sonnet-4-6",
            "claude-sonnet-4-5-20250929",
            "deepseek-v4-flash-free",
            "deepseek-v4-flash",
            "deepseek-ai/deepseek-v4-flash",
            "deepseek-v4-pro",
            "gemini-3.7-flash",
            "gemini-3.5-flash",
            "gemini-3.1-pro",
            "glm-5.3",
            "glm-5.3-flash",
            "glm-5.2",
            "z-ai/glm-5.2",
            "glm-4.7",
        ];
        for model in current_gen_models {
            let rates = get_model_rates(model);
            assert_ne!(
                rates.model_pattern, "default",
                "{model} unexpectedly fell through to the default rate card"
            );
        }
    }

    #[test]
    fn test_claude_sonnet_5_and_opus_5_rates() {
        // Anthropic pricing, https://platform.claude.com/docs/en/about-claude/pricing, read 2026-09-02.
        let sonnet5 = get_model_rates("claude-sonnet-5-20260615");
        assert_eq!(sonnet5.input_per_m, 2.00);
        assert_eq!(sonnet5.output_per_m, 10.00);
        assert_eq!(sonnet5.cache_read_per_m, 0.20);

        let opus5 = get_model_rates("us.anthropic.claude-opus-5-20260615-v1:0");
        assert_eq!(opus5.input_per_m, 5.00);
        assert_eq!(opus5.output_per_m, 25.00);
    }

    #[test]
    fn test_claude_fable_5_1_and_5_have_different_cache_read_rates() {
        // Claude Fable 5.1 and Claude Mythos 5.1 get a 0.025x cache-hit multiplier instead of
        // the standard 0.1x, so they must not collapse onto the plain "claude-fable-5" entry.
        let fable_5_1 = get_model_rates("claude-fable-5-1");
        assert_eq!(fable_5_1.cache_read_per_m, 0.25);

        let fable_5 = get_model_rates("claude-fable-5");
        assert_eq!(fable_5.cache_read_per_m, 1.00);
    }

    #[test]
    fn test_glm_5_3_flash_does_not_collapse_to_glm_5_3_or_legacy_glm_4() {
        let flash = get_model_rates("or-glm-5.3-flash");
        assert_eq!(flash.input_per_m, 0.075);

        let full = get_model_rates("go-glm-5.3");
        assert_eq!(full.input_per_m, 1.40);

        // "glm-4.7" must not fall through to the legacy bare "glm-4" pattern (input 1.40).
        let glm_4_7 = get_model_rates("glm-4.7");
        assert_eq!(glm_4_7.input_per_m, 0.60);
    }

    #[test]
    fn test_unknown_model_still_falls_back_to_default() {
        let unknown = get_model_rates("some-unreleased-lab-experiment-v7");
        assert_eq!(unknown, ModelRates::default());
    }
}
