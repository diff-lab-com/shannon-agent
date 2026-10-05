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
    /// S2-4b bit (schema/wire only — no gating behavior consumes it yet; a
    /// follow-up PR wires tool-path prechecks off this flag). Populated from
    /// the models.dev overlay's `tool_call` field and from user declarations
    /// (`providers.toml` `ModelSpec.capabilities = ["tool_use"]`).
    const TOOL_USE: u8 = 1 << 5;

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
    pub const fn tool_use() -> Self {
        Self(Self::TOOL_USE)
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

/// Provenance of a merged catalog entry (S2-1 / 裁定③). `merge_static_and_dynamic`
/// tags each row so the desktop can render an honest source badge instead of the
/// old always-off `dynamic` flag (redteam 发现#9: the merge used to lose
/// provenance). `Declared` is applied by the picker's whitelist step for rows
/// synthesized from `providers.toml` `ModelSpec` declarations that have no
/// catalog/overlay entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModelEntrySource {
    /// Curated static `MODEL_CATALOG` row (or a locally-detected Ollama model).
    #[default]
    Catalog,
    /// models.dev overlay-only row (no static entry).
    Overlay,
    /// Synthesized from a user declaration (`ModelSpec`) with no
    /// catalog/overlay metadata behind it.
    Declared,
}

impl ModelEntrySource {
    /// Canonical wire token (`ModelInfo.source` on the desktop bridge).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Catalog => "catalog",
            Self::Overlay => "overlay",
            Self::Declared => "declared",
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
    /// Where this row's metadata came from (S2-1 source badge). Static
    /// catalog rows are `Catalog`; the merge and the picker's declared
    /// synthesis retag the rest.
    pub source: ModelEntrySource,
}

impl ModelInfo {
    /// Classify this model into a coarse routing tier.
    ///
    /// Heuristic-based: prioritizes the cheap/speed capability flags, then
    /// inspects the model id for known "pro" suffixes, and finally falls
    /// back to capability-driven reasoning/coding classification.
    pub fn tier_label(&self) -> TierLabel {
        tier_label_for_caps(self.id, self.capabilities)
    }
}

/// The tier heuristic as a free function over `(id, capabilities)` — the
/// shape declared-only rows (S2-1 vault synthesis) have without needing a
/// leaked catalog `ModelInfo`. Kept in lockstep with `ModelInfo::tier_label`
/// (which delegates here).
pub fn tier_label_for_caps(id: &str, caps: ModelCapabilities) -> TierLabel {
    if caps.has(ModelCapabilities::cheap()) || caps.has(ModelCapabilities::speed()) {
        TierLabel::Fast
    } else if id.contains("opus") || id.contains("o1") || id.contains("ultra") || id.contains("max")
    {
        TierLabel::Pro
    } else if caps.has(ModelCapabilities::reasoning()) || caps.has(ModelCapabilities::coding()) {
        TierLabel::Standard
    } else {
        TierLabel::Unknown
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
    },
    ModelInfo {
        id: "glm-5.3-flash",
        display_name: "GLM-5.3 Flash",
        aliases: &[],
        provider: LlmProvider::Zhipu,
        context_window: 1_000_000,
        max_output: 128_000,
        // bigmodel.cn list price ¥0.8/M input, ¥2.8/M output (cache hit
        // ¥0.23/M) — GLM-5.3's 1/10 tier; converted at ≈7 CNY/USD following
        // the catalog's RMB→USD convention (source: bigmodel.cn pricing /
        // Zhipu research announcement, checked 2026-09-06). Without this
        // entry, cost lookup fell through to the $3/$15 fallback (or a
        // random `contains("glm-5")` match at $7.14), inflating eval cost
        // columns ~10-50x.
        cost_per_m_input: 0.114,
        cost_per_m_output: 0.40,
        capabilities: ModelCapabilities::speed()
            .or(ModelCapabilities::cheap())
            .or(ModelCapabilities::vision()),
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
    },
    ModelInfo {
        id: "glm-5.3-flash-intl",
        display_name: "GLM-5.3 Flash (Int'l)",
        aliases: &[],
        provider: LlmProvider::ZhipuInternational,
        context_window: 1_000_000,
        max_output: 128_000,
        // z.ai standard API rates $0.15/M input, $0.50/M output, cached
        // input $0.03/M (checked 2026-09-06). List price is 1/10 of
        // GLM-5.3's; a limited-time discount may bill less.
        cost_per_m_input: 0.15,
        cost_per_m_output: 0.50,
        capabilities: ModelCapabilities::speed()
            .or(ModelCapabilities::cheap())
            .or(ModelCapabilities::vision()),
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
    },
    // ── xAI / Grok ──
    // 2026-09 refresh per xAI official pricing/migration pages: grok-4.6 is
    // the current flagship (500K ctx, $2/$6); grok-4.5 repriced $3/$15 →
    // $2/$6; grok-4.1-fast retired 2026-05-15 — the low-cost slot goes to
    // grok-build-0.1 ($1/$2, 256K), which is also xAI's official redirect
    // target for the retired grok-code-fast-1 coding line.
    ModelInfo {
        id: "grok-4.6",
        display_name: "Grok 4.6",
        aliases: &["grok", "grok4", "grok-4.6"],
        provider: LlmProvider::Xai,
        context_window: 500_000,
        max_output: 100_000,
        cost_per_m_input: 2.0,
        cost_per_m_output: 6.0,
        capabilities: ModelCapabilities::coding()
            .or(ModelCapabilities::reasoning())
            .or(ModelCapabilities::vision()),
        source: ModelEntrySource::Catalog,
    },
    ModelInfo {
        id: "grok-4.5",
        display_name: "Grok 4.5",
        aliases: &["grok-4.5"],
        provider: LlmProvider::Xai,
        context_window: 256_000,
        max_output: 100_000,
        cost_per_m_input: 2.0,
        cost_per_m_output: 6.0,
        capabilities: ModelCapabilities::coding()
            .or(ModelCapabilities::reasoning())
            .or(ModelCapabilities::vision()),
        source: ModelEntrySource::Catalog,
    },
    ModelInfo {
        id: "grok-build-0.1",
        display_name: "Grok Build 0.1",
        aliases: &["grok-fast", "grok-build"],
        provider: LlmProvider::Xai,
        context_window: 256_000,
        max_output: 100_000,
        cost_per_m_input: 1.0,
        cost_per_m_output: 2.0,
        capabilities: ModelCapabilities::speed().or(ModelCapabilities::cheap()),
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
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
        source: ModelEntrySource::Catalog,
    },
    // ── OpenRouter ───────────────────────────────────────────
    // First-switch defaults for `/provider openrouter` (the aggregator had
    // zero catalog entries, so switching kept the previous provider's model).
    // Ids follow OpenRouter's vendor-prefixed convention ("<vendor>/<model>",
    // the same shape as the models.dev overlay ids); prices and capabilities
    // mirror the same model's first-party catalog entry — OpenRouter
    // passthrough bills the upstream rate.
    ModelInfo {
        id: "anthropic/claude-sonnet-4",
        display_name: "Claude Sonnet 4 (OpenRouter)",
        aliases: &[],
        provider: LlmProvider::OpenRouter,
        context_window: 200_000,
        max_output: 16_384,
        cost_per_m_input: 3.0,
        cost_per_m_output: 15.0,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
        source: ModelEntrySource::Catalog,
    },
    ModelInfo {
        id: "anthropic/claude-opus-4",
        display_name: "Claude Opus 4 (OpenRouter)",
        aliases: &[],
        provider: LlmProvider::OpenRouter,
        context_window: 200_000,
        max_output: 32_000,
        cost_per_m_input: 15.0,
        cost_per_m_output: 75.0,
        capabilities: ModelCapabilities::reasoning()
            .or(ModelCapabilities::coding())
            .or(ModelCapabilities::vision()),
        source: ModelEntrySource::Catalog,
    },
    ModelInfo {
        id: "openai/gpt-5",
        display_name: "GPT-5 (OpenRouter)",
        aliases: &[],
        provider: LlmProvider::OpenRouter,
        context_window: 400_000,
        max_output: 128_000,
        cost_per_m_input: 1.25,
        cost_per_m_output: 10.0,
        capabilities: ModelCapabilities::coding()
            .or(ModelCapabilities::reasoning())
            .or(ModelCapabilities::vision()),
        source: ModelEntrySource::Catalog,
    },
    ModelInfo {
        id: "google/gemini-2.5-pro",
        display_name: "Gemini 2.5 Pro (OpenRouter)",
        aliases: &[],
        provider: LlmProvider::OpenRouter,
        context_window: 1_000_000,
        max_output: 65_536,
        cost_per_m_input: 1.25,
        cost_per_m_output: 10.0,
        capabilities: ModelCapabilities::reasoning()
            .or(ModelCapabilities::coding())
            .or(ModelCapabilities::vision()),
        source: ModelEntrySource::Catalog,
    },
    ModelInfo {
        id: "deepseek/deepseek-chat",
        display_name: "DeepSeek V3 (OpenRouter)",
        aliases: &[],
        provider: LlmProvider::OpenRouter,
        context_window: 128_000,
        max_output: 8_192,
        cost_per_m_input: 0.27,
        cost_per_m_output: 1.10,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::cheap()),
        source: ModelEntrySource::Catalog,
    },
    ModelInfo {
        id: "meta-llama/llama-3.3-70b-instruct",
        display_name: "Llama 3.3 70B (OpenRouter)",
        aliases: &[],
        provider: LlmProvider::OpenRouter,
        context_window: 128_000,
        max_output: 32_768,
        // Mirrors the Groq entry's rate for the same open-weights model;
        // OpenRouter's per-vendor pricing fluctuates around it.
        cost_per_m_input: 0.59,
        cost_per_m_output: 0.79,
        capabilities: ModelCapabilities::speed().or(ModelCapabilities::cheap()),
        source: ModelEntrySource::Catalog,
    },
    // ── AWS Bedrock ──────────────────────────────────────────
    // US cross-region inference-profile ids (`us.anthropic.claude-*-v1:0`).
    // Bedrock price parity with the matching Anthropic catalog entry is an
    // acceptable approximation — Bedrock bills Claude at the same USD rates.
    ModelInfo {
        id: "us.anthropic.claude-sonnet-4-20250514-v1:0",
        display_name: "Claude Sonnet 4 (Bedrock)",
        aliases: &[],
        provider: LlmProvider::Bedrock,
        context_window: 200_000,
        max_output: 16_384,
        cost_per_m_input: 3.0,
        cost_per_m_output: 15.0,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
        source: ModelEntrySource::Catalog,
    },
    ModelInfo {
        id: "us.anthropic.claude-opus-4-20250514-v1:0",
        display_name: "Claude Opus 4 (Bedrock)",
        aliases: &[],
        provider: LlmProvider::Bedrock,
        context_window: 200_000,
        max_output: 32_000,
        cost_per_m_input: 15.0,
        cost_per_m_output: 75.0,
        capabilities: ModelCapabilities::reasoning()
            .or(ModelCapabilities::coding())
            .or(ModelCapabilities::vision()),
        source: ModelEntrySource::Catalog,
    },
    ModelInfo {
        id: "us.anthropic.claude-haiku-4-5-20251001-v1:0",
        display_name: "Claude Haiku 4.5 (Bedrock)",
        aliases: &[],
        provider: LlmProvider::Bedrock,
        context_window: 200_000,
        max_output: 8_192,
        cost_per_m_input: 0.80,
        cost_per_m_output: 4.0,
        capabilities: ModelCapabilities::cheap().or(ModelCapabilities::speed()),
        source: ModelEntrySource::Catalog,
    },
    // ── Azure OpenAI ─────────────────────────────────────────
    // First-switch defaults for `/provider azure` (review P-N14: the provider
    // had zero catalog entries, so switching kept the previous provider's
    // model). Azure serves the OpenAI first-party lineup through per-resource
    // deployments; Shannon's engine pins the deployment name to the model id
    // (`endpoint_url` composes
    // /openai/deployments/{model}/chat/completions). Catalog ids therefore
    // take a deployment-legal "-azure" suffix — the same convention as the
    // "-coding" Zhipu entries — instead of the OpenRouter-style
    // "azure/<model>" slash prefix, because "/" is not a legal deployment
    // name. Azure pricing ≈ the OpenAI list price (same models, passthrough
    // rates; region/licensing multipliers aside), so each entry mirrors its
    // OpenAI first-party sibling below; per-resource rates can be overridden
    // via providers.toml per-model metadata (R2-4).
    ModelInfo {
        id: "gpt-5-azure",
        display_name: "GPT-5 (Azure)",
        aliases: &[],
        provider: LlmProvider::Azure,
        context_window: 400_000,
        max_output: 128_000,
        // Mirrors the OpenAI gpt-5 entry ($1.25/$10.00 per Mtok list).
        cost_per_m_input: 1.25,
        cost_per_m_output: 10.0,
        capabilities: ModelCapabilities::coding()
            .or(ModelCapabilities::reasoning())
            .or(ModelCapabilities::vision()),
    },
    ModelInfo {
        id: "gpt-4o-azure",
        display_name: "GPT-4o (Azure)",
        aliases: &[],
        provider: LlmProvider::Azure,
        context_window: 128_000,
        max_output: 16_384,
        // Mirrors the OpenAI gpt-4o entry ($2.50/$10.00 per Mtok list).
        cost_per_m_input: 2.50,
        cost_per_m_output: 10.0,
        capabilities: ModelCapabilities::coding()
            .or(ModelCapabilities::reasoning())
            .or(ModelCapabilities::vision()),
    },
    ModelInfo {
        id: "gpt-4o-mini-azure",
        display_name: "GPT-4o Mini (Azure)",
        aliases: &[],
        provider: LlmProvider::Azure,
        context_window: 128_000,
        max_output: 16_384,
        // Mirrors the OpenAI gpt-4o-mini entry ($0.15/$0.60 per Mtok list).
        cost_per_m_input: 0.15,
        cost_per_m_output: 0.60,
        capabilities: ModelCapabilities::cheap().or(ModelCapabilities::speed()),
    },
    ModelInfo {
        id: "gpt-5-mini-azure",
        display_name: "GPT-5 Mini (Azure)",
        aliases: &[],
        provider: LlmProvider::Azure,
        context_window: 400_000,
        max_output: 128_000,
        // Mirrors the OpenAI gpt-5-mini entry ($0.25/$2.00 per Mtok list).
        cost_per_m_input: 0.25,
        cost_per_m_output: 2.0,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::cheap()),
    },
    // ── GLM / Zhipu Coding & Coding Plan ─────────────────────
    // Both Zhipu coding providers serve the same GLM lineup as the Zhipu
    // entries above: `zhipu-coding` is the Anthropic-compatible
    // /api/anthropic endpoint, `zhipu-coding-plan` the Coding Plan quota at
    // /api/coding/paas/v4. Catalog ids add a provider suffix (same
    // convention as the "-intl" entries) to stay unique; prices mirror the
    // Zhipu (bigmodel.cn) entries.
    ModelInfo {
        id: "glm-5.1-coding",
        display_name: "GLM-5.1 (Coding)",
        aliases: &[],
        provider: LlmProvider::ZhipuCoding,
        context_window: 198_000,
        max_output: 128_000,
        cost_per_m_input: 10.0,
        cost_per_m_output: 10.0,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
        source: ModelEntrySource::Catalog,
    },
    ModelInfo {
        id: "glm-5.3-flash-coding",
        display_name: "GLM-5.3 Flash (Coding)",
        aliases: &[],
        provider: LlmProvider::ZhipuCoding,
        context_window: 1_000_000,
        max_output: 128_000,
        // Mirrors the Zhipu glm-5.3-flash entry (bigmodel.cn ¥0.8/M input,
        // ¥2.8/M output at ≈7 CNY/USD).
        cost_per_m_input: 0.114,
        cost_per_m_output: 0.40,
        capabilities: ModelCapabilities::speed()
            .or(ModelCapabilities::cheap())
            .or(ModelCapabilities::vision()),
        source: ModelEntrySource::Catalog,
    },
    ModelInfo {
        id: "glm-5.1-coding-plan",
        display_name: "GLM-5.1 (Coding Plan)",
        aliases: &[],
        provider: LlmProvider::ZhipuCodingPlan,
        context_window: 198_000,
        max_output: 128_000,
        cost_per_m_input: 10.0,
        cost_per_m_output: 10.0,
        capabilities: ModelCapabilities::coding().or(ModelCapabilities::reasoning()),
        source: ModelEntrySource::Catalog,
    },
    ModelInfo {
        id: "glm-5.3-flash-coding-plan",
        display_name: "GLM-5.3 Flash (Coding Plan)",
        aliases: &[],
        provider: LlmProvider::ZhipuCodingPlan,
        context_window: 1_000_000,
        max_output: 128_000,
        // Mirrors the Zhipu glm-5.3-flash entry (bigmodel.cn ¥0.8/M input,
        // ¥2.8/M output at ≈7 CNY/USD).
        cost_per_m_input: 0.114,
        cost_per_m_output: 0.40,
        capabilities: ModelCapabilities::speed()
            .or(ModelCapabilities::cheap())
            .or(ModelCapabilities::vision()),
        source: ModelEntrySource::Catalog,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    fn find(id: &str) -> &ModelInfo {
        MODEL_CATALOG
            .iter()
            .find(|m| m.id == id)
            .unwrap_or_else(|| panic!("{id} must be present in MODEL_CATALOG"))
    }

    /// C4: glm-5.3-flash (the TB2.1 eval anchor model) must be cataloged
    /// with reference pricing — before this entry the cost lookup fell
    /// through to the $3/$15 fallback or a substring match on "glm-5"
    /// ($7.14), inflating eval cost columns by an order of magnitude.
    #[test]
    fn glm_5_3_flash_entry_carries_reference_pricing() {
        for id in ["glm-5.3-flash", "glm-5.3-flash-intl"] {
            let info = find(id);
            assert!(info.context_window > 0, "{id}: context window recorded");
            assert!(info.max_output > 0, "{id}: max output recorded");
            assert!(
                info.cost_per_m_input > 0.0 && info.cost_per_m_output > 0.0,
                "{id}: reference prices must be positive"
            );
        }
        // Domestic (bigmodel.cn): ¥0.8 in / ¥2.8 out per Mtok at ≈7 CNY/USD.
        let domestic = find("glm-5.3-flash");
        assert_eq!(domestic.context_window, 1_000_000);
        assert_eq!(domestic.max_output, 128_000);
        assert_eq!(domestic.provider, LlmProvider::Zhipu);
        assert!((domestic.cost_per_m_input - 0.114).abs() < 1e-9);
        assert!((domestic.cost_per_m_output - 0.40).abs() < 1e-9);
        // International (z.ai): $0.15 in / $0.50 out per Mtok.
        let intl = find("glm-5.3-flash-intl");
        assert_eq!(intl.provider, LlmProvider::ZhipuInternational);
        assert!((intl.cost_per_m_input - 0.15).abs() < 1e-9);
        assert!((intl.cost_per_m_output - 0.50).abs() < 1e-9);
        // The eval anchor runs on the thinking-enabled multimodal flash
        // tier: fast + cheap + vision.
        assert!(
            domestic
                .capabilities
                .has(ModelCapabilities::cheap().or(ModelCapabilities::vision()))
        );
    }

    /// Catalog hygiene: ids stay unique (an exact-match pricing duplicate
    /// would silently shadow the first entry).
    #[test]
    fn catalog_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for info in MODEL_CATALOG {
            assert!(seen.insert(info.id), "duplicate catalog id: {}", info.id);
        }
    }

    /// Review P1-8 (2026-09-29): OpenRouter / Bedrock / ZhipuCoding /
    /// ZhipuCodingPlan had zero catalog entries, so `/provider <slug>` kept
    /// the previous provider's model (silent misconfiguration). Review
    /// P-N14 (2026-10-05): Azure was the remaining zero-entry mainstream
    /// provider. Each gap provider must now ship first-switch defaults.
    #[test]
    fn gap_providers_have_catalog_entries() {
        for (provider, min) in [
            (LlmProvider::OpenRouter, 4),
            (LlmProvider::Bedrock, 2),
            (LlmProvider::Azure, 4),
            (LlmProvider::ZhipuCoding, 2),
            (LlmProvider::ZhipuCodingPlan, 2),
        ] {
            let count = MODEL_CATALOG
                .iter()
                .filter(|m| m.provider == provider)
                .count();
            assert!(
                count >= min,
                "{provider:?} should have at least {min} catalog entries, got {count}"
            );
        }
    }

    /// Aggregator/channel entries mirror a first-party entry's pricing; keep
    /// the mirrors in sync with their origins (drift here means billing one
    /// provider at another provider's stale rate).
    #[test]
    fn aggregator_and_channel_entries_mirror_first_party_pricing() {
        let pairs: &[(&str, &str)] = &[
            // OpenRouter mirrors.
            ("anthropic/claude-sonnet-4", "claude-sonnet-4-20250514"),
            ("anthropic/claude-opus-4", "claude-opus-4-20250115"),
            ("openai/gpt-5", "gpt-5"),
            ("google/gemini-2.5-pro", "gemini-2.5-pro"),
            ("deepseek/deepseek-chat", "deepseek-chat"),
            (
                "meta-llama/llama-3.3-70b-instruct",
                "llama-3.3-70b-versatile",
            ),
            // Bedrock inference profiles mirror Anthropic (price parity).
            (
                "us.anthropic.claude-sonnet-4-20250514-v1:0",
                "claude-sonnet-4-20250514",
            ),
            (
                "us.anthropic.claude-opus-4-20250514-v1:0",
                "claude-opus-4-20250115",
            ),
            (
                "us.anthropic.claude-haiku-4-5-20251001-v1:0",
                "claude-haiku-4-5-20251001",
            ),
            // Azure deployments mirror the OpenAI first-party list price
            // (same model family served per-resource).
            ("gpt-5-azure", "gpt-5"),
            ("gpt-4o-azure", "gpt-4o"),
            ("gpt-4o-mini-azure", "gpt-4o-mini"),
            ("gpt-5-mini-azure", "gpt-5-mini"),
            // Zhipu coding endpoints mirror the Zhipu (bigmodel.cn) entries.
            ("glm-5.1-coding", "glm-5.1"),
            ("glm-5.3-flash-coding", "glm-5.3-flash"),
            ("glm-5.1-coding-plan", "glm-5.1"),
            ("glm-5.3-flash-coding-plan", "glm-5.3-flash"),
        ];
        for (mirror, origin) in pairs {
            let (a, b) = (find(mirror), find(origin));
            assert!(
                (a.cost_per_m_input - b.cost_per_m_input).abs() < 1e-9,
                "{mirror}: input ${} must mirror {origin}'s ${}",
                a.cost_per_m_input,
                b.cost_per_m_input
            );
            assert!(
                (a.cost_per_m_output - b.cost_per_m_output).abs() < 1e-9,
                "{mirror}: output ${} must mirror {origin}'s ${}",
                a.cost_per_m_output,
                b.cost_per_m_output
            );
        }
    }

    /// Tier-inference sanity for the gap-provider ids: `tier_label` must
    /// classify each new entry the way its capability flags advertise
    /// (Pro flagships, Standard workhorses, Fast cheap models).
    #[test]
    fn gap_provider_entries_tier_labels() {
        let expected: &[(&str, TierLabel)] = &[
            ("anthropic/claude-sonnet-4", TierLabel::Standard),
            ("anthropic/claude-opus-4", TierLabel::Pro),
            ("openai/gpt-5", TierLabel::Standard),
            ("google/gemini-2.5-pro", TierLabel::Standard),
            ("deepseek/deepseek-chat", TierLabel::Fast),
            ("meta-llama/llama-3.3-70b-instruct", TierLabel::Fast),
            (
                "us.anthropic.claude-sonnet-4-20250514-v1:0",
                TierLabel::Standard,
            ),
            ("us.anthropic.claude-opus-4-20250514-v1:0", TierLabel::Pro),
            (
                "us.anthropic.claude-haiku-4-5-20251001-v1:0",
                TierLabel::Fast,
            ),
            ("gpt-5-azure", TierLabel::Standard),
            ("gpt-4o-azure", TierLabel::Standard),
            ("gpt-4o-mini-azure", TierLabel::Fast),
            ("gpt-5-mini-azure", TierLabel::Fast),
            ("glm-5.1-coding", TierLabel::Standard),
            ("glm-5.3-flash-coding", TierLabel::Fast),
            ("glm-5.1-coding-plan", TierLabel::Standard),
            ("glm-5.3-flash-coding-plan", TierLabel::Fast),
        ];
        for (id, tier) in expected {
            assert_eq!(&find(id).tier_label(), tier, "{id} tier label");
        }
    }
}
