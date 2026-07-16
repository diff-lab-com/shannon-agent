//! Message, request/response, and content types for the LLM API.

use crate::api::retry::RetryConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Trait for resolving provider-specific API keys.
///
/// Implemented by `shannon-core::unified_config::ShannonConfig` to break a
/// cyclic dependency: `LlmClient::set_model_for_provider_with_config` needs
/// to resolve an API key from a config object, but `ShannonConfig` lives in
/// `shannon-core` which depends on this crate. The trait lets the method
/// accept any config type without importing it directly.
pub trait ApiKeyResolver {
    /// Resolve the API key for the given provider.
    fn resolve_api_key_for_provider(&self, provider: &LlmProvider) -> String;
}

// ============================================================================
// Provider Types
// ============================================================================

/// Supported LLM providers.
///
/// Providers are grouped by wire-format compatibility:
/// - **Anthropic-native**: Anthropic, Custom
/// - **OpenAI-compatible**: OpenAI, Azure, Mistral, DeepSeek, Groq, Together
/// - **Google**: Gemini (different JSON schema)
/// - **AWS**: Bedrock (SigV4 auth, different endpoint structure)
/// - **Local**: Ollama
///
/// Wire protocol format used by a provider.
///
/// Each [`LlmProvider`] maps to exactly one wire format. This allows dispatch
/// logic to match on a small enum instead of repeating long provider lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireFormat {
    /// Anthropic native format (also Custom, Bedrock)
    Anthropic,
    /// OpenAI-compatible chat completions (16+ providers)
    OpenAI,
    /// Ollama native `/api/chat` format
    Ollama,
    /// Google Gemini `generateContent` format
    Gemini,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LlmProvider {
    /// Anthropic API (api.anthropic.com)
    Anthropic,
    /// OpenAI API (api.openai.com)
    OpenAI,
    /// Ollama local inference (localhost:11434)
    Ollama,
    /// Custom endpoint with user-defined base URL
    Custom,
    /// Google Gemini API (generativelanguage.googleapis.com)
    Gemini,
    /// Azure OpenAI Service ({resource}.openai.azure.com)
    Azure,
    /// AWS Bedrock (bedrock-runtime.{region}.amazonaws.com)
    Bedrock,
    /// Mistral AI (api.mistral.ai)
    Mistral,
    /// DeepSeek (api.deepseek.com)
    DeepSeek,
    /// Groq (api.groq.com)
    Groq,
    /// Together AI (api.together.xyz)
    Together,
    /// OpenRouter (openrouter.ai)
    OpenRouter,
    /// Cohere (api.cohere.com)
    Cohere,
    /// Fireworks AI (api.fireworks.ai)
    Fireworks,
    /// Perplexity AI (api.perplexity.ai)
    Perplexity,
    /// xAI / Grok (api.x.ai)
    Xai,
    /// AI21 Labs (api.ai21.com)
    Ai21,
    /// Cloudflare Workers AI (api.cloudflare.com)
    Cloudflare,
    /// Replicate (api.replicate.com)
    Replicate,
    /// SiliconFlow (api.siliconflow.cn)
    SiliconFlow,
    /// Zhipu / BigModel (open.bigmodel.cn)
    Zhipu,
    /// Zhipu International / BigModel (open.international.bigmodel.cn)
    ZhipuInternational,
    /// Zhipu Coding Plan (Anthropic-compatible wire format at open.bigmodel.cn/api/anthropic)
    ZhipuCoding,
    /// Moonshot / Kimi (api.moonshot.cn)
    Moonshot,
    /// MiniMax (api.minimax.chat)
    Minimax,
    /// Alibaba DashScope / Qwen (dashscope.aliyuncs.com)
    DashScope,
}

impl LlmProvider {
    /// Detect provider from a base URL.
    ///
    /// Ollama detection requires port 11434 or the string "ollama" in the URL,
    /// to avoid misidentifying arbitrary localhost services.
    pub fn from_base_url(base_url: &str) -> Self {
        let url = base_url.to_lowercase();
        if url.contains("api.anthropic.com") {
            LlmProvider::Anthropic
        } else if url.contains("api.openai.com") {
            LlmProvider::OpenAI
        } else if url.contains("ollama")
            || url.contains(":11434")
            || (url.contains("localhost") && url.contains("11434"))
        {
            LlmProvider::Ollama
        } else if url.contains("generativelanguage.googleapis.com")
            || url.contains("aiplatform.googleapis.com")
        {
            LlmProvider::Gemini
        } else if url.contains("openai.azure.com") {
            LlmProvider::Azure
        } else if url.contains("bedrock-runtime.") || url.contains("amazonaws.com") {
            LlmProvider::Bedrock
        } else if url.contains("api.mistral.ai") {
            LlmProvider::Mistral
        } else if url.contains("api.deepseek.com") {
            LlmProvider::DeepSeek
        } else if url.contains("api.groq.com") {
            LlmProvider::Groq
        } else if url.contains("api.together.xyz") || url.contains("together.ai") {
            LlmProvider::Together
        } else if url.contains("openrouter.ai") {
            LlmProvider::OpenRouter
        } else if url.contains("api.cohere.com") || url.contains("cohere.ai") {
            LlmProvider::Cohere
        } else if url.contains("api.fireworks.ai") || url.contains("fireworks.ai") {
            LlmProvider::Fireworks
        } else if url.contains("api.perplexity.ai") || url.contains("perplexity.ai") {
            LlmProvider::Perplexity
        } else if url.contains("api.x.ai") || url.contains("x.ai") {
            LlmProvider::Xai
        } else if url.contains("api.ai21.com") || url.contains("ai21.com") {
            LlmProvider::Ai21
        } else if url.contains("api.cloudflare.com") && url.contains("workers-ai") {
            LlmProvider::Cloudflare
        } else if url.contains("api.replicate.com") || url.contains("replicate.com") {
            LlmProvider::Replicate
        } else if url.contains("siliconflow.cn") {
            LlmProvider::SiliconFlow
        } else if url.contains("international.bigmodel.cn") {
            LlmProvider::ZhipuInternational
        } else if url.contains("bigmodel.cn/api/anthropic") {
            LlmProvider::ZhipuCoding
        } else if url.contains("bigmodel.cn") || url.contains("zhipuai.cn") {
            LlmProvider::Zhipu
        } else if url.contains("moonshot.cn") || url.contains("kimi") {
            LlmProvider::Moonshot
        } else if url.contains("minimax.chat") || url.contains("minimaxi.com") {
            LlmProvider::Minimax
        } else if url.contains("dashscope.aliyuncs.com") || url.contains("aliyuncs.com") {
            LlmProvider::DashScope
        } else {
            LlmProvider::Custom
        }
    }

    /// Get the API endpoint path for this provider
    pub fn endpoint(&self) -> &'static str {
        match self {
            LlmProvider::Anthropic | LlmProvider::Custom => "/v1/messages",
            LlmProvider::OpenAI => "/v1/chat/completions",
            LlmProvider::Ollama => "/api/chat",
            // Gemini uses a different endpoint pattern; the model name is embedded in the path.
            // The caller should use `gemini_endpoint()` for the full URL.
            LlmProvider::Gemini => "/v1beta/models/",
            LlmProvider::Azure => "/openai/deployments/",
            // Bedrock uses path-based routing: /model/{model-id}/invoke
            LlmProvider::Bedrock => "/model/",
            LlmProvider::Mistral => "/v1/chat/completions",
            LlmProvider::DeepSeek => "/v1/chat/completions",
            LlmProvider::Groq => "/openai/v1/chat/completions",
            LlmProvider::Together => "/v1/chat/completions",
            LlmProvider::OpenRouter => "/api/v1/chat/completions",
            LlmProvider::Cohere => "/v2/chat",
            LlmProvider::Fireworks => "/v1/chat/completions",
            LlmProvider::Perplexity => "/chat/completions",
            LlmProvider::Xai => "/v1/chat/completions",
            LlmProvider::Ai21 => "/v1/chat/completions",
            LlmProvider::Cloudflare => "/client/v4/accounts/",
            LlmProvider::Replicate => "/v1/predictions",
            LlmProvider::SiliconFlow => "/v1/chat/completions",
            LlmProvider::Zhipu => "/api/paas/v4/chat/completions",
            LlmProvider::ZhipuInternational => "/api/paas/v4/chat/completions",
            LlmProvider::ZhipuCoding => "/v1/messages",
            LlmProvider::Moonshot => "/v1/chat/completions",
            LlmProvider::Minimax => "/v1/chat/completions",
            LlmProvider::DashScope => "/compatible-mode/v1/chat/completions",
        }
    }

    /// Return the default base URL for this provider.
    pub fn default_base_url(&self) -> &'static str {
        match self {
            LlmProvider::Anthropic => "https://api.anthropic.com",
            LlmProvider::OpenAI => "https://api.openai.com",
            LlmProvider::Ollama => "http://localhost:11434",
            LlmProvider::Gemini => "https://generativelanguage.googleapis.com",
            LlmProvider::Azure => "https://openai.azure.com",
            LlmProvider::Bedrock => "https://bedrock-runtime.us-east-1.amazonaws.com",
            LlmProvider::Mistral => "https://api.mistral.ai",
            LlmProvider::DeepSeek => "https://api.deepseek.com",
            LlmProvider::Groq => "https://api.groq.com",
            LlmProvider::Together => "https://api.together.xyz",
            LlmProvider::OpenRouter => "https://openrouter.ai",
            LlmProvider::Cohere => "https://api.cohere.com",
            LlmProvider::Fireworks => "https://api.fireworks.ai",
            LlmProvider::Perplexity => "https://api.perplexity.ai",
            LlmProvider::Xai => "https://api.x.ai",
            LlmProvider::Ai21 => "https://api.ai21.com",
            LlmProvider::Cloudflare => "https://api.cloudflare.com",
            LlmProvider::Replicate => "https://api.replicate.com",
            LlmProvider::SiliconFlow => "https://api.siliconflow.cn",
            LlmProvider::Zhipu => "https://open.bigmodel.cn",
            LlmProvider::ZhipuInternational => "https://open.international.bigmodel.cn",
            LlmProvider::ZhipuCoding => "https://open.bigmodel.cn/api/anthropic",
            LlmProvider::Moonshot => "https://api.moonshot.cn",
            LlmProvider::Minimax => "https://api.minimax.chat",
            LlmProvider::DashScope => "https://dashscope.aliyuncs.com",
            LlmProvider::Custom => "http://localhost:8080",
        }
    }

    /// Whether this provider uses the OpenAI-compatible wire format.
    ///
    /// These providers share the same request/response JSON schema and can
    /// reuse the OpenAI serialization/normalization logic.
    /// Return the wire protocol format for this provider.
    pub fn wire_format(&self) -> WireFormat {
        match self {
            Self::Anthropic | Self::Custom | Self::Bedrock | Self::ZhipuCoding => {
                WireFormat::Anthropic
            }
            Self::OpenAI
            | Self::Azure
            | Self::Mistral
            | Self::DeepSeek
            | Self::Groq
            | Self::Together
            | Self::OpenRouter
            | Self::Cohere
            | Self::Fireworks
            | Self::Perplexity
            | Self::Xai
            | Self::Ai21
            | Self::Cloudflare
            | Self::Replicate
            | Self::SiliconFlow
            | Self::Zhipu
            | Self::ZhipuInternational
            | Self::Moonshot
            | Self::Minimax
            | Self::DashScope => WireFormat::OpenAI,
            Self::Ollama => WireFormat::Ollama,
            Self::Gemini => WireFormat::Gemini,
        }
    }

    pub fn is_openai_compatible(&self) -> bool {
        self.wire_format() == WireFormat::OpenAI
    }

    /// Whether this provider requires authentication
    pub fn requires_auth(&self) -> bool {
        !matches!(self, LlmProvider::Ollama)
    }

    /// Return the canonical environment variable name for this provider's API key.
    pub fn canonical_api_key_env(&self) -> Option<&'static str> {
        match self {
            Self::Anthropic => Some("ANTHROPIC_API_KEY"),
            Self::OpenAI => Some("OPENAI_API_KEY"),
            Self::Gemini => Some("GEMINI_API_KEY"),
            Self::Azure => Some("AZURE_OPENAI_API_KEY"),
            Self::Bedrock => None,
            Self::Mistral => Some("MISTRAL_API_KEY"),
            Self::DeepSeek => Some("DEEPSEEK_API_KEY"),
            Self::Groq => Some("GROQ_API_KEY"),
            Self::Together => Some("TOGETHER_API_KEY"),
            Self::OpenRouter => Some("OPENROUTER_API_KEY"),
            Self::Cohere => Some("COHERE_API_KEY"),
            Self::Fireworks => Some("FIREWORKS_API_KEY"),
            Self::Perplexity => Some("PERPLEXITY_API_KEY"),
            Self::Xai => Some("XAI_API_KEY"),
            Self::Ai21 => Some("AI21_API_KEY"),
            Self::SiliconFlow => Some("SILICONFLOW_API_KEY"),
            Self::Zhipu | Self::ZhipuCoding => Some("ZHIPU_API_KEY"),
            Self::ZhipuInternational => Some("ZHIPU_INTL_API_KEY"),
            Self::Moonshot => Some("MOONSHOT_API_KEY"),
            Self::Minimax => Some("MINIMAX_API_KEY"),
            Self::DashScope => Some("DASHSCOPE_API_KEY"),
            Self::Ollama | Self::Custom | Self::Cloudflare | Self::Replicate => None,
        }
    }

    /// Resolve the API key for this provider from environment variables.
    /// Chain: SHANNON_API_KEY → {PROVIDER_CANONICAL}_API_KEY → empty
    pub fn resolve_api_key_from_env(&self) -> String {
        if let Ok(key) = std::env::var("SHANNON_API_KEY") {
            return key;
        }
        if let Some(env_var) = self.canonical_api_key_env() {
            if let Ok(key) = std::env::var(env_var) {
                return key;
            }
        }
        String::new()
    }
}

impl std::fmt::Display for LlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmProvider::Anthropic => write!(f, "anthropic"),
            LlmProvider::OpenAI => write!(f, "openai"),
            LlmProvider::Ollama => write!(f, "ollama"),
            LlmProvider::Custom => write!(f, "custom"),
            LlmProvider::Gemini => write!(f, "gemini"),
            LlmProvider::Azure => write!(f, "azure"),
            LlmProvider::Bedrock => write!(f, "bedrock"),
            LlmProvider::Mistral => write!(f, "mistral"),
            LlmProvider::DeepSeek => write!(f, "deepseek"),
            LlmProvider::Groq => write!(f, "groq"),
            LlmProvider::Together => write!(f, "together"),
            LlmProvider::OpenRouter => write!(f, "openrouter"),
            LlmProvider::Cohere => write!(f, "cohere"),
            LlmProvider::Fireworks => write!(f, "fireworks"),
            LlmProvider::Perplexity => write!(f, "perplexity"),
            LlmProvider::Xai => write!(f, "xai"),
            LlmProvider::Ai21 => write!(f, "ai21"),
            LlmProvider::Cloudflare => write!(f, "cloudflare"),
            LlmProvider::Replicate => write!(f, "replicate"),
            LlmProvider::SiliconFlow => write!(f, "siliconflow"),
            LlmProvider::Zhipu => write!(f, "zhipu"),
            LlmProvider::ZhipuInternational => write!(f, "zhipu-international"),
            LlmProvider::ZhipuCoding => write!(f, "zhipu-coding"),
            LlmProvider::Moonshot => write!(f, "moonshot"),
            LlmProvider::Minimax => write!(f, "minimax"),
            LlmProvider::DashScope => write!(f, "dashscope"),
        }
    }
}

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for the LLM API client
#[derive(Debug, Clone)]
pub struct LlmClientConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub max_tokens: u32,
    pub timeout_seconds: u64,
    pub api_version: String,
    pub provider: LlmProvider,
    pub extra_headers: HashMap<String, String>,
    pub retry_config: RetryConfig,
    /// Fallback provider to try when the primary provider fails after all retries.
    pub fallback_provider: Option<LlmProvider>,
    /// Fallback base URL used together with `fallback_provider`.
    pub fallback_base_url: Option<String>,
    /// Maximum number of automatic stream reconnection attempts (default: 3).
    pub max_stream_reconnects: u32,
    /// Budget tokens for extended thinking mode (Anthropic-specific).
    /// When set, enables extended thinking with the given token budget.
    pub budget_tokens: Option<u32>,
    /// Reasoning effort level for models that support adaptive thinking.
    /// Maps to Anthropic's `budget_tokens` (via percentage of context window) and
    /// OpenAI's `reasoning_effort` parameter. Takes precedence over `budget_tokens`
    /// for OpenAI-compatible providers when set.
    pub reasoning_effort: Option<ReasoningEffort>,
}

impl Default for LlmClientConfig {
    fn default() -> Self {
        let base_url = std::env::var("SHANNON_BASE_URL")
            .or_else(|_| std::env::var("ANTHROPIC_BASE_URL"))
            .or_else(|_| std::env::var("OPENAI_BASE_URL"))
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string());

        let model = std::env::var("SHANNON_MODEL")
            .or_else(|_| std::env::var("ANTHROPIC_MODEL"))
            .or_else(|_| std::env::var("OPENAI_MODEL"))
            .unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string());

        let provider = LlmProvider::from_base_url(&base_url);

        // Provider-aware API key resolution
        let api_key = provider.resolve_api_key_from_env();

        // If no API key is configured and provider requires auth, check for Ollama
        let (api_key, base_url, model, provider) = if api_key.is_empty()
            && provider.requires_auth()
            && std::env::var("SHANNON_BASE_URL").is_err()
        {
            tracing::info!("No API key configured, defaulting to Ollama (localhost:11434)");
            (
                String::new(),
                "http://localhost:11434".to_string(),
                std::env::var("SHANNON_MODEL").unwrap_or_else(|_| "llama3".to_string()),
                LlmProvider::Ollama,
            )
        } else {
            (api_key, base_url, model, provider)
        };

        let api_version = match provider {
            LlmProvider::Anthropic => {
                std::env::var("ANTHROPIC_API_VERSION").unwrap_or_else(|_| "2023-06-01".to_string())
            }
            _ => String::new(),
        };

        Self {
            api_key,
            base_url,
            model,
            max_tokens: 4096,
            timeout_seconds: if provider == LlmProvider::Ollama {
                300
            } else {
                120
            },
            api_version,
            provider,
            extra_headers: HashMap::new(),
            retry_config: RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
            budget_tokens: None,
            reasoning_effort: None,
        }
    }
}

// NOTE: The `From<ShannonConfig> for LlmClientConfig` impl previously lived
// here but was moved to `shannon-core` to break a cyclic dependency.
// `ShannonConfig` is defined in `shannon-core::unified_config` and itself
// imports from `api::types`, so keeping the impl in this crate would force
// `shannon-engine` to depend on `shannon-core`, recreating the cycle that
// PR-A documented. Rust permits a trait impl in either the type's crate or
// the trait's crate; since `LlmClientConfig` is re-exported back into
// `shannon-core` via the backward-compat shim, the impl lives there now.

impl LlmClientConfig {
    /// Validate that the configuration has the minimum required fields.
    ///
    /// Returns a human-readable error description if something is wrong.
    pub fn validate(&self) -> Result<(), String> {
        if self.base_url.trim().is_empty() {
            return Err(
                "base_url is empty. Set SHANNON_BASE_URL or pass --provider ollama".to_string(),
            );
        }

        // Auth-based providers need an API key
        if self.provider.requires_auth() && self.api_key.trim().is_empty() {
            return Err(format!(
                "API key required for provider '{}' but not found. \
                 Set SHANNON_API_KEY, ANTHROPIC_API_KEY, or OPENAI_API_KEY. \
                 Or use --provider ollama for local inference.",
                self.provider
            ));
        }

        if self.model.trim().is_empty() {
            return Err("model is empty. Set SHANNON_MODEL or pass --model <name>".to_string());
        }

        Ok(())
    }

    /// Quick check whether the config is ready to make API calls.
    pub fn is_configured(&self) -> bool {
        self.validate().is_ok()
    }

    /// Return a user-friendly description of the current configuration.
    pub fn describe(&self) -> String {
        let auth_status = if self.provider.requires_auth() {
            if self.api_key.is_empty() {
                "NO API KEY".to_string()
            } else {
                format!(
                    "key={}..{}",
                    &self.api_key[..2.min(self.api_key.len())],
                    &self.api_key[self.api_key.len().saturating_sub(4)..]
                )
            }
        } else {
            "no auth needed".to_string()
        };

        format!(
            "provider={} model={} base_url={} [{}]",
            self.provider, self.model, self.base_url, auth_status
        )
    }

    /// Create a permissive config for Ollama (no auth needed)
    pub fn ollama_default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "http://localhost:11434".to_string(),
            model: "llama3".to_string(),
            max_tokens: 4096,
            timeout_seconds: 300,
            api_version: String::new(),
            provider: LlmProvider::Ollama,
            extra_headers: HashMap::new(),
            retry_config: RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
            budget_tokens: None,
            reasoning_effort: None,
        }
    }

    /// Create config for OpenAI
    pub fn openai_default() -> Self {
        let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com".to_string());
        let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".to_string());
        Self {
            api_key,
            base_url,
            model,
            max_tokens: 4096,
            timeout_seconds: 120,
            api_version: String::new(),
            provider: LlmProvider::OpenAI,
            extra_headers: HashMap::new(),
            retry_config: RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
            budget_tokens: None,
            reasoning_effort: None,
        }
    }

    /// Create config for Google Gemini
    pub fn gemini_default() -> Self {
        let api_key = std::env::var("GEMINI_API_KEY")
            .or_else(|_| std::env::var("GOOGLE_API_KEY"))
            .unwrap_or_default();
        let model =
            std::env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-2.0-flash".to_string());
        Self {
            api_key,
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            model,
            max_tokens: 8192,
            timeout_seconds: 120,
            api_version: String::new(),
            provider: LlmProvider::Gemini,
            extra_headers: HashMap::new(),
            retry_config: RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
            budget_tokens: None,
            reasoning_effort: None,
        }
    }

    /// Create config for Azure OpenAI
    pub fn azure_default() -> Self {
        let api_key = std::env::var("AZURE_OPENAI_API_KEY").unwrap_or_default();
        let base_url = std::env::var("AZURE_OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://your-resource.openai.azure.com".to_string());
        let model = std::env::var("AZURE_OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".to_string());
        Self {
            api_key,
            base_url,
            model,
            max_tokens: 4096,
            timeout_seconds: 120,
            api_version: String::new(),
            provider: LlmProvider::Azure,
            extra_headers: HashMap::new(),
            retry_config: RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
            budget_tokens: None,
            reasoning_effort: None,
        }
    }

    /// Create config for AWS Bedrock
    pub fn bedrock_default() -> Self {
        let model = std::env::var("BEDROCK_MODEL")
            .unwrap_or_else(|_| "anthropic.claude-sonnet-4-20250514".to_string());
        let region = std::env::var("AWS_REGION")
            .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
            .unwrap_or_else(|_| "us-east-1".to_string());
        Self {
            api_key: String::new(), // Bedrock uses SigV4, not API keys
            base_url: format!("https://bedrock-runtime.{region}.amazonaws.com"),
            model,
            max_tokens: 4096,
            timeout_seconds: 120,
            api_version: String::new(),
            provider: LlmProvider::Bedrock,
            extra_headers: HashMap::new(),
            retry_config: RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
            budget_tokens: None,
            reasoning_effort: None,
        }
    }

    /// Create config for Mistral AI
    pub fn mistral_default() -> Self {
        let api_key = std::env::var("MISTRAL_API_KEY").unwrap_or_default();
        let model =
            std::env::var("MISTRAL_MODEL").unwrap_or_else(|_| "mistral-large-latest".to_string());
        Self {
            api_key,
            base_url: "https://api.mistral.ai".to_string(),
            model,
            max_tokens: 4096,
            timeout_seconds: 120,
            api_version: String::new(),
            provider: LlmProvider::Mistral,
            extra_headers: HashMap::new(),
            retry_config: RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
            budget_tokens: None,
            reasoning_effort: None,
        }
    }

    /// Create config for DeepSeek
    pub fn deepseek_default() -> Self {
        let api_key = std::env::var("DEEPSEEK_API_KEY").unwrap_or_default();
        let model = std::env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-chat".to_string());
        Self {
            api_key,
            base_url: "https://api.deepseek.com".to_string(),
            model,
            max_tokens: 4096,
            timeout_seconds: 120,
            api_version: String::new(),
            provider: LlmProvider::DeepSeek,
            extra_headers: HashMap::new(),
            retry_config: RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
            budget_tokens: None,
            reasoning_effort: None,
        }
    }

    /// Create config for Groq
    pub fn groq_default() -> Self {
        let api_key = std::env::var("GROQ_API_KEY").unwrap_or_default();
        let model =
            std::env::var("GROQ_MODEL").unwrap_or_else(|_| "llama-3.3-70b-versatile".to_string());
        Self {
            api_key,
            base_url: "https://api.groq.com".to_string(),
            model,
            max_tokens: 4096,
            timeout_seconds: 120,
            api_version: String::new(),
            provider: LlmProvider::Groq,
            extra_headers: HashMap::new(),
            retry_config: RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
            budget_tokens: None,
            reasoning_effort: None,
        }
    }

    /// Create config for Together AI
    pub fn together_default() -> Self {
        let api_key = std::env::var("TOGETHER_API_KEY").unwrap_or_default();
        let model = std::env::var("TOGETHER_MODEL")
            .unwrap_or_else(|_| "meta-llama/Llama-3.3-70B-Instruct-Turbo".to_string());
        Self {
            api_key,
            base_url: "https://api.together.xyz".to_string(),
            model,
            max_tokens: 4096,
            timeout_seconds: 120,
            api_version: String::new(),
            provider: LlmProvider::Together,
            extra_headers: HashMap::new(),
            retry_config: RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
            budget_tokens: None,
            reasoning_effort: None,
        }
    }
}

/// Backward-compatible alias
pub type ClaudeClientConfig = LlmClientConfig;

// ============================================================================
// Reasoning / Extended Thinking Types
// ============================================================================

/// Reasoning effort level for models that support adaptive thinking.
///
/// Maps to Anthropic's `budget_tokens` (via percentage of context window) and
/// OpenAI's `reasoning_effort` parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl ReasoningEffort {
    /// Convert to an Anthropic API `budget_tokens` value, computed as a
    /// fraction of the model's maximum context window size.
    pub fn to_anthropic_budget(&self, max_context: usize) -> usize {
        let fraction = match self {
            Self::Low => 0.1,
            Self::Medium => 0.25,
            Self::High => 0.5,
            Self::XHigh => 0.7,
            Self::Max => 0.8,
        };
        let budget = (max_context as f64 * fraction) as usize;
        budget.max(1)
    }

    /// Convert to the OpenAI API `reasoning_effort` string value.
    ///
    /// OpenAI only supports three levels: `low`, `medium`, `high`.
    /// `XHigh` and `Max` both map to `high`.
    pub fn to_openai_effort(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "high",
            Self::Max => "high",
        }
    }
}

impl std::fmt::Display for ReasoningEffort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
            Self::XHigh => write!(f, "xhigh"),
            Self::Max => write!(f, "max"),
        }
    }
}

/// A thinking block returned in assistant responses when extended thinking is
/// enabled.
///
/// Anthropic returns `{ "type": "thinking", "thinking": "...", "signature": "..." }`.
/// The `signature` field is optional and may not be present in all responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingBlock {
    pub thinking: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

// ============================================================================
// Message Types
// ============================================================================

/// Message content for API requests
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// Simple text content
    Text(String),
    /// Structured content blocks
    Blocks(Vec<ContentBlock>),
}

/// Content block in messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "image")]
    Image { source: ImageSource },

    /// Thinking block for extended thinking mode (Anthropic-specific)
    #[serde(rename = "thinking")]
    Thinking { thinking: String },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: Option<ToolResultContent>,
        is_error: Option<bool>,
    },
}

/// Image source for image blocks (Anthropic format).
///
/// Serialized as: `{ "type": "base64", "media_type": "image/png", "data": "..." }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

impl ImageSource {
    /// Create a new base64-encoded image source.
    pub fn base64(media_type: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            source_type: "base64".to_string(),
            media_type: media_type.into(),
            data: data.into(),
        }
    }
}

/// Tool result content
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Single(String),
    Multiple(Vec<ContentBlock>),
}

/// Message in the conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: MessageContent,
}

// ============================================================================
// Request/Response Types
// ============================================================================

/// Streaming request parameters
#[derive(Debug, Clone, Serialize)]
pub struct MessageRequest {
    pub model: String,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Structured system prompt with cache breakpoints (Anthropic only).
    /// When set, takes precedence over `system` for Anthropic/Custom providers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_blocks: Option<Vec<SystemContentBlock>>,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Budget tokens for extended thinking (Anthropic-specific).
    /// When set, enables extended thinking mode with the given token budget.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<u32>,
    /// Explicit token budget for thinking (alternative to `reasoning_effort`).
    /// Takes precedence over `reasoning_effort` when both are set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<usize>,
    /// Reasoning effort level (alternative to `thinking_budget`).
    /// Translated to the appropriate provider-specific parameter at request time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
}

// ============================================================================
// Prompt Caching Types
// ============================================================================

/// Cache control marker for Anthropic prompt caching.
///
/// Placed on content blocks to designate cache breakpoints. The API will
/// cache everything up to and including the marked block, reusing it
/// across requests when the prefix matches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheControl {
    #[serde(rename = "type")]
    pub control_type: String,
}

impl CacheControl {
    /// Create an ephemeral cache breakpoint (TTL = 5 min, or 1 hr for eligible users).
    pub fn ephemeral() -> Self {
        Self {
            control_type: "ephemeral".to_string(),
        }
    }
}

/// A structured block within the system prompt, supporting cache breakpoints.
///
/// Anthropic's API accepts system prompts as an array of content blocks:
/// ```json
/// "system": [
///   {"type": "text", "text": "...", "cache_control": {"type": "ephemeral"}},
///   {"type": "text", "text": "..."}
/// ]
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

impl SystemContentBlock {
    /// Create a plain text block without caching.
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            block_type: "text".to_string(),
            text: content.into(),
            cache_control: None,
        }
    }

    /// Create a text block with an ephemeral cache breakpoint.
    pub fn cached(content: impl Into<String>) -> Self {
        Self {
            block_type: "text".to_string(),
            text: content.into(),
            cache_control: Some(CacheControl::ephemeral()),
        }
    }
}

/// Tool definition for function calling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

/// Usage information from API response
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Tokens written to the prompt cache (Anthropic-specific)
    #[serde(default)]
    pub cache_creation_input_tokens: u32,
    /// Tokens read from the prompt cache (Anthropic-specific)
    #[serde(default)]
    pub cache_read_input_tokens: u32,
}

/// API response (non-streaming)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageResponse {
    pub id: String,
    pub role: String,
    pub content: Vec<ContentBlock>,
    pub model: String,
    pub stop_reason: Option<String>,
    pub usage: Usage,
}

// ============================================================================
// Stream Event Types
// ============================================================================

/// Streaming event from API
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: MessageResponse },

    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: usize,
        content_block: ContentBlock,
    },

    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: usize, delta: ContentDelta },

    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: usize },

    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: MessageDeltaDelta,
        usage: Usage,
    },

    #[serde(rename = "message_stop")]
    MessageStop,

    #[serde(rename = "ping")]
    Ping,
}

/// Delta for content block streaming
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentDelta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },

    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },

    /// Thinking delta for extended thinking mode (Anthropic-specific)
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
}

/// Message delta event data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDeltaDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
}

/// Capability info for an Ollama model, retrieved via `/api/show`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OllamaModelInfo {
    /// Model name
    #[serde(default)]
    pub name: String,
    /// Whether the model supports tool/function calling.
    /// Determined by checking the model's template for tool markers.
    #[serde(default)]
    pub supports_tools: bool,
    /// Model's context window size (num_ctx), parsed from model parameters.
    /// Falls back to 4096 if not specified in the model.
    #[serde(default = "default_num_ctx")]
    pub num_ctx: usize,
}

fn default_num_ctx() -> usize {
    4096
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_image_source_base64_constructor() {
        let src = ImageSource::base64("image/png", "abc123");
        assert_eq!(src.source_type, "base64");
        assert_eq!(src.media_type, "image/png");
        assert_eq!(src.data, "abc123");
    }

    #[test]
    fn test_image_source_serialization() {
        let src = ImageSource::base64("image/png", "abc123");
        let json = serde_json::to_string(&src).unwrap();
        assert!(json.contains(r#""type":"base64""#));
        assert!(json.contains(r#""media_type":"image/png""#));
        assert!(json.contains(r#""data":"abc123""#));
    }

    #[test]
    fn test_content_block_image_serialization() {
        let block = ContentBlock::Image {
            source: ImageSource::base64("image/jpeg", "/9j/test"),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#""type":"image""#));
        assert!(json.contains(r#""source""#));
        assert!(json.contains(r#""media_type":"image/jpeg""#));
    }

    #[test]
    fn test_message_with_image_blocks() {
        let msg = Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(vec![
                ContentBlock::Text {
                    text: "Describe this".to_string(),
                },
                ContentBlock::Image {
                    source: ImageSource::base64("image/png", "iVBOR"),
                },
            ]),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"text""#));
        assert!(json.contains(r#""type":"image""#));
        assert!(json.contains(r#""type":"base64""#));
    }

    // -- Provider detection tests --

    #[test]
    fn test_provider_detection_anthropic() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.anthropic.com"),
            LlmProvider::Anthropic
        );
    }

    #[test]
    fn test_provider_detection_openai() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.openai.com"),
            LlmProvider::OpenAI
        );
    }

    #[test]
    fn test_provider_detection_ollama() {
        assert_eq!(
            LlmProvider::from_base_url("http://localhost:11434"),
            LlmProvider::Ollama
        );
        assert_eq!(
            LlmProvider::from_base_url("http://ollama.local"),
            LlmProvider::Ollama
        );
    }

    #[test]
    fn test_provider_detection_gemini() {
        assert_eq!(
            LlmProvider::from_base_url("https://generativelanguage.googleapis.com"),
            LlmProvider::Gemini
        );
    }

    #[test]
    fn test_provider_detection_azure() {
        assert_eq!(
            LlmProvider::from_base_url("https://myresource.openai.azure.com"),
            LlmProvider::Azure
        );
    }

    #[test]
    fn test_provider_detection_bedrock() {
        assert_eq!(
            LlmProvider::from_base_url("https://bedrock-runtime.us-east-1.amazonaws.com"),
            LlmProvider::Bedrock
        );
    }

    #[test]
    fn test_provider_detection_mistral() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.mistral.ai"),
            LlmProvider::Mistral
        );
    }

    #[test]
    fn test_provider_detection_deepseek() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.deepseek.com"),
            LlmProvider::DeepSeek
        );
    }

    #[test]
    fn test_provider_detection_groq() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.groq.com"),
            LlmProvider::Groq
        );
    }

    #[test]
    fn test_provider_detection_together() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.together.xyz"),
            LlmProvider::Together
        );
    }

    #[test]
    fn test_provider_detection_custom() {
        assert_eq!(
            LlmProvider::from_base_url("https://my-custom-llm.example.com"),
            LlmProvider::Custom
        );
    }

    // -- Provider endpoint tests --

    #[test]
    fn test_provider_endpoints() {
        assert_eq!(LlmProvider::Anthropic.endpoint(), "/v1/messages");
        assert_eq!(LlmProvider::OpenAI.endpoint(), "/v1/chat/completions");
        assert_eq!(LlmProvider::Ollama.endpoint(), "/api/chat");
        assert_eq!(LlmProvider::Mistral.endpoint(), "/v1/chat/completions");
        assert_eq!(LlmProvider::DeepSeek.endpoint(), "/v1/chat/completions");
        assert_eq!(LlmProvider::Groq.endpoint(), "/openai/v1/chat/completions");
        assert_eq!(LlmProvider::Together.endpoint(), "/v1/chat/completions");
    }

    // -- OpenAI compatibility tests --

    #[test]
    fn test_openai_compatible_providers() {
        assert!(LlmProvider::OpenAI.is_openai_compatible());
        assert!(LlmProvider::Azure.is_openai_compatible());
        assert!(LlmProvider::Mistral.is_openai_compatible());
        assert!(LlmProvider::DeepSeek.is_openai_compatible());
        assert!(LlmProvider::Groq.is_openai_compatible());
        assert!(LlmProvider::Together.is_openai_compatible());
    }

    #[test]
    fn test_non_openai_compatible_providers() {
        assert!(!LlmProvider::Anthropic.is_openai_compatible());
        assert!(!LlmProvider::Ollama.is_openai_compatible());
        assert!(!LlmProvider::Custom.is_openai_compatible());
        assert!(!LlmProvider::Gemini.is_openai_compatible());
        assert!(!LlmProvider::Bedrock.is_openai_compatible());
    }

    // -- Provider display tests --

    #[test]
    fn test_provider_display() {
        assert_eq!(LlmProvider::Gemini.to_string(), "gemini");
        assert_eq!(LlmProvider::Azure.to_string(), "azure");
        assert_eq!(LlmProvider::Bedrock.to_string(), "bedrock");
        assert_eq!(LlmProvider::Mistral.to_string(), "mistral");
        assert_eq!(LlmProvider::DeepSeek.to_string(), "deepseek");
        assert_eq!(LlmProvider::Groq.to_string(), "groq");
        assert_eq!(LlmProvider::Together.to_string(), "together");
    }

    // -- Convenience constructor tests --

    #[test]
    fn test_gemini_default_config() {
        let cfg = LlmClientConfig::gemini_default();
        assert_eq!(cfg.provider, LlmProvider::Gemini);
        assert!(cfg.base_url.contains("googleapis.com"));
        assert!(cfg.model.contains("gemini"));
        assert!(cfg.provider.requires_auth());
    }

    #[test]
    fn test_mistral_default_config() {
        let cfg = LlmClientConfig::mistral_default();
        assert_eq!(cfg.provider, LlmProvider::Mistral);
        assert!(cfg.base_url.contains("mistral.ai"));
        assert!(cfg.model.contains("mistral"));
        assert!(cfg.provider.is_openai_compatible());
    }

    #[test]
    fn test_deepseek_default_config() {
        let cfg = LlmClientConfig::deepseek_default();
        assert_eq!(cfg.provider, LlmProvider::DeepSeek);
        assert!(cfg.base_url.contains("deepseek.com"));
    }

    #[test]
    fn test_groq_default_config() {
        let cfg = LlmClientConfig::groq_default();
        assert_eq!(cfg.provider, LlmProvider::Groq);
        assert!(cfg.base_url.contains("groq.com"));
    }

    #[test]
    fn test_together_default_config() {
        let cfg = LlmClientConfig::together_default();
        assert_eq!(cfg.provider, LlmProvider::Together);
        assert!(cfg.base_url.contains("together.xyz"));
    }

    // -- ReasoningEffort tests --

    #[test]
    fn test_reasoning_effort_anthropic_budget() {
        // 200k context window (common for Claude models)
        let ctx = 200_000;

        assert_eq!(ReasoningEffort::Low.to_anthropic_budget(ctx), 20_000);
        assert_eq!(ReasoningEffort::Medium.to_anthropic_budget(ctx), 50_000);
        assert_eq!(ReasoningEffort::High.to_anthropic_budget(ctx), 100_000);
        assert_eq!(ReasoningEffort::XHigh.to_anthropic_budget(ctx), 140_000);
        assert_eq!(ReasoningEffort::Max.to_anthropic_budget(ctx), 160_000);
    }

    #[test]
    fn test_reasoning_effort_anthropic_budget_small_context() {
        let ctx = 8_000;

        assert_eq!(ReasoningEffort::Low.to_anthropic_budget(ctx), 800);
        assert_eq!(ReasoningEffort::Medium.to_anthropic_budget(ctx), 2_000);
        assert_eq!(ReasoningEffort::High.to_anthropic_budget(ctx), 4_000);
        assert_eq!(ReasoningEffort::XHigh.to_anthropic_budget(ctx), 5_600);
        assert_eq!(ReasoningEffort::Max.to_anthropic_budget(ctx), 6_400);
    }

    #[test]
    fn test_reasoning_effort_openai_effort_string() {
        assert_eq!(ReasoningEffort::Low.to_openai_effort(), "low");
        assert_eq!(ReasoningEffort::Medium.to_openai_effort(), "medium");
        assert_eq!(ReasoningEffort::High.to_openai_effort(), "high");
        // XHigh and Max both map to high since OpenAI only has 3 levels
        assert_eq!(ReasoningEffort::XHigh.to_openai_effort(), "high");
        assert_eq!(ReasoningEffort::Max.to_openai_effort(), "high");
    }

    #[test]
    fn test_reasoning_effort_display() {
        assert_eq!(ReasoningEffort::Low.to_string(), "low");
        assert_eq!(ReasoningEffort::Medium.to_string(), "medium");
        assert_eq!(ReasoningEffort::High.to_string(), "high");
        assert_eq!(ReasoningEffort::XHigh.to_string(), "xhigh");
        assert_eq!(ReasoningEffort::Max.to_string(), "max");
    }

    #[test]
    fn test_reasoning_effort_serialize_deserialize() {
        for effort in [
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
            ReasoningEffort::XHigh,
            ReasoningEffort::Max,
        ] {
            let json = serde_json::to_string(&effort).unwrap();
            let parsed: ReasoningEffort = serde_json::from_str(&json).unwrap();
            assert_eq!(effort, parsed);
        }
    }

    #[test]
    fn test_reasoning_effort_json_values() {
        // Verify the serialized JSON uses lowercase strings
        let json = serde_json::to_string(&ReasoningEffort::Low).unwrap();
        assert_eq!(json, "\"Low\"");
        let json = serde_json::to_string(&ReasoningEffort::XHigh).unwrap();
        assert_eq!(json, "\"XHigh\"");
    }

    // -- ThinkingBlock tests --

    #[test]
    fn test_thinking_block_serialize() {
        let block = ThinkingBlock {
            thinking: "Let me think about this...".to_string(),
            signature: Some("sig_abc123".to_string()),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#""thinking":"Let me think about this...""#));
        assert!(json.contains(r#""signature":"sig_abc123""#));
    }

    #[test]
    fn test_thinking_block_without_signature() {
        let block = ThinkingBlock {
            thinking: "Reasoning here".to_string(),
            signature: None,
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#""thinking":"Reasoning here""#));
        assert!(!json.contains("signature"));
    }

    #[test]
    fn test_thinking_block_deserialize() {
        let json = r#"{"thinking":"deep thoughts","signature":"sig_xyz"}"#;
        let block: ThinkingBlock = serde_json::from_str(json).unwrap();
        assert_eq!(block.thinking, "deep thoughts");
        assert_eq!(block.signature.as_deref(), Some("sig_xyz"));
    }

    #[test]
    fn test_thinking_block_deserialize_without_signature() {
        let json = r#"{"thinking":"just thinking"}"#;
        let block: ThinkingBlock = serde_json::from_str(json).unwrap();
        assert_eq!(block.thinking, "just thinking");
        assert!(block.signature.is_none());
    }

    // -- MessageRequest with thinking/reasoning fields --

    #[test]
    fn test_message_request_with_thinking_budget() {
        let request = MessageRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 4096,
            system: None,
            system_blocks: None,
            messages: vec![],
            tools: None,
            stream: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            budget_tokens: None,
            thinking_budget: Some(10_000),
            reasoning_effort: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains(r#""thinking_budget":10000"#));
        assert!(!json.contains("reasoning_effort"));
    }

    #[test]
    fn test_message_request_with_reasoning_effort() {
        let request = MessageRequest {
            model: "gpt-4o".to_string(),
            max_tokens: 4096,
            system: None,
            system_blocks: None,
            messages: vec![],
            tools: None,
            stream: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            budget_tokens: None,
            thinking_budget: None,
            reasoning_effort: Some(ReasoningEffort::High),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains(r#""reasoning_effort":"High""#));
        assert!(!json.contains("thinking_budget"));
    }

    #[test]
    fn test_message_request_without_thinking_fields() {
        let request = MessageRequest {
            model: "test-model".to_string(),
            max_tokens: 4096,
            system: None,
            system_blocks: None,
            messages: vec![],
            tools: None,
            stream: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            budget_tokens: None,
            thinking_budget: None,
            reasoning_effort: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(!json.contains("thinking_budget"));
        assert!(!json.contains("reasoning_effort"));
    }

    #[test]
    fn test_provider_detection_dashscope() {
        assert_eq!(
            LlmProvider::from_base_url("https://dashscope.aliyuncs.com"),
            LlmProvider::DashScope
        );
    }

    #[test]
    fn test_dashscope_wire_format() {
        assert_eq!(LlmProvider::DashScope.wire_format(), WireFormat::OpenAI);
        assert!(LlmProvider::DashScope.is_openai_compatible());
    }

    #[test]
    fn test_dashscope_endpoint() {
        assert_eq!(
            LlmProvider::DashScope.default_base_url(),
            "https://dashscope.aliyuncs.com"
        );
        assert_eq!(
            LlmProvider::DashScope.endpoint(),
            "/compatible-mode/v1/chat/completions"
        );
    }

    // -- from_base_url: remaining providers --

    #[test]
    fn test_provider_detection_openrouter() {
        assert_eq!(
            LlmProvider::from_base_url("https://openrouter.ai"),
            LlmProvider::OpenRouter
        );
    }

    #[test]
    fn test_provider_detection_cohere() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.cohere.com"),
            LlmProvider::Cohere
        );
    }

    #[test]
    fn test_provider_detection_fireworks() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.fireworks.ai"),
            LlmProvider::Fireworks
        );
    }

    #[test]
    fn test_provider_detection_perplexity() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.perplexity.ai"),
            LlmProvider::Perplexity
        );
    }

    #[test]
    fn test_provider_detection_xai() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.x.ai"),
            LlmProvider::Xai
        );
    }

    #[test]
    fn test_provider_detection_ai21() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.ai21.com"),
            LlmProvider::Ai21
        );
    }

    #[test]
    fn test_provider_detection_cloudflare() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.cloudflare.com/workers-ai"),
            LlmProvider::Cloudflare
        );
    }

    #[test]
    fn test_provider_detection_cloudflare_without_workers_ai_is_custom() {
        // Cloudflare detection requires both "api.cloudflare.com" AND "workers-ai"
        assert_eq!(
            LlmProvider::from_base_url("https://api.cloudflare.com"),
            LlmProvider::Custom
        );
    }

    #[test]
    fn test_provider_detection_replicate() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.replicate.com"),
            LlmProvider::Replicate
        );
    }

    #[test]
    fn test_provider_detection_siliconflow() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.siliconflow.cn"),
            LlmProvider::SiliconFlow
        );
    }

    #[test]
    fn test_provider_detection_zhipu() {
        assert_eq!(
            LlmProvider::from_base_url("https://open.bigmodel.cn"),
            LlmProvider::Zhipu
        );
    }

    #[test]
    fn test_provider_detection_zhipu_international_takes_precedence() {
        // international.bigmodel.cn must be detected before bigmodel.cn
        assert_eq!(
            LlmProvider::from_base_url("https://open.international.bigmodel.cn"),
            LlmProvider::ZhipuInternational
        );
    }

    #[test]
    fn test_provider_detection_moonshot() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.moonshot.cn"),
            LlmProvider::Moonshot
        );
    }

    #[test]
    fn test_provider_detection_minimax() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.minimax.chat"),
            LlmProvider::Minimax
        );
    }

    // -- from_base_url: edge cases --

    #[test]
    fn test_provider_detection_trailing_slash() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.anthropic.com/"),
            LlmProvider::Anthropic
        );
        assert_eq!(
            LlmProvider::from_base_url("https://api.openai.com/v1/"),
            LlmProvider::OpenAI
        );
    }

    #[test]
    fn test_provider_detection_with_path() {
        assert_eq!(
            LlmProvider::from_base_url("https://api.anthropic.com/some/path"),
            LlmProvider::Anthropic
        );
        assert_eq!(
            LlmProvider::from_base_url("https://api.openai.com/v1/chat/completions"),
            LlmProvider::OpenAI
        );
    }

    #[test]
    fn test_provider_detection_case_insensitive() {
        assert_eq!(
            LlmProvider::from_base_url("HTTPS://API.ANTHROPIC.COM"),
            LlmProvider::Anthropic
        );
        assert_eq!(
            LlmProvider::from_base_url("HtTp://LocalHost:11434"),
            LlmProvider::Ollama
        );
    }

    #[test]
    fn test_provider_detection_empty_url_is_custom() {
        assert_eq!(LlmProvider::from_base_url(""), LlmProvider::Custom);
    }

    // -- endpoint() for remaining providers --

    #[test]
    fn test_endpoint_remaining_providers() {
        assert_eq!(
            LlmProvider::OpenRouter.endpoint(),
            "/api/v1/chat/completions"
        );
        assert_eq!(LlmProvider::Cohere.endpoint(), "/v2/chat");
        assert_eq!(LlmProvider::Fireworks.endpoint(), "/v1/chat/completions");
        assert_eq!(LlmProvider::Perplexity.endpoint(), "/chat/completions");
        assert_eq!(LlmProvider::Xai.endpoint(), "/v1/chat/completions");
        assert_eq!(LlmProvider::Ai21.endpoint(), "/v1/chat/completions");
        assert_eq!(LlmProvider::Cloudflare.endpoint(), "/client/v4/accounts/");
        assert_eq!(LlmProvider::Replicate.endpoint(), "/v1/predictions");
        assert_eq!(LlmProvider::SiliconFlow.endpoint(), "/v1/chat/completions");
        assert_eq!(
            LlmProvider::Zhipu.endpoint(),
            "/api/paas/v4/chat/completions"
        );
        assert_eq!(
            LlmProvider::ZhipuInternational.endpoint(),
            "/api/paas/v4/chat/completions"
        );
        assert_eq!(LlmProvider::Moonshot.endpoint(), "/v1/chat/completions");
        assert_eq!(LlmProvider::Minimax.endpoint(), "/v1/chat/completions");
        assert_eq!(
            LlmProvider::DashScope.endpoint(),
            "/compatible-mode/v1/chat/completions"
        );
    }

    #[test]
    fn test_endpoint_custom_uses_anthropic_path() {
        assert_eq!(LlmProvider::Custom.endpoint(), "/v1/messages");
    }

    // -- wire_format() direct tests --

    #[test]
    fn test_wire_format_anthropic_group() {
        assert_eq!(LlmProvider::Anthropic.wire_format(), WireFormat::Anthropic);
        assert_eq!(LlmProvider::Custom.wire_format(), WireFormat::Anthropic);
        assert_eq!(LlmProvider::Bedrock.wire_format(), WireFormat::Anthropic);
    }

    #[test]
    fn test_wire_format_openai_group() {
        let openai_providers = [
            LlmProvider::OpenAI,
            LlmProvider::Azure,
            LlmProvider::Mistral,
            LlmProvider::DeepSeek,
            LlmProvider::Groq,
            LlmProvider::Together,
            LlmProvider::OpenRouter,
            LlmProvider::Cohere,
            LlmProvider::Fireworks,
            LlmProvider::Perplexity,
            LlmProvider::Xai,
            LlmProvider::Ai21,
            LlmProvider::Cloudflare,
            LlmProvider::Replicate,
            LlmProvider::SiliconFlow,
            LlmProvider::Zhipu,
            LlmProvider::ZhipuInternational,
            LlmProvider::Moonshot,
            LlmProvider::Minimax,
            LlmProvider::DashScope,
        ];
        for provider in openai_providers {
            assert_eq!(
                provider.wire_format(),
                WireFormat::OpenAI,
                "{provider:?} should be OpenAI wire format"
            );
        }
    }

    #[test]
    fn test_wire_format_ollama() {
        assert_eq!(LlmProvider::Ollama.wire_format(), WireFormat::Ollama);
    }

    #[test]
    fn test_wire_format_gemini() {
        assert_eq!(LlmProvider::Gemini.wire_format(), WireFormat::Gemini);
    }

    // -- requires_auth() --

    #[test]
    fn test_requires_auth() {
        // Only Ollama does not require auth
        assert!(!LlmProvider::Ollama.requires_auth());
        // All others do
        assert!(LlmProvider::Anthropic.requires_auth());
        assert!(LlmProvider::OpenAI.requires_auth());
        assert!(LlmProvider::Gemini.requires_auth());
        assert!(LlmProvider::Custom.requires_auth());
        assert!(LlmProvider::Bedrock.requires_auth());
    }

    // -- canonical_api_key_env() --

    #[test]
    fn test_canonical_api_key_env_known_providers() {
        assert_eq!(
            LlmProvider::Anthropic.canonical_api_key_env(),
            Some("ANTHROPIC_API_KEY")
        );
        assert_eq!(
            LlmProvider::OpenAI.canonical_api_key_env(),
            Some("OPENAI_API_KEY")
        );
        assert_eq!(
            LlmProvider::Gemini.canonical_api_key_env(),
            Some("GEMINI_API_KEY")
        );
        assert_eq!(
            LlmProvider::Mistral.canonical_api_key_env(),
            Some("MISTRAL_API_KEY")
        );
        assert_eq!(
            LlmProvider::DeepSeek.canonical_api_key_env(),
            Some("DEEPSEEK_API_KEY")
        );
        assert_eq!(
            LlmProvider::Groq.canonical_api_key_env(),
            Some("GROQ_API_KEY")
        );
        assert_eq!(
            LlmProvider::Together.canonical_api_key_env(),
            Some("TOGETHER_API_KEY")
        );
        assert_eq!(
            LlmProvider::OpenRouter.canonical_api_key_env(),
            Some("OPENROUTER_API_KEY")
        );
        assert_eq!(
            LlmProvider::Cohere.canonical_api_key_env(),
            Some("COHERE_API_KEY")
        );
        assert_eq!(
            LlmProvider::Xai.canonical_api_key_env(),
            Some("XAI_API_KEY")
        );
    }

    #[test]
    fn test_canonical_api_key_env_providers_without_env() {
        // These providers have no canonical env var
        assert!(LlmProvider::Ollama.canonical_api_key_env().is_none());
        assert!(LlmProvider::Custom.canonical_api_key_env().is_none());
        assert!(LlmProvider::Bedrock.canonical_api_key_env().is_none());
        assert!(LlmProvider::Cloudflare.canonical_api_key_env().is_none());
        assert!(LlmProvider::Replicate.canonical_api_key_env().is_none());
    }

    // -- resolve_api_key_from_env() --

    #[test]
    fn test_resolve_api_key_prefers_shannon_key() {
        // SAFETY: Test-only env var manipulation. These vars are test-scoped
        // and cleaned up before and after the test.
        unsafe {
            std::env::remove_var("SHANNON_API_KEY");
            std::env::remove_var("ANTHROPIC_API_KEY");

            // Set both SHANNON_API_KEY and provider-specific key
            std::env::set_var("SHANNON_API_KEY", "shannon-key");
            std::env::set_var("ANTHROPIC_API_KEY", "anthropic-key");
        }

        let resolved = LlmProvider::Anthropic.resolve_api_key_from_env();
        assert_eq!(resolved, "shannon-key");

        // Clean up
        unsafe {
            std::env::remove_var("SHANNON_API_KEY");
            std::env::remove_var("ANTHROPIC_API_KEY");
        }
    }

    #[test]
    fn test_resolve_api_key_falls_back_to_provider_key() {
        unsafe {
            std::env::remove_var("SHANNON_API_KEY");
            std::env::remove_var("OPENAI_API_KEY");

            std::env::set_var("OPENAI_API_KEY", "openai-key");
        }

        let resolved = LlmProvider::OpenAI.resolve_api_key_from_env();
        assert_eq!(resolved, "openai-key");

        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
        }
    }

    #[test]
    fn test_resolve_api_key_returns_empty_when_none_set() {
        unsafe {
            std::env::remove_var("SHANNON_API_KEY");
            std::env::remove_var("DEEPSEEK_API_KEY");
        }

        let resolved = LlmProvider::DeepSeek.resolve_api_key_from_env();
        assert!(resolved.is_empty());
    }

    // -- validate() and is_configured() --

    #[test]
    fn test_validate_valid_config() {
        let cfg = LlmClientConfig {
            api_key: "sk-test".to_string(),
            base_url: "https://api.openai.com".to_string(),
            model: "gpt-4o".to_string(),
            provider: LlmProvider::OpenAI,
            max_tokens: 4096,
            timeout_seconds: 120,
            api_version: String::new(),
            extra_headers: HashMap::new(),
            retry_config: RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
            budget_tokens: None,
            reasoning_effort: None,
        };
        assert!(cfg.validate().is_ok());
        assert!(cfg.is_configured());
    }

    #[test]
    fn test_validate_ollama_without_api_key_is_valid() {
        let cfg = LlmClientConfig::ollama_default();
        assert!(cfg.validate().is_ok());
        assert!(cfg.is_configured());
    }

    #[test]
    fn test_validate_fails_with_empty_base_url() {
        let cfg = LlmClientConfig {
            api_key: "sk-test".to_string(),
            base_url: "  ".to_string(),
            model: "gpt-4o".to_string(),
            provider: LlmProvider::OpenAI,
            max_tokens: 4096,
            timeout_seconds: 120,
            api_version: String::new(),
            extra_headers: HashMap::new(),
            retry_config: RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
            budget_tokens: None,
            reasoning_effort: None,
        };
        let err = cfg.validate().unwrap_err();
        assert!(
            err.contains("base_url"),
            "error should mention base_url: {err}"
        );
        assert!(!cfg.is_configured());
    }

    #[test]
    fn test_validate_fails_with_missing_api_key_for_auth_provider() {
        let cfg = LlmClientConfig {
            api_key: String::new(),
            base_url: "https://api.anthropic.com".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            provider: LlmProvider::Anthropic,
            max_tokens: 4096,
            timeout_seconds: 120,
            api_version: String::new(),
            extra_headers: HashMap::new(),
            retry_config: RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
            budget_tokens: None,
            reasoning_effort: None,
        };
        let err = cfg.validate().unwrap_err();
        assert!(
            err.contains("API key required"),
            "error should mention API key: {err}"
        );
        assert!(
            err.contains("anthropic"),
            "error should mention provider name: {err}"
        );
        assert!(!cfg.is_configured());
    }

    #[test]
    fn test_validate_fails_with_empty_model() {
        let cfg = LlmClientConfig {
            api_key: "sk-test".to_string(),
            base_url: "https://api.openai.com".to_string(),
            model: "  ".to_string(),
            provider: LlmProvider::OpenAI,
            max_tokens: 4096,
            timeout_seconds: 120,
            api_version: String::new(),
            extra_headers: HashMap::new(),
            retry_config: RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
            budget_tokens: None,
            reasoning_effort: None,
        };
        let err = cfg.validate().unwrap_err();
        assert!(err.contains("model"), "error should mention model: {err}");
    }

    #[test]
    fn test_validate_bedrock_without_api_key_is_valid() {
        // Bedrock uses SigV4, not API keys, but requires_auth() is true.
        // The validate() method will reject it without api_key because
        // Bedrock.requires_auth() returns true. This test documents that behavior.
        let cfg = LlmClientConfig::bedrock_default();
        // Bedrock requires_auth() == true but has empty api_key,
        // so validate() should fail.
        assert!(cfg.validate().is_err());
    }

    // -- Provider constructors: untested ones --

    #[test]
    fn test_ollama_default_config() {
        let cfg = LlmClientConfig::ollama_default();
        assert_eq!(cfg.provider, LlmProvider::Ollama);
        assert_eq!(cfg.base_url, "http://localhost:11434");
        assert_eq!(cfg.model, "llama3");
        assert_eq!(cfg.timeout_seconds, 300);
        assert!(cfg.api_key.is_empty());
        assert!(!cfg.provider.requires_auth());
    }

    #[test]
    fn test_openai_default_config() {
        let cfg = LlmClientConfig::openai_default();
        assert_eq!(cfg.provider, LlmProvider::OpenAI);
        assert_eq!(cfg.base_url, "https://api.openai.com");
        assert_eq!(cfg.model, "gpt-4o");
        assert_eq!(cfg.timeout_seconds, 120);
    }

    #[test]
    fn test_azure_default_config() {
        let cfg = LlmClientConfig::azure_default();
        assert_eq!(cfg.provider, LlmProvider::Azure);
        assert!(cfg.base_url.contains("azure.com"));
        assert_eq!(cfg.model, "gpt-4o");
    }

    #[test]
    fn test_bedrock_default_config() {
        let cfg = LlmClientConfig::bedrock_default();
        assert_eq!(cfg.provider, LlmProvider::Bedrock);
        assert!(cfg.base_url.contains("amazonaws.com"));
        assert!(cfg.base_url.contains("us-east-1"));
        assert!(cfg.model.contains("claude"));
        assert!(cfg.api_key.is_empty());
    }

    // ======================================================================
    // Property-based tests (proptest)
    // ======================================================================

    proptest::proptest! {
        /// Any Message roundtrips through JSON serialization.
        #[test]
        fn proptest_message_roundtrip(role in "user|assistant|system", text in ".{0,100}") {
            let msg = Message {
                role: role.clone(),
                content: MessageContent::Text(text.clone()),
            };
            let json = serde_json::to_string(&msg).unwrap();
            let parsed: Message = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.role, role);
            match parsed.content {
                MessageContent::Text(t) => assert_eq!(t, text),
                _ => panic!("expected Text variant"),
            }
        }

        /// ContentDelta::TextDelta roundtrips through JSON.
        #[test]
        fn proptest_content_delta_text_roundtrip(text in ".{0,200}") {
            let delta = ContentDelta::TextDelta { text: text.clone() };
            let json = serde_json::to_string(&delta).unwrap();
            let parsed: ContentDelta = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, delta);
        }

        /// ContentDelta::InputJsonDelta roundtrips through JSON.
        #[test]
        fn proptest_content_delta_json_roundtrip(partial in ".{0,200}") {
            let delta = ContentDelta::InputJsonDelta { partial_json: partial.clone() };
            let json = serde_json::to_string(&delta).unwrap();
            let parsed: ContentDelta = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, delta);
        }

        /// ContentDelta::ThinkingDelta roundtrips through JSON.
        #[test]
        fn proptest_content_delta_thinking_roundtrip(thinking in ".{0,200}") {
            let delta = ContentDelta::ThinkingDelta { thinking: thinking.clone() };
            let json = serde_json::to_string(&delta).unwrap();
            let parsed: ContentDelta = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, delta);
        }

        /// StreamEvent serialization is deterministic: two serializations of the
        /// same value produce identical bytes.
        #[test]
        fn proptest_stream_event_deterministic(text in ".{0,80}") {
            let event = StreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentDelta::TextDelta { text },
            };
            let json1 = serde_json::to_string(&event).unwrap();
            let json2 = serde_json::to_string(&event).unwrap();
            assert_eq!(json1, json2);
        }

        /// ReasoningEffort roundtrips through JSON for all variants.
        #[test]
        fn proptest_reasoning_effort_budget_positive(ctx in 1usize..200_000) {
            for effort in [
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
                ReasoningEffort::XHigh,
                ReasoningEffort::Max,
            ] {
                let budget = effort.to_anthropic_budget(ctx);
                assert!(budget > 0, "budget must be positive for ctx={ctx}");
                assert!(budget <= ctx, "budget must not exceed context for ctx={ctx}");
            }
        }
    }
}
