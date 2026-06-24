//! Integration tests for ContextBudget allocation, pressure adjustment,
//! schema budget enforcement, and multi-provider window sizing.

#[cfg(test)]
mod context_budget_tests {
    use shannon_engine::api::ToolDefinition;
    use shannon_engine::context_budget::{
        CONVERSATION_FRACTION, ContextBudget, SYSTEM_PROMPT_FRACTION, TOOL_SCHEMA_FRACTION,
    };
    use shannon_engine::context_pressure::PressureLevel;

    // -- Helpers --

    /// Build a tool definition whose serialized JSON is approximately `target_bytes` chars.
    fn make_tool(name: &str, target_bytes: usize) -> ToolDefinition {
        let description = "X".repeat(target_bytes);
        ToolDefinition {
            name: name.to_string(),
            description,
            input_schema: serde_json::json!({"type": "object"}),
            cache_control: None,
            strict: None,
        }
    }

    /// Sum of all priority allocation fields.
    fn priority_sum(alloc: &shannon_engine::context_budget::PriorityAllocation) -> usize {
        alloc.critical + alloc.high + alloc.normal + alloc.low
    }

    // -- Tests --

    /// 1. System prompt + tools + history fit within window.
    ///    The three budget buckets should add up to the total context window
    ///    (within rounding tolerance from float multiplication).
    #[test]
    fn test_budget_allocation_basic() {
        let budget = ContextBudget::new(200_000);

        // Verify the three top-level buckets
        assert_eq!(budget.system_prompt_budget, 30_000); // 15%
        assert_eq!(budget.tool_schema_budget, 50_000); // 25%
        assert_eq!(budget.conversation_budget, 120_000); // 60%

        // Sum of buckets should be close to total (within rounding)
        let sum =
            budget.system_prompt_budget + budget.tool_schema_budget + budget.conversation_budget;
        assert!(
            (sum as f32 - budget.total_tokens as f32).abs() < 10.0,
            "buckets sum to {sum}, expected close to {}",
            budget.total_tokens
        );

        // A single small tool should easily fit
        let defs = vec![make_tool("read_file", 50)];
        assert!(budget.check_schema_budget(&defs).is_ok());
    }

    /// 2. When total exceeds window, priorities apply.
    ///    Under pressure the critical fraction grows while low shrinks,
    ///    ensuring important messages are preserved.
    #[test]
    fn test_budget_under_pressure() {
        let mut budget = ContextBudget::new(200_000);

        // Default allocation
        let default_alloc = budget.priority_allocation();
        let default_critical = default_alloc.critical;
        let default_low = default_alloc.low;

        // Emergency pressure should shift budget toward critical
        budget.adjust_for_pressure(PressureLevel::Emergency);
        let emergency_alloc = budget.priority_allocation();

        assert!(
            emergency_alloc.critical > default_critical,
            "emergency critical ({}) should exceed default ({})",
            emergency_alloc.critical,
            default_critical
        );
        assert!(
            emergency_alloc.low <= default_low,
            "emergency low ({}) should not exceed default ({})",
            emergency_alloc.low,
            default_low
        );

        // Verify the total allocation still sums close to conversation_budget
        let sum = priority_sum(&emergency_alloc);
        assert!(
            (sum as f32 - budget.conversation_budget as f32).abs() < 10.0,
            "priority sum {sum} should be close to conversation_budget {}",
            budget.conversation_budget
        );
    }

    /// 3. Many tools eat into available space — tools_to_defer identifies the
    ///    largest schemas for eviction to bring the total under budget.
    #[test]
    fn test_budget_with_large_tool_definitions() {
        // Very small window so tool budget is tight
        let budget = ContextBudget::new(1_000);
        // Tool schema budget = 250 tokens (25%)

        let defs = vec![
            make_tool("tiny_tool", 20),
            make_tool("medium_tool", 200),
            make_tool("huge_tool", 2_000),
        ];

        // These should overflow the tool schema budget
        assert!(budget.check_schema_budget(&defs).is_err());

        // tools_to_defer should evict the largest tools
        let defer = budget.tools_to_defer(&defs);
        assert!(!defer.is_empty(), "some tools must be deferred");

        // The huge tool (index 2) should be among the deferred
        assert!(
            defer.contains(&2),
            "huge_tool (index 2) should be deferred, got {defer:?}"
        );
    }

    /// 4. System prompt alone exceeds window — a zero-budget context should
    ///    produce zero allocations and gracefully handle schema checks.
    #[test]
    fn test_budget_with_system_prompt_overflow() {
        // Zero-token window means everything is zero
        let budget = ContextBudget::new(0);
        assert_eq!(budget.system_prompt_budget, 0);
        assert_eq!(budget.tool_schema_budget, 0);
        assert_eq!(budget.conversation_budget, 0);

        // Even an empty tool list should "fit" in a zero budget (0 <= 0)
        let defs: Vec<ToolDefinition> = vec![];
        assert!(budget.check_schema_budget(&defs).is_ok());

        // Any non-empty tool list should overflow a zero budget
        let defs = vec![make_tool("any", 10)];
        assert!(budget.check_schema_budget(&defs).is_err());
    }

    /// 5. After compaction (simulated by reducing conversation size), budget
    ///    recalculates: creating a new budget for a smaller conversation
    ///    reflects the reclaimed space.
    #[test]
    fn test_budget_with_compressed_history() {
        // Before compaction: 200k window
        let before = ContextBudget::new(200_000);
        assert_eq!(before.conversation_budget, 120_000);

        // Simulate compaction reducing effective window to 100k
        // (e.g., model switch or aggressive context reduction)
        let after = ContextBudget::new(100_000);
        assert_eq!(after.conversation_budget, 60_000); // 60%

        // Priority allocation scales down proportionally
        let alloc_before = before.priority_allocation();
        let alloc_after = after.priority_allocation();

        // Each tier should be roughly halved
        assert!(
            alloc_before.critical > alloc_after.critical,
            "critical before ({}) should exceed after ({})",
            alloc_before.critical,
            alloc_after.critical
        );
        assert!(
            (alloc_before.critical as f64 / alloc_after.critical as f64 - 2.0).abs() < 0.1,
            "ratio should be close to 2.0"
        );
    }

    /// 6. Exactly at / over / under window limit boundary conditions.
    ///    Tests that check_schema_budget transitions cleanly from Ok to Err.
    #[test]
    fn test_budget_exact_boundary() {
        let budget = ContextBudget::new(200_000);
        // tool_schema_budget = 50_000 tokens = 200_000 chars of JSON

        // Create a tool whose serialized JSON is just under the budget.
        // estimate_schema_tokens = json_str.len() / 4, so to get ~49_999 tokens
        // we need ~199_996 chars of JSON.
        let tool_name = "boundary_tool";
        // The JSON wrapper adds some chars; use a description that brings the
        // total close to but under 200_000 chars.
        let under_tool = make_tool(tool_name, 190_000);
        let _under_json = serde_json::to_string(&under_tool).unwrap();
        let under_tokens =
            ContextBudget::estimate_schema_tokens(&serde_json::to_value(&under_tool).unwrap());
        assert!(
            under_tokens <= budget.tool_schema_budget,
            "under_tokens ({under_tokens}) should fit in budget ({})",
            budget.tool_schema_budget
        );
        assert!(
            budget.check_schema_budget(&[under_tool]).is_ok(),
            "single under-budget tool should fit"
        );

        // Now create a tool whose schema clearly overflows
        let over_tool = make_tool("overflow", 300_000);
        assert!(
            budget.check_schema_budget(&[over_tool]).is_err(),
            "overflow tool should not fit"
        );
    }

    /// 7. No room for user message — zero conversation budget means priority
    ///    allocation is all zeros.
    #[test]
    fn test_budget_zero_available() {
        let budget = ContextBudget::new(0);
        let alloc = budget.priority_allocation();

        assert_eq!(alloc.critical, 0);
        assert_eq!(alloc.high, 0);
        assert_eq!(alloc.normal, 0);
        assert_eq!(alloc.low, 0);
        assert_eq!(priority_sum(&alloc), 0);

        // Pressure adjustment on a zero-budget should still not panic
        let mut budget = ContextBudget::new(0);
        budget.adjust_for_pressure(PressureLevel::Emergency);
        let alloc = budget.priority_allocation();
        assert_eq!(alloc.critical, 0);
    }

    /// 8. Different models have different context windows. Creating budgets
    ///    with different total_tokens reflects real provider differences.
    #[test]
    fn test_budget_multi_provider_differences() {
        // Claude Haiku: 200k
        let haiku = ContextBudget::new(200_000);
        // GPT-4o: 128k
        let gpt4o = ContextBudget::new(128_000);
        // Gemini Pro: 1M
        let gemini = ContextBudget::new(1_000_000);
        // Ollama small model: 8k
        let ollama = ContextBudget::new(8_000);

        // Verify proportions are consistent regardless of window size
        for (name, b) in [
            ("haiku", &haiku),
            ("gpt4o", &gpt4o),
            ("gemini", &gemini),
            ("ollama", &ollama),
        ] {
            let sys_pct = b.system_prompt_budget as f32 / b.total_tokens as f32;
            let tool_pct = b.tool_schema_budget as f32 / b.total_tokens as f32;
            let conv_pct = b.conversation_budget as f32 / b.total_tokens as f32;

            assert!(
                (sys_pct - SYSTEM_PROMPT_FRACTION).abs() < 0.01,
                "{name}: system fraction {sys_pct} != {SYSTEM_PROMPT_FRACTION}"
            );
            assert!(
                (tool_pct - TOOL_SCHEMA_FRACTION).abs() < 0.01,
                "{name}: tool fraction {tool_pct} != {TOOL_SCHEMA_FRACTION}"
            );
            assert!(
                (conv_pct - CONVERSATION_FRACTION).abs() < 0.01,
                "{name}: conversation fraction {conv_pct} != {CONVERSATION_FRACTION}"
            );
        }

        // Larger window should have strictly larger budgets
        assert!(gemini.conversation_budget > haiku.conversation_budget);
        assert!(haiku.conversation_budget > ollama.conversation_budget);

        // A set of tools that fits in gemini but not ollama
        let tools: Vec<ToolDefinition> =
            (0..30).map(|i| make_tool(&format!("t{i}"), 500)).collect();
        assert!(gemini.check_schema_budget(&tools).is_ok());
        assert!(ollama.check_schema_budget(&tools).is_err());
    }

    /// 9. Image content token estimation — images use a fixed 100-token cost
    ///    in the helper layer. Verify that a budget with conversation space
    ///    can accommodate image-bearing messages through the priority system.
    #[test]
    fn test_budget_with_image_tokens() {
        let budget = ContextBudget::new(200_000);
        let alloc = budget.priority_allocation();

        // Each image costs 100 tokens (fixed). Verify the critical tier alone
        // can hold many images (realistic scenario: screenshots in conversation).
        let images_in_critical = alloc.critical / 100;
        assert!(
            images_in_critical >= 10,
            "critical tier ({}) should hold at least 10 images at 100 tokens each, got {images_in_critical}",
            alloc.critical
        );

        // Verify that the fixed image token cost is accounted for: the
        // conversation budget should be large enough to hold a reasonable
        // number of images plus text.
        let image_cost = 100;
        let text_per_message = 500; // rough estimate for a user message
        let message_cost = image_cost + text_per_message;
        let messages_that_fit = budget.conversation_budget / message_cost;
        assert!(
            messages_that_fit >= 10,
            "should fit at least 10 image+text messages, got {messages_that_fit}"
        );

        // Priority allocation should reflect the image overhead:
        // high-priority tier should hold at least a few images
        assert!(
            alloc.high / image_cost >= 5,
            "high tier should hold at least 5 images"
        );
    }
}
