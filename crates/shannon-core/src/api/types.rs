//! Message, request/response, and content types for the LLM API.

use crate::api::retry::RetryConfig;
use crate::unified_config::ShannonConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

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
        } else if url.contains("bedrock-runtime.")
            || url.contains("amazonaws.com")
        {
            LlmProvider::Bedrock
        } else if url.contains("api.mistral.ai") {
            LlmProvider::Mistral
        } else if url.contains("api.deepseek.com") {
            LlmProvider::DeepSeek
        } else if url.contains("api.groq.com") {
            LlmProvider::Groq
        } else if url.contains("api.together.xyz")
            || url.contains("together.ai")
        {
            LlmProvider::Together
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
        }
    }

    /// Whether this provider uses the OpenAI-compatible wire format.
    ///
    /// These providers share the same request/response JSON schema and can
    /// reuse the OpenAI serialization/normalization logic.
    pub fn is_openai_compatible(&self) -> bool {
        matches!(
            self,
            LlmProvider::OpenAI
                | LlmProvider::Azure
                | LlmProvider::Mistral
                | LlmProvider::DeepSeek
                | LlmProvider::Groq
                | LlmProvider::Together
        )
    }

    /// Whether this provider requires authentication
    pub fn requires_auth(&self) -> bool {
        !matches!(self, LlmProvider::Ollama)
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
}

impl Default for LlmClientConfig {
    fn default() -> Self {
        let api_key = std::env::var("SHANNON_API_KEY")
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .unwrap_or_default();

        let base_url = std::env::var("SHANNON_BASE_URL")
            .or_else(|_| std::env::var("ANTHROPIC_BASE_URL"))
            .or_else(|_| std::env::var("OPENAI_BASE_URL"))
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string());

        let model = std::env::var("SHANNON_MODEL")
            .or_else(|_| std::env::var("ANTHROPIC_MODEL"))
            .or_else(|_| std::env::var("OPENAI_MODEL"))
            .unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string());

        let provider = LlmProvider::from_base_url(&base_url);

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
            LlmProvider::Anthropic => std::env::var("ANTHROPIC_API_VERSION")
                .unwrap_or_else(|_| "2023-06-01".to_string()),
            _ => String::new(),
        };

        Self {
            api_key,
            base_url,
            model,
            max_tokens: 4096,
            timeout_seconds: if provider == LlmProvider::Ollama { 300 } else { 120 },
            api_version,
            provider,
            extra_headers: HashMap::new(),
            retry_config: RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
        }
    }
}

impl From<ShannonConfig> for LlmClientConfig {
    /// Convert a merged [`ShannonConfig`] into an [`LlmClientConfig`].
    ///
    /// Fields that are `Some` in `ShannonConfig` take precedence; everything
    /// else falls back to the same env-var and default logic that
    /// [`LlmClientConfig::default`] uses.
    fn from(cfg: ShannonConfig) -> Self {
        let has_explicit_base_url = cfg.base_url.is_some();
        let has_explicit_model = cfg.model.is_some();

        // --- Resolve api_key ------------------------------------------------
        let api_key = cfg
            .api_key
            .or_else(|| std::env::var("SHANNON_API_KEY").ok())
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .unwrap_or_default();

        // --- Resolve base_url -----------------------------------------------
        let base_url = cfg
            .base_url
            .or_else(|| std::env::var("SHANNON_BASE_URL").ok())
            .or_else(|| std::env::var("ANTHROPIC_BASE_URL").ok())
            .or_else(|| std::env::var("OPENAI_BASE_URL").ok())
            .unwrap_or_else(|| "https://api.anthropic.com".to_string());

        // --- Resolve model --------------------------------------------------
        let model = cfg
            .model
            .or_else(|| std::env::var("SHANNON_MODEL").ok())
            .or_else(|| std::env::var("ANTHROPIC_MODEL").ok())
            .or_else(|| std::env::var("OPENAI_MODEL").ok())
            .unwrap_or_else(|| "claude-sonnet-4-20250514".to_string());

        // --- Resolve provider -----------------------------------------------
        let provider = if let Some(ref p) = cfg.provider {
            match p.to_lowercase().as_str() {
                "anthropic" => LlmProvider::Anthropic,
                "openai" => LlmProvider::OpenAI,
                "ollama" => LlmProvider::Ollama,
                "gemini" | "google" => LlmProvider::Gemini,
                "azure" | "azure-openai" => LlmProvider::Azure,
                "bedrock" | "aws" => LlmProvider::Bedrock,
                "mistral" | "mistral-ai" => LlmProvider::Mistral,
                "deepseek" => LlmProvider::DeepSeek,
                "groq" => LlmProvider::Groq,
                "together" | "together-ai" => LlmProvider::Together,
                _ => LlmProvider::from_base_url(&base_url),
            }
        } else {
            LlmProvider::from_base_url(&base_url)
        };

        // --- Auto-fallback to Ollama when no key & no explicit base_url ----
        let (api_key, base_url, model, provider) = if api_key.is_empty()
            && provider.requires_auth()
            && !has_explicit_base_url
            && std::env::var("SHANNON_BASE_URL").is_err()
        {
            tracing::info!("No API key configured, defaulting to Ollama (localhost:11434)");
            let ollama_model = if has_explicit_model {
                model
            } else {
                std::env::var("SHANNON_MODEL").unwrap_or_else(|_| "llama3".to_string())
            };
            (String::new(), "http://localhost:11434".to_string(), ollama_model, LlmProvider::Ollama)
        } else {
            (api_key, base_url, model, provider)
        };

        // --- Resolve api_version --------------------------------------------
        let api_version = match provider {
            LlmProvider::Anthropic => std::env::var("ANTHROPIC_API_VERSION")
                .unwrap_or_else(|_| "2023-06-01".to_string()),
            _ => String::new(),
        };

        // --- Resolve max_tokens / timeout -----------------------------------
        let max_tokens = cfg.max_tokens.unwrap_or(4096) as u32;
        let timeout_seconds = cfg
            .timeout
            .unwrap_or(if provider == LlmProvider::Ollama {
                300
            } else {
                120
            });

        Self {
            api_key,
            base_url,
            model,
            max_tokens,
            timeout_seconds,
            api_version,
            provider,
            extra_headers: HashMap::new(),
            retry_config: RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 3,
        }
    }
}

impl LlmClientConfig {
    /// Validate that the configuration has the minimum required fields.
    ///
    /// Returns a human-readable error description if something is wrong.
    pub fn validate(&self) -> Result<(), String> {
        if self.base_url.trim().is_empty() {
            return Err("base_url is empty. Set SHANNON_BASE_URL or pass --provider ollama".to_string());
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
                format!("key={}..{}", &self.api_key[..2.min(self.api_key.len())], &self.api_key[self.api_key.len().saturating_sub(4)..])
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
        }
    }

    /// Create config for OpenAI
    pub fn openai_default() -> Self {
        let api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com".to_string());
        let model = std::env::var("OPENAI_MODEL")
            .unwrap_or_else(|_| "gpt-4o".to_string());
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
        }
    }

    /// Create config for Google Gemini
    pub fn gemini_default() -> Self {
        let api_key = std::env::var("GEMINI_API_KEY")
            .or_else(|_| std::env::var("GOOGLE_API_KEY"))
            .unwrap_or_default();
        let model = std::env::var("GEMINI_MODEL")
            .unwrap_or_else(|_| "gemini-2.0-flash".to_string());
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
        }
    }

    /// Create config for Azure OpenAI
    pub fn azure_default() -> Self {
        let api_key = std::env::var("AZURE_OPENAI_API_KEY").unwrap_or_default();
        let base_url = std::env::var("AZURE_OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://your-resource.openai.azure.com".to_string());
        let model = std::env::var("AZURE_OPENAI_MODEL")
            .unwrap_or_else(|_| "gpt-4o".to_string());
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
        }
    }

    /// Create config for Mistral AI
    pub fn mistral_default() -> Self {
        let api_key = std::env::var("MISTRAL_API_KEY").unwrap_or_default();
        let model = std::env::var("MISTRAL_MODEL")
            .unwrap_or_else(|_| "mistral-large-latest".to_string());
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
        }
    }

    /// Create config for DeepSeek
    pub fn deepseek_default() -> Self {
        let api_key = std::env::var("DEEPSEEK_API_KEY").unwrap_or_default();
        let model = std::env::var("DEEPSEEK_MODEL")
            .unwrap_or_else(|_| "deepseek-chat".to_string());
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
        }
    }

    /// Create config for Groq
    pub fn groq_default() -> Self {
        let api_key = std::env::var("GROQ_API_KEY").unwrap_or_default();
        let model = std::env::var("GROQ_MODEL")
            .unwrap_or_else(|_| "llama-3.3-70b-versatile".to_string());
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
        }
    }
}

/// Backward-compatible alias
pub type ClaudeClientConfig = LlmClientConfig;

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
    Image {
        source: ImageSource,
    },

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
}

/// Tool definition for function calling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

/// Usage information from API response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
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
    ContentBlockDelta {
        index: usize,
        delta: ContentDelta,
    },

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
}

/// Message delta event data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDeltaDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
}

#[cfg(test)]
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
                ContentBlock::Text { text: "Describe this".to_string() },
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
        assert_eq!(LlmProvider::from_base_url("https://api.anthropic.com"), LlmProvider::Anthropic);
    }

    #[test]
    fn test_provider_detection_openai() {
        assert_eq!(LlmProvider::from_base_url("https://api.openai.com"), LlmProvider::OpenAI);
    }

    #[test]
    fn test_provider_detection_ollama() {
        assert_eq!(LlmProvider::from_base_url("http://localhost:11434"), LlmProvider::Ollama);
        assert_eq!(LlmProvider::from_base_url("http://ollama.local"), LlmProvider::Ollama);
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
        assert_eq!(LlmProvider::from_base_url("https://api.mistral.ai"), LlmProvider::Mistral);
    }

    #[test]
    fn test_provider_detection_deepseek() {
        assert_eq!(LlmProvider::from_base_url("https://api.deepseek.com"), LlmProvider::DeepSeek);
    }

    #[test]
    fn test_provider_detection_groq() {
        assert_eq!(LlmProvider::from_base_url("https://api.groq.com"), LlmProvider::Groq);
    }

    #[test]
    fn test_provider_detection_together() {
        assert_eq!(LlmProvider::from_base_url("https://api.together.xyz"), LlmProvider::Together);
    }

    #[test]
    fn test_provider_detection_custom() {
        assert_eq!(LlmProvider::from_base_url("https://my-custom-llm.example.com"), LlmProvider::Custom);
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
}
