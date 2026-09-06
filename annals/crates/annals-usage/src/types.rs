use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_field_names)]
pub struct TokenUsageBreakdown {
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    #[serde(default)]
    pub cache_write_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
}

impl TokenUsageBreakdown {
    #[must_use]
    pub fn ordinary_input_tokens(self) -> Option<i64> {
        self.input_tokens
            .checked_sub(self.cached_input_tokens)?
            .checked_sub(self.cache_write_input_tokens)
            .filter(|tokens| *tokens >= 0)
    }

    #[must_use]
    pub fn is_consistent(self) -> bool {
        self.input_tokens >= 0
            && self.cached_input_tokens >= 0
            && self.cache_write_input_tokens >= 0
            && self.output_tokens >= 0
            && self.reasoning_output_tokens >= 0
            && self.total_tokens >= 0
            && self.input_tokens.checked_add(self.output_tokens) == Some(self.total_tokens)
            && self.ordinary_input_tokens().is_some()
            && self.reasoning_output_tokens <= self.output_tokens
    }
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadTokenUsage {
    pub last: TokenUsageBreakdown,
    pub total: TokenUsageBreakdown,
    #[serde(default)]
    pub model_context_window: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::TokenUsageBreakdown;

    #[test]
    fn token_categories_are_not_additive_buckets() {
        let usage = TokenUsageBreakdown {
            input_tokens: 100,
            cached_input_tokens: 60,
            cache_write_input_tokens: 10,
            output_tokens: 20,
            reasoning_output_tokens: 15,
            total_tokens: 120,
        };
        assert_eq!(usage.ordinary_input_tokens(), Some(30));
        assert!(usage.is_consistent());
    }

    #[test]
    fn inconsistent_totals_are_detected() {
        let usage = TokenUsageBreakdown {
            input_tokens: 100,
            output_tokens: 20,
            total_tokens: 121,
            ..TokenUsageBreakdown::default()
        };
        assert!(!usage.is_consistent());
    }

    #[test]
    fn overflowing_totals_are_inconsistent_without_panicking() {
        let usage = TokenUsageBreakdown {
            input_tokens: i64::MAX,
            output_tokens: 1,
            total_tokens: i64::MAX,
            ..TokenUsageBreakdown::default()
        };
        assert!(!usage.is_consistent());
    }
}
