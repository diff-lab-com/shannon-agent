//! Agent execution abstraction.
//!
//! Defines the `AgentExecutor` trait for running agent tasks against an LLM,
//! and provides `LlmAgentExecutor` as the default implementation using
//! `shannon_core::LlmClient`.

use async_trait::async_trait;
use shannon_core::tools::ToolOutput;
use shannon_engine::api::{ContentBlock, LlmClient, Message, MessageContent};
use std::collections::HashMap;
use std::sync::Arc;

/// A single turn in a conversation, used for multi-turn agent execution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatTurn {
    /// Speaker role: "user" or "assistant"
    pub role: String,
    /// Message content
    pub content: String,
}

/// Trait for executing agent tasks. Implementations can use different backends
/// (LLM API, subprocess, mock for testing, etc.).
#[async_trait]
pub trait AgentExecutor: Send + Sync {
    /// Execute a single-turn task and return the output.
    async fn execute(
        &self,
        system_prompt: &str,
        task: &str,
        model: Option<&str>,
        tools: Option<&[String]>,
    ) -> Result<ToolOutput, String>;

    /// Execute a multi-turn conversation with history.
    async fn execute_with_history(
        &self,
        system_prompt: &str,
        history: &[ChatTurn],
        task: &str,
        model: Option<&str>,
        tools: Option<&[String]>,
    ) -> Result<ToolOutput, String>;
}

/// Real LLM-backed agent executor using the shared `LlmClient`.
///
/// Constructs a single-turn conversation (system prompt + user task) and
/// returns the assistant's text response.
pub struct LlmAgentExecutor {
    client: LlmClient,
}

impl LlmAgentExecutor {
    pub fn new(client: LlmClient) -> Self {
        Self { client }
    }

    /// Return a client instance with an optional model override applied.
    ///
    /// When `model` is `Some`, clones the client and switches its model.
    /// When `None`, returns a reference-counted clone using the default model.
    fn client_for_model(&self, model: Option<&str>) -> LlmClient {
        match model {
            Some(m) => {
                let mut c = self.client.clone();
                c.set_model(m.to_string());
                c
            }
            None => self.client.clone(),
        }
    }
}

#[async_trait]
impl AgentExecutor for LlmAgentExecutor {
    async fn execute(
        &self,
        system_prompt: &str,
        task: &str,
        model: Option<&str>,
        _tools: Option<&[String]>,
    ) -> Result<ToolOutput, String> {
        let messages = vec![Message {
            role: "user".to_string(),
            content: MessageContent::Text(task.to_string()),
        }];

        let client = self.client_for_model(model);
        let result = client
            .send_message(messages, None, Some(system_prompt.to_string()))
            .await
            .map_err(|e| format!("LLM error: {e}"))?;

        extract_text_output(result)
    }

    async fn execute_with_history(
        &self,
        system_prompt: &str,
        history: &[ChatTurn],
        task: &str,
        model: Option<&str>,
        _tools: Option<&[String]>,
    ) -> Result<ToolOutput, String> {
        // Build full message list from conversation history
        let mut messages: Vec<Message> = history
            .iter()
            .map(|turn| Message {
                role: turn.role.clone(),
                content: MessageContent::Text(turn.content.clone()),
            })
            .collect();

        // Append current task as the latest user message
        messages.push(Message {
            role: "user".to_string(),
            content: MessageContent::Text(task.to_string()),
        });

        let client = self.client_for_model(model);
        let result = client
            .send_message(messages, None, Some(system_prompt.to_string()))
            .await
            .map_err(|e| format!("LLM error: {e}"))?;

        extract_text_output(result)
    }
}

/// Extract text content from LLM response blocks.
fn extract_text_output(blocks: Vec<ContentBlock>) -> Result<ToolOutput, String> {
    let text: String = blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<&str>>()
        .join("\n");

    if text.is_empty() {
        Ok(ToolOutput {
            content: "[Agent returned no text]".to_string(),
            is_error: false,
            metadata: HashMap::new(),
        })
    } else {
        Ok(ToolOutput {
            content: text,
            is_error: false,
            metadata: HashMap::new(),
        })
    }
}

/// A no-op executor that returns canned responses (for testing).
pub struct MockAgentExecutor {
    pub response: String,
}

impl MockAgentExecutor {
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }
}

#[async_trait]
impl AgentExecutor for MockAgentExecutor {
    async fn execute(
        &self,
        _system_prompt: &str,
        _task: &str,
        _model: Option<&str>,
        _tools: Option<&[String]>,
    ) -> Result<ToolOutput, String> {
        Ok(ToolOutput {
            content: self.response.clone(),
            is_error: false,
            metadata: HashMap::new(),
        })
    }

    async fn execute_with_history(
        &self,
        _system_prompt: &str,
        _history: &[ChatTurn],
        _task: &str,
        _model: Option<&str>,
        _tools: Option<&[String]>,
    ) -> Result<ToolOutput, String> {
        Ok(ToolOutput {
            content: self.response.clone(),
            is_error: false,
            metadata: HashMap::new(),
        })
    }
}

/// Helper to create a shared executor reference.
pub fn shared_executor(client: LlmClient) -> Arc<dyn AgentExecutor> {
    Arc::new(LlmAgentExecutor::new(client))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_executor() {
        let executor = MockAgentExecutor::new("test response");
        let result = executor
            .execute("system", "task", None, None)
            .await
            .unwrap();
        assert_eq!(result.content, "test response");
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_shared_executor_creates_arc() {
        // Just verify the type compiles and creates an Arc
        let client = LlmClient::from_env();
        {
            let _executor: Arc<dyn AgentExecutor> = shared_executor(client);
        }
        // If no API key, just pass — the compilation check is the real test
    }
}
