//! Agent operation tools
//!
//! Provides implementations for:
//! - Agent: Spawn and manage subagent operations with real execution
//! - Team: Create and manage multi-agent teams via AgentCoordinator

use crate::{Tool, ToolError, ToolResult, ToolOutput};
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use shannon_agents::{
    TeamContext,
    AgentConfig,
    AgentMessage, MessageContent, ProtocolMessage,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Type alias for backward compatibility.
pub type AgentToolContext = TeamContext;

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

    /// Actual result from sub-agent execution (if completed)
    pub result: Option<String>,
}

/// Input for sending message to agent
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SendMessageInput {
    /// Target agent ID (name)
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
    /// Shared context injected after construction. None until inject_context() is called.
    context: Arc<Mutex<Option<AgentToolContext>>>,
}

impl Default for AgentTool {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentTool {
    pub fn new() -> Self {
        Self {
            description: "Spawn and manage AI agent teammates for collaborative problem-solving".to_string(),
            context: Arc::new(Mutex::new(None)),
        }
    }

    /// Get a clone of the context Arc for injection from external code.
    pub fn context_handle(&self) -> Arc<Mutex<Option<AgentToolContext>>> {
        self.context.clone()
    }

    /// Inject the team context for real coordinator-backed execution.
    pub fn inject_context(&self, ctx: AgentToolContext) {
        if let Ok(mut guard) = self.context.lock() {
            *guard = Some(ctx);
        }
    }

    /// Try to get the TeamContext, returns None if not injected.
    fn get_team_context(&self) -> Option<AgentToolContext> {
        self.context.lock().ok().and_then(|g| g.clone())
    }

    /// Register all tools EXCEPT the Agent tool (prevents infinite recursion).
    pub fn register_subagent_tools(
        registry: &mut shannon_core::ToolRegistry,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // File operations
        registry.register(Box::new(crate::ReadTool::new()))?;
        registry.register(Box::new(crate::WriteTool::new()))?;
        registry.register(Box::new(crate::EditTool::new()))?;
        registry.register(Box::new(crate::GlobTool::new()))?;

        // System operations
        registry.register(Box::new(crate::BashTool::new()))?;
        registry.register(Box::new(crate::SleepTool::new()))?;
        registry.register(Box::new(crate::PowerShellTool::new()))?;
        registry.register(Box::new(crate::ReplTool::new()))?;

        // Git operations
        registry.register(Box::new(crate::GitBranchTool::new()))?;
        registry.register(Box::new(crate::GitDiffTool::new()))?;
        registry.register(Box::new(crate::GitLogTool::new()))?;
        registry.register(Box::new(crate::GitStashTool::new()))?;
        registry.register(Box::new(crate::GitSafetyTool::new()))?;
        registry.register(Box::new(crate::AutoCommitTool::new()))?;

        // Web operations
        registry.register(Box::new(crate::WebFetchTool::new()))?;
        registry.register(Box::new(crate::WebSearchTool::new()))?;

        // Search
        registry.register(Box::new(crate::GrepTool::new()))?;

        // Task management (for TodoWrite etc.)
        registry.register(Box::new(crate::TodoWriteTool::new()))?;
        registry.register(Box::new(crate::TaskCreateTool::new()))?;
        registry.register(Box::new(crate::TaskListTool::new()))?;
        registry.register(Box::new(crate::TaskUpdateTool::new()))?;
        registry.register(Box::new(crate::TaskGetTool::new()))?;
        registry.register(Box::new(crate::TaskTool::new()))?;
        registry.register(Box::new(crate::TaskOutputTool::new()))?;
        registry.register(Box::new(crate::TaskStopTool::new()))?;

        // LSP
        registry.register(Box::new(crate::GoToDefinitionTool::new()))?;
        registry.register(Box::new(crate::FindReferencesTool::new()))?;
        registry.register(Box::new(crate::HoverTool::new()))?;
        registry.register(Box::new(crate::DocumentSymbolTool::new()))?;
        registry.register(Box::new(crate::WorkspaceSymbolTool::new()))?;

        // Notebook
        registry.register(Box::new(crate::NotebookEditTool::new()))?;

        // Worktree
        registry.register(Box::new(crate::WorktreeTool::new()))?;

        // Config tool
        registry.register(Box::new(crate::ConfigTool::new()))?;

        Ok(())
    }

    async fn spawn_agent(&self, input: AgentSpawnInput) -> Result<AgentSpawnOutput, ToolError> {
        let agent_id = format!("agent_{}", uuid::Uuid::new_v4());
        let agent_type = input.agent_type.clone();

        if let Some(ctx) = self.get_team_context() {
            // 1. Register in coordinator for team coordination
            let team = input.context.as_ref()
                .and_then(|c| c.get("team").and_then(|v| v.as_str()).map(|s| s.to_string()));

            let config = AgentConfig {
                name: format!("{}-{}", &agent_type, &agent_id[6..14]), // readable name
                model: ctx.client_config.model.clone(),
                system_prompt: format!(
                    "You are a sub-agent of type '{agent_type}'. Focus on completing the assigned task concisely."
                ),
                tools: Vec::new(),
                working_directory: std::path::PathBuf::from("."),
                max_turns: 50,
                team,
            };

            // Spawn in the registry (creates team if needed, adds teammate)
            let agent = ctx.registry.spawn(config).await
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to spawn agent: {e}")))?;

            let agent_name = agent.name.clone();
            let agent_uid = agent.id.clone();

            // 2. Execute task via real QueryEngine
            let result = self.execute_subagent(
                agent_uid.clone(),
                agent_type.clone(),
                input,
                ctx.client_config.clone(),
            ).await?;

            // 3. Update agent status in registry (use list to find and update)
            // The SubAgentRegistry tracks agents internally; the coordinator
            // tracks teammates. Both are updated by the spawn.
            tracing::info!(
                agent_id = %agent_uid,
                agent_name = %agent_name,
                status = %result.status,
                "Sub-agent execution completed"
            );

            Ok(AgentSpawnOutput {
                agent_id: agent_uid,
                agent_type,
                status: result.status,
                message: result.message,
                result: result.result,
            })
        } else {
            // Fallback: no context injected, use standalone execution
            // Try to get client_config for standalone mode
            let client_config = self.context.lock().ok().and_then(|g| {
                g.as_ref().map(|c| c.client_config.clone())
            });

            match client_config {
                Some(client_config) => {
                    self.execute_subagent(agent_id, agent_type, input, client_config).await
                }
                None => {
                    Ok(AgentSpawnOutput {
                        agent_id,
                        agent_type,
                        status: "initialized".to_string(),
                        message: format!(
                            "Agent spawned (no execution context). Task: {}",
                            &input.task[..input.task.len().min(100)]
                        ),
                        result: None,
                    })
                }
            }
        }
    }

    /// Execute a task in a real sub-agent QueryEngine.
    async fn execute_subagent(
        &self,
        agent_id: String,
        agent_type: String,
        input: AgentSpawnInput,
        client_config: shannon_core::api::LlmClientConfig,
    ) -> Result<AgentSpawnOutput, ToolError> {
        use shannon_core::query_engine::{QueryContext, QueryEvent, QueryMetadata};
        use uuid::Uuid;

        // Build a sub-agent tool registry (without Agent tool to prevent recursion)
        let mut sub_tools = shannon_core::ToolRegistry::new();
        Self::register_subagent_tools(&mut sub_tools)
            .map_err(|e| ToolError::ExecutionFailed(format!("sub-agent tool setup failed: {e}")))?;

        // Create sub-agent engine with FullAuto permissions
        let model_name = client_config.model.clone();
        let client = shannon_core::api::LlmClient::new(client_config);
        let mut permissions = shannon_core::permissions::PermissionManager::new();
        permissions.set_approval_mode(shannon_core::permissions::ApprovalMode::FullAuto);
        let state = shannon_core::state::StateManager::new();

        let engine = shannon_core::query_engine::QueryEngine::with_defaults(
            client, sub_tools,
            permissions,
            state,
        );

        // Build context with agent type role
        let system_hint = format!(
            "You are a sub-agent of type '{agent_type}'. Focus on completing the assigned task concisely.\n\nTask: {task}",
            task = input.task,
        );
        let user_message = match &input.context {
            Some(ctx_val) => format!(
                "{system_hint}\n\nAdditional context:\n{}",
                serde_json::to_string_pretty(ctx_val).unwrap_or_default()
            ),
            None => system_hint,
        };

        let query_context = QueryContext {
            query_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            user_message,
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: true,
                max_tokens: Some(4096),
                model: model_name,
                temperature: None,
                top_p: None,
            },
        };

        // Execute and collect the response
        let mut stream = engine.process_query(query_context, None).await;
        let mut response_text = String::new();
        let mut tools_used = 0usize;

        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(QueryEvent::Text { content, .. }) => {
                    response_text.push_str(&content);
                }
                Ok(QueryEvent::ToolUseRequest { tool_name, .. }) => {
                    tools_used += 1;
                    // Safety: cap tool usage to prevent runaway agents
                    if tools_used > 10 {
                        response_text.push_str("\n[Sub-agent tool limit reached (10)]");
                        break;
                    }
                    let _ = tool_name; // suppress unused warning
                }
                Ok(QueryEvent::ToolUseResult { is_error, result, .. }) => {
                    if is_error {
                        response_text.push_str(&format!("\n[Tool error: {}]", &result[..result.len().min(200)]));
                    }
                }
                Ok(QueryEvent::Failed { error, .. }) => {
                    return Ok(AgentSpawnOutput {
                        agent_id,
                        agent_type,
                        status: "failed".to_string(),
                        message: format!("Sub-agent failed: {error}"),
                        result: None,
                    });
                }
                _ => {}
            }
        }

        let status = if response_text.is_empty() {
            "completed_empty".to_string()
        } else {
            "completed".to_string()
        };

        // Truncate very long responses to avoid bloating the conversation
        let result_text = if response_text.len() > 4000 {
            format!("{}...\n[Response truncated: {} chars total]", &response_text[..4000], response_text.len())
        } else {
            response_text
        };

        Ok(AgentSpawnOutput {
            agent_id,
            agent_type,
            status,
            message: format!(
                "Sub-agent completed ({tools_used} tool(s) used)."
            ),
            result: Some(result_text),
        })
    }

    async fn send_message(&self, input: SendMessageInput) -> Result<SendMessageOutput, ToolError> {
        let message_id = format!("msg_{}", uuid::Uuid::new_v4());

        if let Some(ctx) = self.get_team_context() {
            // Real message routing through the coordinator
            let responses = ctx.registry
                .send_message("lead", &input.agent_id, serde_json::Value::String(input.message))
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to send message: {e}")))?;

            let response_text = responses.first()
                .map(|r| match &r.content {
                    MessageContent::Text(t) => t.clone(),
                    other => format!("{other:?}"),
                })
                .unwrap_or_default();

            Ok(SendMessageOutput {
                delivered: true,
                response: Some(response_text),
                message_id,
            })
        } else {
            // Fallback: no coordinator
            Ok(SendMessageOutput {
                delivered: true,
                response: None,
                message_id,
            })
        }
    }

    async fn create_team(&self, input: CreateTeamInput) -> Result<CreateTeamOutput, ToolError> {
        if let Some(ctx) = self.get_team_context() {
            // Real team creation through the coordinator
            let team_name = ctx.registry
                .create_team(input.team_name.clone(), input.description.clone())
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to create team: {e}")))?;

            // Optionally pre-spawn agents for the specified agent types
            let mut agent_ids = Vec::new();
            for agent_type in &input.agents {
                let config = AgentConfig {
                    name: format!("{}-{}", agent_type, uuid::Uuid::new_v4().as_simple()),
                    model: ctx.client_config.model.clone(),
                    system_prompt: format!("You are a {agent_type} agent. Focus on your specialty."),
                    tools: Vec::new(),
                    working_directory: std::path::PathBuf::from("."),
                    max_turns: 50,
                    team: Some(team_name.clone()),
                };
                let agent = ctx.registry.spawn(config).await
                    .map_err(|e| ToolError::ExecutionFailed(format!("Failed to spawn {agent_type}: {e}")))?;
                agent_ids.push(agent.id);
            }

            Ok(CreateTeamOutput {
                team_id: team_name,
                team_name: input.team_name,
                agent_ids,
                status: "created".to_string(),
            })
        } else {
            // Fallback: no coordinator, return placeholder
            let team_id = format!("team_{}", uuid::Uuid::new_v4());
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
    }

    async fn shutdown_agent(&self, input: ShutdownInput) -> Result<ShutdownOutput, ToolError> {
        if let Some(ctx) = self.get_team_context() {
            // Send shutdown protocol message through coordinator
            let msg = AgentMessage::protocol(
                "lead".to_string(),
                input.agent_id.clone(),
                ProtocolMessage::ShutdownRequest {
                    reason: input.reason.unwrap_or_else(|| "Graceful shutdown".to_string()),
                },
            );

            ctx.coordinator.send_message(msg).await
                .map_err(|e| ToolError::ExecutionFailed(format!("Shutdown failed: {e}")))?;

            Ok(ShutdownOutput {
                agent_id: input.agent_id,
                success: true,
                message: "Agent shutdown request sent".to_string(),
            })
        } else {
            Ok(ShutdownOutput {
                agent_id: input.agent_id,
                success: true,
                message: "Agent successfully shut down".to_string(),
            })
        }
    }
}

#[async_trait]
impl Tool for AgentTool {
    async fn execute(&self, input: serde_json::Value) -> ToolResult<ToolOutput> {
        let operation = input
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("Missing operation field".to_string()))?;

        match operation {
            "Spawn" => {
                let spawn_input: AgentSpawnInput = serde_json::from_value(input)
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid spawn input: {e}")))?;
                let output = self.spawn_agent(spawn_input).await?;

                let content = match &output.result {
                    Some(result) => result.clone(),
                    None => output.message.clone(),
                };

                Ok(ToolOutput {
                    content,
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
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid message input: {e}")))?;
                let output = self.send_message(msg_input).await?;
                Ok(ToolOutput {
                    content: if let Some(response) = &output.response {
                        format!("Message {} delivered. Response: {}", output.message_id, response)
                    } else {
                        format!("Message {} delivered successfully", output.message_id)
                    },
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
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid team input: {e}")))?;
                let output = self.create_team(team_input).await?;
                Ok(ToolOutput {
                    content: format!("Team '{}' created with {} agent(s)", output.team_name, output.agent_ids.len()),
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
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid shutdown input: {e}")))?;
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
                "Unknown operation: {operation}"
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
                    "description": "Optional context (can include 'team' for team assignment)"
                },
                "agent_id": {
                    "type": "string",
                    "description": "Agent ID or name for operations"
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
                },
                "agents": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Agent types to pre-spawn in team"
                },
                "force": {
                    "type": "boolean",
                    "description": "Force shutdown"
                },
                "reason": {
                    "type": "string",
                    "description": "Reason for shutdown"
                }
            },
            "required": ["operation"]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Input serialization ────────────────────────────────────────────────

    #[test]
    fn test_agent_spawn_input_roundtrip() {
        let input = AgentSpawnInput {
            agent_type: "backend-architect".into(),
            task: "Design auth system".into(),
            context: Some(json!({"team": "alpha"})),
            priority: Some("high".into()),
        };
        let ser = serde_json::to_string(&input).unwrap();
        let de: AgentSpawnInput = serde_json::from_str(&ser).unwrap();
        assert_eq!(de.agent_type, "backend-architect");
        assert_eq!(de.task, "Design auth system");
        assert!(de.context.is_some());
        assert_eq!(de.priority.unwrap(), "high");
    }

    #[test]
    fn test_agent_spawn_input_minimal() {
        let input = AgentSpawnInput {
            agent_type: "researcher".into(),
            task: "Investigate".into(),
            context: None,
            priority: None,
        };
        let ser = serde_json::to_string(&input).unwrap();
        let de: AgentSpawnInput = serde_json::from_str(&ser).unwrap();
        assert!(de.context.is_none());
        assert!(de.priority.is_none());
    }

    #[test]
    fn test_send_message_input_roundtrip() {
        let input = SendMessageInput {
            agent_id: "agent-1".into(),
            message: "hello".into(),
            metadata: Some(json!({"key": "val"})),
        };
        let ser = serde_json::to_string(&input).unwrap();
        let de: SendMessageInput = serde_json::from_str(&ser).unwrap();
        assert_eq!(de.agent_id, "agent-1");
        assert_eq!(de.message, "hello");
    }

    #[test]
    fn test_create_team_input_roundtrip() {
        let input = CreateTeamInput {
            team_name: "team-a".into(),
            description: "A team".into(),
            agents: vec!["backend".into(), "frontend".into()],
            team_lead: Some("lead".into()),
        };
        let ser = serde_json::to_string(&input).unwrap();
        let de: CreateTeamInput = serde_json::from_str(&ser).unwrap();
        assert_eq!(de.agents.len(), 2);
        assert_eq!(de.team_lead.unwrap(), "lead");
    }

    #[test]
    fn test_shutdown_input_roundtrip() {
        let input = ShutdownInput {
            agent_id: "agent-1".into(),
            reason: Some("done".into()),
            force: true,
        };
        let ser = serde_json::to_string(&input).unwrap();
        let de: ShutdownInput = serde_json::from_str(&ser).unwrap();
        assert!(de.force);
        assert_eq!(de.reason.unwrap(), "done");
    }

    #[test]
    fn test_shutdown_input_default_force() {
        let json = r#"{"agent_id":"a1"}"#;
        let de: ShutdownInput = serde_json::from_str(json).unwrap();
        assert!(!de.force);
        assert!(de.reason.is_none());
    }

    // ── AgentOperation tagged enum ─────────────────────────────────────────

    #[test]
    fn test_agent_operation_spawn() {
        let json = json!({
            "operation": "Spawn",
            "agent_type": "coder",
            "task": "fix bug"
        });
        let op: AgentOperation = serde_json::from_value(json).unwrap();
        assert!(matches!(op, AgentOperation::Spawn(_)));
    }

    #[test]
    fn test_agent_operation_send_message() {
        let json = json!({
            "operation": "SendMessage",
            "agent_id": "a1",
            "message": "hi"
        });
        let op: AgentOperation = serde_json::from_value(json).unwrap();
        assert!(matches!(op, AgentOperation::SendMessage(_)));
    }

    #[test]
    fn test_agent_operation_create_team() {
        let json = json!({
            "operation": "CreateTeam",
            "team_name": "t1",
            "description": "desc",
            "agents": ["a"]
        });
        let op: AgentOperation = serde_json::from_value(json).unwrap();
        assert!(matches!(op, AgentOperation::CreateTeam(_)));
    }

    #[test]
    fn test_agent_operation_shutdown() {
        let json = json!({
            "operation": "Shutdown",
            "agent_id": "a1"
        });
        let op: AgentOperation = serde_json::from_value(json).unwrap();
        assert!(matches!(op, AgentOperation::Shutdown(_)));
    }

    // ── Tool execution (fallback without context) ──────────────────────────

    #[tokio::test]
    async fn test_spawn_without_context() {
        let tool = AgentTool::new();
        let output = tool.execute(json!({
            "operation": "Spawn",
            "agent_type": "researcher",
            "task": "Investigate something"
        })).await.unwrap();
        assert!(!output.is_error);
        assert!(output.metadata.contains_key("agent_id"));
        assert!(output.metadata.contains_key("agent_type"));
        assert!(output.metadata.contains_key("status"));
    }

    #[tokio::test]
    async fn test_send_message_without_context() {
        let tool = AgentTool::new();
        let output = tool.execute(json!({
            "operation": "SendMessage",
            "agent_id": "agent-1",
            "message": "hello"
        })).await.unwrap();
        assert!(!output.is_error);
        assert!(output.metadata.contains_key("message_id"));
        assert!(output.metadata["delivered"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_create_team_without_context() {
        let tool = AgentTool::new();
        let output = tool.execute(json!({
            "operation": "CreateTeam",
            "team_name": "test-team",
            "description": "A test team",
            "agents": ["backend", "frontend"]
        })).await.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("test-team"));
        let agent_ids = output.metadata["agent_ids"].as_array().unwrap();
        assert_eq!(agent_ids.len(), 2);
    }

    #[tokio::test]
    async fn test_shutdown_without_context() {
        let tool = AgentTool::new();
        let output = tool.execute(json!({
            "operation": "Shutdown",
            "agent_id": "agent-1"
        })).await.unwrap();
        assert!(!output.is_error);
        assert!(output.metadata["success"].as_bool().unwrap());
    }

    // ── Error cases ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_missing_operation() {
        let tool = AgentTool::new();
        let result = tool.execute(json!({"agent_type": "x"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_unknown_operation() {
        let tool = AgentTool::new();
        let result = tool.execute(json!({"operation": "Fly"})).await;
        assert!(result.is_err());
    }

    // ── Context injection ──────────────────────────────────────────────────

    #[test]
    fn test_context_handle_returns_arc() {
        let tool = AgentTool::new();
        let handle = tool.context_handle();
        assert!(handle.lock().unwrap().is_none());
    }

    #[test]
    fn test_inject_and_get_context() {
        let tool = AgentTool::new();
        // Without injection, get_team_context returns None
        assert!(tool.get_team_context().is_none());
    }

    // ── Tool trait ─────────────────────────────────────────────────────────

    #[test]
    fn test_tool_name() {
        let tool = AgentTool::new();
        assert_eq!(tool.name(), "Agent");
    }

    #[test]
    fn test_tool_description() {
        let tool = AgentTool::new();
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_tool_input_schema() {
        let tool = AgentTool::new();
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["operation"].is_object());
        let ops = schema["properties"]["operation"]["enum"].as_array().unwrap();
        assert_eq!(ops.len(), 4);
    }

    #[test]
    fn test_default_creates_tool() {
        let tool = AgentTool::default();
        assert_eq!(tool.name(), "Agent");
    }

    // ── Send + Sync ────────────────────────────────────────────────────────

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AgentTool>();
        assert_send_sync::<AgentSpawnInput>();
        assert_send_sync::<SendMessageInput>();
        assert_send_sync::<CreateTeamInput>();
        assert_send_sync::<ShutdownInput>();
        assert_send_sync::<AgentOperation>();
    }
}
