//! Agent messaging system for inter-agent communication

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Priority levels for agent messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MessagePriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

/// Types of messages that can be sent between agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    /// General communication
    Chat,
    /// Structured protocol message (e.g., shutdown request)
    Protocol,
    /// Task assignment
    TaskAssignment,
    /// Task status update
    TaskUpdate,
    /// Error report
    Error,
    /// Status notification
    Status,
}

/// A message sent between agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    /// Unique message ID
    pub id: Uuid,
    /// Sender agent name
    pub from: String,
    /// Recipient agent name (or "*" for broadcast)
    pub to: String,
    /// Message type
    pub message_type: MessageType,
    /// Message priority
    pub priority: MessagePriority,
    /// Message content (can be text or structured data)
    pub content: MessageContent,
    /// Timestamp when message was sent
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Content of an agent message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageContent {
    /// Plain text message
    Text(String),
    /// Structured JSON data
    Structured(serde_json::Value),
    /// Protocol-specific message
    Protocol(ProtocolMessage),
}

/// Protocol messages for agent coordination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProtocolMessage {
    /// Request agent to gracefully shut down
    ShutdownRequest {
        reason: String,
    },
    /// Response to shutdown request
    ShutdownResponse {
        request_id: Uuid,
        approve: bool,
        reason: Option<String>,
    },
    /// Request plan approval (for plan-mode-required agents)
    PlanApprovalRequest {
        request_id: Uuid,
        plan: String,
    },
    /// Response to plan approval request
    PlanApprovalResponse {
        request_id: Uuid,
        approve: bool,
        feedback: Option<String>,
    },
    /// Assign a task to an agent
    TaskAssign {
        task_id: Uuid,
        description: String,
        priority: Option<String>,
    },
    /// Agent reports task result
    TaskResult {
        task_id: Uuid,
        success: bool,
        output: String,
    },
    /// Request agent status
    StatusRequest,
    /// Agent reports its status
    StatusResponse {
        status: String,
        active_tasks: usize,
        metadata: serde_json::Value,
    },
}

impl AgentMessage {
    /// Create a new text message
    pub fn new_text(from: String, to: String, content: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            from,
            to,
            message_type: MessageType::Chat,
            priority: MessagePriority::Normal,
            content: MessageContent::Text(content),
            timestamp: chrono::Utc::now(),
        }
    }

    /// Create a broadcast message to all teammates
    pub fn broadcast(from: String, content: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            from,
            to: "*".to_string(),
            message_type: MessageType::Chat,
            priority: MessagePriority::Normal,
            content: MessageContent::Text(content),
            timestamp: chrono::Utc::now(),
        }
    }

    /// Create a protocol message
    pub fn protocol(from: String, to: String, content: ProtocolMessage) -> Self {
        Self {
            id: Uuid::new_v4(),
            from,
            to,
            message_type: MessageType::Protocol,
            priority: MessagePriority::High,
            content: MessageContent::Protocol(content),
            timestamp: chrono::Utc::now(),
        }
    }
}
