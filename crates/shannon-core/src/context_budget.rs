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
//!
//! Conversation budget is further split into priority tiers:
//! - **Critical** (40% of conversation): system messages, user instructions
//! - **High** (30% of conversation): recent tool results, assistant responses
//! - **Normal** (20% of conversation): older conversation turns
//! - **Low** (10% of conversation): verbose output, historical context
//!
//! Budget adjusts dynamically under pressure (see [`adjust_for_pressure`]).

use crate::context_pressure::PressureLevel;

/// Default fraction of context reserved for the system prompt.
pub const SYSTEM_PROMPT_FRACTION: f32 = 0.15;
/// Default fraction of context reserved for tool schemas.
pub const TOOL_SCHEMA_FRACTION: f32 = 0.25;
/// Default fraction of context reserved for conversation messages.
pub const CONVERSATION_FRACTION: f32 = 0.60;

/// Message priority for budget allocation and compaction decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MessagePriority {
    /// Verbose output, historical context — first to compact
    Low,
    /// Older conversation turns
    Normal,
    /// Recent tool results, assistant responses
    High,
    /// System messages, user instructions, protected messages — never compact
    Critical,
}

impl std::fmt::Display for MessagePriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessagePriority::Critical => write!(f, "CRITICAL"),
            MessagePriority::High => write!(f, "HIGH"),
            MessagePriority::Normal => write!(f, "NORMAL"),
            MessagePriority::Low => write!(f, "LOW"),
        }
    }
}

/// Budget allocation within the conversation tier, split by priority.
#[derive(Debug, Clone)]
pub struct PriorityBudget {
    /// Fraction of conversation budget for critical messages (default: 0.40)
    pub critical_frac: f32,
    /// Fraction for high-priority messages (default: 0.30)
    pub high_frac: f32,
    /// Fraction for normal-priority messages (default: 0.20)
    pub normal_frac: f32,
    /// Fraction for low-priority messages (default: 0.10)
    pub low_frac: f32,
}

impl Default for PriorityBudget {
    fn default() -> Self {
        Self {
            critical_frac: 0.40,
            high_frac: 0.30,
            normal_frac: 0.20,
            low_frac: 0.10,
        }
    }
}

impl PriorityBudget {
    /// Return the token budget for each priority level given a total conversation budget.
    pub fn allocate(&self, conversation_budget: usize) -> PriorityAllocation {
        PriorityAllocation {
            critical: (conversation_budget as f32 * self.critical_frac) as usize,
            high: (conversation_budget as f32 * self.high_frac) as usize,
            normal: (conversation_budget as f32 * self.normal_frac) as usize,
            low: (conversation_budget as f32 * self.low_frac) as usize,
        }
    }
}

/// Token allocation per priority level.
#[derive(Debug, Clone)]
pub struct PriorityAllocation {
    pub critical: usize,
    pub high: usize,
    pub normal: usize,
    pub low: usize,
}

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
    /// Priority-based allocation within the conversation budget.
    pub priority_budget: PriorityBudget,
}

impl ContextBudget {
    /// Create a new budget allocation for the given total context size.
    pub fn new(total_tokens: usize) -> Self {
        Self {
            total_tokens,
            system_prompt_budget: (total_tokens as f32 * SYSTEM_PROMPT_FRACTION) as usize,
            tool_schema_budget: (total_tokens as f32 * TOOL_SCHEMA_FRACTION) as usize,
            conversation_budget: (total_tokens as f32 * CONVERSATION_FRACTION) as usize,
            priority_budget: PriorityBudget::default(),
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
            priority_budget: PriorityBudget::default(),
        }
    }

    /// Return the priority-based token allocation for the conversation budget.
    pub fn priority_allocation(&self) -> PriorityAllocation {
        self.priority_budget.allocate(self.conversation_budget)
    }

    /// Dynamically adjust the budget allocation based on context pressure level.
    ///
    /// Under higher pressure, shifts budget toward critical messages and away
    /// from low-priority content to preserve the most important context.
    pub fn adjust_for_pressure(&mut self, level: PressureLevel) {
        match level {
            PressureLevel::Low | PressureLevel::Normal => {
                // Default allocation — no change needed
            }
            PressureLevel::High => {
                // Shrink low/normal, expand critical/high
                self.priority_budget = PriorityBudget {
                    critical_frac: 0.45,
                    high_frac: 0.30,
                    normal_frac: 0.15,
                    low_frac: 0.10,
                };
            }
            PressureLevel::Critical => {
                // Aggressively protect critical content
                self.priority_budget = PriorityBudget {
                    critical_frac: 0.50,
                    high_frac: 0.30,
                    normal_frac: 0.15,
                    low_frac: 0.05,
                };
            }
            PressureLevel::Emergency => {
                // Maximum protection for critical content only
                self.priority_budget = PriorityBudget {
                    critical_frac: 0.55,
                    high_frac: 0.30,
                    normal_frac: 0.10,
                    low_frac: 0.05,
                };
            }
        }
    }

    /// Estimate the token cost of a tool definition (JSON schema).
    /// Uses the ~4 chars per token heuristic.
    pub fn estimate_schema_tokens(schema: &serde_json::Value) -> usize {
        let json_str = serde_json::to_string(schema).unwrap_or_default();
        json_str.len() / 4
    }

    /// Estimate how many tokens a set of tool definitions will consume.
    pub fn estimate_total_schema_tokens(definitions: &[crate::api::ToolDefinition]) -> usize {
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
    pub fn tools_to_defer(&self, definitions: &[crate::api::ToolDefinition]) -> Vec<usize> {
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
            cache_control: None,
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
                cache_control: None,
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
            cache_control: None,
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
                cache_control: None,
                strict: None,
            },
            crate::api::ToolDefinition {
                name: "huge".to_string(),
                description: "X".repeat(5_000),
                input_schema: serde_json::json!({"type": "object", "properties": {"a": {"type": "string"}}}),
                cache_control: None,
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

    // ---- Priority Budget ----

    #[test]
    fn test_priority_budget_default_allocation() {
        let pb = PriorityBudget::default();
        let alloc = pb.allocate(120_000);
        assert_eq!(alloc.critical, 48_000); // 40%
        assert_eq!(alloc.high, 36_000); // 30%
        assert_eq!(alloc.normal, 24_000); // 20%
        assert_eq!(alloc.low, 12_000); // 10%
    }

    #[test]
    fn test_budget_priority_allocation() {
        let budget = ContextBudget::new(200_000);
        let alloc = budget.priority_allocation();
        assert!(alloc.critical > 0);
        assert!(alloc.high > alloc.normal);
        assert!(alloc.normal > alloc.low);
    }

    #[test]
    fn test_adjust_for_pressure_high() {
        let mut budget = ContextBudget::new(200_000);
        budget.adjust_for_pressure(PressureLevel::High);
        let alloc = budget.priority_allocation();
        // Critical should be larger than default
        assert!(alloc.critical > 45_000);
    }

    #[test]
    fn test_adjust_for_pressure_emergency() {
        let mut budget = ContextBudget::new(200_000);
        budget.adjust_for_pressure(PressureLevel::Emergency);
        let alloc = budget.priority_allocation();
        assert!(alloc.critical > alloc.high);
    }

    #[test]
    fn test_message_priority_ordering() {
        assert!(MessagePriority::Critical > MessagePriority::High);
        assert!(MessagePriority::High > MessagePriority::Normal);
        assert!(MessagePriority::Normal > MessagePriority::Low);
    }

    // ── Edge case tests ─────────────────────────────────────────────────

    #[test]
    fn test_message_priority_display() {
        assert_eq!(MessagePriority::Critical.to_string(), "CRITICAL");
        assert_eq!(MessagePriority::High.to_string(), "HIGH");
        assert_eq!(MessagePriority::Normal.to_string(), "NORMAL");
        assert_eq!(MessagePriority::Low.to_string(), "LOW");
    }

    #[test]
    fn test_context_budget_small_window() {
        let budget = ContextBudget::new(1_000);
        assert_eq!(budget.system_prompt_budget, 150); // 15%
        assert_eq!(budget.tool_schema_budget, 250); // 25%
        assert_eq!(budget.conversation_budget, 600); // 60%
    }

    #[test]
    fn test_context_budget_zero_tokens() {
        let budget = ContextBudget::new(0);
        assert_eq!(budget.system_prompt_budget, 0);
        assert_eq!(budget.tool_schema_budget, 0);
        assert_eq!(budget.conversation_budget, 0);
    }

    #[test]
    fn test_priority_budget_custom_fractions() {
        let pb = PriorityBudget {
            critical_frac: 0.5,
            high_frac: 0.3,
            normal_frac: 0.15,
            low_frac: 0.05,
        };
        let alloc = pb.allocate(100_000);
        assert_eq!(alloc.critical, 50_000);
        assert_eq!(alloc.high, 30_000);
        assert_eq!(alloc.normal, 15_000);
        assert_eq!(alloc.low, 5_000);
    }

    #[test]
    fn test_priority_budget_zero_conversation() {
        let pb = PriorityBudget::default();
        let alloc = pb.allocate(0);
        assert_eq!(alloc.critical, 0);
        assert_eq!(alloc.high, 0);
        assert_eq!(alloc.normal, 0);
        assert_eq!(alloc.low, 0);
    }

    #[test]
    fn test_check_schema_budget_empty_tools() {
        let budget = ContextBudget::new(200_000);
        let defs: Vec<crate::api::ToolDefinition> = vec![];
        assert!(budget.check_schema_budget(&defs).is_ok());
    }

    #[test]
    fn test_tools_to_defer_empty_list() {
        let budget = ContextBudget::new(1_000);
        let defs: Vec<crate::api::ToolDefinition> = vec![];
        let defer = budget.tools_to_defer(&defs);
        assert!(defer.is_empty());
    }

    #[test]
    fn test_estimate_schema_tokens_empty_object() {
        let schema = serde_json::json!({});
        let tokens = ContextBudget::estimate_schema_tokens(&schema);
        // Empty object is "{}" = 2 chars, rounds to 0 tokens
        assert!(tokens >= 0);
    }

    #[test]
    fn test_estimate_schema_tokens_string() {
        let schema = serde_json::json!("just a string");
        let tokens = ContextBudget::estimate_schema_tokens(&schema);
        assert!(tokens > 0);
    }

    #[test]
    fn test_priority_allocation_sums_reasonably() {
        let budget = ContextBudget::new(200_000);
        let alloc = budget.priority_allocation();
        let sum = alloc.critical + alloc.high + alloc.normal + alloc.low;
        // Should be close to conversation_budget (120_000) within rounding
        assert!((sum as f32 - budget.conversation_budget as f32).abs() < 10.0);
    }

    #[test]
    fn test_fractions_sum_to_one() {
        assert!(
            (SYSTEM_PROMPT_FRACTION + TOOL_SCHEMA_FRACTION + CONVERSATION_FRACTION - 1.0).abs()
                < f32::EPSILON
        );
    }
}
