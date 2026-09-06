//! # Context Breakdown — six-category token estimate for one assembled request
//!
//! The context window budget ([`crate::context_budget`]) splits the window
//! into *buckets by policy* (system 15% / tools 25% / conversation 60%) and
//! priority tiers. This module is the observability counterpart: it measures
//! how many tokens the **actual** session state occupies per *source
//! category*:
//!
//! | key            | content measured                                              |
//! |----------------|---------------------------------------------------------------|
//! | `system`       | base system prompt block (engine.rs assembly point)           |
//! | `tools`        | non-MCP tool JSON schemas in the `ToolRegistry`               |
//! | `skills`       | skill content riding the same registry as `skill_<id>` tools  |
//! | `memory`       | `MemoryStore::format_for_injection` product                   |
//! | `mcp`          | MCP tool schemas (same registry pool, `mcp__` name prefix)    |
//! | `conversation` | the message history                                           |
//!
//! Estimation reuses the repository's existing estimators — the CJK-aware
//! `estimate_text_tokens` / `estimate_tokens` from [`crate::compact::helpers`]
//! for text and messages, and `ContextBudget::estimate_schema_tokens` (the
//! ~4 chars/token heuristic) for tool JSON schemas. No new tokenizer
//! dependency is introduced.
//!
//! The measurement is deliberately **off the request-assembly path**: hosts
//! call [`compute_breakdown`] (or the `QueryEngine::context_breakdown`
//! wrapper in `shannon-core`) on the *current* session state whenever they
//! need the numbers. An empty category simply reports `0`; the total is
//! always the sum of the categories.

use serde::Serialize;

use crate::api::{Message, ToolDefinition};
use crate::compact::helpers::{estimate_text_tokens, estimate_tokens};
use crate::context_budget::ContextBudget;

/// Name prefix under which MCP-host tools join the `ToolRegistry` pool.
/// Counted in the `mcp` category (never in `tools`).
pub const MCP_TOOL_PREFIX: &str = "mcp__";

/// Name prefix under which skills ride the `ToolRegistry` pool
/// (`register_skills_as_tools` registers each user-invocable skill as a
/// `skill_<id>` tool whose schema carries the skill content). Counted in
/// the `skills` category (never in `tools`).
pub const SKILL_TOOL_PREFIX: &str = "skill_";

/// The six source categories, in stable display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BreakdownCategory {
    /// Base system prompt block.
    System,
    /// Non-MCP, non-skill tool JSON schemas.
    Tools,
    /// Injected skill content (`skill_<id>` tool schemas).
    Skills,
    /// `MemoryStore::format_for_injection` product.
    Memory,
    /// MCP tool JSON schemas (`mcp__*` names).
    Mcp,
    /// The conversation message history.
    Conversation,
}

impl BreakdownCategory {
    /// All categories in stable display order.
    pub const ALL: [BreakdownCategory; 6] = [
        BreakdownCategory::System,
        BreakdownCategory::Tools,
        BreakdownCategory::Skills,
        BreakdownCategory::Memory,
        BreakdownCategory::Mcp,
        BreakdownCategory::Conversation,
    ];

    /// Wire key (frozen contract — frontend colors/labels key off these).
    pub const fn as_str(self) -> &'static str {
        match self {
            BreakdownCategory::System => "system",
            BreakdownCategory::Tools => "tools",
            BreakdownCategory::Skills => "skills",
            BreakdownCategory::Memory => "memory",
            BreakdownCategory::Mcp => "mcp",
            BreakdownCategory::Conversation => "conversation",
        }
    }
}

/// Per-category usage row. `key` is [`BreakdownCategory::as_str`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryUsage {
    pub key: String,
    pub tokens: u64,
}

/// Six-category token estimate for the current session state.
///
/// Frozen wire shape (P0-4): `{ totalTokens, contextWindow, categories }`
/// with `categories[i] = { key, tokens }`. `contextWindow` is `null` when
/// the window is genuinely unknown — no fabricated fallback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBreakdown {
    /// Sum of all category estimates.
    pub total_tokens: u64,
    /// Resolved model context window, `None` when unknown.
    pub context_window: Option<u64>,
    /// One row per category, always all six in stable order.
    pub categories: Vec<CategoryUsage>,
}

impl ContextBreakdown {
    /// Tokens for `key`, or `0` when the category is absent.
    pub fn tokens_for(&self, key: &str) -> u64 {
        self.categories
            .iter()
            .find(|c| c.key == key)
            .map(|c| c.tokens)
            .unwrap_or(0)
    }
}

/// Raw ingredients for [`compute_breakdown`]. Hosts gather these from their
/// engine/session state; keeping them in a plain struct keeps the estimator
/// pure and unit-testable without a `QueryEngine`.
#[derive(Debug, Clone, Default)]
pub struct ContextBreakdownInput {
    /// Base system prompt (the engine assembles further blocks — smart
    /// context, project instructions, repo map — on top; those ambient
    /// reads are host-dependent and intentionally **not** approximated
    /// here, so `system` is a lower bound for the assembled prompt).
    pub system_prompt: Option<String>,
    /// Full tool-definition pool as sent to the LLM (already filtered for
    /// allow-list and deferred tools by `ToolRegistry::to_tool_definitions`).
    /// Split by name prefix: `mcp__*` → `mcp`, `skill_*` → `skills`, rest →
    /// `tools`.
    pub tool_definitions: Vec<ToolDefinition>,
    /// `MemoryStore::format_for_injection` output (`None` when the session
    /// has no memory store or the project has no memories).
    pub memory_text: Option<String>,
    /// The conversation history.
    pub messages: Vec<Message>,
    /// Resolved context window (`None` when genuinely unknown).
    pub context_window: Option<u64>,
}

/// Text token estimate that keeps truly empty content at `0` — the shared
/// estimator floors at 1 token (a message is never empty in practice), but a
/// *present yet empty* ingredient must not fabricate a token here.
fn estimate_non_empty(text: &str) -> u64 {
    if text.trim().is_empty() {
        0
    } else {
        estimate_text_tokens(text) as u64
    }
}

/// Estimate the six-category token breakdown for the given ingredients.
///
/// Never panics on empty input — every category of an empty session is `0`
/// and the total is `0`. The total always equals the sum of the categories.
pub fn compute_breakdown(input: &ContextBreakdownInput) -> ContextBreakdown {
    let mut tools_tokens: u64 = 0;
    let mut skills_tokens: u64 = 0;
    let mut mcp_tokens: u64 = 0;
    for def in &input.tool_definitions {
        let schema_tokens =
            ContextBudget::estimate_schema_tokens(&serde_json::to_value(def).unwrap_or_default())
                as u64;
        if def.name.starts_with(MCP_TOOL_PREFIX) {
            mcp_tokens = mcp_tokens.saturating_add(schema_tokens);
        } else if def.name.starts_with(SKILL_TOOL_PREFIX) {
            skills_tokens = skills_tokens.saturating_add(schema_tokens);
        } else {
            tools_tokens = tools_tokens.saturating_add(schema_tokens);
        }
    }

    let system_tokens = input
        .system_prompt
        .as_deref()
        .map(estimate_non_empty)
        .unwrap_or(0);
    let memory_tokens = input
        .memory_text
        .as_deref()
        .map(estimate_non_empty)
        .unwrap_or(0);
    let conversation_tokens = estimate_tokens(&input.messages) as u64;

    let per_category = [
        system_tokens,
        tools_tokens,
        skills_tokens,
        memory_tokens,
        mcp_tokens,
        conversation_tokens,
    ];
    let total = per_category
        .iter()
        .fold(0u64, |acc, t| acc.saturating_add(*t));

    ContextBreakdown {
        total_tokens: total,
        context_window: input.context_window,
        categories: BreakdownCategory::ALL
            .iter()
            .zip(per_category)
            .map(|(cat, tokens)| CategoryUsage {
                key: cat.as_str().to_string(),
                tokens,
            })
            .collect(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::api::{Message, MessageContent};

    fn text_msg(role: &str, text: &str) -> Message {
        Message {
            role: role.to_string(),
            content: MessageContent::Text(text.to_string()),
        }
    }

    fn tool_def(name: &str, description: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: description.to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string", "description": "long enough to count"}
                }
            }),
            cache_control: None,
            strict: None,
        }
    }

    #[test]
    fn six_categories_present_in_stable_order() {
        let breakdown = compute_breakdown(&ContextBreakdownInput::default());
        let keys: Vec<&str> = breakdown
            .categories
            .iter()
            .map(|c| c.key.as_str())
            .collect();
        assert_eq!(
            keys,
            vec!["system", "tools", "skills", "memory", "mcp", "conversation"]
        );
    }

    #[test]
    fn empty_state_is_all_zero_without_panicking() {
        let breakdown = compute_breakdown(&ContextBreakdownInput::default());
        assert_eq!(breakdown.total_tokens, 0);
        assert_eq!(breakdown.context_window, None);
        assert!(breakdown.categories.iter().all(|c| c.tokens == 0));
    }

    #[test]
    fn populated_state_counts_all_six_categories_and_total_is_sum() {
        let input = ContextBreakdownInput {
            system_prompt: Some("You are Shannon, a helpful AI assistant.".to_string()),
            tool_definitions: vec![
                tool_def("Bash", "Run a shell command"),
                tool_def("mcp__github__search", "Search GitHub"),
                tool_def("mcp__slack__post", "Post a Slack message"),
                tool_def("skill_commit", "Create a conventional commit"),
            ],
            memory_text: Some("## Project Memories\n### preference\n- prefers Rust\n".into()),
            messages: vec![
                text_msg("user", "What is the context window?"),
                text_msg("assistant", "It depends on the model in use."),
            ],
            context_window: Some(200_000),
        };
        let breakdown = compute_breakdown(&input);

        for key in ["system", "tools", "skills", "memory", "mcp", "conversation"] {
            assert!(
                breakdown.tokens_for(key) > 0,
                "category {key} must be > 0 in the populated fixture"
            );
        }
        // MCP pool is counted separately, never inside `tools` — two MCP
        // schemas outweigh the single built-in one.
        assert!(breakdown.tokens_for("mcp") > breakdown.tokens_for("tools"));
        assert!(breakdown.tokens_for("skills") > breakdown.tokens_for("tools"));

        let sum: u64 = breakdown.categories.iter().map(|c| c.tokens).sum();
        assert_eq!(breakdown.total_tokens, sum, "total must equal the sum");
        assert_eq!(breakdown.context_window, Some(200_000));
    }

    #[test]
    fn ascii_and_cjk_text_are_estimated() {
        // ASCII: ~4 chars/token heuristic.
        let ascii = ContextBreakdownInput {
            system_prompt: Some("a".repeat(400)),
            ..Default::default()
        };
        let b = compute_breakdown(&ascii);
        assert_eq!(b.tokens_for("system"), 100);

        // CJK: ~1.5 tokens per char — strictly more than the chars/4 ASCII
        // heuristic would give for the same character count.
        let cjk = ContextBreakdownInput {
            system_prompt: Some("上下文".repeat(100)),
            ..Default::default()
        };
        let b = compute_breakdown(&cjk);
        assert!(b.tokens_for("system") > 100);
    }

    #[test]
    fn empty_strings_estimate_zero_not_one() {
        // estimate_text_tokens floors at 1 token; a *present but empty*
        // system prompt must not fabricate a token. The breakdown treats
        // whitespace-only text as empty.
        let input = ContextBreakdownInput {
            system_prompt: Some("".to_string()),
            memory_text: Some("   ".to_string()),
            ..Default::default()
        };
        let b = compute_breakdown(&input);
        assert_eq!(b.tokens_for("system"), 0);
        assert_eq!(b.tokens_for("memory"), 0);
    }

    #[test]
    fn serde_shape_is_frozen_camel_case() {
        let input = ContextBreakdownInput {
            system_prompt: Some("sys".to_string()),
            context_window: Some(128_000),
            ..Default::default()
        };
        let b = compute_breakdown(&input);
        let json = serde_json::to_value(&b).unwrap();
        assert!(json.get("totalTokens").is_some(), "{json}");
        assert!(json.get("contextWindow").is_some(), "{json}");
        assert!(json.get("categories").is_some(), "{json}");
        let first = &json["categories"][0];
        assert!(
            first.get("key").is_some() && first.get("tokens").is_some(),
            "{json}"
        );
        assert_eq!(json["categories"][0]["key"], "system");
    }
}
