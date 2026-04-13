//! Message, request/response, and content types for the LLM API.

use crate::api::retry::RetryConfig;
use crate::unified_config::ShannonConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// ============================================================================
// Provider Types
// ============================================================================

/// Supported LLM providers
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
        } else {
            LlmProvider::Custom
        }
    }

    /// Get the API endpoint path for this provider
    pub fn endpoint(&self) -> &'static str {
        match self {
            LlmProvider::Anthropic => "/v1/messages",
            LlmProvider::OpenAI => "/v1/chat/completions",
            LlmProvider::Ollama => "/api/chat",
            LlmProvider::Custom => "/v1/messages",
        }
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

/// Image source for image blocks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub media_type: String,
    pub data: String,
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
