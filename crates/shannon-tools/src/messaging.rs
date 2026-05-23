//! Agent messaging tools
//!
//! Provides implementations for:
//! - SendMessage: Send messages to agent teammates
//!
//! Supports both plain text messages and structured protocol messages
//! for team coordination.

use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Message types for structured protocol communication
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StructuredMessage {
    #[serde(rename = "shutdown_request")]
    ShutdownRequest { reason: Option<String> },
    #[serde(rename = "shutdown_response")]
    ShutdownResponse {
        request_id: String,
        approve: bool,
        reason: Option<String>,
    },
    #[serde(rename = "plan_approval_response")]
    PlanApprovalResponse {
        request_id: String,
        approve: bool,
        feedback: Option<String>,
    },
}

#[cfg(test)]
mod structured_message_tests {
    use super::*;

    #[test]
    fn test_shutdown_request_serialization() {
        let msg = StructuredMessage::ShutdownRequest {
            reason: Some("done".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("shutdown_request"));
        assert!(json.contains("done"));
        let parsed: StructuredMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, StructuredMessage::ShutdownRequest { .. }));
    }

    #[test]
    fn test_shutdown_request_no_reason() {
        let msg = StructuredMessage::ShutdownRequest { reason: None };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("shutdown_request"));
        let parsed: StructuredMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            StructuredMessage::ShutdownRequest { reason: None }
        ));
    }

    #[test]
    fn test_shutdown_response_serialization() {
        let msg = StructuredMessage::ShutdownResponse {
            request_id: "req-123".into(),
            approve: true,
            reason: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("shutdown_response"));
        assert!(json.contains("req-123"));
        assert!(json.contains("\"approve\":true"));
        let parsed: StructuredMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            StructuredMessage::ShutdownResponse { approve: true, .. }
        ));
    }

    #[test]
    fn test_plan_approval_response_serialization() {
        let msg = StructuredMessage::PlanApprovalResponse {
            request_id: "plan-456".into(),
            approve: false,
            feedback: Some("needs work".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("plan_approval_response"));
        assert!(json.contains("needs work"));
        let parsed: StructuredMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            parsed,
            StructuredMessage::PlanApprovalResponse { approve: false, .. }
        ));
    }

    #[test]
    fn test_message_content_text() {
        let content = MessageContent::Text("hello teammate".into());
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("hello teammate"));
    }

    #[test]
    fn test_send_message_input_text_roundtrip() {
        let input = SendMessageInput {
            to: "researcher".into(),
            summary: Some("task update".into()),
            message: MessageContent::Text("done with task 1".into()),
        };
        let json = serde_json::to_string(&input).unwrap();
        let parsed: SendMessageInput = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.to, "researcher");
    }

    #[test]
    fn test_send_message_input_structured_roundtrip() {
        let input = SendMessageInput {
            to: "team-lead".into(),
            summary: Some("plan response".into()),
            message: MessageContent::Structured(StructuredMessage::PlanApprovalResponse {
                request_id: "p1".into(),
                approve: true,
                feedback: None,
            }),
        };
        let json = serde_json::to_string(&input).unwrap();
        let parsed: SendMessageInput = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.to, "team-lead");
    }

    #[test]
    fn test_structured_message_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<StructuredMessage>();
        assert_send_sync::<MessageContent>();
        assert_send_sync::<SendMessageInput>();
    }
}

/// Message content
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Structured(StructuredMessage),
}

/// Input for sending a message
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SendMessageInput {
    /// Recipient: teammate name, or "*" for broadcast to all teammates
    pub to: String,

    /// Optional summary shown as a preview in the UI (required when message is a string)
    pub summary: Option<String>,

    /// Message content - either plain text or structured protocol message
    pub message: MessageContent,
}

/// Output from sending a message
#[derive(Debug, Serialize)]
pub struct SendMessageOutput {
    /// Whether the message was sent successfully
    pub success: bool,

    /// Status message
    pub message: String,

    /// Optional request ID (for structured protocol messages)
    pub request_id: Option<String>,

    /// Target recipient
    pub target: Option<String>,
}

/// Inbox message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxMessage {
    /// Sender name
    pub from: String,

    /// Message content
    pub content: MessageContent,

    /// Optional summary
    pub summary: Option<String>,

    /// Timestamp
    pub timestamp: String,

    /// Optional color for UI
    pub color: Option<String>,
}

/// Team member information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    /// Member name
    pub name: String,

    /// Agent ID
    pub agent_id: Option<String>,

    /// Color for UI
    pub color: Option<String>,
}

/// Team context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamContext {
    /// Team name
    pub team_name: Option<String>,

    /// Team members
    pub members: Vec<TeamMember>,
}

/// Global team context (in-memory mock)
fn get_team_context() -> TeamContext {
    TeamContext {
        team_name: Some("default-team".to_string()),
        members: vec![
            TeamMember {
                name: "team-lead".to_string(),
                agent_id: Some(Uuid::new_v4().to_string()),
                color: Some("#3498db".to_string()),
            },
            TeamMember {
                name: "backend-architect".to_string(),
                agent_id: Some(Uuid::new_v4().to_string()),
                color: Some("#e74c3c".to_string()),
            },
            TeamMember {
                name: "frontend-developer".to_string(),
                agent_id: Some(Uuid::new_v4().to_string()),
                color: Some("#2ecc71".to_string()),
            },
        ],
    }
}

/// Global inbox (in-memory message storage)
type InboxRegistry = Arc<RwLock<HashMap<String, Vec<InboxMessage>>>>;

fn get_inbox() -> InboxRegistry {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Send message tool
pub struct SendMessageTool {
    description: String,
    inbox: InboxRegistry,
}

impl Default for SendMessageTool {
    fn default() -> Self {
        Self::new()
    }
}

impl SendMessageTool {
    pub fn new() -> Self {
        Self {
            description: "Send messages to agent teammates for collaborative problem-solving"
                .to_string(),
            inbox: get_inbox(),
        }
    }

    /// Send a message to a specific recipient
    async fn send_message(
        &self,
        recipient: &str,
        content: &MessageContent,
        summary: Option<&str>,
        _team_context: &TeamContext,
    ) -> Result<SendMessageOutput, ToolError> {
        let sender = "agent"; // In real implementation, would get actual agent name

        let inbox_message = InboxMessage {
            from: sender.to_string(),
            content: content.clone(),
            summary: summary.map(|s| s.to_string()),
            timestamp: chrono::Utc::now().to_rfc3339(),
            color: None,
        };

        // Store in inbox
        {
            let mut inbox = self.inbox.write().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire inbox lock: {e}"))
            })?;
            inbox
                .entry(recipient.to_string())
                .or_insert_with(Vec::new)
                .push(inbox_message);
        }

        Ok(SendMessageOutput {
            success: true,
            message: format!("Message sent to @{recipient}"),
            request_id: None,
            target: Some(recipient.to_string()),
        })
    }

    /// Broadcast a message to all team members
    async fn broadcast_message(
        &self,
        content: &MessageContent,
        summary: Option<&str>,
        team_context: &TeamContext,
    ) -> Result<SendMessageOutput, ToolError> {
        if team_context.team_name.is_none() {
            return Ok(SendMessageOutput {
                success: true,
                message: "No teammates to broadcast to (not in a team context)".to_string(),
                request_id: None,
                target: Some("@team".to_string()),
            });
        }

        let recipients: Vec<&TeamMember> = team_context
            .members
            .iter()
            .filter(|m| m.name != "agent") // Don't send to self
            .collect();

        if recipients.is_empty() {
            return Ok(SendMessageOutput {
                success: true,
                message: "No teammates to broadcast to (you are the only team member)".to_string(),
                request_id: None,
                target: Some("@team".to_string()),
            });
        }

        for recipient in &recipients {
            self.send_message(&recipient.name, content, summary, team_context)
                .await?;
        }

        let recipient_names: Vec<&str> = recipients.iter().map(|m| m.name.as_str()).collect();

        Ok(SendMessageOutput {
            success: true,
            message: format!(
                "Message broadcast to {} teammate(s): {}",
                recipient_names.len(),
                recipient_names.join(", ")
            ),
            request_id: None,
            target: Some("@team".to_string()),
        })
    }

    /// Handle shutdown request protocol message
    async fn handle_shutdown_request(
        &self,
        target: &str,
        reason: Option<&str>,
        _team_context: &TeamContext,
    ) -> Result<SendMessageOutput, ToolError> {
        let request_id = Uuid::new_v4().to_string();

        let message = InboxMessage {
            from: "agent".to_string(),
            content: MessageContent::Structured(StructuredMessage::ShutdownRequest {
                reason: reason.map(|r| r.to_string()),
            }),
            summary: Some("Shutdown request".to_string()),
            timestamp: chrono::Utc::now().to_rfc3339(),
            color: None,
        };

        {
            let mut inbox = self.inbox.write().map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to acquire inbox lock: {e}"))
            })?;
            inbox
                .entry(target.to_string())
                .or_insert_with(Vec::new)
                .push(message);
        }

        Ok(SendMessageOutput {
            success: true,
            message: format!("Shutdown request sent to {target}. Request ID: {request_id}"),
            request_id: Some(request_id),
            target: Some(target.to_string()),
        })
    }

    /// Execute send message operation
    async fn execute_send(&self, input: SendMessageInput) -> Result<SendMessageOutput, ToolError> {
        let team_context = get_team_context();

        match &input.message {
            MessageContent::Text(_content) => {
                if input.to == "*" {
                    self.broadcast_message(&input.message, input.summary.as_deref(), &team_context)
                        .await
                } else {
                    // Validate summary is provided for text messages
                    if input
                        .summary
                        .as_ref()
                        .map(|s| s.trim())
                        .unwrap_or("")
                        .is_empty()
                    {
                        return Ok(SendMessageOutput {
                            success: false,
                            message: "summary is required when message is a string".to_string(),
                            request_id: None,
                            target: Some(input.to.clone()),
                        });
                    }

                    self.send_message(
                        &input.to,
                        &input.message,
                        input.summary.as_deref(),
                        &team_context,
                    )
                    .await
                }
            }
            MessageContent::Structured(structured) => match structured {
                StructuredMessage::ShutdownRequest { reason } => {
                    if input.to == "*" {
                        return Ok(SendMessageOutput {
                            success: false,
                            message: "structured messages cannot be broadcast (to: \"*\")"
                                .to_string(),
                            request_id: None,
                            target: Some("*".to_string()),
                        });
                    }
                    self.handle_shutdown_request(&input.to, reason.as_deref(), &team_context)
                        .await
                }
                StructuredMessage::ShutdownResponse {
                    request_id,
                    approve,
                    reason: _,
                } => {
                    let message = InboxMessage {
                        from: "agent".to_string(),
                        content: input.message.clone(),
                        summary: Some(if *approve {
                            "Shutdown approved".to_string()
                        } else {
                            "Shutdown rejected".to_string()
                        }),
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        color: None,
                    };

                    {
                        let mut inbox = self.inbox.write().map_err(|e| {
                            ToolError::ExecutionFailed(format!("Failed to acquire inbox lock: {e}"))
                        })?;
                        inbox
                            .entry(input.to.clone())
                            .or_insert_with(Vec::new)
                            .push(message);
                    }

                    Ok(SendMessageOutput {
                        success: true,
                        message: format!(
                            "Shutdown {} for request {}",
                            if *approve { "approved" } else { "rejected" },
                            request_id
                        ),
                        request_id: Some(request_id.clone()),
                        target: Some(input.to.clone()),
                    })
                }
                StructuredMessage::PlanApprovalResponse {
                    request_id,
                    approve,
                    feedback,
                } => {
                    let message = InboxMessage {
                        from: "agent".to_string(),
                        content: input.message.clone(),
                        summary: Some(if *approve {
                            "Plan approved".to_string()
                        } else {
                            "Plan rejected".to_string()
                        }),
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        color: None,
                    };

                    {
                        let mut inbox = self.inbox.write().map_err(|e| {
                            ToolError::ExecutionFailed(format!("Failed to acquire inbox lock: {e}"))
                        })?;
                        inbox
                            .entry(input.to.clone())
                            .or_insert_with(Vec::new)
                            .push(message);
                    }

                    Ok(SendMessageOutput {
                        success: true,
                        message: format!(
                            "Plan {} for request {}{}",
                            if *approve { "approved" } else { "rejected" },
                            request_id,
                            feedback
                                .as_ref()
                                .map(|f| format!(": {f}"))
                                .unwrap_or_default()
                        ),
                        request_id: Some(request_id.clone()),
                        target: Some(input.to.clone()),
                    })
                }
            },
        }
    }
}

#[async_trait]
impl Tool for SendMessageTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let send_input: SendMessageInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid send message input: {e}")))?;
        let output = self.execute_send(send_input).await?;
        Ok(ToolOutput {
            content: output.message,
            is_error: !output.success,
            metadata: {
                let mut map = HashMap::new();
                map.insert("success".to_string(), json!(output.success));
                map.insert("target".to_string(), json!(output.target));
                if let Some(request_id) = output.request_id {
                    map.insert("request_id".to_string(), json!(request_id));
                }
                map
            },
        })
    }

    fn name(&self) -> &str {
        "SendMessage"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "to": {
                    "type": "string",
                    "description": "Recipient: teammate name, or '*' for broadcast to all teammates"
                },
                "summary": {
                    "type": "string",
                    "description": "Optional summary shown as a preview in the UI"
                },
                "message": {
                    "type": "string",
                    "description": "Message content"
                }
            },
            "required": ["to", "message"]
        })
    }
}
