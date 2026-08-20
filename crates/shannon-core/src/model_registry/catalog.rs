//! Static model catalog — types + built-in `MODEL_CATALOG` (ADR-0008 P2-8).
//!
//! Split out of the parent registry so the ~680-line static data table and its
//! supporting types live in one focused module. Everything here is re-exported
//! by the parent, so `model_registry::MODEL_CATALOG` / `ModelInfo` /
//! `ModelCapabilities` / `TierLabel` continue to resolve for every caller.

use shannon_engine::api::LlmProvider;

/// Model capability flags for routing decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModelCapabilities(u8);

impl ModelCapabilities {
    const REASONING: u8 = 1 << 0;
    const CODING: u8 = 1 << 1;
    const SPEED: u8 = 1 << 2;
    const CHEAP: u8 = 1 << 3;
    const VISION: u8 = 1 << 4;

    pub const fn empty() -> Self {
        Self(0)
    }
    pub const fn reasoning() -> Self {
        Self(Self::REASONING)
    }
    pub const fn coding() -> Self {
        Self(Self::CODING)
    }
    pub const fn speed() -> Self {
        Self(Self::SPEED)
    }
    pub const fn cheap() -> Self {
        Self(Self::CHEAP)
    }
    pub const fn vision() -> Self {
        Self(Self::VISION)
    }

    pub const fn has(self, cap: ModelCapabilities) -> bool {
        self.0 & cap.0 != 0
    }
    pub const fn or(self, other: ModelCapabilities) -> Self {
        Self(self.0 | other.0)
    }
}

/// Coarse routing tier classification for a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierLabel {
    Fast,
    Standard,
    Pro,
    Unknown,
}

impl TierLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            TierLabel::Fast => "fast",
            TierLabel::Standard => "standard",
            TierLabel::Pro => "pro",
            TierLabel::Unknown => "unknown",
        }
    }
}

/// Metadata for a single model offering.
#[derive(Debug, Clone)]
pub struct ModelInfo {
    /// Canonical model ID sent to the API (e.g. "claude-sonnet-4-20250514").
    pub id: &'static str,
    /// Human-readable display name (e.g. "Claude Sonnet 4").
    pub display_name: &'static str,
    /// Short aliases for quick selection (e.g. "sonnet", "glm5").
    pub aliases: &'static [&'static str],
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

impl ModelInfo {
    /// Classify this model into a coarse routing tier.
    ///
    /// Heuristic-based: prioritizes the cheap/speed capability flags, then
    /// inspects the model id for known "pro" suffixes, and finally falls
    /// back to capability-driven reasoning/coding classification.
    pub fn tier_label(&self) -> TierLabel {
        let caps = self.capabilities;
        let id = self.id;
        if caps.has(ModelCapabilities::cheap()) || caps.has(ModelCapabilities::speed()) {
            TierLabel::Fast
        } else if id.contains("opus")
            || id.contains("o1")
            || id.contains("ultra")
            || id.contains("max")
        {
            TierLabel::Pro
        } else if caps.has(ModelCapabilities::reasoning()) || caps.has(ModelCapabilities::coding())
        {
            TierLabel::Standard
        } else {
            TierLabel::Unknown
        }
    }
}

// ── Built-in catalog ──────────────────────────────────────────────

/// Static catalog of well-known models. Ollama models are appended at
/// runtime by `detect_local_models`.
pub static MODEL_CATALOG: &[ModelInfo] = &[
    // ── Anthropic ──────────────────────────────────────────────
    ModelInfo {
        id: "claude-sonnet-4-20250514",
        display_name: "Claude Sonnet 4",
        aliases: &["sonnet", "sonnet4", "claude-sonnet"],
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
        aliases: &["opus", "opus4", "claude-opus"],
        provider: LlmProvider::Anthropic,
        context_window: 200_000,
        max_output: 32_000,
        cost_per_m_input: 15.0,
        cost_per_m_output: 75.0,
        capabilities: ModelCapabilities::reasoning()
            .or(ModelCapabilities::coding())
            .or(ModelCapabilities::vision()),
    },
    ModelInfo {
        id: "claude-haiku-4-5-20251001",
        display_name: "Claude Haiku 4.5",
        aliases: &["haiku", "haiku4", "claude-haiku"],
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
        aliases: &[],
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
        aliases: &["gpt4o", "4o"],
        provider: LlmProvider::OpenAI,
        context_window: 128_000,
        max_output: 16_384,
        cost_per_m_input: 2.50,
        cost_per_m_output: 10.0,
        capabilities: ModelCapabilities::coding()
            .or(ModelCapabilities::reasoning())
            .or(ModelCapabilities::vision()),
    },
    ModelInfo {
        id: "gpt-4o-mini",
        display_name: "GPT-4o Mini",
        aliases: &[],
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
        aliases: &[],
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
        aliases: &[],
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
        aliases: &[],
        provider: LlmProvider::Gemini,
        context_window: 1_000_000,
        max_output: 65_536,
        cost_per_m_input: 1.25,
        cost_per_m_output: 10.0,
        capabilities: ModelCapabilities::reasoning()
            .or(ModelCapabilities::coding())
            .or(ModelCapabilities::vision()),
    },
    ModelInfo {
        id: "gemini-2.5-flash",
        display_name: "Gemini 2.5 Flash",
        aliases: &[],
        provider: LlmProvider::Gemini,
        context_window: 1_000_000,
        max_output: 65_536,
        cost_per_m_input: 0.15,
        cost_per_m_output: 0.60,
        capabilities: ModelCapabilities::cheap()
            .or(ModelCapabilities::speed().or(ModelCapabilities::vision())),
    },
    // ── DeepSeek ───────────────────────────────────────────────
    ModelInfo {
        id: "deepseek-chat",
        display_name: "DeepSeek V3",
        aliases: &["ds-chat", "deepseek-chat", "v3"],
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
        aliases: &["ds-r1", "deepseek-reasoner", "r1"],
        provider: LlmProvider::DeepSeek,
        context_window: 128_000,
        max_output: 8_192,
        cost_per_m_input: 0.55,
        cost_per_m_output: 2.19,
        capabilities: ModelCapabilities::reasoning().or(ModelCapabilities::cheap()),
    },
    ModelInfo {
        id: "deepseek-v4-flash",
        display_name: "DeepSeek V4 Flash",
        aliases: &[],
        provider: LlmProvider::DeepSeek,
        context_window: 1_000_000,
        max_output: 384_000,
        cost_per_m_input: 0.14,
        cost_per_m_output: 0.28,
        capabilities: ModelCapabilities::coding()
            .or(ModelCapabilities::cheap())
            .or(ModelCapabilities::speed()),
    },
    ModelInfo {
        id: "deepseek-v4-pro",
        display_name: "DeepSeek V4 Pro",
        aliases: &[],
        provider: LlmProvider::DeepSeek,
        context_window: 1_000_000,
        max_output: 384_000,
        cost_per_m_input: 0.435,
        cost_per_m_output: 0.87,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
    },
    // ── GLM / Zhipu ──────────────────────────────────────────
    ModelInfo {
        id: "glm-4-plus",
        display_name: "GLM-4 Plus",
        aliases: &[],
        provider: LlmProvider::Zhipu,
        context_window: 128_000,
        max_output: 4_096,
        cost_per_m_input: 7.14,
        cost_per_m_output: 7.14,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
    },
    ModelInfo {
        id: "glm-4-flash",
        display_name: "GLM-4 Flash",
        aliases: &[],
        provider: LlmProvider::Zhipu,
        context_window: 128_000,
        max_output: 4_096,
        cost_per_m_input: 0.14,
        cost_per_m_output: 0.14,
        capabilities: ModelCapabilities::speed().or(ModelCapabilities::cheap()),
    },
    ModelInfo {
        id: "glm-4-long",
        display_name: "GLM-4 Long",
        aliases: &[],
        provider: LlmProvider::Zhipu,
        context_window: 1_000_000,
        max_output: 4_096,
        cost_per_m_input: 0.14,
        cost_per_m_output: 0.14,
        capabilities: ModelCapabilities::cheap(),
    },
    ModelInfo {
        id: "glm-4-air",
        display_name: "GLM-4 Air",
        aliases: &[],
        provider: LlmProvider::Zhipu,
        context_window: 128_000,
        max_output: 4_096,
        cost_per_m_input: 0.14,
        cost_per_m_output: 0.14,
        capabilities: ModelCapabilities::speed().or(ModelCapabilities::cheap()),
    },
    ModelInfo {
        id: "glm-4v-flash",
        display_name: "GLM-4V Flash",
        aliases: &[],
        provider: LlmProvider::Zhipu,
        context_window: 128_000,
        max_output: 4_096,
        cost_per_m_input: 0.14,
        cost_per_m_output: 0.14,
        capabilities: ModelCapabilities::vision().or(ModelCapabilities::cheap()),
    },
    ModelInfo {
        id: "glm-5",
        display_name: "GLM-5",
        aliases: &["glm5"],
        provider: LlmProvider::Zhipu,
        context_window: 198_000,
        max_output: 16_384,
        cost_per_m_input: 7.14,
        cost_per_m_output: 7.14,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
    },
    ModelInfo {
        id: "glm-5.1",
        display_name: "GLM-5.1",
        aliases: &["glm51"],
        provider: LlmProvider::Zhipu,
        context_window: 198_000,
        max_output: 128_000,
        cost_per_m_input: 10.0,
        cost_per_m_output: 10.0,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
    },
    ModelInfo {
        id: "glm-5-flash",
        display_name: "GLM-5 Flash",
        aliases: &[],
        provider: LlmProvider::Zhipu,
        context_window: 198_000,
        max_output: 16_384,
        cost_per_m_input: 0.14,
        cost_per_m_output: 0.14,
        capabilities: ModelCapabilities::speed().or(ModelCapabilities::cheap()),
    },
    ModelInfo {
        id: "glm-5.1-flash",
        display_name: "GLM-5.1 Flash",
        aliases: &[],
        provider: LlmProvider::Zhipu,
        context_window: 198_000,
        max_output: 16_384,
        cost_per_m_input: 0.14,
        cost_per_m_output: 0.14,
        capabilities: ModelCapabilities::speed().or(ModelCapabilities::cheap()),
    },
    // ── GLM / Zhipu International ──────────────────────────────
    ModelInfo {
        id: "glm-4-plus-intl",
        display_name: "GLM-4 Plus (Int'l)",
        aliases: &[],
        provider: LlmProvider::ZhipuInternational,
        context_window: 128_000,
        max_output: 4_096,
        cost_per_m_input: 7.14,
        cost_per_m_output: 7.14,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
    },
    ModelInfo {
        id: "glm-4-flash-intl",
        display_name: "GLM-4 Flash (Int'l)",
        aliases: &[],
        provider: LlmProvider::ZhipuInternational,
        context_window: 128_000,
        max_output: 4_096,
        cost_per_m_input: 0.14,
        cost_per_m_output: 0.14,
        capabilities: ModelCapabilities::speed().or(ModelCapabilities::cheap()),
    },
    ModelInfo {
        id: "glm-4-long-intl",
        display_name: "GLM-4 Long (Int'l)",
        aliases: &[],
        provider: LlmProvider::ZhipuInternational,
        context_window: 1_000_000,
        max_output: 4_096,
        cost_per_m_input: 0.14,
        cost_per_m_output: 0.14,
        capabilities: ModelCapabilities::cheap(),
    },
    ModelInfo {
        id: "glm-5-intl",
        display_name: "GLM-5 (Int'l)",
        aliases: &[],
        provider: LlmProvider::ZhipuInternational,
        context_window: 198_000,
        max_output: 16_384,
        cost_per_m_input: 7.14,
        cost_per_m_output: 7.14,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
    },
    ModelInfo {
        id: "glm-5.1-intl",
        display_name: "GLM-5.1 (Int'l)",
        aliases: &[],
        provider: LlmProvider::ZhipuInternational,
        context_window: 198_000,
        max_output: 128_000,
        cost_per_m_input: 10.0,
        cost_per_m_output: 10.0,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
    },
    ModelInfo {
        id: "glm-5-flash-intl",
        display_name: "GLM-5 Flash (Int'l)",
        aliases: &[],
        provider: LlmProvider::ZhipuInternational,
        context_window: 198_000,
        max_output: 16_384,
        cost_per_m_input: 0.14,
        cost_per_m_output: 0.14,
        capabilities: ModelCapabilities::speed().or(ModelCapabilities::cheap()),
    },
    // ── Kimi / Moonshot ──────────────────────────────────────
    ModelInfo {
        id: "kimi-k2.6",
        display_name: "Kimi K2.6",
        aliases: &["kimi", "k2"],
        provider: LlmProvider::Moonshot,
        context_window: 256_000,
        max_output: 96_000,
        cost_per_m_input: 0.91,
        cost_per_m_output: 3.78,
        capabilities: ModelCapabilities::coding()
            .or(ModelCapabilities::reasoning())
            .or(ModelCapabilities::vision()),
    },
    ModelInfo {
        id: "kimi-k2.5",
        display_name: "Kimi K2.5",
        aliases: &[],
        provider: LlmProvider::Moonshot,
        context_window: 256_000,
        max_output: 96_000,
        cost_per_m_input: 0.56,
        cost_per_m_output: 2.94,
        capabilities: ModelCapabilities::coding()
            .or(ModelCapabilities::reasoning())
            .or(ModelCapabilities::vision()),
    },
    ModelInfo {
        id: "moonshot-v1-128k",
        display_name: "Moonshot V1 128K",
        aliases: &[],
        provider: LlmProvider::Moonshot,
        context_window: 128_000,
        max_output: 4_096,
        cost_per_m_input: 1.43,
        cost_per_m_output: 4.29,
        capabilities: ModelCapabilities::cheap(),
    },
    ModelInfo {
        id: "moonshot-v1-32k",
        display_name: "Moonshot V1 32K",
        aliases: &[],
        provider: LlmProvider::Moonshot,
        context_window: 32_000,
        max_output: 4_096,
        cost_per_m_input: 0.71,
        cost_per_m_output: 2.86,
        capabilities: ModelCapabilities::cheap(),
    },
    ModelInfo {
        id: "moonshot-v1-8k",
        display_name: "Moonshot V1 8K",
        aliases: &[],
        provider: LlmProvider::Moonshot,
        context_window: 8_000,
        max_output: 4_096,
        cost_per_m_input: 0.29,
        cost_per_m_output: 1.43,
        capabilities: ModelCapabilities::cheap().or(ModelCapabilities::speed()),
    },
    // ── Mistral ────────────────────────────────────────────────
    ModelInfo {
        id: "mistral-large-latest",
        display_name: "Mistral Large",
        aliases: &[],
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
        aliases: &[],
        provider: LlmProvider::Mistral,
        context_window: 256_000,
        max_output: 8_192,
        cost_per_m_input: 0.30,
        cost_per_m_output: 0.90,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::cheap()),
    },
    // ── Qwen / DashScope ──────────────────────────────────────
    ModelInfo {
        id: "qwen3.7-max",
        display_name: "Qwen 3.7 Max",
        aliases: &["qwen-max"],
        provider: LlmProvider::DashScope,
        context_window: 1_000_000,
        max_output: 64_000,
        cost_per_m_input: 1.43,
        cost_per_m_output: 5.71,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
    },
    ModelInfo {
        id: "qwen3.6-plus",
        display_name: "Qwen 3.6 Plus",
        aliases: &[],
        provider: LlmProvider::DashScope,
        context_window: 1_000_000,
        max_output: 64_000,
        cost_per_m_input: 0.57,
        cost_per_m_output: 2.29,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
    },
    ModelInfo {
        id: "qwen3.6-flash",
        display_name: "Qwen 3.6 Flash",
        aliases: &[],
        provider: LlmProvider::DashScope,
        context_window: 1_000_000,
        max_output: 64_000,
        cost_per_m_input: 0.14,
        cost_per_m_output: 0.57,
        capabilities: ModelCapabilities::coding()
            .or(ModelCapabilities::speed())
            .or(ModelCapabilities::cheap()),
    },
    // ── MiniMax ───────────────────────────────────────────────
    ModelInfo {
        id: "MiniMax-M3",
        display_name: "MiniMax M3",
        aliases: &["MiniMax-M3.0"],
        provider: LlmProvider::Minimax,
        context_window: 1_000_000,
        max_output: 64_000,
        // Official M3 pricing not published at catalog time; mirrors M2.7.
        cost_per_m_input: 0.29,
        cost_per_m_output: 1.18,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
    },
    ModelInfo {
        id: "MiniMax-M2.7",
        display_name: "MiniMax M2.7",
        aliases: &[],
        provider: LlmProvider::Minimax,
        context_window: 1_000_000,
        max_output: 64_000,
        cost_per_m_input: 0.29,
        cost_per_m_output: 1.18,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
    },
    ModelInfo {
        id: "MiniMax-M2.5",
        display_name: "MiniMax M2.5",
        aliases: &[],
        provider: LlmProvider::Minimax,
        context_window: 192_000,
        max_output: 32_000,
        cost_per_m_input: 0.29,
        cost_per_m_output: 1.18,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::cheap()),
    },
    ModelInfo {
        id: "MiniMax-M2.7-highspeed",
        display_name: "MiniMax M2.7 Highspeed",
        aliases: &[],
        provider: LlmProvider::Minimax,
        context_window: 1_000_000,
        max_output: 64_000,
        cost_per_m_input: 0.59,
        cost_per_m_output: 2.35,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::speed()),
    },
    // ── Groq ───────────────────────────────────────────────────
    ModelInfo {
        id: "llama-3.3-70b-versatile",
        display_name: "Llama 3.3 70B",
        aliases: &[],
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
        aliases: &[],
        provider: LlmProvider::Groq,
        context_window: 32_000,
        max_output: 4_096,
        cost_per_m_input: 0.24,
        cost_per_m_output: 0.24,
        capabilities: ModelCapabilities::speed().or(ModelCapabilities::cheap()),
    },
    // ── Anthropic (2026 frontier, 1M context GA — no beta header needed) ──
    ModelInfo {
        id: "claude-sonnet-4-6",
        display_name: "Claude Sonnet 4.6",
        aliases: &["sonnet46", "sonnet-4-6"],
        provider: LlmProvider::Anthropic,
        context_window: 1_000_000,
        max_output: 64_000,
        cost_per_m_input: 3.0,
        cost_per_m_output: 15.0,
        capabilities: ModelCapabilities::coding()
            .or(ModelCapabilities::reasoning())
            .or(ModelCapabilities::vision()),
    },
    ModelInfo {
        id: "claude-opus-4-6",
        display_name: "Claude Opus 4.6",
        aliases: &["opus46", "opus-4-6"],
        provider: LlmProvider::Anthropic,
        context_window: 1_000_000,
        max_output: 64_000,
        cost_per_m_input: 15.0,
        cost_per_m_output: 75.0,
        capabilities: ModelCapabilities::reasoning()
            .or(ModelCapabilities::coding())
            .or(ModelCapabilities::vision()),
    },
    // ── OpenAI (2026 frontier) ───────────────────────────────
    ModelInfo {
        id: "gpt-5",
        display_name: "GPT-5",
        aliases: &["gpt5"],
        provider: LlmProvider::OpenAI,
        context_window: 400_000,
        max_output: 128_000,
        cost_per_m_input: 1.25,
        cost_per_m_output: 10.0,
        capabilities: ModelCapabilities::coding()
            .or(ModelCapabilities::reasoning())
            .or(ModelCapabilities::vision()),
    },
    ModelInfo {
        id: "gpt-5-mini",
        display_name: "GPT-5 Mini",
        aliases: &["gpt5-mini"],
        provider: LlmProvider::OpenAI,
        context_window: 400_000,
        max_output: 128_000,
        cost_per_m_input: 0.25,
        cost_per_m_output: 2.0,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::cheap()),
    },
    // ── xAI / Grok (grok-4 retired → 4.5 is current frontier) ──
    ModelInfo {
        id: "grok-4.5",
        display_name: "Grok 4.5",
        aliases: &["grok", "grok4", "grok-4.5"],
        provider: LlmProvider::Xai,
        context_window: 256_000,
        max_output: 100_000,
        cost_per_m_input: 3.0,
        cost_per_m_output: 15.0,
        capabilities: ModelCapabilities::coding()
            .or(ModelCapabilities::reasoning())
            .or(ModelCapabilities::vision()),
    },
    ModelInfo {
        id: "grok-4.1-fast",
        display_name: "Grok 4.1 Fast",
        aliases: &["grok-fast"],
        provider: LlmProvider::Xai,
        context_window: 256_000,
        max_output: 100_000,
        cost_per_m_input: 0.20,
        cost_per_m_output: 1.50,
        capabilities: ModelCapabilities::speed().or(ModelCapabilities::cheap()),
    },
    // ── Perplexity ───────────────────────────────────────────
    ModelInfo {
        id: "sonar-pro",
        display_name: "Sonar Pro",
        aliases: &["sonar"],
        provider: LlmProvider::Perplexity,
        context_window: 200_000,
        max_output: 8_192,
        cost_per_m_input: 3.0,
        cost_per_m_output: 15.0,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
    },
    ModelInfo {
        id: "sonar-reasoning-pro",
        display_name: "Sonar Reasoning Pro",
        aliases: &[],
        provider: LlmProvider::Perplexity,
        context_window: 200_000,
        max_output: 8_192,
        cost_per_m_input: 2.0,
        cost_per_m_output: 8.0,
        capabilities: ModelCapabilities::reasoning().or(ModelCapabilities::coding()),
    },
    // ── Cohere ───────────────────────────────────────────────
    ModelInfo {
        id: "command-r-plus",
        display_name: "Command R+",
        aliases: &["command-r"],
        provider: LlmProvider::Cohere,
        context_window: 128_000,
        max_output: 4_096,
        cost_per_m_input: 2.50,
        cost_per_m_output: 10.0,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
    },
    // ── SiliconFlow ──────────────────────────────────────────
    ModelInfo {
        id: "deepseek-ai/DeepSeek-V3",
        display_name: "DeepSeek V3 (SiliconFlow)",
        aliases: &["sf-dsv3"],
        provider: LlmProvider::SiliconFlow,
        context_window: 128_000,
        max_output: 8_192,
        cost_per_m_input: 0.27,
        cost_per_m_output: 1.10,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::cheap()),
    },
    // ── Together AI ──────────────────────────────────────────
    ModelInfo {
        id: "meta-llama/Llama-3.3-70B-Instruct-Turbo",
        display_name: "Llama 3.3 70B Turbo (Together)",
        aliases: &[],
        provider: LlmProvider::Together,
        context_window: 128_000,
        max_output: 8_192,
        cost_per_m_input: 0.88,
        cost_per_m_output: 0.88,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::cheap()),
    },
    // ── Fireworks AI ─────────────────────────────────────────
    ModelInfo {
        id: "accounts/fireworks/models/llama-v3p1-70b-instruct",
        display_name: "Llama 3.1 70B (Fireworks)",
        aliases: &[],
        provider: LlmProvider::Fireworks,
        context_window: 128_000,
        max_output: 8_192,
        cost_per_m_input: 0.90,
        cost_per_m_output: 0.90,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::cheap()),
    },
    // ── AI21 Labs ────────────────────────────────────────────
    ModelInfo {
        id: "jamba-1.5-large",
        display_name: "Jamba 1.5 Large",
        aliases: &["jamba"],
        provider: LlmProvider::Ai21,
        context_window: 256_000,
        max_output: 4_096,
        cost_per_m_input: 2.0,
        cost_per_m_output: 8.0,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
    },
];
