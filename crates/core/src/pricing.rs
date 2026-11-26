//! Public API list prices (USD per 1M tokens) for the cost estimate.
//!
//! The estimate answers "what would this usage cost at pay-as-you-go API prices?" — it is
//! not what a subscription is billed. Claude prices follow Anthropic's published list
//! (Nov 2026); the other providers use their published list prices at the time of writing
//! and may drift. Unknown models return `None` and are reported as unpriced.

use crate::usage::ledger::Tokens;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
}

const fn p(input: f64, output: f64, cache_write: f64, cache_read: f64) -> Price {
    Price {
        input,
        output,
        cache_write,
        cache_read,
    }
}

// Most specific pattern first: "opus-5-5" must win over "opus-5".
const CLAUDE: &[(&str, Price)] = &[
    ("fable-5-2", p(8.0, 40.0, 10.0, 0.20)),
    ("mythos-5-2", p(8.0, 40.0, 10.0, 0.20)),
    ("fable-5-1", p(10.0, 50.0, 12.5, 0.25)),
    ("mythos-5-1", p(10.0, 50.0, 12.5, 0.25)),
    ("fable-5", p(10.0, 50.0, 12.5, 1.0)),
    ("mythos-5", p(10.0, 50.0, 12.5, 1.0)),
    ("opus-5-5", p(4.0, 20.0, 5.0, 0.20)),
    ("opus-5", p(5.0, 25.0, 6.25, 0.50)),
    ("opus-4-8", p(5.0, 25.0, 6.25, 0.50)),
    ("opus-4-7", p(5.0, 25.0, 6.25, 0.50)),
    ("opus-4-6", p(5.0, 25.0, 6.25, 0.50)),
    ("opus-4-5", p(5.0, 25.0, 6.25, 0.50)),
    ("opus-4", p(15.0, 75.0, 18.75, 1.50)),
    ("3-opus", p(15.0, 75.0, 18.75, 1.50)),
    ("sonnet-5-5", p(2.0, 10.0, 2.5, 0.10)),
    ("sonnet-5", p(2.0, 10.0, 2.5, 0.20)),
    ("sonnet-4", p(3.0, 15.0, 3.75, 0.30)),
    ("3-7-sonnet", p(3.0, 15.0, 3.75, 0.30)),
    ("3-5-sonnet", p(3.0, 15.0, 3.75, 0.30)),
    ("haiku-5-5", p(0.10, 0.50, 0.125, 0.01)),
    ("haiku-4-5", p(1.0, 5.0, 1.25, 0.10)),
    ("3-5-haiku", p(0.80, 4.0, 1.0, 0.08)),
    ("3-haiku", p(0.25, 1.25, 0.30, 0.03)),
];

const OPENAI: &[(&str, Price)] = &[
    ("gpt-5-nano", p(0.05, 0.40, 0.05, 0.005)),
    ("gpt-5-mini", p(0.25, 2.0, 0.25, 0.025)),
    ("codex-mini", p(1.5, 6.0, 1.5, 0.375)),
    ("gpt-5", p(1.25, 10.0, 1.25, 0.125)),
    ("gpt-4-1", p(2.0, 8.0, 2.0, 0.50)),
    ("o4-mini", p(1.1, 4.4, 1.1, 0.275)),
    ("o3", p(2.0, 8.0, 2.0, 0.50)),
];

const GEMINI: &[(&str, Price)] = &[
    ("gemini-3-pro", p(2.0, 12.0, 2.0, 0.20)),
    ("gemini-3-flash", p(0.50, 3.0, 0.50, 0.05)),
    ("flash-lite", p(0.10, 0.40, 0.10, 0.01)),
    ("2-5-pro", p(1.25, 10.0, 1.25, 0.125)),
    ("2-5-flash", p(0.30, 2.50, 0.30, 0.03)),
    ("2-0-flash", p(0.10, 0.40, 0.10, 0.025)),
];

const GROK: &[(&str, Price)] = &[
    ("grok-code-fast", p(0.20, 1.50, 0.20, 0.02)),
    ("grok-4-fast", p(0.20, 0.50, 0.20, 0.05)),
    ("grok-4", p(3.0, 15.0, 3.0, 0.75)),
    ("grok-3-mini", p(0.30, 0.50, 0.30, 0.075)),
    ("grok-3", p(3.0, 15.0, 3.0, 0.75)),
];

// Qwen Code: Alibaba Cloud Model Studio international list prices (lowest tier).
const QWEN: &[(&str, Price)] = &[
    ("qwen3-coder-flash", p(0.30, 1.50, 0.30, 0.06)),
    ("qwen3-coder-plus", p(1.0, 5.0, 1.0, 0.20)),
    ("qwen3-coder", p(1.0, 5.0, 1.0, 0.20)),
    ("coder-model", p(1.0, 5.0, 1.0, 0.20)),
    ("qwen3-max", p(1.2, 6.0, 1.2, 0.24)),
    ("qwen-max", p(1.6, 6.4, 1.6, 0.32)),
    ("qwen-plus", p(0.40, 1.20, 0.40, 0.08)),
    ("qwen-flash", p(0.05, 0.40, 0.05, 0.01)),
    ("qwen-turbo", p(0.05, 0.20, 0.05, 0.01)),
];

/// Price for a model id as it appears in the logs (date suffixes and dots are fine).
pub fn price(provider: &str, model: &str) -> Option<Price> {
    let m = model.to_ascii_lowercase().replace(['.', '_'], "-");
    let table = match provider {
        "claude" => CLAUDE,
        "codex" => OPENAI,
        "gemini" => GEMINI,
        "grok" => GROK,
        "qwen" => QWEN,
        _ => return None,
    };
    table
        .iter()
        .find(|(pat, _)| m.contains(pat))
        .map(|(_, pr)| *pr)
}

/// Estimated USD cost of one usage event, or `None` when the model is unknown.
pub fn cost(provider: &str, model: Option<&str>, t: &Tokens) -> Option<f64> {
    let pr = price(provider, model?)?;
    Some(
        (t.input * pr.input
            + t.output * pr.output
            + t.cache_write * pr.cache_write
            + t.cache_read * pr.cache_read)
            / 1_000_000.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_most_specific_model() {
        assert_eq!(price("claude", "claude-opus-5-5").unwrap().input, 4.0);
        assert_eq!(price("claude", "claude-opus-5").unwrap().input, 5.0);
        assert_eq!(
            price("claude", "claude-opus-4-5-20251101").unwrap().output,
            25.0
        );
        assert_eq!(
            price("claude", "claude-opus-4-1-20250805").unwrap().input,
            15.0
        );
        assert_eq!(
            price("claude", "claude-sonnet-4-5-20250929").unwrap().input,
            3.0
        );
        assert_eq!(price("claude", "claude-haiku-4.5").unwrap().input, 1.0);
        assert_eq!(price("codex", "gpt-5.3-codex").unwrap().input, 1.25);
        assert_eq!(price("gemini", "gemini-2.5-pro").unwrap().output, 10.0);
        assert_eq!(price("grok", "grok-code-fast-1").unwrap().output, 1.5);
        assert_eq!(price("qwen", "qwen3-coder-plus").unwrap().input, 1.0);
        assert_eq!(price("qwen", "qwen3-coder-flash").unwrap().output, 1.5);
        assert!(price("claude", "<synthetic>").is_none());
        assert!(price("unknown", "gpt-5").is_none());
    }

    #[test]
    fn computes_cost() {
        let t = Tokens {
            input: 1_000_000.0,
            output: 100_000.0,
            cache_read: 2_000_000.0,
            cache_write: 0.0,
        };
        // 4 + 2 + 0.4
        let c = cost("claude", Some("claude-opus-5-5"), &t).unwrap();
        assert!((c - 6.4).abs() < 1e-9, "{c}");
        assert!(cost("claude", None, &t).is_none());
    }
}
