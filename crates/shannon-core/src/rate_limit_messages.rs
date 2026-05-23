//! # Rate Limit Message Builder
//!
//! User-friendly rate limit message generation for displaying to end users
//! when they hit API limits, tool limits, or token limits.

/// Builds human-readable rate limit messages for display in the CLI.
pub struct RateLimitMessageBuilder {
    /// Whether to include tips in messages.
    include_tips: bool,
}

impl Default for RateLimitMessageBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimitMessageBuilder {
    /// Create a new message builder with default settings.
    pub fn new() -> Self {
        Self { include_tips: true }
    }

    /// Create a new message builder with the given tip inclusion setting.
    pub fn with_tips(include_tips: bool) -> Self {
        Self { include_tips }
    }

    /// Build a general rate limit message.
    ///
    /// Displays the type of limit, remaining quota, and optionally
    /// when the limit window resets.
    pub fn build_message(
        &self,
        limit_type: &str,
        remaining: usize,
        reset_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> String {
        let mut msg = format!("Rate limit reached for {limit_type}. Remaining: {remaining}.");

        if let Some(reset) = reset_at {
            let now = chrono::Utc::now();
            let diff = reset.signed_duration_since(now);
            let minutes = diff.num_minutes();
            let seconds = diff.num_seconds() % 60;

            if minutes > 0 {
                msg.push_str(&format!(
                    " Resets in approximately {}m {}s (at {}).",
                    minutes,
                    seconds,
                    reset.format("%H:%M:%S UTC")
                ));
            } else {
                msg.push_str(&format!(
                    " Resets in approximately {}s (at {}).",
                    seconds,
                    reset.format("%H:%M:%S UTC")
                ));
            }
        } else {
            msg.push_str(" Reset time unknown.");
        }

        if self.include_tips {
            msg.push_str(
                " Tip: wait for the limit window to reset, or reduce your request frequency.",
            );
        }

        msg
    }

    /// Build a tool-specific rate limit message.
    ///
    /// Displays the tool name, its limit, and the time window.
    pub fn build_tool_limit_message(&self, tool_name: &str, limit: usize, window: &str) -> String {
        let mut msg =
            format!("Tool '{tool_name}' has reached its rate limit of {limit} calls per {window}.");

        if self.include_tips {
            msg.push_str(&format!(
                " Tip: you can continue using other tools, or wait for the {window} window to reset."
            ));
        }

        msg
    }

    /// Build a token usage limit message.
    ///
    /// Displays the tokens used vs. the limit, with a usage percentage.
    pub fn build_token_limit_message(&self, used: usize, limit: usize) -> String {
        let percentage = if limit > 0 {
            ((used as f64 / limit as f64) * 100.0) as usize
        } else {
            100
        };

        let remaining = limit.saturating_sub(used);

        let mut msg = format!(
            "Token usage limit: {used}/{limit} tokens used ({percentage}%). Remaining: {remaining} tokens."
        );

        if self.include_tips {
            if percentage >= 90 {
                msg.push_str(
                    " Tip: you are near the limit. Consider starting a new conversation \
                     or reducing the context size.",
                );
            } else if percentage >= 70 {
                msg.push_str(
                    " Tip: you have used a significant portion of your token budget. \
                     Consider summarizing or compressing context.",
                );
            } else {
                msg.push_str(
                    " Tip: to reduce token usage, try shorter prompts or fewer tool calls.",
                );
            }
        }

        msg
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    #[test]
    fn test_new_builder() {
        let builder = RateLimitMessageBuilder::new();
        assert!(builder.include_tips);
    }

    #[test]
    fn test_default_builder() {
        let builder = RateLimitMessageBuilder::default();
        assert!(builder.include_tips);
    }

    #[test]
    fn test_with_tips() {
        let builder = RateLimitMessageBuilder::with_tips(false);
        assert!(!builder.include_tips);
    }

    #[test]
    fn test_build_message_with_reset() {
        let builder = RateLimitMessageBuilder::with_tips(false);
        let reset_at = Utc::now() + Duration::minutes(5) + Duration::seconds(30);
        let msg = builder.build_message("API requests", 0, Some(reset_at));

        assert!(msg.contains("API requests"));
        assert!(msg.contains("Remaining: 0"));
        assert!(msg.contains("Resets in approximately 5m"));
        assert!(msg.contains("UTC"));
    }

    #[test]
    fn test_build_message_without_reset() {
        let builder = RateLimitMessageBuilder::with_tips(false);
        let msg = builder.build_message("requests", 5, None);

        assert!(msg.contains("requests"));
        assert!(msg.contains("Remaining: 5"));
        assert!(msg.contains("Reset time unknown"));
    }

    #[test]
    fn test_build_message_with_tips() {
        let builder = RateLimitMessageBuilder::with_tips(true);
        let msg = builder.build_message("requests", 0, None);

        assert!(msg.contains("Tip:"));
        assert!(msg.contains("wait for the limit window"));
    }

    #[test]
    fn test_build_message_without_tips() {
        let builder = RateLimitMessageBuilder::with_tips(false);
        let msg = builder.build_message("requests", 0, None);

        assert!(!msg.contains("Tip:"));
    }

    #[test]
    fn test_build_message_seconds_only() {
        let builder = RateLimitMessageBuilder::with_tips(false);
        let reset_at = Utc::now() + Duration::seconds(45);
        let msg = builder.build_message("requests", 0, Some(reset_at));

        // Allow for small timing variance (44s or 45s)
        assert!(
            msg.contains("Resets in approximately 44s")
                || msg.contains("Resets in approximately 45s")
        );
        assert!(!msg.contains("0m"));
    }

    #[test]
    fn test_build_tool_limit_message() {
        let builder = RateLimitMessageBuilder::with_tips(false);
        let msg = builder.build_tool_limit_message("bash", 10, "minute");

        assert!(msg.contains("'bash'"));
        assert!(msg.contains("10 calls per minute"));
    }

    #[test]
    fn test_build_tool_limit_message_with_tips() {
        let builder = RateLimitMessageBuilder::with_tips(true);
        let msg = builder.build_tool_limit_message("read", 50, "hour");

        assert!(msg.contains("'read'"));
        assert!(msg.contains("50 calls per hour"));
        assert!(msg.contains("Tip:"));
        assert!(msg.contains("other tools"));
    }

    #[test]
    fn test_build_token_limit_message_normal() {
        let builder = RateLimitMessageBuilder::with_tips(false);
        let msg = builder.build_token_limit_message(5000, 10000);

        assert!(msg.contains("5000/10000"));
        assert!(msg.contains("50%"));
        assert!(msg.contains("Remaining: 5000"));
    }

    #[test]
    fn test_build_token_limit_message_high_usage() {
        let builder = RateLimitMessageBuilder::new();
        let msg = builder.build_token_limit_message(95000, 100000);

        assert!(msg.contains("95%"));
        assert!(msg.contains("near the limit"));
        assert!(msg.contains("new conversation"));
    }

    #[test]
    fn test_build_token_limit_message_medium_usage() {
        let builder = RateLimitMessageBuilder::new();
        let msg = builder.build_token_limit_message(75000, 100000);

        assert!(msg.contains("75%"));
        assert!(msg.contains("significant portion"));
        assert!(msg.contains("summarizing"));
    }

    #[test]
    fn test_build_token_limit_message_low_usage() {
        let builder = RateLimitMessageBuilder::new();
        let msg = builder.build_token_limit_message(1000, 100000);

        assert!(msg.contains("1%"));
        assert!(msg.contains("shorter prompts"));
    }

    #[test]
    fn test_build_token_limit_message_at_limit() {
        let builder = RateLimitMessageBuilder::with_tips(false);
        let msg = builder.build_token_limit_message(100000, 100000);

        assert!(msg.contains("100000/100000"));
        assert!(msg.contains("100%"));
        assert!(msg.contains("Remaining: 0"));
    }

    #[test]
    fn test_build_token_limit_message_zero_limit() {
        let builder = RateLimitMessageBuilder::with_tips(false);
        let msg = builder.build_token_limit_message(0, 0);

        assert!(msg.contains("100%"));
    }
}
