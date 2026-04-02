//! Agent operation tools
//!
//! Provides implementations for:
//! - Agent: Spawn and manage subagent operations
//! - Team: Create and manage multi-agent teams

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

/// Agent operation types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation")]
pub enum AgentOperation {
    Spawn(AgentSpawnInput),
    SendMessage(SendMessageInput),
    CreateTeam(CreateTeamInput),
    Shutdown(ShutdownInput),
}

/// Input for spawning a new agent
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentSpawnInput {
    /// Agent type to spawn (e.g., "backend-architect", "security-engineer")
    pub agent_type: String,

    /// Task description for the agent
    pub task: String,

    /// Optional context for the agent
    pub context: Option<serde_json::Value>,

    /// Optional priority level
    pub priority: Option<String>,
}

/// Output from agent spawn
#[derive(Debug, Serialize)]
pub struct AgentSpawnOutput {
    /// Unique agent ID
    pub agent_id: String,

    /// Agent type that was spawned
    pub agent_type: String,

    /// Current status
    pub status: String,

    /// Message for user
    pub message: String,
}

/// Input for sending message to agent
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SendMessageInput {
    /// Target agent ID
    pub agent_id: String,

    /// Message content
    pub message: String,

    /// Optional metadata
    pub metadata: Option<serde_json::Value>,
}

/// Output from send message
#[derive(Debug, Serialize)]
pub struct SendMessageOutput {
    /// Whether message was delivered
    pub delivered: bool,

    /// Agent response (if available)
    pub response: Option<String>,

    /// Message ID
    pub message_id: String,
}

/// Input for creating a team
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreateTeamInput {
    /// Team name
    pub team_name: String,

    /// Team description
    pub description: String,

    /// List of agent types to include
    pub agents: Vec<String>,

    /// Optional team lead agent type
    pub team_lead: Option<String>,
}

/// Output from team creation
#[derive(Debug, Serialize)]
pub struct CreateTeamOutput {
    /// Team ID
    pub team_id: String,

    /// Team name
    pub team_name: String,

    /// Agent IDs in the team
    pub agent_ids: Vec<String>,

    /// Status
    pub status: String,
}

/// Input for shutting down agent
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ShutdownInput {
    /// Agent ID to shutdown
    pub agent_id: String,

    /// Optional reason for shutdown
    pub reason: Option<String>,

    /// Whether to force shutdown
    #[serde(default)]
    pub force: bool,
}

/// Output from shutdown
#[derive(Debug, Serialize)]
pub struct ShutdownOutput {
    /// Agent that was shut down
    pub agent_id: String,

    /// Whether shutdown was successful
    pub success: bool,

    /// Status message
    pub message: String,
}

/// Agent tool implementation
pub struct AgentTool {
    description: String,
}

impl AgentTool {
    pub fn new() -> Self {
        Self {
            description: "Spawn and manage AI agent teammates for collaborative problem-solving".to_string(),
        }
    }

    async fn spawn_agent(&self, input: AgentSpawnInput) -> Result<AgentSpawnOutput, ToolError> {
        // Generate unique agent ID
        let agent_id = format!("agent_{}", uuid::Uuid::new_v4());
        let agent_type = input.agent_type.clone();

        Ok(AgentSpawnOutput {
            agent_id: agent_id.clone(),
            agent_type,
            status: "initialized".to_string(),
            message: format!("Spawned {} agent with ID {}", input.agent_type, agent_id),
        })
    }

    async fn send_message(&self, input: SendMessageInput) -> Result<SendMessageOutput, ToolError> {
        let message_id = format!("msg_{}", uuid::Uuid::new_v4());

        Ok(SendMessageOutput {
            delivered: true,
            response: None, // Would be populated by actual agent
            message_id,
        })
    }

    async fn create_team(&self, input: CreateTeamInput) -> Result<CreateTeamOutput, ToolError> {
        let team_id = format!("team_{}", uuid::Uuid::new_v4());

        // Mock agent IDs for team members
        let agent_ids: Vec<String> = input
            .agents
            .iter()
            .map(|_| format!("agent_{}", uuid::Uuid::new_v4()))
            .collect();

        Ok(CreateTeamOutput {
            team_id,
            team_name: input.team_name,
            agent_ids,
            status: "created".to_string(),
        })
    }

    async fn shutdown_agent(&self, input: ShutdownInput) -> Result<ShutdownOutput, ToolError> {
        Ok(ShutdownOutput {
            agent_id: input.agent_id,
            success: true,
            message: "Agent successfully shut down".to_string(),
        })
    }
}

#[async_trait]
impl Tool for AgentTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        // Parse operation type from input
        let operation = input
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("Missing operation field".to_string()))?;

        match operation {
            "Spawn" => {
                let spawn_input: AgentSpawnInput = serde_json::from_value(input)
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid spawn input: {}", e)))?;
                let output = self.spawn_agent(spawn_input).await?;
                Ok(ToolOutput {
                    content: output.message,
                    is_error: false,
                    metadata: {
                        let mut map = HashMap::new();
                        map.insert("agent_id".to_string(), json!(output.agent_id));
                        map.insert("agent_type".to_string(), json!(output.agent_type));
                        map.insert("status".to_string(), json!(output.status));
                        map
                    },
                })
            }
            "SendMessage" => {
                let msg_input: SendMessageInput = serde_json::from_value(input)
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid message input: {}", e)))?;
                let output = self.send_message(msg_input).await?;
                Ok(ToolOutput {
                    content: format!("Message {} delivered successfully", output.message_id),
                    is_error: false,
                    metadata: {
                        let mut map = HashMap::new();
                        map.insert("message_id".to_string(), json!(output.message_id));
                        map.insert("delivered".to_string(), json!(output.delivered));
                        if let Some(response) = output.response {
                            map.insert("response".to_string(), json!(response));
                        }
                        map
                    },
                })
            }
            "CreateTeam" => {
                let team_input: CreateTeamInput = serde_json::from_value(input)
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid team input: {}", e)))?;
                let output = self.create_team(team_input).await?;
                Ok(ToolOutput {
                    content: format!("Team {} created successfully", output.team_name),
                    is_error: false,
                    metadata: {
                        let mut map = HashMap::new();
                        map.insert("team_id".to_string(), json!(output.team_id));
                        map.insert("team_name".to_string(), json!(output.team_name));
                        map.insert("agent_ids".to_string(), json!(output.agent_ids));
                        map.insert("status".to_string(), json!(output.status));
                        map
                    },
                })
            }
            "Shutdown" => {
                let shutdown_input: ShutdownInput = serde_json::from_value(input)
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid shutdown input: {}", e)))?;
                let output = self.shutdown_agent(shutdown_input).await?;
                Ok(ToolOutput {
                    content: output.message,
                    is_error: false,
                    metadata: {
                        let mut map = HashMap::new();
                        map.insert("agent_id".to_string(), json!(output.agent_id));
                        map.insert("success".to_string(), json!(output.success));
                        map
                    },
                })
            }
            _ => Err(ToolError::InvalidInput(format!(
                "Unknown operation: {}",
                operation
            ))),
        }
    }

    fn name(&self) -> &str {
        "Agent"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "description": "Operation type",
                    "enum": ["Spawn", "SendMessage", "CreateTeam", "Shutdown"]
                },
                "agent_type": {
                    "type": "string",
                    "description": "Agent type to spawn"
                },
                "task": {
                    "type": "string",
                    "description": "Task description"
                },
                "context": {
                    "type": "object",
                    "description": "Optional context"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent ID for operations"
                },
                "message": {
                    "type": "string",
                    "description": "Message content"
                },
                "team_name": {
                    "type": "string",
                    "description": "Team name"
                },
                "description": {
                    "type": "string",
                    "description": "Team description"
                }
            },
            "required": ["operation"]
        })
    }
}
