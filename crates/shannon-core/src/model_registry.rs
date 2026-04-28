//! # Model Registry
//!
//! Static catalog of known LLM models grouped by provider, with metadata
//! for context window size and max output tokens. Used by the `/models`
//! picker and Tab completion for the `/model` command.
//!
//! Also provides [`ModelRouter`] for intelligent model selection based on
//! task type, cost, and speed requirements.

use crate::api::LlmProvider;

/// Model capability flags for routing decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModelCapabilities(u8);

impl ModelCapabilities {
    const REASONING: u8 = 1 << 0;
    const CODING: u8 = 1 << 1;
    const SPEED: u8 = 1 << 2;
    const CHEAP: u8 = 1 << 3;
    const VISION: u8 = 1 << 4;

    pub const fn empty() -> Self { Self(0) }
    pub const fn reasoning() -> Self { Self(Self::REASONING) }
    pub const fn coding() -> Self { Self(Self::CODING) }
    pub const fn speed() -> Self { Self(Self::SPEED) }
    pub const fn cheap() -> Self { Self(Self::CHEAP) }
    pub const fn vision() -> Self { Self(Self::VISION) }

    pub const fn has(self, cap: ModelCapabilities) -> bool {
        self.0 & cap.0 != 0
    }
    pub const fn or(self, other: ModelCapabilities) -> Self {
        Self(self.0 | other.0)
    }
}

/// Metadata for a single model offering.
#[derive(Debug, Clone)]
pub struct ModelInfo {
    /// Canonical model ID sent to the API (e.g. "claude-sonnet-4-20250514").
    pub id: &'static str,
    /// Human-readable display name (e.g. "Claude Sonnet 4").
    pub display_name: &'static str,
    /// Provider that serves this model.
    pub provider: LlmProvider,
    /// Context window size in tokens.
    pub context_window: usize,
    /// Maximum output tokens per request.
    pub max_output: usize,
    /// Estimated cost per 1M input tokens in USD (0.0 if unknown).
    pub cost_per_m_input: f64,
    /// Estimated cost per 1M output tokens in USD (0.0 if unknown).
    pub cost_per_m_output: f64,
    /// Capability flags for routing.
    pub capabilities: ModelCapabilities,
}

// ── Built-in catalog ──────────────────────────────────────────────

/// Static catalog of well-known models. Ollama models are appended at
/// runtime by [`detect_local_models`].
pub static MODEL_CATALOG: &[ModelInfo] = &[
    // ── Anthropic ──────────────────────────────────────────────
    ModelInfo {
        id: "claude-sonnet-4-20250514",
        display_name: "Claude Sonnet 4",
        provider: LlmProvider::Anthropic,
        context_window: 200_000,
        max_output: 16_384,
        cost_per_m_input: 3.0,
        cost_per_m_output: 15.0,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
    },
    ModelInfo {
        id: "claude-opus-4-20250115",
        display_name: "Claude Opus 4",
        provider: LlmProvider::Anthropic,
        context_window: 200_000,
        max_output: 32_000,
        cost_per_m_input: 15.0,
        cost_per_m_output: 75.0,
        capabilities: ModelCapabilities::reasoning().or(ModelCapabilities::coding()).or(ModelCapabilities::vision()),
    },
    ModelInfo {
        id: "claude-haiku-4-5-20251001",
        display_name: "Claude Haiku 4.5",
        provider: LlmProvider::Anthropic,
        context_window: 200_000,
        max_output: 8_192,
        cost_per_m_input: 0.80,
        cost_per_m_output: 4.0,
        capabilities: ModelCapabilities::cheap().or(ModelCapabilities::speed()),
    },
    ModelInfo {
        id: "claude-3-5-sonnet-20241022",
        display_name: "Claude 3.5 Sonnet",
        provider: LlmProvider::Anthropic,
        context_window: 200_000,
        max_output: 8_192,
        cost_per_m_input: 3.0,
        cost_per_m_output: 15.0,
        capabilities: ModelCapabilities::coding(),
    },
    // ── OpenAI ─────────────────────────────────────────────────
    ModelInfo {
        id: "gpt-4o",
        display_name: "GPT-4o",
        provider: LlmProvider::OpenAI,
        context_window: 128_000,
        max_output: 16_384,
        cost_per_m_input: 2.50,
        cost_per_m_output: 10.0,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()).or(ModelCapabilities::vision()),
    },
    ModelInfo {
        id: "gpt-4o-mini",
        display_name: "GPT-4o Mini",
        provider: LlmProvider::OpenAI,
        context_window: 128_000,
        max_output: 16_384,
        cost_per_m_input: 0.15,
        cost_per_m_output: 0.60,
        capabilities: ModelCapabilities::cheap().or(ModelCapabilities::speed()),
    },
    ModelInfo {
        id: "o3-mini",
        display_name: "o3-mini",
        provider: LlmProvider::OpenAI,
        context_window: 200_000,
        max_output: 100_000,
        cost_per_m_input: 1.10,
        cost_per_m_output: 4.40,
        capabilities: ModelCapabilities::reasoning().or(ModelCapabilities::coding()),
    },
    ModelInfo {
        id: "gpt-4-turbo",
        display_name: "GPT-4 Turbo",
        provider: LlmProvider::OpenAI,
        context_window: 128_000,
        max_output: 4_096,
        cost_per_m_input: 10.0,
        cost_per_m_output: 30.0,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::vision()),
    },
    // ── Google Gemini ──────────────────────────────────────────
    ModelInfo {
        id: "gemini-2.5-pro",
        display_name: "Gemini 2.5 Pro",
        provider: LlmProvider::Gemini,
        context_window: 1_000_000,
        max_output: 65_536,
        cost_per_m_input: 1.25,
        cost_per_m_output: 10.0,
        capabilities: ModelCapabilities::reasoning().or(ModelCapabilities::coding()).or(ModelCapabilities::vision()),
    },
    ModelInfo {
        id: "gemini-2.5-flash",
        display_name: "Gemini 2.5 Flash",
        provider: LlmProvider::Gemini,
        context_window: 1_000_000,
        max_output: 65_536,
        cost_per_m_input: 0.15,
        cost_per_m_output: 0.60,
        capabilities: ModelCapabilities::cheap().or(ModelCapabilities::speed().or(ModelCapabilities::vision())),
    },
    // ── DeepSeek ───────────────────────────────────────────────
    ModelInfo {
        id: "deepseek-chat",
        display_name: "DeepSeek V3",
        provider: LlmProvider::DeepSeek,
        context_window: 128_000,
        max_output: 8_192,
        cost_per_m_input: 0.27,
        cost_per_m_output: 1.10,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::cheap()),
    },
    ModelInfo {
        id: "deepseek-reasoner",
        display_name: "DeepSeek R1",
        provider: LlmProvider::DeepSeek,
        context_window: 128_000,
        max_output: 8_192,
        cost_per_m_input: 0.55,
        cost_per_m_output: 2.19,
        capabilities: ModelCapabilities::reasoning().or(ModelCapabilities::cheap()),
    },
    // ── Mistral ────────────────────────────────────────────────
    ModelInfo {
        id: "mistral-large-latest",
        display_name: "Mistral Large",
        provider: LlmProvider::Mistral,
        context_window: 128_000,
        max_output: 8_192,
        cost_per_m_input: 2.0,
        cost_per_m_output: 6.0,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
    },
    ModelInfo {
        id: "codestral-latest",
        display_name: "Codestral",
        provider: LlmProvider::Mistral,
        context_window: 256_000,
        max_output: 8_192,
        cost_per_m_input: 0.30,
        cost_per_m_output: 0.90,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::cheap()),
    },
    // ── Groq ───────────────────────────────────────────────────
    ModelInfo {
        id: "llama-3.3-70b-versatile",
        display_name: "Llama 3.3 70B",
        provider: LlmProvider::Groq,
        context_window: 128_000,
        max_output: 32_768,
        cost_per_m_input: 0.59,
        cost_per_m_output: 0.79,
        capabilities: ModelCapabilities::speed().or(ModelCapabilities::cheap()),
    },
    ModelInfo {
        id: "mixtral-8x7b-32768",
        display_name: "Mixtral 8x7B",
        provider: LlmProvider::Groq,
        context_window: 32_000,
        max_output: 4_096,
        cost_per_m_input: 0.24,
        cost_per_m_output: 0.24,
        capabilities: ModelCapabilities::speed().or(ModelCapabilities::cheap()),
    },
];

// ── Query helpers ──────────────────────────────────────────────────

/// Return all models in the catalog for a given provider.
pub fn models_for_provider(provider: LlmProvider) -> Vec<&'static ModelInfo> {
    MODEL_CATALOG
        .iter()
        .filter(|m| m.provider == provider)
        .collect()
}

/// Return all distinct providers that have models in the catalog.
pub fn all_providers() -> Vec<LlmProvider> {
    let mut providers: Vec<LlmProvider> = MODEL_CATALOG
        .iter()
        .map(|m| m.provider.clone())
        .collect();
    providers.sort_by_key(provider_order);
    providers.dedup();
    providers
}

/// Provider display ordering (lower = shown first).
fn provider_order(p: &LlmProvider) -> u8 {
    match p {
        LlmProvider::Anthropic => 0,
        LlmProvider::OpenAI => 1,
        LlmProvider::Gemini => 2,
        LlmProvider::DeepSeek => 3,
        LlmProvider::Mistral => 4,
        LlmProvider::Groq => 5,
        LlmProvider::Ollama => 6,
        _ => 99,
    }
}

/// Format a provider name for display (e.g. "OpenAI", "DeepSeek").
pub fn provider_display_name(p: &LlmProvider) -> &'static str {
    match p {
        LlmProvider::Anthropic => "Anthropic",
        LlmProvider::OpenAI => "OpenAI",
        LlmProvider::Gemini => "Google",
        LlmProvider::DeepSeek => "DeepSeek",
        LlmProvider::Mistral => "Mistral",
        LlmProvider::Groq => "Groq",
        LlmProvider::Ollama => "Ollama",
        LlmProvider::Azure => "Azure",
        LlmProvider::Bedrock => "Bedrock",
        LlmProvider::Together => "Together",
        LlmProvider::Custom => "Custom",
    }
}

/// Attempt to detect locally running Ollama models via `ollama list`.
///
/// Returns an empty Vec silently if Ollama is not installed or not running.
pub fn detect_local_models() -> Vec<ModelInfo> {
    let output = match std::process::Command::new("ollama")
        .arg("list")
        .output()
    {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut models = Vec::new();

    for line in stdout.lines().skip(1) {
        // Ollama output: "NAME\tID\tSIZE\tMODIFIED"
        let name = line.split_whitespace().next().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        models.push(ModelInfo {
            id: Box::leak(name.clone().into_boxed_str()),
            display_name: Box::leak(name.into_boxed_str()),
            provider: LlmProvider::Ollama,
            context_window: 128_000,
            max_output: 4_096,
            cost_per_m_input: 0.0,
            cost_per_m_output: 0.0,
            capabilities: ModelCapabilities::cheap().or(ModelCapabilities::speed()),
        });
    }

    models
}

/// Return all model IDs from the catalog (for Tab completion).
pub fn all_model_ids() -> Vec<&'static str> {
    MODEL_CATALOG.iter().map(|m| m.id).collect()
}

/// Look up a model's context window by its ID. Returns a reasonable default
/// (200 000) if the model is not found in the catalog.
pub fn context_window_for(model_id: &str) -> usize {
    MODEL_CATALOG
        .iter()
        .find(|m| m.id == model_id)
        .map(|m| m.context_window)
        .unwrap_or(200_000)
}

/// Look up model info by ID.
pub fn model_info_for(model_id: &str) -> Option<&'static ModelInfo> {
    MODEL_CATALOG.iter().find(|m| m.id == model_id)
}

// ============================================================================
// Model Router
// ============================================================================

/// Task type hint for model routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskType {
    /// Simple question, quick lookup — prefer cheap/fast models
    QuickQuery,
    /// Code generation, editing, debugging — prefer coding models
    CodeGeneration,
    /// Architecture design, complex reasoning — prefer reasoning models
    ArchitectureDesign,
    /// Multi-step workflow — prefer coding + reasoning
    ComplexWorkflow,
}

/// Recommends a model based on task type and preferences.
pub struct ModelRouter;

impl ModelRouter {
    /// Recommend the best model ID for a given task type.
    ///
    /// Falls back to the first model in the catalog if no match is found.
    pub fn recommend(task: TaskType) -> &'static str {
        let required = match task {
            TaskType::QuickQuery => ModelCapabilities::cheap(),
            TaskType::CodeGeneration => ModelCapabilities::coding(),
            TaskType::ArchitectureDesign => ModelCapabilities::reasoning(),
            TaskType::ComplexWorkflow => ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
        };

        // Find cheapest model that has the required capabilities
        let mut best: Option<&'static ModelInfo> = None;
        let mut best_cost = f64::MAX;

        for model in MODEL_CATALOG {
            if model.capabilities.has(required) {
                let cost = model.cost_per_m_input + model.cost_per_m_output;
                if cost < best_cost {
                    best_cost = cost;
                    best = Some(model);
                }
            }
        }

        match best {
            Some(m) => m.id,
            None => MODEL_CATALOG[0].id,
        }
    }

    /// Recommend a model for the given task, with a preference for speed.
    pub fn recommend_fast(task: TaskType) -> &'static str {
        let required = match task {
            TaskType::QuickQuery => ModelCapabilities::cheap().or(ModelCapabilities::speed()),
            TaskType::CodeGeneration => ModelCapabilities::coding().or(ModelCapabilities::speed()),
            TaskType::ArchitectureDesign => ModelCapabilities::reasoning(),
            TaskType::ComplexWorkflow => ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
        };

        for model in MODEL_CATALOG {
            if model.capabilities.has(required) && model.capabilities.has(ModelCapabilities::speed()) {
                return model.id;
            }
        }

        Self::recommend(task)
    }

    /// Estimate cost for a request with the given model and token counts.
    pub fn estimate_cost(model_id: &str, input_tokens: usize, output_tokens: usize) -> f64 {
        if let Some(info) = model_info_for(model_id) {
            let input_cost = (input_tokens as f64 / 1_000_000.0) * info.cost_per_m_input;
            let output_cost = (output_tokens as f64 / 1_000_000.0) * info.cost_per_m_output;
            input_cost + output_cost
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_models_for_provider_anthropic() {
        let models = models_for_provider(LlmProvider::Anthropic);
        assert!(!models.is_empty());
        assert!(models.iter().all(|m| m.provider == LlmProvider::Anthropic));
    }

    #[test]
    fn test_all_providers_contains_major() {
        let providers = all_providers();
        assert!(providers.contains(&LlmProvider::Anthropic));
        assert!(providers.contains(&LlmProvider::OpenAI));
        assert!(providers.contains(&LlmProvider::Gemini));
    }

    #[test]
    fn test_all_model_ids() {
        let ids = all_model_ids();
        assert!(ids.contains(&"claude-sonnet-4-20250514"));
        assert!(ids.contains(&"gpt-4o"));
        assert!(ids.len() >= 14);
    }

    #[test]
    fn test_provider_display_name() {
        assert_eq!(provider_display_name(&LlmProvider::Anthropic), "Anthropic");
        assert_eq!(provider_display_name(&LlmProvider::OpenAI), "OpenAI");
        assert_eq!(provider_display_name(&LlmProvider::Gemini), "Google");
    }

    #[test]
    fn test_provider_order() {
        assert!(provider_order(&LlmProvider::Anthropic) < provider_order(&LlmProvider::OpenAI));
        assert!(provider_order(&LlmProvider::OpenAI) < provider_order(&LlmProvider::Groq));
    }

    #[test]
    fn test_capabilities_or_and_has() {
        let caps = ModelCapabilities::coding().or(ModelCapabilities::reasoning());
        assert!(caps.has(ModelCapabilities::coding()));
        assert!(caps.has(ModelCapabilities::reasoning()));
        assert!(!caps.has(ModelCapabilities::vision()));
    }

    #[test]
    fn test_model_info_for_known() {
        let info = model_info_for("claude-sonnet-4-20250514").unwrap();
        assert_eq!(info.display_name, "Claude Sonnet 4");
        assert_eq!(info.context_window, 200_000);
        assert!(info.cost_per_m_input > 0.0);
    }

    #[test]
    fn test_model_info_for_unknown() {
        assert!(model_info_for("nonexistent-model").is_none());
    }

    #[test]
    fn test_router_recommend_code() {
        let id = ModelRouter::recommend(TaskType::CodeGeneration);
        let info = model_info_for(id).unwrap();
        assert!(info.capabilities.has(ModelCapabilities::coding()));
    }

    #[test]
    fn test_router_recommend_fast() {
        let id = ModelRouter::recommend_fast(TaskType::QuickQuery);
        // Should return a speed-capable model or fallback
        assert!(model_info_for(id).is_some());
    }

    #[test]
    fn test_router_estimate_cost() {
        let cost = ModelRouter::estimate_cost("claude-sonnet-4-20250514", 1_000_000, 1_000_000);
        assert!(cost > 0.0);
        // $3/M input + $15/M output = $18 for 1M each
        assert!((cost - 18.0).abs() < 0.01);
    }

    #[test]
    fn test_router_estimate_cost_unknown() {
        assert_eq!(ModelRouter::estimate_cost("nonexistent", 1000, 1000), 0.0);
    }
}
