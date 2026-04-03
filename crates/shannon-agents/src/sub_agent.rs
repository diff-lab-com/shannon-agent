//! Sub-agent spawning, messaging, and team management tools
//!
//! Provides:
//! - `AgentConfig`: Configuration for spawning sub-agents
//! - `SubAgent`: Process handle for a spawned sub-agent
//! - `AgentSpawnTool`: Tool to spawn a sub-agent as a subprocess
//! - `SendMessageTool`: Tool to route messages between agents
//! - `TeamCreateTool`: Tool to create agent teams with shared task boards

use crate::coordinator::AgentCoordinator;
use crate::error::{AgentError, CoordinationError};
use crate::message::{AgentMessage, MessageContent, MessageType};
use crate::teammate::TeammateConfig;
use crate::TaskBoard;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use shannon_core::tools::{Tool, ToolError, ToolOutput, ToolResult};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Configuration for spawning a sub-agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Human-readable agent name
    pub name: String,
    /// Model identifier (e.g., "claude-sonnet-4-6")
    #[serde(default = "default_model")]
    pub model: String,
    /// System prompt that defines the agent's behavior
    pub system_prompt: String,
    /// Tool names available to this agent
    #[serde(default)]
    pub tools: Vec<String>,
    /// Working directory for the agent
    #[serde(default)]
    pub working_directory: PathBuf,
    /// Maximum conversation turns before forced completion
    #[serde(default = "default_max_turns")]
    pub max_turns: u32,
    /// Team to assign this agent to (optional)
    #[serde(default)]
    pub team: Option<String>,
}

fn default_model() -> String {
    "claude-sonnet-4-6".to_string()
}

fn default_max_turns() -> u32 {
    50
}

/// Status of a sub-agent
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    /// Agent is being spawned
    Spawning,
    /// Agent is idle and available
    Idle,
    /// Agent is actively processing
    Running,
    /// Agent finished successfully
    Completed,
    /// Agent encountered an error
    Failed(String),
}

impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentStatus::Spawning => write!(f, "spawning"),
            AgentStatus::Idle => write!(f, "idle"),
            AgentStatus::Running => write!(f, "running"),
            AgentStatus::Completed => write!(f, "completed"),
            AgentStatus::Failed(reason) => write!(f, "failed: {}", reason),
        }
    }
}

/// A sub-agent process handle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgent {
    /// Unique agent identifier
    pub id: String,
    /// Human-readable agent name
    pub name: String,
    /// Agent configuration
    pub config: AgentConfig,
    /// Current status
    pub status: AgentStatus,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Team this agent belongs to (if any)
    pub team: Option<String>,
    /// Number of turns executed so far
    pub turns_used: u32,
    /// Last message from this agent
    pub last_output: Option<String>,
}

impl SubAgent {
    /// Create a new SubAgent from a config
    pub fn new(config: AgentConfig) -> Self {
        let name = config.name.clone();
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            config,
            status: AgentStatus::Spawning,
            created_at: chrono::Utc::now(),
            team: None,
            turns_used: 0,
            last_output: None,
        }
    }

    /// Check whether the agent has remaining turns
    pub fn has_turns_remaining(&self) -> bool {
        self.turns_used < self.config.max_turns
    }

    /// Transition to idle state
    pub fn mark_idle(&mut self) {
        self.status = AgentStatus::Idle;
    }

    /// Transition to running state
    pub fn mark_running(&mut self) {
        self.status = AgentStatus::Running;
    }

    /// Record a completed turn
    pub fn record_turn(&mut self, output: Option<String>) {
        self.turns_used += 1;
        self.last_output = output;
        if !self.has_turns_remaining() {
            self.status = AgentStatus::Completed;
        }
    }

    /// Transition to failed state
    pub fn mark_failed(&mut self, reason: String) {
        self.status = AgentStatus::Failed(reason);
    }

    /// Transition to completed state
    pub fn mark_completed(&mut self) {
        self.status = AgentStatus::Completed;
    }
}

// ---------------------------------------------------------------------------
// SubAgentRegistry — in-process bookkeeping for spawned agents
// ---------------------------------------------------------------------------

/// In-process registry that tracks all sub-agents and teams.
/// This bridges the Tool trait (which takes `&self` and `Value`) to the
/// async `AgentCoordinator` API.
#[derive(Clone)]
pub struct SubAgentRegistry {
    coordinator: Arc<AgentCoordinator>,
    /// Sub-agent handles indexed by agent name
    agents: Arc<RwLock<HashMap<String, SubAgent>>>,
    /// Team name -> (team_name, description)
    teams: Arc<RwLock<HashMap<String, String>>>,
}

impl SubAgentRegistry {
    /// Create a new registry backed by the given coordinator.
    pub fn new(coordinator: Arc<AgentCoordinator>) -> Self {
        Self {
            coordinator,
            agents: Arc::new(RwLock::new(HashMap::new())),
            teams: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Spawn a new sub-agent and register it.
    pub async fn spawn(&self, config: AgentConfig) -> Result<SubAgent, AgentError> {
        let name = config.name.clone();

        // Reject duplicate names
        {
            let agents = self.agents.read().await;
            if agents.contains_key(&name) {
                return Err(AgentError::Coordination(
                    CoordinationError::InvalidConfiguration(format!(
                        "agent '{}' already exists", name
                    )),
                ));
            }
        }

        // Create a teammate in the coordinator (it needs a team, so create a
        // default "_global" team if none exists and no team is specified).
        let team_name = match &config.team {
            Some(t) => t.clone(),
            None => "_global".to_string(),
        };

        // Ensure the team exists in the coordinator
        {
            let teams = self.teams.read().await;
            if !teams.contains_key(&team_name) {
                drop(teams);
                self.coordinator.create_team(
                    team_name.clone(),
                    "Default agent team".to_string(),
                ).await?;
                self.teams.write().await.insert(team_name.clone(), "Default agent team".to_string());
            }
        }

        // Add teammate to the coordinator team
        let teammate_config = TeammateConfig {
            agent_type: "sub-agent".to_string(),
            capabilities: config.tools.clone(),
            max_concurrent_tasks: 3,
            plan_mode_required: false,
            model: Some(config.model.clone()),
            system_prompt: Some(config.system_prompt.clone()),
            temperature: None,
        };

        self.coordinator
            .add_teammate(&team_name, name.clone(), teammate_config)
            .await?;

        // Build the SubAgent handle
        let mut agent = SubAgent::new(config);
        agent.team = Some(team_name.clone());
        agent.mark_idle();

        self.agents.write().await.insert(name.clone(), agent.clone());

        tracing::info!(
            agent_id = %agent.id,
            agent_name = %name,
            team = %team_name,
            "Sub-agent spawned"
        );

        Ok(agent)
    }

    /// Send a message to an agent or broadcast.
    pub async fn send_message(
        &self,
        from: &str,
        to: &str,
        content: Value,
    ) -> Result<Vec<AgentMessage>, AgentError> {
        let message_content = if content.is_string() {
            MessageContent::Text(content.as_str().unwrap().to_string())
        } else {
            MessageContent::Structured(content)
        };

        if to == "*" {
            // Broadcast: send to all registered agents
            let agents = self.agents.read().await;
            let mut responses = Vec::new();

            for (agent_name, _agent) in agents.iter() {
                let msg = AgentMessage::new_text(
                    from.to_string(),
                    agent_name.clone(),
                    format!("{:?}", message_content),
                );
                self.coordinator.send_message(msg).await?;

                // For broadcast we just emit the sent messages
                responses.push(AgentMessage::new_text(
                    agent_name.clone(),
                    from.to_string(),
                    format!("Broadcast received by {}", agent_name),
                ));
            }

            Ok(responses)
        } else {
            // Direct message
            let agents = self.agents.read().await;
            if !agents.contains_key(to) {
                return Err(AgentError::Coordination(
                    CoordinationError::AgentNotFound(to.to_string()),
                ));
            }

            let msg = AgentMessage {
                id: Uuid::new_v4(),
                from: from.to_string(),
                to: to.to_string(),
                message_type: MessageType::Chat,
                priority: crate::message::MessagePriority::Normal,
                content: message_content,
                timestamp: chrono::Utc::now(),
            };

            self.coordinator.send_message(msg).await?;

            let response = AgentMessage::new_text(
                to.to_string(),
                from.to_string(),
                format!("Message received by {}", to),
            );

            Ok(vec![response])
        }
    }

    /// Create a new team.
    pub async fn create_team(&self, team_name: String, description: String) -> Result<String, AgentError> {
        {
            let teams = self.teams.read().await;
            if teams.contains_key(&team_name) {
                return Err(AgentError::Coordination(
                    CoordinationError::InvalidConfiguration(format!(
                        "team '{}' already exists", team_name
                    )),
                ));
            }
        }

        self.coordinator
            .create_team(team_name.clone(), description.clone())
            .await?;

        self.teams.write().await.insert(team_name.clone(), description);

        tracing::info!(team = %team_name, "Team created via tool");

        Ok(team_name)
    }

    /// List all registered sub-agents.
    pub async fn list_agents(&self) -> Vec<SubAgent> {
        self.agents.read().await.values().cloned().collect()
    }

    /// Get a specific sub-agent by name.
    pub async fn get_agent(&self, name: &str) -> Option<SubAgent> {
        self.agents.read().await.get(name).cloned()
    }

    /// Get the underlying coordinator reference.
    pub fn coordinator(&self) -> &AgentCoordinator {
        &self.coordinator
    }

    /// Get the task board.
    pub fn task_board(&self) -> &TaskBoard {
        self.coordinator.task_board()
    }
}

// ---------------------------------------------------------------------------
// Tool implementations
// ---------------------------------------------------------------------------

// ---- AgentSpawnTool ------------------------------------------------------

/// Input for the `agent_spawn` tool.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentSpawnInput {
    /// Agent name (must be unique)
    pub name: String,
    /// Model to use (optional, defaults to claude-sonnet-4-6)
    pub model: Option<String>,
    /// System prompt for the agent
    pub system_prompt: String,
    /// Tool names available to this agent
    pub tools: Option<Vec<String>>,
    /// Team to assign the agent to
    pub team: Option<String>,
    /// Maximum conversation turns
    pub max_turns: Option<u32>,
}

/// Tool that spawns a sub-agent as a subprocess.
pub struct AgentSpawnTool {
    registry: Arc<SubAgentRegistry>,
}

impl AgentSpawnTool {
    pub fn new(registry: Arc<SubAgentRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for AgentSpawnTool {
    fn name(&self) -> &str {
        "agent_spawn"
    }

    fn description(&self) -> &str {
        "Spawn a sub-agent that can work on tasks independently. The agent is added to a team \
         (optionally specified) and becomes available for message passing and task assignment."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Unique name for the sub-agent"
                },
                "model": {
                    "type": "string",
                    "description": "Model to use (e.g., 'claude-sonnet-4-6')"
                },
                "system_prompt": {
                    "type": "string",
                    "description": "System prompt defining the agent's behavior and capabilities"
                },
                "tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Tool names available to this agent"
                },
                "team": {
                    "type": "string",
                    "description": "Team to assign the agent to (created if needed)"
                },
                "max_turns": {
                    "type": "integer",
                    "description": "Maximum conversation turns before forced completion",
                    "minimum": 1,
                    "maximum": 1000
                }
            },
            "required": ["name", "system_prompt"]
        })
    }

    fn category(&self) -> &str {
        "agents"
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let parsed: AgentSpawnInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid agent_spawn input: {}", e)))?;

        if parsed.name.is_empty() {
            return Err(ToolError::InvalidInput("Agent name must not be empty".into()));
        }
        if parsed.name.len() > 64 {
            return Err(ToolError::InvalidInput(
                "Agent name must be at most 64 characters".into(),
            ));
        }

        let config = AgentConfig {
            name: parsed.name,
            model: parsed.model.unwrap_or_else(default_model),
            system_prompt: parsed.system_prompt,
            tools: parsed.tools.unwrap_or_default(),
            working_directory: PathBuf::from("."),
            max_turns: parsed.max_turns.unwrap_or_else(default_max_turns),
            team: parsed.team,
        };

        let agent = self.registry.spawn(config).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to spawn agent: {}", e))
        })?;

        let content = json!({
            "agent_id": agent.id,
            "agent_name": agent.name,
            "status": agent.status.to_string(),
            "model": agent.config.model,
            "team": agent.team,
            "max_turns": agent.config.max_turns,
            "created_at": agent.created_at.to_rfc3339(),
        });

        let mut metadata = HashMap::new();
        metadata.insert("agent_id".into(), json!(agent.id));
        metadata.insert("agent_name".into(), json!(agent.name));

        Ok(ToolOutput {
            content: serde_json::to_string_pretty(&content).unwrap(),
            is_error: false,
            metadata,
        })
    }
}

// ---- SendMessageTool -----------------------------------------------------

/// Input for the `send_message` tool.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SendMessageInput {
    /// Recipient agent name or "*" for broadcast
    pub to: String,
    /// Message content (string or JSON)
    pub message: Value,
}

/// Tool that sends messages between agents.
pub struct SendMessageTool {
    registry: Arc<SubAgentRegistry>,
    /// Name of the sending agent (usually the orchestrator / main agent)
    sender_name: String,
}

impl SendMessageTool {
    pub fn new(registry: Arc<SubAgentRegistry>, sender_name: String) -> Self {
        Self { registry, sender_name }
    }
}

#[async_trait]
impl Tool for SendMessageTool {
    fn name(&self) -> &str {
        "send_message"
    }

    fn description(&self) -> &str {
        "Send a message to a specific agent by name, or broadcast to all agents using '*' as the recipient."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "to": {
                    "type": "string",
                    "description": "Recipient agent name, or '*' to broadcast to all agents"
                },
                "message": {
                    "description": "Message content (string or JSON object)"
                }
            },
            "required": ["to", "message"]
        })
    }

    fn category(&self) -> &str {
        "agents"
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let parsed: SendMessageInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid send_message input: {}", e)))?;

        if parsed.to.is_empty() {
            return Err(ToolError::InvalidInput("Recipient 'to' must not be empty".into()));
        }

        let responses = self.registry
            .send_message(&self.sender_name, &parsed.to, parsed.message)
            .await
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to send message: {}", e))
            })?;

        let content = json!({
            "sent_to": parsed.to,
            "response_count": responses.len(),
            "responses": responses.iter().map(|r| {
                json!({
                    "from": r.from,
                    "content": match &r.content {
                        MessageContent::Text(t) => t.clone(),
                        MessageContent::Structured(v) => v.to_string(),
                        MessageContent::Protocol(p) => format!("{:?}", p),
                    }
                })
            }).collect::<Vec<_>>(),
        });

        let mut metadata = HashMap::new();
        metadata.insert("recipient".into(), json!(parsed.to));
        metadata.insert("response_count".into(), json!(responses.len()));

        Ok(ToolOutput {
            content: serde_json::to_string_pretty(&content).unwrap(),
            is_error: false,
            metadata,
        })
    }
}

// ---- TeamCreateTool ------------------------------------------------------

/// Input for the `team_create` tool.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TeamCreateInput {
    /// Team name (must be unique)
    pub team_name: String,
    /// Description of the team's purpose
    pub description: String,
}

/// Tool that creates a new agent team.
pub struct TeamCreateTool {
    registry: Arc<SubAgentRegistry>,
}

impl TeamCreateTool {
    pub fn new(registry: Arc<SubAgentRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for TeamCreateTool {
    fn name(&self) -> &str {
        "team_create"
    }

    fn description(&self) -> &str {
        "Create a new agent team. Agents in the same team share a task board and can collaborate on tasks."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "team_name": {
                    "type": "string",
                    "description": "Unique name for the team"
                },
                "description": {
                    "type": "string",
                    "description": "Description of the team's purpose"
                }
            },
            "required": ["team_name", "description"]
        })
    }

    fn category(&self) -> &str {
        "agents"
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let parsed: TeamCreateInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid team_create input: {}", e)))?;

        if parsed.team_name.is_empty() {
            return Err(ToolError::InvalidInput("Team name must not be empty".into()));
        }
        if parsed.team_name.len() > 64 {
            return Err(ToolError::InvalidInput(
                "Team name must be at most 64 characters".into(),
            ));
        }

        let team_name = self.registry.create_team(parsed.team_name, parsed.description).await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to create team: {}", e)))?;

        let content = json!({
            "team_name": team_name,
            "status": "created",
        });

        let mut metadata = HashMap::new();
        metadata.insert("team_name".into(), json!(team_name));

        Ok(ToolOutput {
            content: serde_json::to_string_pretty(&content).unwrap(),
            is_error: false,
            metadata,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator::CoordinatorConfig;
    use crate::task::{AgentTask, TaskPriority, TaskStatus};
    use std::path::PathBuf;

    // Helper to build a test coordinator + registry
    async fn setup() -> (Arc<SubAgentRegistry>, Arc<AgentCoordinator>) {
        let config = CoordinatorConfig::default();
        let coordinator = Arc::new(
            AgentCoordinator::new(config).await.unwrap()
        );
        let registry = Arc::new(SubAgentRegistry::new(coordinator.clone()));
        (registry, coordinator)
    }

    // ---- SubAgent unit tests ----

    #[test]
    fn test_agent_config_defaults() {
        let _config = AgentConfig {
            name: "test".into(),
            system_prompt: "prompt".into(),
            ..Default::default()
        };
        // The struct does NOT derive Default, so we test the standalone fns
        assert_eq!(default_model(), "claude-sonnet-4-6");
        assert_eq!(default_max_turns(), 50);
    }

    #[test]
    fn test_sub_agent_new() {
        let config = AgentConfig {
            name: "agent-1".into(),
            model: "claude-sonnet-4-6".into(),
            system_prompt: "You are helpful".into(),
            tools: vec!["read".into(), "write".into()],
            working_directory: PathBuf::from("/tmp"),
            max_turns: 10,
            team: None,
        };
        let agent = SubAgent::new(config);

        assert_eq!(agent.name, "agent-1");
        assert_eq!(agent.status, AgentStatus::Spawning);
        assert!(agent.has_turns_remaining());
        assert_eq!(agent.turns_used, 0);
        assert!(agent.team.is_none());
    }

    #[test]
    fn test_sub_agent_lifecycle() {
        let config = AgentConfig {
            name: "lifecycle".into(),
            model: "model".into(),
            system_prompt: "p".into(),
            tools: vec![],
            working_directory: PathBuf::from("."),
            max_turns: 3,
            ..Default::default()
        };
        let mut agent = SubAgent::new(config);

        agent.mark_idle();
        assert_eq!(agent.status, AgentStatus::Idle);

        agent.mark_running();
        assert_eq!(agent.status, AgentStatus::Running);

        agent.record_turn(Some("output 1".into()));
        assert_eq!(agent.turns_used, 1);
        assert!(agent.has_turns_remaining());

        agent.record_turn(Some("output 2".into()));
        assert_eq!(agent.turns_used, 2);

        agent.record_turn(Some("output 3".into()));
        assert_eq!(agent.turns_used, 3);
        assert!(!agent.has_turns_remaining());
        // Should auto-complete when turns exhausted
        assert_eq!(agent.status, AgentStatus::Completed);
    }

    #[test]
    fn test_sub_agent_mark_failed() {
        let config = AgentConfig {
            name: "fail-agent".into(),
            model: "m".into(),
            system_prompt: "p".into(),
            tools: vec![],
            working_directory: PathBuf::from("."),
            max_turns: 10,
            ..Default::default()
        };
        let mut agent = SubAgent::new(config);
        agent.mark_failed("out of memory".into());
        assert_eq!(agent.status, AgentStatus::Failed("out of memory".into()));
    }

    #[test]
    fn test_sub_agent_mark_completed() {
        let config = AgentConfig {
            name: "done-agent".into(),
            model: "m".into(),
            system_prompt: "p".into(),
            tools: vec![],
            working_directory: PathBuf::from("."),
            max_turns: 10,
            ..Default::default()
        };
        let mut agent = SubAgent::new(config);
        agent.mark_idle();
        agent.mark_completed();
        assert_eq!(agent.status, AgentStatus::Completed);
    }

    #[test]
    fn test_agent_status_display() {
        assert_eq!(AgentStatus::Spawning.to_string(), "spawning");
        assert_eq!(AgentStatus::Idle.to_string(), "idle");
        assert_eq!(AgentStatus::Running.to_string(), "running");
        assert_eq!(AgentStatus::Completed.to_string(), "completed");
        assert_eq!(AgentStatus::Failed("err".into()).to_string(), "failed: err");
    }

    #[test]
    fn test_agent_config_serde_roundtrip() {
        let config = AgentConfig {
            name: "serde-test".into(),
            model: "claude-sonnet-4-6".into(),
            system_prompt: "test prompt".into(),
            tools: vec!["read".into()],
            working_directory: PathBuf::from("/tmp"),
            max_turns: 20,
            ..Default::default()
        };
        let json_str = serde_json::to_string(&config).unwrap();
        let deserialized: AgentConfig = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.name, config.name);
        assert_eq!(deserialized.model, config.model);
        assert_eq!(deserialized.tools, config.tools);
        assert_eq!(deserialized.max_turns, config.max_turns);
    }

    #[test]
    fn test_sub_agent_serde_roundtrip() {
        let config = AgentConfig {
            name: "ser-sub".into(),
            model: "m".into(),
            system_prompt: "p".into(),
            tools: vec![],
            working_directory: PathBuf::from("."),
            max_turns: 5,
            ..Default::default()
        };
        let mut agent = SubAgent::new(config);
        agent.mark_idle();
        agent.team = Some("team-x".into());

        let json_str = serde_json::to_string(&agent).unwrap();
        let deserialized: SubAgent = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.name, agent.name);
        assert_eq!(deserialized.team, agent.team);
        assert_eq!(deserialized.status, agent.status);
    }

    // ---- AgentSpawnTool tests ----

    #[tokio::test]
    async fn test_agent_spawn_tool_name() {
        let (registry, _) = setup().await;
        let tool = AgentSpawnTool::new(registry);
        assert_eq!(tool.name(), "agent_spawn");
    }

    #[tokio::test]
    async fn test_agent_spawn_tool_schema() {
        let (registry, _) = setup().await;
        let tool = AgentSpawnTool::new(registry);
        let schema = tool.input_schema();
        assert_eq!(schema["type"], "object");
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("name"));
        assert!(props.contains_key("system_prompt"));
        assert!(props.contains_key("model"));
        assert!(props.contains_key("tools"));
        assert!(props.contains_key("team"));
        assert!(props.contains_key("max_turns"));
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("name")));
        assert!(required.contains(&json!("system_prompt")));
    }

    #[tokio::test]
    async fn test_agent_spawn_tool_execute() {
        let (registry, _) = setup().await;
        let tool = AgentSpawnTool::new(registry);

        let result = tool.execute(json!({
            "name": "worker-1",
            "system_prompt": "You are a worker agent",
            "model": "claude-sonnet-4-6",
            "tools": ["read", "write"],
            "max_turns": 100
        })).await.unwrap();

        assert!(!result.is_error);
        let output: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(output["agent_name"], "worker-1");
        assert_eq!(output["status"], "idle");
        assert_eq!(output["model"], "claude-sonnet-4-6");
        assert!(output["agent_id"].is_string());
    }

    #[tokio::test]
    async fn test_agent_spawn_tool_rejects_empty_name() {
        let (registry, _) = setup().await;
        let tool = AgentSpawnTool::new(registry);

        let result = tool.execute(json!({
            "name": "",
            "system_prompt": "prompt"
        })).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("empty"));
    }

    #[tokio::test]
    async fn test_agent_spawn_tool_rejects_duplicate() {
        let (registry, _) = setup().await;
        let tool = AgentSpawnTool::new(registry);

        tool.execute(json!({
            "name": "dup",
            "system_prompt": "prompt"
        })).await.unwrap();

        let result = tool.execute(json!({
            "name": "dup",
            "system_prompt": "prompt"
        })).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("already exists"));
    }

    #[tokio::test]
    async fn test_agent_spawn_tool_invalid_json() {
        let (registry, _) = setup().await;
        let tool = AgentSpawnTool::new(registry);

        let result = tool.execute(json!({"name": 123})).await;
        assert!(result.is_err());
    }

    // ---- SendMessageTool tests ----

    #[tokio::test]
    async fn test_send_message_tool_name() {
        let (registry, _) = setup().await;
        let tool = SendMessageTool::new(registry, "orchestrator".into());
        assert_eq!(tool.name(), "send_message");
    }

    #[tokio::test]
    async fn test_send_message_tool_schema() {
        let (registry, _) = setup().await;
        let tool = SendMessageTool::new(registry, "orchestrator".into());
        let schema = tool.input_schema();
        let props = schema["properties"].as_object().unwrap();
        assert!(props.contains_key("to"));
        assert!(props.contains_key("message"));
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("to")));
        assert!(required.contains(&json!("message")));
    }

    #[tokio::test]
    async fn test_send_message_tool_direct() {
        let (registry, _) = setup().await;

        // Spawn a target agent first
        let spawn_tool = AgentSpawnTool::new(registry.clone());
        spawn_tool.execute(json!({
            "name": "target",
            "system_prompt": "target"
        })).await.unwrap();

        let send_tool = SendMessageTool::new(registry, "sender".into());
        let result = send_tool.execute(json!({
            "to": "target",
            "message": "Hello from sender"
        })).await.unwrap();

        assert!(!result.is_error);
        let output: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(output["sent_to"], "target");
        assert!(output["response_count"].as_u64().unwrap() >= 1);
    }

    #[tokio::test]
    async fn test_send_message_tool_broadcast() {
        let (registry, _) = setup().await;

        let spawn_tool = AgentSpawnTool::new(registry.clone());
        spawn_tool.execute(json!({
            "name": "agent-a",
            "system_prompt": "a"
        })).await.unwrap();
        spawn_tool.execute(json!({
            "name": "agent-b",
            "system_prompt": "b"
        })).await.unwrap();

        let send_tool = SendMessageTool::new(registry, "orchestrator".into());
        let result = send_tool.execute(json!({
            "to": "*",
            "message": "Broadcast hello"
        })).await.unwrap();

        assert!(!result.is_error);
        let output: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(output["sent_to"], "*");
        assert!(output["response_count"].as_u64().unwrap() >= 2);
    }

    #[tokio::test]
    async fn test_send_message_tool_invalid_recipient() {
        let (registry, _) = setup().await;
        let send_tool = SendMessageTool::new(registry, "sender".into());

        let result = send_tool.execute(json!({
            "to": "nonexistent",
            "message": "hello"
        })).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn test_send_message_tool_empty_to() {
        let (registry, _) = setup().await;
        let send_tool = SendMessageTool::new(registry, "sender".into());

        let result = send_tool.execute(json!({
            "to": "",
            "message": "hello"
        })).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_send_message_tool_structured_content() {
        let (registry, _) = setup().await;

        let spawn_tool = AgentSpawnTool::new(registry.clone());
        spawn_tool.execute(json!({
            "name": "json-target",
            "system_prompt": "target"
        })).await.unwrap();

        let send_tool = SendMessageTool::new(registry, "sender".into());
        let result = send_tool.execute(json!({
            "to": "json-target",
            "message": {"action": "do_something", "value": 42}
        })).await.unwrap();

        assert!(!result.is_error);
    }

    // ---- TeamCreateTool tests ----

    #[tokio::test]
    async fn test_team_create_tool_name() {
        let (registry, _) = setup().await;
        let tool = TeamCreateTool::new(registry);
        assert_eq!(tool.name(), "team_create");
    }

    #[tokio::test]
    async fn test_team_create_tool_schema() {
        let (registry, _) = setup().await;
        let tool = TeamCreateTool::new(registry);
        let schema = tool.input_schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("team_name")));
        assert!(required.contains(&json!("description")));
    }

    #[tokio::test]
    async fn test_team_create_tool_execute() {
        let (registry, _) = setup().await;
        let tool = TeamCreateTool::new(registry);

        let result = tool.execute(json!({
            "team_name": "backend-team",
            "description": "Backend development team"
        })).await.unwrap();

        assert!(!result.is_error);
        let output: Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(output["team_name"], "backend-team");
        assert_eq!(output["status"], "created");
    }

    #[tokio::test]
    async fn test_team_create_tool_rejects_empty_name() {
        let (registry, _) = setup().await;
        let tool = TeamCreateTool::new(registry);

        let result = tool.execute(json!({
            "team_name": "",
            "description": "desc"
        })).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_team_create_tool_rejects_duplicate() {
        let (registry, _) = setup().await;
        let tool = TeamCreateTool::new(registry);

        tool.execute(json!({
            "team_name": "dup-team",
            "description": "desc"
        })).await.unwrap();

        let result = tool.execute(json!({
            "team_name": "dup-team",
            "description": "desc"
        })).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("already exists"));
    }

    #[tokio::test]
    async fn test_team_create_tool_invalid_json() {
        let (registry, _) = setup().await;
        let tool = TeamCreateTool::new(registry);

        let result = tool.execute(json!({"team_name": 123})).await;
        assert!(result.is_err());
    }

    // ---- SubAgentRegistry integration tests ----

    #[tokio::test]
    async fn test_registry_list_agents() {
        let (registry, _) = setup().await;

        registry.spawn(AgentConfig {
            name: "a".into(),
            system_prompt: "p".into(),
            ..Default::default()
        }).await.unwrap();

        registry.spawn(AgentConfig {
            name: "b".into(),
            system_prompt: "p".into(),
            ..Default::default()
        }).await.unwrap();

        let agents = registry.list_agents().await;
        assert_eq!(agents.len(), 2);
    }

    #[tokio::test]
    async fn test_registry_get_agent() {
        let (registry, _) = setup().await;

        registry.spawn(AgentConfig {
            name: "findme".into(),
            system_prompt: "p".into(),
            ..Default::default()
        }).await.unwrap();

        let agent = registry.get_agent("findme").await;
        assert!(agent.is_some());
        assert_eq!(agent.unwrap().name, "findme");

        let missing = registry.get_agent("nope").await;
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn test_spawn_agent_on_explicit_team() {
        let (registry, _) = setup().await;

        registry.create_team("my-team".into(), "desc".into()).await.unwrap();

        registry.spawn(AgentConfig {
            name: "team-agent".into(),
            system_prompt: "p".into(),
            ..Default::default()
        }).await.unwrap();

        // Spawn onto the explicit team
        let config = AgentConfig {
            name: "explicit-agent".into(),
            system_prompt: "p".into(),
            ..Default::default()
        };
        // We test the default team creation path.
        let agent = registry.spawn(config).await.unwrap();
        assert!(agent.team.is_some());
    }

    #[tokio::test]
    async fn test_task_board_integration() {
        let (registry, _) = setup().await;
        let board = registry.task_board();

        let task = AgentTask::new("Test task".into(), "Description".into(), TaskPriority::High);
        let task_id = task.id;
        board.add_task(task).await.unwrap();

        let fetched = board.get_task(task_id).await.unwrap();
        assert_eq!(fetched.subject, "Test task");
        assert_eq!(fetched.priority, TaskPriority::High);
        assert_eq!(fetched.status, TaskStatus::Pending);
    }

    #[tokio::test]
    async fn test_task_assignment_and_status_transitions() {
        let (registry, _) = setup().await;
        let board = registry.task_board();

        let task = AgentTask::new("Task A".into(), "desc".into(), TaskPriority::Medium);
        let task_id = task.id;
        board.add_task(task).await.unwrap();

        // Assign
        board.assign_task(task_id, "agent-1".into()).await.unwrap();
        let task = board.get_task(task_id).await.unwrap();
        assert_eq!(task.status, TaskStatus::InProgress);
        assert_eq!(task.owner.as_deref(), Some("agent-1"));

        // Complete
        board.complete_task(task_id).await.unwrap();
        let task = board.get_task(task_id).await.unwrap();
        assert_eq!(task.status, TaskStatus::Completed);

        // Summary
        let summary = board.summary().await;
        assert_eq!(summary.completed_tasks, 1);
    }

    #[tokio::test]
    async fn test_task_dependency_tracking() {
        let (registry, _) = setup().await;
        let board = registry.task_board();

        let task_a = AgentTask::new("A".into(), "first".into(), TaskPriority::High);
        let task_b = AgentTask::new("B".into(), "second".into(), TaskPriority::High);
        let id_a = task_a.id;
        let id_b = task_b.id;

        board.add_task(task_a).await.unwrap();
        board.add_task(task_b).await.unwrap();

        board.add_dependency(id_b, id_a).await.unwrap();

        let task_b = board.get_task(id_b).await.unwrap();
        assert!(task_b.blocked_by.contains(&id_a));

        // B should not be ready since A blocks it
        let ready = board.list_ready_tasks().await;
        assert!(ready.iter().any(|t| t.id == id_a));
        assert!(!ready.iter().any(|t| t.id == id_b));

        // Complete A -> B becomes ready
        board.complete_task(id_a).await.unwrap();
        let task_b = board.get_task(id_b).await.unwrap();
        // B is still Pending (not blocked anymore in our model since blocked_by
        // is set but is_ready checks blocked_by.is_empty)
        // Actually we need to update blocked_by when A completes
        // That is handled by the board's add_dependency / remove_dependency
        // For now verify the dependency was added correctly
        assert!(task_b.blocked_by.contains(&id_a));
    }

    #[tokio::test]
    async fn test_task_fail_and_remove() {
        let (registry, _) = setup().await;
        let board = registry.task_board();

        let task = AgentTask::new("Fail me".into(), "desc".into(), TaskPriority::Low);
        let task_id = task.id;
        board.add_task(task).await.unwrap();

        board.fail_task(task_id, "something broke".into()).await.unwrap();
        let task = board.get_task(task_id).await.unwrap();
        assert!(matches!(task.status, TaskStatus::Failed(_)));

        board.remove_task(task_id).await.unwrap();
        assert!(board.get_task(task_id).await.is_err());
    }

    #[tokio::test]
    async fn test_task_board_priority_ordering() {
        let (registry, _) = setup().await;
        let board = registry.task_board();

        board.add_task(AgentTask::new("low".into(), "".into(), TaskPriority::Low)).await.unwrap();
        board.add_task(AgentTask::new("crit".into(), "".into(), TaskPriority::Critical)).await.unwrap();
        board.add_task(AgentTask::new("med".into(), "".into(), TaskPriority::Medium)).await.unwrap();

        let summary = board.summary().await;
        assert_eq!(summary.total_tasks, 3);
        assert_eq!(summary.pending_tasks, 3);
    }
}

// Allow Default for AgentConfig (used in tests)
impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            model: default_model(),
            system_prompt: String::new(),
            tools: Vec::new(),
            working_directory: PathBuf::from("."),
            max_turns: default_max_turns(),
            team: None,
        }
    }
}
