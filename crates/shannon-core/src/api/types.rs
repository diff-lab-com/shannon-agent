//! Message, request/response, and content types for the LLM API.

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
            timeout_seconds: 120,
            api_version,
            provider,
            extra_headers: HashMap::new(),
        }
    }
}

impl LlmClientConfig {
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
