//! Discovery functions — start MCP servers and discover their tools via the pool.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tracing::debug;

use super::adapter::PooledMcpToolAdapter;
use super::{ElicitationProvider, McpProcessPool, SamplingProvider};

// ---------------------------------------------------------------------------
// Discovery via pool
// ---------------------------------------------------------------------------

/// Result of discovering tools via the persistent pool.
pub struct PooledDiscoveryResult {
    /// Server name.
    pub server_name: String,
    /// Tool adapters ready to register.
    pub tools: Vec<PooledMcpToolAdapter>,
}

// ---------------------------------------------------------------------------
// Sampling provider bridge
// ---------------------------------------------------------------------------

/// Create a sampling provider that delegates to an [`shannon_core::api::client::LlmClient`].
///
/// This wires MCP `sampling/createMessage` requests through to Shannon's LLM
/// backend. The provider:
/// - Converts `SamplingMessageRole` → LLM message roles (`"user"` / `"assistant"`)
/// - Converts `SamplingContent` → LLM content types
/// - Passes `system_prompt` through as the system message
/// - Logs each request for observability
///
/// Returns a `SamplingProvider` suitable for [`McpProcessPool::set_sampling_provider`].
pub fn make_sampling_provider(
    client: std::sync::Arc<shannon_core::api::client::LlmClient>,
) -> SamplingProvider {
    use shannon_core::api::types::{ContentBlock, Message, MessageContent};
    use crate::{CreateMessageRequest, CreateMessageResult, SamplingContent, SamplingMessageRole};

    Arc::new(move |req: CreateMessageRequest| {
        let client = client.clone();
        Box::pin(async move {
            tracing::info!(
                messages = req.messages.len(),
                model_hint = ?req.model_preferences.as_ref().and_then(|p| p.hints.as_ref().and_then(|h| h.first().and_then(|h| h.name.as_deref()))),
                "MCP sampling request"
            );

            // Convert sampling messages → LLM messages.
            let messages: Vec<Message> = req.messages.into_iter().map(|msg| {
                let role = match msg.role {
                    SamplingMessageRole::User => "user".to_string(),
                    SamplingMessageRole::Assistant => "assistant".to_string(),
                };
                let content = match msg.content {
                    SamplingContent::Text { text } => MessageContent::Text(text),
                    SamplingContent::Image { data, mime_type } => {
                        MessageContent::Blocks(vec![ContentBlock::Image {
                            source: shannon_core::api::types::ImageSource::base64(mime_type, data),
                        }])
                    }
                };
                Message { role, content }
            }).collect();

            let response = client
                .send_message(messages, None, req.system_prompt)
                .await
                .map_err(|e| format!("Sampling LLM call failed: {e}"))?;

            // Extract text from response content blocks.
            let mut text = String::new();
            for block in &response {
                if let ContentBlock::Text { text: t } = block {
                    text.push_str(t);
                }
            }

            Ok(CreateMessageResult {
                role: SamplingMessageRole::Assistant,
                model: "shannon-code".to_string(),
                content: SamplingContent::Text { text },
                stop_reason: Some(crate::StopReason::EndTurn),
            })
        })
    })
}

/// User prompt callback type for elicitation.
///
/// Receives the server's message and optional JSON Schema,
/// returns `(ElicitationAction, Option<Value>)` where the value
/// is the user's structured input on accept.
pub type UserPromptCallback = Arc<
    dyn Fn(String, Option<serde_json::Value>) -> Pin<Box<dyn Future<Output = (crate::ElicitationAction, Option<serde_json::Value>)> + Send>>
        + Send
        + Sync,
>;

/// Create an elicitation provider that delegates to a user prompt callback.
///
/// When an MCP server sends `elicitation/create`, the callback is invoked
/// with the server's message and optional schema. The callback should
/// present the prompt to the user (e.g., via the TUI) and return the result.
///
/// If no callback is provided, all elicitation requests are auto-declined.
pub fn make_elicitation_provider(
    prompt_callback: Option<UserPromptCallback>,
) -> ElicitationProvider {
    use crate::{ElicitationRequest, ElicitationResult, ElicitationAction};

    Arc::new(move |req: ElicitationRequest| {
        let callback = prompt_callback.clone();
        Box::pin(async move {
            tracing::info!(
                message = %req.message,
                has_schema = req.requested_schema.is_some(),
                "MCP elicitation request"
            );

            match callback {
                Some(cb) => {
                    let (action, content) = cb(req.message, req.requested_schema).await;
                    Ok(ElicitationResult { action, content })
                }
                None => {
                    tracing::warn!("Elicitation request auto-declined (no callback configured)");
                    Ok(ElicitationResult {
                        action: ElicitationAction::Decline,
                        content: None,
                    })
                }
            }
        })
    })
}

/// Discover tools from an MCP server using the persistent pool.
///
/// Starts the server in the pool, sends `initialize` + `tools/list`,
/// and returns pooled adapters for each discovered tool.
pub async fn discover_pooled_tools(
    pool: Arc<McpProcessPool>,
    server_name: &str,
    command: &str,
    args: &[String],
    env: &HashMap<String, String>,
) -> Result<PooledDiscoveryResult, String> {
    // Start the server in the pool (handles initialize handshake)
    pool.start_server(server_name, command, args, env).await?;

    // Check capabilities before attempting tools/list.
    if !pool.has_tools(server_name).await {
        debug!(
            server = %server_name,
            "Server does not advertise tools capability; skipping tools/list"
        );
        return Ok(PooledDiscoveryResult {
            server_name: server_name.to_string(),
            tools: Vec::new(),
        });
    }

    // Now send tools/list via the pool's persistent connection
    let handle = pool
        .handles
        .get(server_name)
        .ok_or_else(|| format!("Server '{server_name}' not found after start"))?;

    let response = handle
        .send_request("tools/list", serde_json::json!({}))
        .await?;

    let mut tools = Vec::new();

    if let Some(tools_array) = response
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
    {
        for tool_value in tools_array {
            let name = tool_value
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string();
            let description = tool_value
                .get("description")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("MCP tool: {name}"));
            let input_schema = tool_value
                .get("inputSchema")
                .cloned()
                .unwrap_or(serde_json::json!({"type": "object"}));

            // Parse tool annotations (behavioral hints) if present.
            let annotations: Option<crate::ToolAnnotations> = tool_value
                .get("annotations")
                .and_then(|a| serde_json::from_value(a.clone()).ok());

            // Parse per-tool output limit from _meta.maxResultSizeChars.
            let max_output_chars: Option<usize> = tool_value
                .get("_meta")
                .and_then(|m| m.get("maxResultSizeChars"))
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);

            // Parse per-tool timeout from _meta.timeoutSeconds.
            let tool_timeout_secs: Option<u64> = tool_value
                .get("_meta")
                .and_then(|m| m.get("timeoutSeconds"))
                .and_then(|v| v.as_u64());

            // Store the real schema for deferred retrieval if enabled.
            let adapter = PooledMcpToolAdapter::with_output_limit(
                pool.clone(),
                server_name.to_string(),
                name.clone(),
                description,
                input_schema.clone(),
                annotations,
                max_output_chars,
                tool_timeout_secs,
            );

            // When deferred mode is on, store the real schema and let the adapter
            // return a minimal stub via input_schema().
            if pool.is_defer_tool_schemas() {
                pool.store_deferred_schema(&adapter.tool_name, input_schema);
            }

            tools.push(adapter);
        }
    }

    drop(handle);

    Ok(PooledDiscoveryResult {
        server_name: server_name.to_string(),
        tools,
    })
}

/// Discover tools from a remote MCP server using the pool.
///
/// Starts the remote server via `start_remote_server`, then sends `tools/list`
/// over the persistent connection and returns `PooledMcpToolAdapter` instances.
pub async fn discover_pooled_remote_tools(
    pool: Arc<McpProcessPool>,
    server_name: &str,
    url: &str,
    headers: HashMap<String, crate::config::HeaderSource>,
    auth: Option<crate::config::McpAuthConfig>,
) -> Result<PooledDiscoveryResult, String> {
    // Start the remote server in the pool (handles initialize handshake)
    pool.start_remote_server(server_name, url, headers, auth).await?;

    // Check capabilities before attempting tools/list.
    if !pool.has_tools(server_name).await {
        debug!(
            server = %server_name,
            "Remote server does not advertise tools capability; skipping tools/list"
        );
        return Ok(PooledDiscoveryResult {
            server_name: server_name.to_string(),
            tools: Vec::new(),
        });
    }

    // Send tools/list via the pool's persistent connection
    let response = pool
        .send_server_request(server_name, "tools/list", serde_json::json!({}))
        .await
        .map_err(|e| format!("tools/list failed for remote server '{server_name}': {e}"))?;

    let mut tools = Vec::new();

    if let Some(tools_array) = response
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(|t| t.as_array())
    {
        for tool_value in tools_array {
            let name = tool_value
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string();
            let description = tool_value
                .get("description")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("MCP tool: {name}"));
            let input_schema = tool_value
                .get("inputSchema")
                .cloned()
                .unwrap_or(serde_json::json!({"type": "object"}));

            let annotations: Option<crate::ToolAnnotations> = tool_value
                .get("annotations")
                .and_then(|a| serde_json::from_value(a.clone()).ok());

            let max_output_chars: Option<usize> = tool_value
                .get("_meta")
                .and_then(|m| m.get("maxResultSizeChars"))
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);

            // Parse per-tool timeout from _meta.timeoutSeconds.
            let tool_timeout_secs: Option<u64> = tool_value
                .get("_meta")
                .and_then(|m| m.get("timeoutSeconds"))
                .and_then(|v| v.as_u64());

            let adapter = PooledMcpToolAdapter::with_output_limit(
                pool.clone(),
                server_name.to_string(),
                name.clone(),
                description,
                input_schema.clone(),
                annotations,
                max_output_chars,
                tool_timeout_secs,
            );

            if pool.is_defer_tool_schemas() {
                pool.store_deferred_schema(&adapter.tool_name, input_schema);
            }

            tools.push(adapter);
        }
    }

    Ok(PooledDiscoveryResult {
        server_name: server_name.to_string(),
        tools,
    })
}
