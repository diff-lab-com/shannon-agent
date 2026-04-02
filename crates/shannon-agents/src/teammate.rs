//! Individual agent teammate implementation

use crate::error::AgentError;
use crate::message::{AgentMessage, MessageContent, MessageType, ProtocolMessage};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock, Mutex};
use uuid::Uuid;

/// Configuration for a teammate agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateConfig {
    /// Agent type/role
    pub agent_type: String,
    /// Special capabilities of this agent
    pub capabilities: Vec<String>,
    /// Maximum concurrent tasks
    pub max_concurrent_tasks: usize,
    /// Enable plan mode for this agent
    pub plan_mode_required: bool,
    /// Agent model/version
    pub model: Option<String>,
    /// System prompt for the agent
    pub system_prompt: Option<String>,
    /// Temperature for AI responses (0.0 - 1.0)
    pub temperature: Option<f32>,
}

impl Default for TeammateConfig {
    fn default() -> Self {
        Self {
            agent_type: "general-purpose".to_string(),
            capabilities: Vec::new(),
            max_concurrent_tasks: 3,
            plan_mode_required: false,
            model: None,
            system_prompt: None,
            temperature: None,
        }
    }
}

/// Current status of a teammate
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TeammateStatus {
    /// Agent is idle and available for work
    Idle,
    /// Agent is working on a task
    Busy,
    /// Agent is in plan mode
    Planning,
    /// Agent is shutting down
    ShuttingDown,
    /// Agent has stopped
    Stopped,
    /// Agent encountered an error
    Error,
}

/// State information for a teammate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateState {
    /// Current status
    pub status: TeammateStatus,
    /// Number of active tasks
    pub active_tasks: usize,
    /// Current worktree (if any)
    pub current_worktree: Option<String>,
    /// Last activity timestamp
    pub last_activity: chrono::DateTime<chrono::Utc>,
}

/// Message inbox for a teammate
#[derive(Clone)]
struct MessageInbox {
    sender: mpsc::Sender<AgentMessage>,
    receiver: Arc<Mutex<mpsc::Receiver<AgentMessage>>>,
}

impl MessageInbox {
    fn new(buffer_size: usize) -> Self {
        let (sender, receiver) = mpsc::channel(buffer_size);
        Self {
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }

    async fn send(&self, message: AgentMessage) -> Result<(), AgentError> {
        self.sender.send(message).await
            .map_err(|_| AgentError::Communication("Inbox closed".to_string()))
    }

    async fn recv(&self) -> Option<AgentMessage> {
        let mut receiver = self.receiver.lock().await;
        receiver.recv().await
    }
}

/// An individual agent teammate
#[derive(Clone)]
pub struct Teammate {
    /// Unique name/identifier for this agent
    pub name: String,
    /// Agent configuration
    config: TeammateConfig,
    /// Current status
    status: Arc<RwLock<TeammateStatus>>,
    /// Currently assigned tasks
    assigned_tasks: Arc<RwLock<Vec<Uuid>>>,
    /// Message inbox
    inbox: Arc<MessageInbox>,
    /// Agent state metadata
    metadata: Arc<RwLock<HashMap<String, serde_json::Value>>>,
    /// Creation timestamp
    created_at: chrono::DateTime<chrono::Utc>,
}

impl fmt::Debug for Teammate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Teammate")
            .field("name", &self.name)
            .field("config", &self.config)
            .finish()
    }
}

impl Teammate {
    /// Create a new teammate
    pub fn new(name: String, config: TeammateConfig) -> Self {
        Self {
            name,
            config,
            status: Arc::new(RwLock::new(TeammateStatus::Idle)),
            assigned_tasks: Arc::new(RwLock::new(Vec::new())),
            inbox: Arc::new(MessageInbox::new(100)),
            metadata: Arc::new(RwLock::new(HashMap::new())),
            created_at: chrono::Utc::now(),
        }
    }

    /// Get current status
    pub async fn status(&self) -> TeammateStatus {
        *self.status.read().await
    }

    /// Get full state
    pub async fn state(&self) -> TeammateState {
        let active_tasks = self.assigned_tasks.read().await.len();

        TeammateState {
            status: *self.status.read().await,
            active_tasks,
            current_worktree: self.get_metadata("current_worktree").await
                .and_then(|v| v.as_str().map(|s| s.to_string())),
            last_activity: chrono::Utc::now(),
        }
    }

    /// Check if agent is available for work
    pub async fn is_available(&self) -> bool {
        let status = *self.status.read().await;
        let task_count = self.assigned_tasks.read().await.len();

        matches!(status, TeammateStatus::Idle) && task_count < self.config.max_concurrent_tasks
    }

    /// Check if agent has a specific capability
    pub fn has_capability(&self, capability: &str) -> bool {
        self.config.capabilities.iter()
            .any(|c| c.eq_ignore_ascii_case(capability))
    }

    /// Assign a task to this teammate
    pub async fn assign_task(&self, task_id: Uuid) -> Result<(), AgentError> {
        if !self.is_available().await {
            return Err(AgentError::Communication(
                format!("Agent '{}' is not available", self.name)
            ));
        }

        let mut tasks = self.assigned_tasks.write().await;
        tasks.push(task_id);

        *self.status.write().await = TeammateStatus::Busy;

        tracing::debug!(
            agent = %self.name,
            task_id = %task_id,
            "Task assigned to agent"
        );

        Ok(())
    }

    /// Handle an incoming message
    pub async fn handle_message(&self, message: AgentMessage) -> Result<AgentMessage, AgentError> {
        tracing::debug!(
            from = %message.from,
            to = %message.to,
            message_type = ?message.message_type,
            "Agent '{}' received message", self.name
        );

        match message.message_type {
            MessageType::Chat => {
                self.handle_chat_message(message).await
            }
            MessageType::Protocol => {
                self.handle_protocol_message(message).await
            }
            MessageType::TaskAssignment => {
                self.handle_task_assignment(message).await
            }
            MessageType::TaskUpdate => {
                self.handle_task_update(message).await
            }
            MessageType::Status => {
                self.handle_status_request(message).await
            }
            _ => {
                Ok(AgentMessage::new_text(
                    self.name.clone(),
                    message.from,
                    "Message received".to_string()
                ))
            }
        }
    }

    /// Send a message to this teammate's inbox
    pub async fn send(&self, message: AgentMessage) -> Result<(), AgentError> {
        self.inbox.send(message).await
    }

    /// Receive next message from inbox
    pub async fn recv(&self) -> Option<AgentMessage> {
        self.inbox.recv().await
    }

    /// Handle a chat message
    async fn handle_chat_message(&self, message: AgentMessage) -> Result<AgentMessage, AgentError> {
        let content = match &message.content {
            MessageContent::Text(text) => text.clone(),
            MessageContent::Structured(data) => {
                // For structured messages, extract meaningful content
                data.to_string()
            }
            MessageContent::Protocol(_) => {
                return Err(AgentError::Communication(
                    "Protocol message in chat handler".to_string()
                ))
            }
        };

        tracing::debug!(
            from = %message.from,
            content_len = content.len(),
            "Agent '{}' processing chat message", self.name
        );

        // Process message content (placeholder for actual AI processing)
        // In a real implementation, this would:
        // 1. Format the message for the AI model
        // 2. Include conversation history
        // 3. Call the model API
        // 4. Parse and return the response

        let response = format!("Agent '{}' received: {}", self.name, content);

        Ok(AgentMessage::new_text(
            self.name.clone(),
            message.from,
            response
        ))
    }

    /// Handle a protocol message
    async fn handle_protocol_message(&self, message: AgentMessage) -> Result<AgentMessage, AgentError> {
        if let MessageContent::Protocol(protocol) = &message.content {
            match protocol {
                ProtocolMessage::ShutdownRequest { reason } => {
                    tracing::info!(
                        agent = %self.name,
                        reason = %reason,
                        "Shutdown request received"
                    );

                    *self.status.write().await = TeammateStatus::ShuttingDown;

                    let response = ProtocolMessage::ShutdownResponse {
                        request_id: message.id,
                        approve: true,
                        reason: None,
                    };

                    return Ok(AgentMessage::protocol(
                        self.name.clone(),
                        message.from,
                        response
                    ));
                }
                ProtocolMessage::PlanApprovalRequest { request_id, plan } => {
                    tracing::debug!(
                        agent = %self.name,
                        request_id = %request_id,
                        plan_len = plan.len(),
                        "Plan approval request received"
                    );

                    // For now, auto-approve plan requests
                    let response = ProtocolMessage::PlanApprovalResponse {
                        request_id: *request_id,
                        approve: true,
                        feedback: None,
                    };

                    return Ok(AgentMessage::protocol(
                        self.name.clone(),
                        message.from,
                        response
                    ));
                }
                ProtocolMessage::ShutdownResponse { .. } |
                ProtocolMessage::PlanApprovalResponse { .. } => {
                    // These are responses, not requests
                }
            }
        }

        Ok(AgentMessage::new_text(
            self.name.clone(),
            message.from,
            "Protocol message received".to_string()
        ))
    }

    /// Handle a task assignment
    async fn handle_task_assignment(&self, message: AgentMessage) -> Result<AgentMessage, AgentError> {
        if let MessageContent::Structured(data) = &message.content {
            if let Some(task_id) = data.get("task_id").and_then(|v| v.as_str()) {
                if let Ok(id) = Uuid::parse_str(task_id) {
                    self.assign_task(id).await?;

                    return Ok(AgentMessage::new_text(
                        self.name.clone(),
                        message.from,
                        format!("Task {} accepted", id)
                    ));
                }
            }
        }

        Ok(AgentMessage::new_text(
            self.name.clone(),
            message.from,
            "Invalid task assignment".to_string()
        ))
    }

    /// Handle a task update
    async fn handle_task_update(&self, message: AgentMessage) -> Result<AgentMessage, AgentError> {
        tracing::debug!(
            agent = %self.name,
            "Task update received: {:?}",
            message.content
        );

        Ok(AgentMessage::new_text(
            self.name.clone(),
            message.from,
            "Task update acknowledged".to_string()
        ))
    }

    /// Handle a status request
    async fn handle_status_request(&self, message: AgentMessage) -> Result<AgentMessage, AgentError> {
        let state = self.state().await;

        let status_data = serde_json::json!({
            "agent": self.name,
            "status": format!("{:?}", state.status),
            "active_tasks": state.active_tasks,
            "current_worktree": state.current_worktree,
            "last_activity": state.last_activity.to_rfc3339(),
        });

        Ok(AgentMessage {
            id: Uuid::new_v4(),
            from: self.name.clone(),
            to: message.from,
            message_type: MessageType::Status,
            priority: crate::message::MessagePriority::Normal,
            content: MessageContent::Structured(status_data),
            timestamp: chrono::Utc::now(),
        })
    }

    /// Complete a task
    pub async fn complete_task(&self, task_id: Uuid) {
        let mut tasks = self.assigned_tasks.write().await;
        tasks.retain(|t| *t != task_id);

        if tasks.is_empty() {
            *self.status.write().await = TeammateStatus::Idle;
        }

        tracing::debug!(
            agent = %self.name,
            task_id = %task_id,
            "Task completed by agent"
        );
    }

    /// Fail a task
    pub async fn fail_task(&self, task_id: Uuid, reason: String) {
        self.complete_task(task_id).await;

        tracing::warn!(
            agent = %self.name,
            task_id = %task_id,
            reason = %reason,
            "Task failed by agent"
        );
    }

    /// Set metadata value
    pub async fn set_metadata(&self, key: String, value: serde_json::Value) {
        let mut metadata = self.metadata.write().await;
        metadata.insert(key, value);
    }

    /// Get metadata value
    pub async fn get_metadata(&self, key: &str) -> Option<serde_json::Value> {
        let metadata = self.metadata.read().await;
        metadata.get(key).cloned()
    }

    /// Enter plan mode
    pub async fn enter_plan_mode(&self) -> Result<(), AgentError> {
        if !self.config.plan_mode_required {
            return Ok(());
        }

        *self.status.write().await = TeammateStatus::Planning;

        tracing::debug!(agent = %self.name, "Entered plan mode");

        Ok(())
    }

    /// Exit plan mode
    pub async fn exit_plan_mode(&self) -> Result<(), AgentError> {
        if *self.status.read().await != TeammateStatus::Planning {
            return Err(AgentError::Communication(
                "Agent not in plan mode".to_string()
            ));
        }

        let task_count = self.assigned_tasks.read().await.len();

        *self.status.write().await = if task_count > 0 {
            TeammateStatus::Busy
        } else {
            TeammateStatus::Idle
        };

        tracing::debug!(agent = %self.name, "Exited plan mode");

        Ok(())
    }

    /// Get agent creation time
    pub fn created_at(&self) -> chrono::DateTime<chrono::Utc> {
        self.created_at
    }
}
