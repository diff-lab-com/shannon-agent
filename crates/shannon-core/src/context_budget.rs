//! # Context Window Budget Allocation
//!
//! Allocates the model's context window into three buckets:
//!
//! - **System prompt** (15%): instructions, CLAUDE.md, project context
//! - **Tool schemas** (25%): tool definitions sent to the model
//! - **Conversation** (60%): messages, tool results, and assistant responses
//!
//! When tool schemas exceed their budget, the overflow tools are automatically
//! deferred (hidden from the API schema and discoverable via `ToolSearch`).

/// Default fraction of context reserved for the system prompt.
pub const SYSTEM_PROMPT_FRACTION: f32 = 0.15;
/// Default fraction of context reserved for tool schemas.
pub const TOOL_SCHEMA_FRACTION: f32 = 0.25;
/// Default fraction of context reserved for conversation messages.
pub const CONVERSATION_FRACTION: f32 = 0.60;

/// Context window budget allocation.
#[derive(Debug, Clone)]
pub struct ContextBudget {
    /// Total context window size in tokens.
    pub total_tokens: usize,
    /// Maximum tokens for the system prompt.
    pub system_prompt_budget: usize,
    /// Maximum tokens for tool schemas.
    pub tool_schema_budget: usize,
    /// Maximum tokens for conversation messages.
    pub conversation_budget: usize,
}

impl ContextBudget {
    /// Create a new budget allocation for the given total context size.
    pub fn new(total_tokens: usize) -> Self {
        Self {
            total_tokens,
            system_prompt_budget: (total_tokens as f32 * SYSTEM_PROMPT_FRACTION) as usize,
            tool_schema_budget: (total_tokens as f32 * TOOL_SCHEMA_FRACTION) as usize,
            conversation_budget: (total_tokens as f32 * CONVERSATION_FRACTION) as usize,
        }
    }

    /// Create with custom fractions. Values should sum to ~1.0.
    pub fn with_fractions(
        total_tokens: usize,
        system_frac: f32,
        tool_frac: f32,
        conversation_frac: f32,
    ) -> Self {
        Self {
            total_tokens,
            system_prompt_budget: (total_tokens as f32 * system_frac) as usize,
            tool_schema_budget: (total_tokens as f32 * tool_frac) as usize,
            conversation_budget: (total_tokens as f32 * conversation_frac) as usize,
        }
    }

    /// Estimate the token cost of a tool definition (JSON schema).
    /// Uses the ~4 chars per token heuristic.
    pub fn estimate_schema_tokens(schema: &serde_json::Value) -> usize {
        let json_str = serde_json::to_string(schema).unwrap_or_default();
        json_str.len() / 4
    }

    /// Estimate how many tokens a set of tool definitions will consume.
    pub fn estimate_total_schema_tokens(
        definitions: &[crate::api::ToolDefinition],
    ) -> usize {
        definitions
            .iter()
            .map(|def| {
                let json = serde_json::to_value(def).unwrap_or_default();
                Self::estimate_schema_tokens(&json)
            })
            .sum()
    }

    /// Check if the given tool definitions fit within the tool schema budget.
    /// Returns `Ok(())` if they fit, or `Err(overflow_tokens)` with the excess.
    pub fn check_schema_budget(
        &self,
        definitions: &[crate::api::ToolDefinition],
    ) -> Result<(), usize> {
        let tokens = Self::estimate_total_schema_tokens(definitions);
        if tokens <= self.tool_schema_budget {
            Ok(())
        } else {
            Err(tokens - self.tool_schema_budget)
        }
    }

    /// Given a registry, return the indices of tools that should be deferred
    /// to fit within the tool schema budget. Returns tools in order of largest
    /// schema first (greedy eviction).
    pub fn tools_to_defer(
        &self,
        definitions: &[crate::api::ToolDefinition],
    ) -> Vec<usize> {
        let total = Self::estimate_total_schema_tokens(definitions);
        if total <= self.tool_schema_budget {
            return Vec::new();
        }

        // Calculate how much we need to shed
        let mut excess = total - self.tool_schema_budget;

        // Sort tool indices by schema size (largest first)
        let mut indexed_sizes: Vec<(usize, usize)> = definitions
            .iter()
            .enumerate()
            .map(|(i, def)| {
                let json = serde_json::to_value(def).unwrap_or_default();
                (i, Self::estimate_schema_tokens(&json))
            })
            .collect();
        indexed_sizes.sort_by(|a, b| b.1.cmp(&a.1));

        // Greedily evict largest tools until we're under budget
        let mut to_defer = Vec::new();
        for (idx, size) in &indexed_sizes {
            if excess == 0 {
                break;
            }
            to_defer.push(*idx);
            excess = excess.saturating_sub(*size);
        }

        to_defer
    }
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self::new(200_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_budget_allocation() {
        let budget = ContextBudget::default();
        assert_eq!(budget.total_tokens, 200_000);
        assert_eq!(budget.system_prompt_budget, 30_000); // 15%
        assert_eq!(budget.tool_schema_budget, 50_000); // 25%
        assert_eq!(budget.conversation_budget, 120_000); // 60%
    }

    #[test]
    fn test_custom_fractions() {
        let budget = ContextBudget::with_fractions(100_000, 0.2, 0.3, 0.5);
        assert_eq!(budget.system_prompt_budget, 20_000);
        assert_eq!(budget.tool_schema_budget, 30_000);
        assert_eq!(budget.conversation_budget, 50_000);
    }

    #[test]
    fn test_check_schema_budget_fits() {
        let budget = ContextBudget::new(200_000);
        // Small set of definitions
        let defs = vec![crate::api::ToolDefinition {
            name: "test".to_string(),
            description: "A test tool".to_string(),
            input_schema: serde_json::json!({"type": "object", "properties": {}}),
            strict: None,
        }];
        assert!(budget.check_schema_budget(&defs).is_ok());
    }

    #[test]
    fn test_check_schema_budget_overflow() {
        let budget = ContextBudget::new(1_000); // very small
        let defs: Vec<crate::api::ToolDefinition> = (0..50)
            .map(|i| crate::api::ToolDefinition {
                name: format!("tool_{i}"),
                description: "A tool with a long description to increase schema size".repeat(10),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "input": {"type": "string", "description": "Some long input description here"}
                    }
                }),
                strict: None,
            })
            .collect();
        let result = budget.check_schema_budget(&defs);
        assert!(result.is_err());
        let overflow = result.unwrap_err();
        assert!(overflow > 0);
    }

    #[test]
    fn test_tools_to_defer_empty_when_fits() {
        let budget = ContextBudget::new(200_000);
        let defs = vec![crate::api::ToolDefinition {
            name: "small_tool".to_string(),
            description: "Small".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
            strict: None,
        }];
        let defer = budget.tools_to_defer(&defs);
        assert!(defer.is_empty());
    }

    #[test]
    fn test_tools_to_defer_evicts_largest() {
        let budget = ContextBudget::new(1_000);
        let defs = vec![
            crate::api::ToolDefinition {
                name: "tiny".to_string(),
                description: "T".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
                strict: None,
            },
            crate::api::ToolDefinition {
                name: "huge".to_string(),
                description: "X".repeat(5_000),
                input_schema: serde_json::json!({"type": "object", "properties": {"a": {"type": "string"}}}),
                strict: None,
            },
        ];
        let defer = budget.tools_to_defer(&defs);
        // Should defer the huge tool (index 1)
        assert!(defer.contains(&1), "Should defer the largest tool");
    }

    #[test]
    fn test_estimate_schema_tokens() {
        let schema = serde_json::json!({"type": "object", "properties": {"x": {"type": "string"}}});
        let tokens = ContextBudget::estimate_schema_tokens(&schema);
        assert!(tokens > 0);
        // JSON string is ~65 chars, so ~16 tokens
        assert!((10..=30).contains(&tokens), "Got {tokens}");
    }
}
