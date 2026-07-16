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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    ShutdownRequest { reason: String },
    /// Response to shutdown request
    ShutdownResponse {
        request_id: Uuid,
        approve: bool,
        reason: Option<String>,
    },
    /// Request plan approval (for plan-mode-required agents)
    PlanApprovalRequest { request_id: Uuid, plan: String },
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn message_new_text() {
        let msg = AgentMessage::new_text("alice".into(), "bob".into(), "hello".into());
        assert_eq!(msg.from, "alice");
        assert_eq!(msg.to, "bob");
        assert_eq!(msg.message_type, MessageType::Chat);
        assert_eq!(msg.priority, MessagePriority::Normal);
        assert!(matches!(msg.content, MessageContent::Text(ref t) if t == "hello"));
    }

    #[test]
    fn message_broadcast() {
        let msg = AgentMessage::broadcast("lead".into(), "status update".into());
        assert_eq!(msg.from, "lead");
        assert_eq!(msg.to, "*");
        assert_eq!(msg.priority, MessagePriority::Normal);
    }

    #[test]
    fn message_protocol_high_priority() {
        let msg = AgentMessage::protocol(
            "lead".into(),
            "worker".into(),
            ProtocolMessage::ShutdownRequest {
                reason: "done".into(),
            },
        );
        assert_eq!(msg.priority, MessagePriority::High);
        assert_eq!(msg.message_type, MessageType::Protocol);
    }

    #[test]
    fn message_priority_ordering() {
        assert!(MessagePriority::Critical > MessagePriority::High);
        assert!(MessagePriority::High > MessagePriority::Normal);
        assert!(MessagePriority::Normal > MessagePriority::Low);
    }

    #[test]
    fn message_type_serde() {
        let types = vec![
            MessageType::Chat,
            MessageType::Protocol,
            MessageType::TaskAssignment,
            MessageType::Error,
        ];
        let json = serde_json::to_string(&types).unwrap();
        let de: Vec<MessageType> = serde_json::from_str(&json).unwrap();
        assert_eq!(de, types);
    }

    #[test]
    fn message_priority_serde() {
        let json = serde_json::to_string(&MessagePriority::Critical).unwrap();
        assert!(json.contains("Critical"));
        let de: MessagePriority = serde_json::from_str("\"Low\"").unwrap();
        assert_eq!(de, MessagePriority::Low);
    }

    #[test]
    fn message_content_text_roundtrip() {
        let content = MessageContent::Text("hello".into());
        let json = serde_json::to_string(&content).unwrap();
        let de: MessageContent = serde_json::from_str(&json).unwrap();
        assert!(matches!(de, MessageContent::Text(ref t) if t == "hello"));
    }

    #[test]
    fn message_content_structured_roundtrip() {
        let content = MessageContent::Structured(serde_json::json!({"key": "value"}));
        let json = serde_json::to_string(&content).unwrap();
        let de: MessageContent = serde_json::from_str(&json).unwrap();
        assert!(matches!(de, MessageContent::Structured(_)));
    }

    #[test]
    fn protocol_message_shutdown_roundtrip() {
        let msg = ProtocolMessage::ShutdownResponse {
            request_id: Uuid::new_v4(),
            approve: true,
            reason: Some("ok".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let de: ProtocolMessage = serde_json::from_str(&json).unwrap();
        if let ProtocolMessage::ShutdownResponse { approve, .. } = de {
            assert!(approve);
        } else {
            panic!("Expected ShutdownResponse");
        }
    }

    #[test]
    fn protocol_message_plan_approval_roundtrip() {
        let msg = ProtocolMessage::PlanApprovalRequest {
            request_id: Uuid::new_v4(),
            plan: "Do X then Y".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let de: ProtocolMessage = serde_json::from_str(&json).unwrap();
        if let ProtocolMessage::PlanApprovalRequest { plan, .. } = de {
            assert_eq!(plan, "Do X then Y");
        } else {
            panic!("Expected PlanApprovalRequest");
        }
    }

    #[test]
    fn protocol_message_task_assign_roundtrip() {
        let msg = ProtocolMessage::TaskAssign {
            task_id: Uuid::new_v4(),
            description: "Fix bug".into(),
            priority: Some("High".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let de: ProtocolMessage = serde_json::from_str(&json).unwrap();
        if let ProtocolMessage::TaskAssign { description, .. } = de {
            assert_eq!(description, "Fix bug");
        } else {
            panic!("Expected TaskAssign");
        }
    }

    #[test]
    fn protocol_message_status_response_roundtrip() {
        let msg = ProtocolMessage::StatusResponse {
            status: "idle".into(),
            active_tasks: 2,
            metadata: serde_json::json!({}),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let de: ProtocolMessage = serde_json::from_str(&json).unwrap();
        if let ProtocolMessage::StatusResponse {
            status,
            active_tasks,
            ..
        } = de
        {
            assert_eq!(status, "idle");
            assert_eq!(active_tasks, 2);
        } else {
            panic!("Expected StatusResponse");
        }
    }

    #[test]
    fn agent_message_roundtrip() {
        let msg = AgentMessage::new_text("a".into(), "b".into(), "hi".into());
        let json = serde_json::to_string(&msg).unwrap();
        let de: AgentMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(de.from, "a");
        assert_eq!(de.to, "b");
    }

    #[test]
    fn send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AgentMessage>();
        assert_send_sync::<MessagePriority>();
        assert_send_sync::<MessageType>();
        assert_send_sync::<MessageContent>();
        assert_send_sync::<ProtocolMessage>();
    }
}
