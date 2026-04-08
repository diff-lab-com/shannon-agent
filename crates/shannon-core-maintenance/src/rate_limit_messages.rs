//! Rate limit message builder

use std::collections::HashMap;

/// Rate limit message builder
pub struct RateLimitMessageBuilder {
    templates: HashMap<String, String>,
}

impl RateLimitMessageBuilder {
    pub fn new() -> Self {
        let mut templates = HashMap::new();

        // Default templates
        templates.insert(
            "api_limit".to_string(),
            "API rate limit exceeded. Please try again in {retry_after} seconds.".to_string()
        );
        templates.insert(
            "tool_limit".to_string(),
            "Tool execution limit exceeded. Please wait before running more tools.".to_string()
        );
        templates.insert(
            "token_limit".to_string(),
            "Token limit reached. Consider upgrading your plan for higher limits.".to_string()
        );

        Self { templates }
    }

    /// Build a rate limit message
    pub fn build(&self, limit_type: &str, context: &HashMap<String, String>) -> String {
        let default = "Rate limit exceeded.".to_string();
        let template = self.templates.get(limit_type)
            .unwrap_or(&default);

        let mut message = template.clone();
        for (key, value) in context {
            message = message.replace(&format!("{{{}}}", key), value);
        }

        message
    }

    /// Add a custom template
    pub fn add_template(&mut self, name: String, template: String) {
        self.templates.insert(name, template);
    }

    /// Get template
    pub fn get_template(&self, name: &str) -> Option<&String> {
        self.templates.get(name)
    }
}

impl Default for RateLimitMessageBuilder {
    fn default() -> Self {
        Self::new()
    }
}
