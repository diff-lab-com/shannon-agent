//! # Model Registry
//!
//! Static catalog of known LLM models grouped by provider, with metadata
//! for context window size and max output tokens. Used by the `/models`
//! picker and Tab completion for the `/model` command.

use crate::api::LlmProvider;

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
    },
    ModelInfo {
        id: "claude-opus-4-20250115",
        display_name: "Claude Opus 4",
        provider: LlmProvider::Anthropic,
        context_window: 200_000,
        max_output: 32_000,
    },
    ModelInfo {
        id: "claude-haiku-4-5-20251001",
        display_name: "Claude Haiku 4.5",
        provider: LlmProvider::Anthropic,
        context_window: 200_000,
        max_output: 8_192,
    },
    ModelInfo {
        id: "claude-3-5-sonnet-20241022",
        display_name: "Claude 3.5 Sonnet",
        provider: LlmProvider::Anthropic,
        context_window: 200_000,
        max_output: 8_192,
    },
    // ── OpenAI ─────────────────────────────────────────────────
    ModelInfo {
        id: "gpt-4o",
        display_name: "GPT-4o",
        provider: LlmProvider::OpenAI,
        context_window: 128_000,
        max_output: 16_384,
    },
    ModelInfo {
        id: "gpt-4o-mini",
        display_name: "GPT-4o Mini",
        provider: LlmProvider::OpenAI,
        context_window: 128_000,
        max_output: 16_384,
    },
    ModelInfo {
        id: "o3-mini",
        display_name: "o3-mini",
        provider: LlmProvider::OpenAI,
        context_window: 200_000,
        max_output: 100_000,
    },
    ModelInfo {
        id: "gpt-4-turbo",
        display_name: "GPT-4 Turbo",
        provider: LlmProvider::OpenAI,
        context_window: 128_000,
        max_output: 4_096,
    },
    // ── Google Gemini ──────────────────────────────────────────
    ModelInfo {
        id: "gemini-2.5-pro",
        display_name: "Gemini 2.5 Pro",
        provider: LlmProvider::Gemini,
        context_window: 1_000_000,
        max_output: 65_536,
    },
    ModelInfo {
        id: "gemini-2.5-flash",
        display_name: "Gemini 2.5 Flash",
        provider: LlmProvider::Gemini,
        context_window: 1_000_000,
        max_output: 65_536,
    },
    // ── DeepSeek ───────────────────────────────────────────────
    ModelInfo {
        id: "deepseek-chat",
        display_name: "DeepSeek V3",
        provider: LlmProvider::DeepSeek,
        context_window: 128_000,
        max_output: 8_192,
    },
    ModelInfo {
        id: "deepseek-reasoner",
        display_name: "DeepSeek R1",
        provider: LlmProvider::DeepSeek,
        context_window: 128_000,
        max_output: 8_192,
    },
    // ── Mistral ────────────────────────────────────────────────
    ModelInfo {
        id: "mistral-large-latest",
        display_name: "Mistral Large",
        provider: LlmProvider::Mistral,
        context_window: 128_000,
        max_output: 8_192,
    },
    ModelInfo {
        id: "codestral-latest",
        display_name: "Codestral",
        provider: LlmProvider::Mistral,
        context_window: 256_000,
        max_output: 8_192,
    },
    // ── Groq ───────────────────────────────────────────────────
    ModelInfo {
        id: "llama-3.3-70b-versatile",
        display_name: "Llama 3.3 70B",
        provider: LlmProvider::Groq,
        context_window: 128_000,
        max_output: 32_768,
    },
    ModelInfo {
        id: "mixtral-8x7b-32768",
        display_name: "Mixtral 8x7B",
        provider: LlmProvider::Groq,
        context_window: 32_000,
        max_output: 4_096,
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
}
